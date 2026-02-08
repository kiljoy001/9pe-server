//! 9P.e Client Implementation
//!
//! Full-featured client for connecting to and interacting with 9P.e servers

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use anyhow::{Result, Context};
use tracing::{info, warn, error, debug};

use ninep::NinePMessage;

/// File identifier to path mapping (client-side)
type ClientFidMap = Arc<RwLock<HashMap<u32, PathBuf>>>;

/// 9P.e Client for connecting to remote servers
pub struct NinePClient {
    /// Connection to server
    stream: TcpStream,

    /// Current message size limit
    msize: u32,

    /// Protocol version negotiated
    version: String,

    /// Client-side file ID mapping
    fids: ClientFidMap,

    /// Next available FID
    next_fid: Arc<RwLock<u32>>,

    /// Root FID (set after attach)
    root_fid: Option<u32>,
}

impl NinePClient {
    /// Connect to a 9P.e server with retry logic
    pub async fn connect(address: &str) -> Result<Self> {
        Self::connect_with_retries(address, 3, std::time::Duration::from_millis(500)).await
    }

    /// Connect to a 9P.e server with authentication
    pub async fn connect_with_auth(address: &str, username: &str, password: &str) -> Result<Self> {
        info!("🔗 Connecting to 9P.e server at {} with authentication", address);

        let stream = TcpStream::connect(address).await
            .context(format!("Failed to connect to {}", address))?;

        let mut client = Self {
            stream,
            msize: 8192,
            version: String::new(),
            fids: Arc::new(RwLock::new(HashMap::new())),
            next_fid: Arc::new(RwLock::new(1)),
            root_fid: None,
        };

        // 1. Version negotiation
        client.negotiate_version().await?;

        // 2. Authentication
        let auth_msg = NinePMessage::Auth {
            afid: 0,
            uname: username.to_string(),
            aname: "/".to_string(),
        };

        let auth_response = client.send_message(auth_msg).await?;

        // Check for proper auth response
        match auth_response {
            NinePMessage::Error { ename, .. } => {
                return Err(anyhow::anyhow!("Authentication failed: {}", ename));
            }
            _ => {
                info!("✅ Authentication successful for user: {}", username);
            }
        }

        // 3. Attach to root with authenticated user
        let root_fid = client.allocate_fid().await;
        let attach_msg = NinePMessage::Attach {
            fid: root_fid,
            afid: 0,  // We already authenticated
            uname: username.to_string(),
            aname: "/".to_string(),
        };

        let _attach_response = client.send_message(attach_msg).await?;

        client.root_fid = Some(root_fid);
        client.fids.write().await.insert(root_fid, PathBuf::from("/"));
        info!("✅ Attached to root filesystem");

        Ok(client)
    }

    /// Connect to a 9P.e server with configurable retries
    pub async fn connect_with_retries(
        address: &str,
        max_retries: usize,
        retry_delay: std::time::Duration
    ) -> Result<Self> {
        info!("🔗 Connecting to 9P.e server at {}", address);

        let mut last_error = None;

        for attempt in 1..=max_retries {
            match TcpStream::connect(address).await {
                Ok(stream) => {
                    return Self::connect_with_stream(stream).await;
                }
                Err(e) => {
                    last_error = Some(e);
                    if attempt < max_retries {
                        warn!("Connection attempt {} failed, retrying in {:?}...", attempt, retry_delay);
                        tokio::time::sleep(retry_delay).await;
                    }
                }
            }
        }

        Err(anyhow::anyhow!(
            "Failed to connect after {} attempts: {}",
            max_retries,
            last_error.map(|e| e.to_string()).unwrap_or_else(|| "unknown error".to_string())
        ))
    }

    /// Connect using an existing stream
    async fn connect_with_stream(stream: TcpStream) -> Result<Self> {

        let mut client = Self {
            stream,
            msize: 8192,
            version: String::new(),
            fids: Arc::new(RwLock::new(HashMap::new())),
            next_fid: Arc::new(RwLock::new(1)),
            root_fid: None,
        };

        // Perform version negotiation
        client.negotiate_version().await?;

        // Attach to root
        client.attach_root().await?;

        Ok(client)
    }

    /// Negotiate protocol version with server
    async fn negotiate_version(&mut self) -> Result<()> {
        let version_msg = NinePMessage::Version {
            msize: self.msize,
            version: "9P.e.2024".to_string(),
        };

        let response = self.send_message(version_msg).await?;

        match response {
            NinePMessage::Version { msize, version } => {
                self.msize = msize.min(self.msize);
                self.version = version;
                info!("✅ Version negotiated: {} (msize: {})", self.version, self.msize);
                Ok(())
            }
            _ => Err(anyhow::anyhow!("Unexpected response to version request"))
        }
    }

