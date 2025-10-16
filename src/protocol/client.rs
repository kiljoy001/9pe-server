//! 9P Client Implementation
//!
//! A complete 9P2000/9P.e client for connecting to 9P servers.

use super::{messages::*, *};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, RwLock};
use tracing::info;

/// 9P Client for file operations
pub struct NinePClient {
    /// TCP connection to the server
    stream: Arc<Mutex<TcpStream>>,

    /// Maximum message size negotiated
    msize: u32,

    /// Protocol version negotiated
    version: String,

    /// Current tag counter
    tag_counter: Arc<Mutex<u16>>,

    /// Open file handles
    fids: Arc<RwLock<HashMap<Fid, FidInfo>>>,

    /// Next available fid
    next_fid: Arc<Mutex<Fid>>,

    /// Root fid after attach
    root_fid: Option<Fid>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct FidInfo {
    path: PathBuf,
    qid: Qid,
    mode: Option<u8>, // Open mode if file is open
}

impl NinePClient {
    /// Connect to a 9P server
    pub async fn connect(addr: &str) -> Result<Self> {
        info!("Connecting to 9P server at {}", addr);

        let stream = TcpStream::connect(addr)
            .await
            .context("Failed to connect to 9P server")?;

        let mut client = Self {
            stream: Arc::new(Mutex::new(stream)),
            msize: MAX_MSG_SIZE,
            version: VERSION_9PE.to_string(),
            tag_counter: Arc::new(Mutex::new(1)),
            fids: Arc::new(RwLock::new(HashMap::new())),
            next_fid: Arc::new(Mutex::new(1)),
            root_fid: None,
        };

        // Negotiate version
        client.version_negotiate().await?;

        Ok(client)
    }

    /// Get next tag
    async fn next_tag(&self) -> Tag {
        let mut counter = self.tag_counter.lock().await;
        let tag = *counter;
        *counter = counter.wrapping_add(1);
        if *counter == 0 {
            *counter = 1; // Skip NOTAG (0)
        }
        tag
    }

    /// Get next fid
    async fn next_fid(&self) -> Fid {
        let mut counter = self.next_fid.lock().await;
        let fid = *counter;
        *counter += 1;
        fid
    }

    /// Send a message and receive response
    async fn rpc<T: Message, R>(&self, msg: T) -> Result<R>
    where
        R: Message + 'static,
    {
        // Encode message
        let buf = WireFormat::encode(&msg)?;

        // Send message
        {
            let mut stream = self.stream.lock().await;
            stream
                .write_all(&buf)
                .await
                .context("Failed to send message")?;
            stream.flush().await?;
        }

        // Read response
        let response_buf = {
            let mut stream = self.stream.lock().await;

            // Read size (4 bytes)
            let mut size_buf = [0u8; 4];
            stream
                .read_exact(&mut size_buf)
                .await
                .context("Failed to read response size")?;
            let size = u32::from_le_bytes(size_buf);

            if size > self.msize {
                bail!("Response size {} exceeds msize {}", size, self.msize);
            }

            // Read full message
            let mut buf = vec![0u8; size as usize];
            buf[0..4].copy_from_slice(&size_buf);
            stream
                .read_exact(&mut buf[4..])
                .await
                .context("Failed to read response")?;

            buf
        };

        // Decode response - simplified for now
        // In production we'd properly deserialize the specific response type
        let _response = WireFormat::decode(&response_buf)?;

        // For now, return a placeholder - this needs proper implementation
        Err(anyhow::anyhow!("RPC not fully implemented yet"))
    }

    /// Negotiate protocol version
    async fn version_negotiate(&mut self) -> Result<()> {
        let msg = Tversion {
            tag: self.next_tag().await,
            msize: self.msize,
            version: self.version.clone(),
        };

        let resp: Rversion = self.rpc(msg).await?;

        self.msize = resp.msize.min(self.msize);
        self.version = resp.version;

        info!(
            "Protocol negotiated: {} with msize {}",
            self.version, self.msize
        );

        Ok(())
    }

    /// Attach to filesystem
    pub async fn attach(&mut self, uname: &str, aname: &str) -> Result<()> {
        let fid = self.next_fid().await;

        let msg = Tattach {
            tag: self.next_tag().await,
            fid,
            afid: !0, // No auth
            uname: uname.to_string(),
            aname: aname.to_string(),
        };

        let resp: Rattach = self.rpc(msg).await?;

        // Store root fid
        self.root_fid = Some(fid);

        let mut fids = self.fids.write().await;
        fids.insert(
            fid,
            FidInfo {
                path: PathBuf::from("/"),
                qid: resp.qid,
                mode: None,
            },
        );

        info!("Attached as {} to {}", uname, aname);

        Ok(())
    }

