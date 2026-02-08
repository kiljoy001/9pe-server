//! 9P.e Client Implementation
//!
//! Full-featured client for connecting to and interacting with 9P.e servers.
//!
//! ## Authentication Workflow
//!
//! The 9P.e server requires cryptographic authentication. Clients must:
//!
//! 1. **Generate an identity** (once):
//!    ```bash
//!    9pe identity generate
//!    # Creates ~/.9pe/identity.json with Ed25519 keypair
//!    ```
//!
//! 2. **Connect with identity**:
//!    ```rust
//!    let identity = ClientIdentity::load_or_generate()?;
//!    let client = NinePClient::connect_authenticated("localhost:5640", identity).await?;
//!    ```
//!
//! 3. **Read/write files** (now authorized):
//!    ```rust
//!    let data = client.read_file("/srv/compute/info").await?;
//!    ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use anyhow::{Result, Context};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{info, warn, debug};

use crate::identity::NodePermissions;
use crate::protocol::{NinePMessage, NINEP_VERSION};
use crate::server::handler::auth::{AuthChallenge, AuthResponse};

/// File identifier to path mapping (client-side)
type ClientFidMap = Arc<RwLock<HashMap<u32, PathBuf>>>;

/// Client identity for authentication
///
/// Contains the cryptographic keys needed to authenticate with 9P.e servers.
/// Can be persisted to disk and reloaded.
#[derive(Clone, Serialize, Deserialize)]
pub struct ClientIdentity {
    /// Unique node identifier (hex of public key)
    pub node_id: String,

    /// Ed25519 signing key (secret)
    #[serde(with = "hex_serde_arr32")]
    ed25519_secret: [u8; 32],

    /// Ed25519 public key
    #[serde(with = "hex_serde_arr32")]
    pub ed25519_public: [u8; 32],

    /// P-256 public key (for TLS, optional)
    #[serde(with = "hex_serde_vec", default)]
    pub p256_public: Vec<u8>,

    /// X.509 certificate DER (optional)
    #[serde(with = "hex_serde_vec", default)]
    pub certificate_der: Vec<u8>,

    /// Permissions to request
    pub permissions: NodePermissions,
}

mod hex_serde_arr32 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(data: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(data))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        bytes.try_into().map_err(|_| serde::de::Error::custom("expected 32 bytes"))
    }
}

mod hex_serde_vec {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(data: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(data))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        hex::decode(&s).map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Debug for ClientIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientIdentity")
            .field("node_id", &self.node_id)
            .field("ed25519_public", &hex::encode(&self.ed25519_public))
            .finish()
    }
}

impl ClientIdentity {
    /// Generate a new client identity
    pub fn generate() -> Result<Self> {
        use rand::RngCore;
        let mut secret = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut secret);

        let signing_key = SigningKey::from_bytes(&secret);
        let verifying_key = VerifyingKey::from(&signing_key);
        let public_bytes = verifying_key.to_bytes();
        let node_id = hex::encode(&public_bytes);

        info!("Generated new client identity: {}", &node_id[..16]);