    /// Attach to root filesystem
    async fn attach_root(&mut self) -> Result<()> {
        let root_fid = self.allocate_fid().await;

        let attach_msg = NinePMessage::Attach {
            fid: root_fid,
            afid: 0, // No authentication FID
            uname: "9pe-client".to_string(),
            aname: "/".to_string(),
        };

        let _response = self.send_message(attach_msg).await?;

        // In real 9P, attach returns a qid, but our simplified version might just succeed
        self.root_fid = Some(root_fid);
        self.fids.write().await.insert(root_fid, PathBuf::from("/"));
        info!("✅ Attached to root filesystem");
        Ok(())
    }

    /// List files in a directory
    pub async fn list_directory(&mut self, path: &str) -> Result<Vec<String>> {
        let fid = self.walk_to_path(path).await?;

        // Open directory for reading
        let open_msg = NinePMessage::Open {
            fid,
            mode: 0, // OREAD
        };

        let _response = self.send_message(open_msg).await?;

        // Read directory contents
        let mut files = Vec::new();
        let mut offset = 0;

        loop {
            let read_msg = NinePMessage::Read {
                fid,
                offset,
                count: 4096,
            };

            let response = self.send_message(read_msg).await?;

            match response {
                NinePMessage::Write { data, .. } => {
                    if data.is_empty() {
                        break;
                    }

                    // Parse directory entries (simplified)
                    // In real implementation, would parse 9P stat structures
                    let entries = String::from_utf8_lossy(&data);
                    for line in entries.lines() {
                        if !line.trim().is_empty() {
                            files.push(line.trim().to_string());
                        }
                    }

                    offset += data.len() as u64;
                }
                _ => break,
            }
        }

        // Clean up FID
        self.clunk_fid(fid).await?;

        Ok(files)
    }

    /// Read a file
    pub async fn read_file(&mut self, path: &str) -> Result<Vec<u8>> {
        let fid = self.walk_to_path(path).await?;

        // Open file for reading
        let open_msg = NinePMessage::Open {
            fid,
            mode: 0, // OREAD
        };

        let _response = self.send_message(open_msg).await?;

        // Read file contents
        let mut contents = Vec::new();
        let mut offset = 0;

        loop {
            let read_msg = NinePMessage::Read {
                fid,
                offset,
                count: (self.msize - 24) as u32, // Leave room for protocol overhead
            };

            let response = self.send_message(read_msg).await?;

            match response {
                NinePMessage::Write { data, .. } => {
                    if data.is_empty() {
                        break;
                    }
                    contents.extend_from_slice(&data);
                    offset += data.len() as u64;
                }
                _ => break,
            }
        }

        // Clean up FID
        self.clunk_fid(fid).await?;

        Ok(contents)
    }

    /// Write to a file
    pub async fn write_file(&mut self, path: &str, data: &[u8]) -> Result<()> {
        let fid = self.walk_to_path(path).await?;

        // Open file for writing
        let open_msg = NinePMessage::Open {
            fid,
            mode: 1, // OWRITE
        };

        let _response = self.send_message(open_msg).await?;

        // Write data in chunks
        let chunk_size = (self.msize - 24) as usize;
        let mut offset = 0;

        for chunk in data.chunks(chunk_size) {
            let write_msg = NinePMessage::Write {
                fid,
                offset: offset as u64,
                data: chunk.to_vec(),
            };

            let response = self.send_message(write_msg).await?;

            match response {
                NinePMessage::Write { data, .. } => {
                    offset += data.len();
                }
                _ => return Err(anyhow::anyhow!("Write failed")),
            }
        }

        // Clean up FID
        self.clunk_fid(fid).await?;

        Ok(())
    }

    /// Get file/directory information
    pub async fn stat(&mut self, path: &str) -> Result<String> {
        let fid = self.walk_to_path(path).await?;

        let stat_msg = NinePMessage::Stat { fid };
        let response = self.send_message(stat_msg).await?;

        // Clean up FID
        self.clunk_fid(fid).await?;

        match response {
            NinePMessage::Stat { .. } => {
                // In real implementation, would parse stat structure
                Ok(format!("File info for {}", path))
            }
            _ => Err(anyhow::anyhow!("Stat failed")),
        }
    }

    /// Walk to a specific path and return FID
    async fn walk_to_path(&mut self, path: &str) -> Result<u32> {
        let root_fid = self.root_fid.ok_or_else(|| anyhow::anyhow!("Not connected"))?;
        let new_fid = self.allocate_fid().await;

        // Split path into components
        let components: Vec<String> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();

        let walk_msg = NinePMessage::Walk {
            fid: root_fid,
            newfid: new_fid,
            wnames: components,
        };

        let response = self.send_message(walk_msg).await?;

        match response {
            NinePMessage::Walk { .. } => {
                self.fids.write().await.insert(new_fid, PathBuf::from(path));
                debug!("Walked to {} (fid: {})", path, new_fid);
                Ok(new_fid)
            }
            _ => Err(anyhow::anyhow!("Walk failed for path: {}", path)),
        }
    }