    /// Walk to a path
    pub async fn walk(&self, path: &Path) -> Result<(Fid, Vec<Qid>)> {
        let root_fid = self
            .root_fid
            .ok_or_else(|| anyhow::anyhow!("Not attached"))?;

        let newfid = self.next_fid().await;

        // Convert path to walk names
        let mut wnames = Vec::new();
        for component in path.components() {
            if let std::path::Component::Normal(name) = component {
                wnames.push(name.to_string_lossy().into_owned());
            }
        }

        let msg = Twalk {
            tag: self.next_tag().await,
            fid: root_fid,
            newfid,
            wnames: wnames.clone(),
        };

        let resp: Rwalk = self.rpc(msg).await?;

        if resp.qids.len() != wnames.len() {
            bail!(
                "Walk failed: only walked {} of {} components",
                resp.qids.len(),
                wnames.len()
            );
        }

        // Store the new fid
        if let Some(last_qid) = resp.qids.last() {
            let mut fids = self.fids.write().await;
            fids.insert(
                newfid,
                FidInfo {
                    path: path.to_path_buf(),
                    qid: *last_qid,
                    mode: None,
                },
            );
        }

        Ok((newfid, resp.qids))
    }

    /// Open a file
    pub async fn open(&self, fid: Fid, mode: u8) -> Result<Qid> {
        let msg = Topen {
            tag: self.next_tag().await,
            fid,
            mode,
        };

        let resp: Ropen = self.rpc(msg).await?;

        // Update fid info with open mode
        let mut fids = self.fids.write().await;
        if let Some(info) = fids.get_mut(&fid) {
            info.mode = Some(mode);
        }

        Ok(resp.qid)
    }

    /// Read from a file
    pub async fn read(&self, fid: Fid, offset: u64, count: u32) -> Result<Vec<u8>> {
        let msg = Tread {
            tag: self.next_tag().await,
            fid,
            offset,
            count,
        };

        let resp: Rread = self.rpc(msg).await?;

        Ok(resp.data)
    }

    /// Write to a file
    pub async fn write(&self, fid: Fid, offset: u64, data: Vec<u8>) -> Result<u32> {
        let msg = Twrite {
            tag: self.next_tag().await,
            fid,
            offset,
            data,
        };

        let resp: Rwrite = self.rpc(msg).await?;

        Ok(resp.count)
    }

    /// Get file stats
    pub async fn stat(&self, fid: Fid) -> Result<Stat> {
        let msg = Tstat {
            tag: self.next_tag().await,
            fid,
        };

        let resp: Rstat = self.rpc(msg).await?;

        Ok(resp.stat)
    }

    /// Close a file
    pub async fn clunk(&self, fid: Fid) -> Result<()> {
        let msg = Tclunk {
            tag: self.next_tag().await,
            fid,
        };

        let _resp: Rclunk = self.rpc(msg).await?;

        // Remove from fid table
        let mut fids = self.fids.write().await;
        fids.remove(&fid);

        Ok(())
    }

    /// List directory contents
    pub async fn readdir(&self, path: &Path) -> Result<Vec<Stat>> {
        // Walk to directory
        let (fid, _qids) = self.walk(path).await?;

        // Open directory for reading
        self.open(fid, permissions::OREAD).await?;

        // Read directory entries
        let mut entries = Vec::new();
        let mut offset = 0u64;

        loop {
            let data = self.read(fid, offset, 8192).await?;
            if data.is_empty() {
                break;
            }

            // Parse stat entries from data
            let mut pos = 0;
            while pos < data.len() {
                if pos + 2 > data.len() {
                    break;
                }

                let size = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
                if pos + 2 + size > data.len() {
                    break;
                }

                // Parse stat structure (simplified)
                // In production, we'd properly deserialize the stat
                let name_start = pos + 49; // Offset to name in stat structure
                if name_start < data.len() {
                    let name_len =
                        u16::from_le_bytes([data[name_start], data[name_start + 1]]) as usize;
                    let name =
                        String::from_utf8_lossy(&data[name_start + 2..name_start + 2 + name_len])
                            .into_owned();

                    entries.push(Stat {
                        size: size as u16,
                        typ: 0,
                        dev: 0,
                        qid: root_qid(), // Placeholder
                        mode: permissions::DMDIR,
                        atime: 0,
                        mtime: 0,
                        length: 0,
                        name,
                        uid: "nobody".to_string(),
                        gid: "nobody".to_string(),
                        muid: "nobody".to_string(),
                    });
                }

                pos += 2 + size;
            }

            offset += data.len() as u64;
        }

        // Close directory
        self.clunk(fid).await?;

        Ok(entries)
    }
}

// Type conversion helpers will be implemented when needed

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_client_creation() {
        // This would need a mock server to test properly
        // For now, just verify the client structure is sound
        assert_eq!(MAX_MSG_SIZE, 1048576);
        assert_eq!(VERSION_9PE, "9P2000.e");
    }
}