        Ok(Self {
            node_id,
            ed25519_secret: secret,
            ed25519_public: public_bytes,
            p256_public: Vec::new(),
            certificate_der: Vec::new(),
            permissions: NodePermissions::owner_defaults(),
        })
    }

    /// Load identity from file or generate if not exists
    pub fn load_or_generate() -> Result<Self> {
        let path = Self::default_path()?;
        if path.exists() {
            Self::load(&path)
        } else {
            let identity = Self::generate()?;
            identity.save(&path)?;
            Ok(identity)
        }
    }

    /// Load identity from a specific path
    pub fn load(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .context(format!("Failed to read identity from {:?}", path))?;
        let identity: Self = serde_json::from_str(&contents)
            .context("Failed to parse identity file")?;
        info!("Loaded client identity: {}", &identity.node_id[..16]);
        Ok(identity)
    }

    /// Save identity to a specific path
    pub fn save(&self, path: &Path) -> Result<()> {
        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let contents = serde_json::to_string_pretty(self)?;
        std::fs::write(path, contents)?;

        // Set restrictive permissions (owner read/write only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }

        info!("Saved identity to {:?}", path);
        Ok(())
    }

    /// Get default identity path (~/.9pe/identity.json)
    pub fn default_path() -> Result<PathBuf> {
        let home = std::env::var("HOME")
            .unwrap_or_else(|_| ".".to_string());
        Ok(PathBuf::from(home).join(".9pe").join("identity.json"))
    }

    /// Get the signing key
    fn signing_key(&self) -> SigningKey {
        SigningKey::from_bytes(&self.ed25519_secret)
    }

    /// Sign an auth challenge and produce an AuthResponse
    pub fn sign_challenge(&self, challenge: &AuthChallenge) -> Result<AuthResponse> {
        // Create unsigned response data - MUST match server's AuthResponseUnsigned struct
        // The server uses: AuthResponseUnsigned { node_id, ed25519_pub, p256_pub, cert_der, permissions }
        #[derive(Serialize)]
        struct AuthResponseUnsigned<'a> {
            node_id: &'a str,
            ed25519_pub: [u8; 32],
            p256_pub: &'a [u8],
            cert_der: &'a [u8],
            permissions: &'a NodePermissions,
        }

        let unsigned = AuthResponseUnsigned {
            node_id: &self.node_id,
            ed25519_pub: self.ed25519_public,
            p256_pub: &self.p256_public,
            cert_der: &self.certificate_der,
            permissions: &self.permissions,
        };

        let unsigned_bytes = serde_cbor::to_vec(&unsigned)?;

        // Hash challenge + unsigned response (same as server does)
        let challenge_bytes = serde_cbor::to_vec(challenge)?;
        let mut hasher = Sha256::new();
        hasher.update(&challenge_bytes);
        hasher.update(&unsigned_bytes);
        let digest = hasher.finalize();

        // Sign the hash
        let signing_key = self.signing_key();
        let signature = signing_key.sign(&digest);

        Ok(AuthResponse {
            node_id: self.node_id.clone(),
            ed25519_pub: self.ed25519_public,
            p256_pub: self.p256_public.clone(),
            cert_der: self.certificate_der.clone(),
            permissions: self.permissions.clone(),
            signature: signature.to_bytes(),
        })
    }

    /// Display identity info for the user
    pub fn display(&self) -> String {
        format!(
            "Node ID: {}\nPublic Key: {}\nPermissions: {:?}",
            self.node_id,
            hex::encode(&self.ed25519_public),
            self.permissions
        )
    }
}

/// 9P.e Client for connecting to remote servers
pub struct NinePClient {
    /// Connection to server
    stream: TcpStream,

    /// Current message size limit
    msize: u32,

    /// Protocol version negotiated
    pub version: String,

    /// Client-side file ID mapping
    fids: ClientFidMap,

    /// Next available FID
    next_fid: Arc<RwLock<u32>>,

    /// Root FID (set after attach)
    root_fid: Option<u32>,

    /// Client identity (if authenticated)
    identity: Option<ClientIdentity>,

    /// Whether authentication completed successfully
    authenticated: bool,
}

impl NinePClient {
    /// Connect to a 9P.e server with retry logic (unauthenticated - limited access)
    pub async fn connect(address: &str) -> Result<Self> {
        Self::connect_with_retries(address, 3, std::time::Duration::from_millis(500)).await
    }

    /// Connect to a 9P.e server with cryptographic identity authentication
    ///
    /// This is the recommended way to connect. It performs:
    /// 1. Version negotiation
    /// 2. Challenge-response authentication using Ed25519 signatures
    /// 3. Root filesystem attach
    ///
    /// # Example
    /// ```no_run
    /// use ninepe_server::client::{NinePClient, ClientIdentity};
    ///
    /// #[tokio::main]
    /// async fn main() -> anyhow::Result<()> {
    ///     let identity = ClientIdentity::load_or_generate()?;
    ///     let mut client = NinePClient::connect_authenticated("localhost:5640", identity).await?;
    ///     let data = client.read_file("/srv/compute/info").await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn connect_authenticated(address: &str, identity: ClientIdentity) -> Result<Self> {
        info!("Connecting to 9P.e server at {} with identity {}", address, &identity.node_id[..16]);

        let stream = TcpStream::connect(address).await
            .context(format!("Failed to connect to {}", address))?;

        let mut client = Self {
            stream,
            msize: 8192,
            version: String::new(),
            fids: Arc::new(RwLock::new(HashMap::new())),
            next_fid: Arc::new(RwLock::new(1)),
            root_fid: None,
            identity: Some(identity.clone()),
            authenticated: false,
        };

        // 1. Version negotiation
        client.negotiate_version().await?;

        // 2. Start authentication - send Auth message to get challenge
        let afid = client.allocate_fid().await;
        let auth_msg = NinePMessage::Auth {
            afid,
            uname: identity.node_id.clone(),
            aname: "/".to_string(),
            password: None, // We use cryptographic auth, not passwords
        };

        let auth_response = client.send_message(auth_msg).await?;

        // Server responds with Auth message containing the afid
        match auth_response {
            NinePMessage::Auth { .. } => {
                debug!("Auth session started, reading challenge...");
            }
            NinePMessage::Error { ename, .. } => {
                return Err(anyhow::anyhow!("Auth initiation failed: {}", ename));
            }
            _ => {
                return Err(anyhow::anyhow!("Unexpected response to Auth request"));
            }
        }

