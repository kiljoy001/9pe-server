//! 9P Protocol Handler
//!
//! Server-side handler for processing 9P protocol messages.

use super::{messages::*, *};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Server-side protocol handler
#[allow(dead_code)]
pub struct ProtocolHandler {
    /// Root directory for the filesystem
    root: PathBuf,

    /// Maximum message size
    msize: u32,

    /// Active fids
    fids: Arc<RwLock<HashMap<Fid, FidState>>>,

    /// Session information
    sessions: Arc<RwLock<HashMap<String, SessionInfo>>>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct FidState {
    path: PathBuf,
    qid: Qid,
    is_open: bool,
    mode: Option<u8>,
    user: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct SessionInfo {
    user: String,
    attached: bool,
    root_fid: Option<Fid>,
}

#[allow(dead_code)]
impl ProtocolHandler {
    /// Create a new protocol handler
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            msize: MAX_MSG_SIZE,
            fids: Arc::new(RwLock::new(HashMap::new())),
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Handle an incoming message
    pub async fn handle_message(&self, msg: Box<dyn Message>) -> Result<Box<dyn Message>> {
        let msg_type = msg.msg_type();
        debug!("Handling message type: {:?}", msg_type);

        // For now, return a simple error response
        // In production, we'd properly deserialize and handle each message type
        match msg_type {
            MessageType::Tversion => {
                // Handle version negotiation with default response
                Ok(Box::new(Rversion {
                    tag: msg.tag(),
                    msize: self.msize,
                    version: VERSION_9P2000.to_string(),
                }))
            }
            _ => {
                bail!(
                    "Message handling not yet fully implemented for: {:?}",
                    msg_type
                );
            }
        }
    }

    /// Handle version negotiation
    async fn handle_version(&self, msg: &Tversion) -> Result<Box<dyn Message>> {
        info!(
            "Version negotiation: client wants {} with msize {}",
            msg.version, msg.msize
        );

        // Negotiate msize and version
        let msize = msg.msize.min(self.msize);
        let version = if msg.version.starts_with("9P2000") {
            msg.version.clone()
        } else {
            VERSION_9P2000.to_string()
        };

        Ok(Box::new(Rversion {
            tag: msg.tag,
            msize,
            version,
        }))
    }

    /// Handle attach
    async fn handle_attach(&self, msg: &Tattach) -> Result<Box<dyn Message>> {
        info!("Attach: user {} to {}", msg.uname, msg.aname);

        // Create session
        let mut sessions = self.sessions.write().await;
        sessions.insert(
            msg.uname.clone(),
            SessionInfo {
                user: msg.uname.clone(),
                attached: true,
                root_fid: Some(msg.fid),
            },
        );

        // Create fid for root
        let root_qid = self.path_to_qid(&self.root).await?;
        let mut fids = self.fids.write().await;
        fids.insert(
            msg.fid,
            FidState {
                path: self.root.clone(),
                qid: root_qid,
                is_open: false,
                mode: None,
                user: msg.uname.clone(),
            },
        );

        Ok(Box::new(Rattach {
            tag: msg.tag,
            qid: root_qid,
        }))
    }

    /// Handle walk
    async fn handle_walk(&self, msg: &Twalk) -> Result<Box<dyn Message>> {
        let fids = self.fids.read().await;
        let base_fid = fids
            .get(&msg.fid)
            .ok_or_else(|| anyhow::anyhow!("Invalid fid"))?;

        let mut current_path = base_fid.path.clone();
        let mut qids = Vec::new();

        // Walk through each name
        for name in &msg.wnames {
            current_path = current_path.join(name);

            // Check if path exists and is accessible
            if !current_path.exists() {
                break;
            }

            let qid = self.path_to_qid(&current_path).await?;
            qids.push(qid);
        }

        // If we walked successfully, create new fid
        if qids.len() == msg.wnames.len() {
            let mut fids = self.fids.write().await;
            fids.insert(
                msg.newfid,
                FidState {
                    path: current_path,
                    qid: *qids.last().unwrap(),
                    is_open: false,
                    mode: None,
                    user: base_fid.user.clone(),
                },
            );
        }

        Ok(Box::new(Rwalk { tag: msg.tag, qids }))
    }

    /// Handle open
    async fn handle_open(&self, msg: &Topen) -> Result<Box<dyn Message>> {
        let mut fids = self.fids.write().await;
        let fid_state = fids
            .get_mut(&msg.fid)
            .ok_or_else(|| anyhow::anyhow!("Invalid fid"))?;

        // Check permissions (simplified)
        // In production, check actual file permissions against user

        fid_state.is_open = true;
        fid_state.mode = Some(msg.mode);

        Ok(Box::new(Ropen {
            tag: msg.tag,
            qid: fid_state.qid,
            iounit: 8192, // Default IO unit
        }))
    }

    /// Handle read
    async fn handle_read(&self, msg: &Tread) -> Result<Box<dyn Message>> {
        let fids = self.fids.read().await;
        let fid_state = fids
            .get(&msg.fid)
            .ok_or_else(|| anyhow::anyhow!("Invalid fid"))?;

        if !fid_state.is_open {
            bail!("Fid not open");
        }

        let path = &fid_state.path;

        let data = if path.is_dir() {
            // Read directory
            self.read_directory(path, msg.offset, msg.count).await?
        } else {
            // Read file
            self.read_file(path, msg.offset, msg.count).await?
        };

        Ok(Box::new(Rread { tag: msg.tag, data }))
    }

    /// Handle write
    async fn handle_write(&self, msg: &Twrite) -> Result<Box<dyn Message>> {
        let fids = self.fids.read().await;
        let fid_state = fids
            .get(&msg.fid)
            .ok_or_else(|| anyhow::anyhow!("Invalid fid"))?;

        if !fid_state.is_open {
            bail!("Fid not open");
        }

        let path = &fid_state.path;

        if path.is_dir() {
            bail!("Cannot write to directory");
        }

        // Write to file
        let count = self.write_file(path, msg.offset, &msg.data).await?;

        Ok(Box::new(Rwrite {
            tag: msg.tag,
            count: count as u32,
        }))
    }

    /// Handle stat
    async fn handle_stat(&self, msg: &Tstat) -> Result<Box<dyn Message>> {
        let fids = self.fids.read().await;
        let fid_state = fids
            .get(&msg.fid)
            .ok_or_else(|| anyhow::anyhow!("Invalid fid"))?;

        let stat = self.path_to_stat(&fid_state.path).await?;

        Ok(Box::new(Rstat { tag: msg.tag, stat }))
    }

    /// Handle clunk
    async fn handle_clunk(&self, msg: &Tclunk) -> Result<Box<dyn Message>> {
        let mut fids = self.fids.write().await;
        fids.remove(&msg.fid);

        Ok(Box::new(Rclunk { tag: msg.tag }))
    }

    /// Convert path to Qid
    #[allow(dead_code)]
    async fn path_to_qid(&self, path: &Path) -> Result<Qid> {
        let metadata = fs::metadata(path).await.context("Failed to get metadata")?;

        let qtype = if metadata.is_dir() {
            permissions::DMDIR as u8
        } else {
            0
        };

        Ok(Qid {
            qtype,
            version: metadata.mtime() as u32,
            path: metadata.ino(),
        })
    }

    /// Convert path to Stat
    #[allow(dead_code)]
    async fn path_to_stat(&self, path: &Path) -> Result<Stat> {
        let metadata = fs::metadata(path).await.context("Failed to get metadata")?;

        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        let mode = if metadata.is_dir() {
            permissions::DMDIR | 0o755
        } else {
            0o644
        };

        Ok(Stat {
            size: 0, // Will be calculated
            typ: 0,
            dev: 0,
            qid: self.path_to_qid(path).await?,
            mode,
            atime: metadata.atime() as u32,
            mtime: metadata.mtime() as u32,
            length: metadata.len(),
            name,
            uid: "nobody".to_string(),
            gid: "nobody".to_string(),
            muid: "nobody".to_string(),
        })
    }

    /// Read file contents
    #[allow(dead_code)]
    async fn read_file(&self, path: &Path, offset: u64, count: u32) -> Result<Vec<u8>> {
        use tokio::io::{AsyncReadExt, AsyncSeekExt};

        let mut file = tokio::fs::File::open(path)
            .await
            .context("Failed to open file")?;

        file.seek(std::io::SeekFrom::Start(offset)).await?;

        let mut buffer = vec![0u8; count as usize];
        let n = file.read(&mut buffer).await?;
        buffer.truncate(n);

        Ok(buffer)
    }

    /// Write file contents
    #[allow(dead_code)]
    async fn write_file(&self, path: &Path, offset: u64, data: &[u8]) -> Result<usize> {
        use tokio::io::{AsyncSeekExt, AsyncWriteExt};

        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .await
            .context("Failed to open file for writing")?;

        file.seek(std::io::SeekFrom::Start(offset)).await?;
        let n = file.write(data).await?;

        Ok(n)
    }

    /// Read directory entries
    #[allow(dead_code)]
    async fn read_directory(&self, path: &Path, offset: u64, count: u32) -> Result<Vec<u8>> {
        let mut entries = fs::read_dir(path)
            .await
            .context("Failed to read directory")?;

        let mut buffer = Vec::new();
        let mut current_offset = 0u64;

        while let Some(entry) = entries.next_entry().await? {
            if current_offset < offset {
                current_offset += 1;
                continue;
            }

            let _stat = self.path_to_stat(&entry.path()).await?;

            // Encode stat to buffer (simplified)
            // In production, properly encode the stat structure
            let name = entry.file_name().to_string_lossy().into_owned();
            buffer.extend_from_slice(name.as_bytes());
            buffer.push(0); // Null terminator

            if buffer.len() >= count as usize {
                break;
            }
        }

        Ok(buffer)
    }
}

// Type conversion helpers will be implemented when needed
// For now we'll use a simpler approach without downcasting