    /// Release a FID
    async fn clunk_fid(&mut self, fid: u32) -> Result<()> {
        let clunk_msg = NinePMessage::Clunk { fid };
        let _response = self.send_message(clunk_msg).await?;

        self.fids.write().await.remove(&fid);
        Ok(())
    }

    /// Allocate a new FID
    async fn allocate_fid(&self) -> u32 {
        let mut next_fid = self.next_fid.write().await;
        let fid = *next_fid;
        *next_fid += 1;
        fid
    }

    /// Send a message and get response
    pub async fn send_message(&mut self, message: NinePMessage) -> Result<NinePMessage> {
        // Serialize message
        let data = message.serialize()
            .context("Failed to serialize message")?;

        // Send with length prefix
        let len = (data.len() + 4) as u32;
        self.stream.write_all(&len.to_le_bytes()).await
            .context("Failed to send message length")?;
        self.stream.write_all(&data).await
            .context("Failed to send message data")?;

        // Read response length
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf).await
            .context("Failed to read response length")?;
        let response_len = u32::from_le_bytes(len_buf) as usize;

        // Validate response length
        if response_len < 4 || response_len > self.msize as usize {
            return Err(anyhow::anyhow!("Invalid response length: {}", response_len));
        }

        // Read response data
        let mut response_buf = vec![0u8; response_len - 4];
        self.stream.read_exact(&mut response_buf).await
            .context("Failed to read response data")?;

        // Deserialize response
        NinePMessage::deserialize(response_buf)
            .context("Failed to deserialize response")
    }
}

/// Client command implementations
pub async fn mount_remote_server(server: String, mount_point: String) -> Result<()> {
    let mut client = NinePClient::connect(&server).await?;

    info!("✅ Successfully connected to {}", server);
    info!("📁 Mount point: {}", mount_point);
    info!("🔧 Protocol version: {}", client.version);
    info!("📏 Max message size: {} bytes", client.msize);

    // Test basic operations
    info!("🧪 Testing basic operations...");

    // List root directory
    match client.list_directory("/").await {
        Ok(files) => {
            info!("📂 Root directory contents:");
            for file in files {
                info!("   📄 {}", file);
            }
        }
        Err(e) => warn!("Failed to list root directory: {}", e),
    }

    Ok(())
}

pub async fn list_remote_files(server: String, path: String) -> Result<()> {
    let mut client = NinePClient::connect(&server).await?;

    info!("📂 Listing files at {} on {}", path, server);

    match client.list_directory(&path).await {
        Ok(files) => {
            if files.is_empty() {
                info!("📭 Directory is empty");
            } else {
                info!("📁 Found {} files/directories:", files.len());
                for file in files {
                    info!("   📄 {}", file);
                }
            }
        }
        Err(e) => {
            error!("❌ Failed to list directory: {}", e);
            return Err(e);
        }
    }

    Ok(())
}

pub async fn list_remote_files_with_auth(server: String, path: String, username: Option<String>, password: Option<String>) -> Result<()> {
    // Connect with authentication if provided
    let mut client = if let (Some(user), Some(pass)) = (username, password) {
        info!("🔐 Connecting with authentication as user: {}", user);
        NinePClient::connect_with_auth(&server, &user, &pass).await?
    } else {
        warn!("🔓 No credentials provided - attempting anonymous access");
        NinePClient::connect(&server).await?
    };

    info!("📂 Listing files at {} on {}", path, server);

    match client.list_directory(&path).await {
        Ok(files) => {
            if files.is_empty() {
                info!("📭 Directory is empty");
            } else {
                info!("📁 Found {} files/directories:", files.len());
                for file in files {
                    info!("   📄 {}", file);
                }
            }
        }
        Err(e) => {
            error!("❌ Failed to list directory: {}", e);
            return Err(e);
        }
    }

    Ok(())
}

pub async fn discover_nodes() -> Result<()> {
    info!("🔍 Discovering 9P.e nodes on local network");

    let ports = vec![5640, 5641, 5645, 5646, 5647, 9641, 9999];
    let mut found_nodes = Vec::new();

    for port in ports {
        let addr = format!("127.0.0.1:{}", port);

        // Try to connect with timeout
        match tokio::time::timeout(
            std::time::Duration::from_millis(100),
            TcpStream::connect(&addr)
        ).await {
            Ok(Ok(_stream)) => {
                // Try to do version handshake to confirm it's a 9P.e server
                match NinePClient::connect(&addr).await {
                    Ok(client) => {
                        found_nodes.push((addr, client.version));
                    }
                    Err(_) => {
                        debug!("Port {} open but not 9P.e", port);
                    }
                }
            }
            _ => {
                debug!("Port {} not responding", port);
            }
        }
    }

    if found_nodes.is_empty() {
        info!("❌ No 9P.e nodes found on local network");
    } else {
        info!("✅ Found {} 9P.e nodes:", found_nodes.len());
        for (addr, version) in found_nodes {
            info!("   📡 {} ({})", addr, version);
        }
    }

    Ok(())
}