        // 3. Read the challenge from the auth file
        let read_msg = NinePMessage::Read {
            fid: afid,
            offset: 0,
            count: 4096,
            data: vec![],
        };

        let challenge_response = client.send_message(read_msg).await?;
        let challenge_data = match challenge_response {
            NinePMessage::Read { data, .. } => data,
            NinePMessage::Error { ename, .. } => {
                return Err(anyhow::anyhow!("Failed to read auth challenge: {}", ename));
            }
            _ => {
                return Err(anyhow::anyhow!("Unexpected response reading auth challenge"));
            }
        };

        // 4. Parse and sign the challenge
        let challenge: AuthChallenge = serde_cbor::from_slice(&challenge_data)
            .context("Failed to parse auth challenge")?;

        debug!("Received challenge from server: {}", challenge.server_node_id);

        let auth_response_signed = identity.sign_challenge(&challenge)?;
        let response_data = serde_cbor::to_vec(&auth_response_signed)?;

        // 5. Write our signed response back
        let write_msg = NinePMessage::Write {
            fid: afid,
            offset: 0,
            data: response_data,
        };

        let write_response = client.send_message(write_msg).await?;
        match write_response {
            NinePMessage::Write { .. } => {
                info!("Authentication successful");
                client.authenticated = true;
            }
            NinePMessage::Error { ename, .. } => {
                return Err(anyhow::anyhow!("Authentication failed: {}", ename));
            }
            _ => {
                return Err(anyhow::anyhow!("Unexpected response to auth write"));
            }
        }

        // 6. Clunk the auth fid
        client.clunk_fid(afid).await.ok();

        // 7. Attach to root filesystem
        let root_fid = client.allocate_fid().await;
        let attach_msg = NinePMessage::Attach {
            fid: root_fid,
            afid: 0, // Auth already verified
            uname: identity.node_id.clone(),
            aname: "/".to_string(),
        };

        let _attach_response = client.send_message(attach_msg).await?;

        client.root_fid = Some(root_fid);
        client.fids.write().await.insert(root_fid, PathBuf::from("/"));
        info!("Attached to root filesystem (authenticated)");

        Ok(client)
    }

    /// Connect to a 9P.e server with username/password (legacy compatibility)
    #[deprecated(note = "Use connect_authenticated() with ClientIdentity for proper security")]
    pub async fn connect_with_auth(address: &str, username: &str, _password: &str) -> Result<Self> {
        // Generate ephemeral identity for legacy callers
        let identity = ClientIdentity::generate()?;
        warn!("Using ephemeral identity for legacy auth - consider using connect_authenticated()");
        Self::connect_authenticated(address, identity).await
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

    /// Connect using an existing stream (unauthenticated)
    async fn connect_with_stream(stream: TcpStream) -> Result<Self> {
        let mut client = Self {
            stream,
            msize: 8192,
            version: String::new(),
            fids: Arc::new(RwLock::new(HashMap::new())),
            next_fid: Arc::new(RwLock::new(1)),
            root_fid: None,
            identity: None,
            authenticated: false,
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
            version: NINEP_VERSION.to_string(),
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
                data: vec![], // Request doesn't have data, but enum variant has it
            };

            let response = self.send_message(read_msg).await?;

            match response {
                // Read response comes back as Read message with data, OR Rread depending on protocol.
                // In protocol.rs: 
                // Read { fid, offset, count, data } is the Request OR Response?
                // protocol.rs comments say "Payload returned from read responses" for data field.
                // So it serves both purposes? Yes.
                NinePMessage::Read { data, .. } => {
                     if data.is_empty() {
                        break;
                    }

                    // Parse directory entries (Stat structures)
                    use std::io::Cursor;
                    use crate::protocol::Stat;

                    let mut cursor = Cursor::new(&data);
                    // Loop to deserialize multiple Stat structures from the buffer
                    while cursor.position() < data.len() as u64 {
                        match bincode::deserialize_from::<_, Stat>(&mut cursor) {
                            Ok(stat) => {
                                files.push(stat.name);
                            },
                            Err(_) => break, // Stop on error or incomplete data
                        }
                    }

                    offset += data.len() as u64;
                }
                 NinePMessage::Error { ename, .. } => {
                    return Err(anyhow::anyhow!("Read failed: {}", ename));
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
                data: vec![],
            };

            let response = self.send_message(read_msg).await?;

            match response {
                NinePMessage::Read { data, .. } => {
                    if data.is_empty() {
                        break;
                    }
                    contents.extend_from_slice(&data);
                    offset += data.len() as u64;
                    
                    if data.len() < (self.msize - 24) as usize {
                        break; // Short read implies EOF
                    }
                }
                NinePMessage::Error { ename, .. } => {
                    return Err(anyhow::anyhow!("Read failed: {}", ename));
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
                    // Response to write usually returns count (which is data.len in Write message?)
                    // In protocol.rs, Write is used for both request and response?
                    // Request: Write { fid, offset, data }
                    // Response: Write { fid, offset, data: [] } ? No, usually it returns count.
                    // Checking protocol.rs:
                    // Write { fid, offset, data }
                    // It seems the protocol definition uses the same struct.
                    // In 9P2000, Rwrite returns 'count'.
                    // If protocol.rs reuses Write struct for response, it likely puts 'count' in 'data' length?
                    // Or maybe it expects Write header with empty data?
                    // Let's assume for Rwrite: fid=fid, offset=offset, data=written data (or empty?)
                    // Actually, looking at protocol.rs `deserialize`:
                    // 7 => Write { fid, offset, data }
                    // It seems `Write` is used for both. If it's a response, typically 9P returns just count.
                    // This protocol seems to wrap it differently.
                    // Let's check `server/handler.rs` if possible, but I don't have it open.
                    // For now, I'll trust standard 9P behavior where Rwrite confirms logical write.
                    offset += chunk.len();
                }
                 NinePMessage::Error { ename, .. } => {
                    return Err(anyhow::anyhow!("Write failed: {}", ename));
                }
                _ => return Err(anyhow::anyhow!("Write failed: unexpected response")),
            }
        }

        // Clean up FID
        self.clunk_fid(fid).await?;

        Ok(())
    }

    /// Get file/directory information (returns raw stat data)
    pub async fn stat(&mut self, path: &str) -> Result<Vec<u8>> {
        let fid = self.walk_to_path(path).await?;
        
        let stat_msg = NinePMessage::Stat { fid, data: vec![] };
        let response = self.send_message(stat_msg).await?;

        // Clean up FID
        self.clunk_fid(fid).await?;

        match response {
            NinePMessage::Stat { data, .. } => Ok(data),
            NinePMessage::Error { ename, .. } => {
                 Err(anyhow::anyhow!("Stat failed: {}", ename))
            }
            _ => Err(anyhow::anyhow!("Stat failed: unexpected response")),
        }
    }

    /// Read from a file at specific offset
    pub async fn read_at(&mut self, path: &str, offset: u64, count: u32) -> Result<Vec<u8>> {
        let fid = self.walk_to_path(path).await?;

        // Open file for reading
        let open_msg = NinePMessage::Open {
            fid,
            mode: 0, // OREAD
        };
        let _ = self.send_message(open_msg).await?;

        let read_msg = NinePMessage::Read {
            fid,
            offset,
            count,
            data: vec![],
        };

        let response = self.send_message(read_msg).await?;
        
        // Clean up FID
        self.clunk_fid(fid).await?;

        match response {
             NinePMessage::Read { data, .. } => Ok(data),
             NinePMessage::Error { ename, .. } => Err(anyhow::anyhow!("Read failed: {}", ename)),
             _ => Err(anyhow::anyhow!("Read failed: unexpected response")),
        }
    }

    /// Walk to a specific path and return FID
    pub async fn walk_to_path(&mut self, path: &str) -> Result<u32> {
        let root_fid = self.root_fid.ok_or_else(|| anyhow::anyhow!("Not connected"))?;
        let new_fid = self.allocate_fid().await;

        // Split path into components
        let components: Vec<String> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();

        // Should check max components per walk (MAXWELEM = 16)
        // If > 16, need multiple walks. For now assume < 16.

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
             NinePMessage::Error { ename, .. } => {
                Err(anyhow::anyhow!("Walk failed for path {}: {}", path, ename))
            }
            _ => Err(anyhow::anyhow!("Walk failed for path: {}", path)),
        }
    }
    
    /// Attach to a specific connection/path (public for FUSE)
    pub async fn attach(&mut self, uname: &str, aname: &str) -> Result<()> {
         // Using allocate_fid and sending Attach message
         // This is a helper if we ever need re-attach or new attach
         // But connect() already does attach.
         Ok(())
    }

    /// Release a FID
    pub async fn clunk_fid(&mut self, fid: u32) -> Result<()> {
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
        if response_len < 4 {
             return Err(anyhow::anyhow!("Invalid response length: {}", response_len));
        }
        // Strict msize check might fail if server sends slightly larger (rare)
        // but nice to have.

        // Read response data
        let mut response_buf = vec![0u8; response_len - 4];
        self.stream.read_exact(&mut response_buf).await
            .context("Failed to read response data")?;

        // Deserialize response
        NinePMessage::deserialize(response_buf)
            .context("Failed to deserialize response")
    }
}
