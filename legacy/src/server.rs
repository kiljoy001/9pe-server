//! 9P.e Server Implementation
//!
//! Bridges the 9PE core protocol to actual filesystem operations

use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::{Result, Context};
use tracing::{info, warn, error, debug};
use std::time::Instant;

use plan9e::protocol::{NinePeeMessage, NINEPEE_VERSION, LEGACY_VERSION};
use plan9e::transport::Session;
use crate::metrics;
use crate::synthetic::{SyntheticGenerator, CpuInfoGenerator, MemInfoGenerator};
use crate::translator_base::{TranslatorRegistry, RegistryConfig};
use crate::namespace_translator::NamespaceTranslator;
use crate::namespaces::{NamespaceManager, GhostDAGConsensus};

/// File identifier to path mapping
type FidMap = Arc<RwLock<HashMap<u32, PathBuf>>>;

/// 9P.e filesystem server
pub struct FileSystemServer {
    /// Root directory being served
    root: PathBuf,

    /// Mapping of file IDs to paths
    fids: FidMap,

    /// Next available file ID
    next_fid: Arc<RwLock<u32>>,

    /// Maximum message size
    max_message_size: u32,

    /// Synthetic file generators
    cpu_info: CpuInfoGenerator,
    mem_info: MemInfoGenerator,

    /// Translator registry for settrans functionality
    translator_registry: Arc<TranslatorRegistry>,
}

impl FileSystemServer {
    /// Create a new filesystem server
    pub async fn new(root: PathBuf) -> Result<Self> {
        let canonical_root = root.canonicalize()
            .context("Failed to canonicalize root path")?;

        info!("Filesystem server root: {:?}", canonical_root);

        // Initialize translator registry
        let settrans_dir = canonical_root.join("srv").join("settrans");
        let registry = TranslatorRegistry::new(settrans_dir, RegistryConfig::default()).await?;
        let registry = Arc::new(registry);

        // Initialize consensus for namespace management
        let consensus = Arc::new(GhostDAGConsensus::new());

        // Initialize namespace manager and translator
        let namespace_manager = Arc::new(NamespaceManager::new(consensus));
        let namespace_translator = NamespaceTranslator::new(namespace_manager);

        // Register the namespace translator
        registry.register_translator(Box::new(namespace_translator)).await?;

        Ok(Self {
            root: canonical_root,
            fids: Arc::new(RwLock::new(HashMap::new())),
            next_fid: Arc::new(RwLock::new(100)), // Start at 100 to avoid conflicts
            max_message_size: 8192 * 1024, // 8MB default
            cpu_info: CpuInfoGenerator,
            mem_info: MemInfoGenerator,
            translator_registry: registry,
        })
    }

    /// Process a 9P.e message and return response
    pub async fn process_message(&self, msg: NinePeeMessage) -> Result<NinePeeMessage> {
        debug!("Processing message: {:?}", msg);
        let start = Instant::now();
        let msg_type = format!("{:?}", msg);

        let result = match msg {
            NinePeeMessage::Version { msize, version } => {
                self.handle_version(msize, version).await
            }

            NinePeeMessage::Attach { fid, afid: _, uname, aname } => {
                self.handle_attach(fid, uname, aname).await
            }

            NinePeeMessage::Walk { fid, newfid, wnames } => {
                self.handle_walk(fid, newfid, wnames).await
            }

            NinePeeMessage::Open { fid, mode } => {
                self.handle_open(fid, mode).await
            }

            NinePeeMessage::Read { fid, offset, count } => {
                self.handle_read(fid, offset, count).await
            }

            NinePeeMessage::Write { fid, offset, data } => {
                self.handle_write(fid, offset, data).await
            }

            NinePeeMessage::Clunk { fid } => {
                self.handle_clunk(fid).await
            }

            NinePeeMessage::Stat { fid } => {
                self.handle_stat(fid).await
            }

            NinePeeMessage::Remove { fid } => {
                self.handle_remove(fid).await
            }

            _ => {
                warn!("Unhandled message type");
                Ok(NinePeeMessage::Error {
                    ename: "Not implemented".to_string(),
                    errno: 1,
                })
            }
        };

        // Record metrics
        let duration = start.elapsed().as_secs_f64();
        let msg_type_short = msg_type.split('(').next().unwrap_or(&msg_type).trim();
        metrics::record_message(msg_type_short, result.is_ok(), duration);

        result
    }

    /// Handle version negotiation
    async fn handle_version(&self, msize: u32, version: String) -> Result<NinePeeMessage> {
        info!("Version negotiation: {} with msize {}", version, msize);

        // Support both 9P.e and legacy 9P2000
        let negotiated_version = if version.starts_with("9P.e") {
            NINEPEE_VERSION.to_string()
        } else if version == LEGACY_VERSION {
            LEGACY_VERSION.to_string()
        } else {
            return Ok(NinePeeMessage::Error {
                ename: format!("Unknown version: {}", version),
                errno: 1,
            });
        };

        let negotiated_msize = msize.min(self.max_message_size);

        Ok(NinePeeMessage::Version {
            msize: negotiated_msize,
            version: negotiated_version,
        })
    }

    /// Handle attach request
    async fn handle_attach(&self, fid: u32, uname: String, aname: String) -> Result<NinePeeMessage> {
        info!("Attach request: fid={}, user={}, aname={}", fid, uname, aname);

        // Store root directory for this fid
        let mut fids = self.fids.write().await;
        fids.insert(fid, self.root.clone());

        // Return success with attach response
        Ok(NinePeeMessage::Attach {
            fid,
            afid: 0, // No authentication required for now
            uname,
            aname,
        })
    }

    /// Handle walk request
    async fn handle_walk(&self, fid: u32, newfid: u32, wnames: Vec<String>) -> Result<NinePeeMessage> {
        debug!("Walk: fid={}, newfid={}, path={:?}", fid, newfid, wnames);

        let fids = self.fids.read().await;
        let base_path = fids.get(&fid)
            .ok_or_else(|| anyhow::anyhow!("Unknown fid: {}", fid))?;

        let mut current_path = base_path.clone();

        // Walk through each path component
        for name in &wnames {
            if name == ".." {
                // Go up one directory, but stay within root
                if let Some(parent) = current_path.parent() {
                    if parent.starts_with(&self.root) {
                        current_path = parent.to_path_buf();
                    }
                }
            } else if name != "." {
                current_path = current_path.join(name);
            }
        }

        // Ensure we're still within root
        let canonical = current_path.canonicalize()
            .unwrap_or_else(|_| current_path.clone());

        if !canonical.starts_with(&self.root) {
            return Ok(NinePeeMessage::Error {
                ename: "Path outside root".to_string(),
                errno: 2,
            });
        }

        // Store the new path
        drop(fids);
        let mut fids = self.fids.write().await;
        fids.insert(newfid, canonical);

        // Return success
        Ok(NinePeeMessage::Walk {
            fid: newfid,
            newfid,
            wnames: vec![],
        })
    }

    /// Handle open request
    async fn handle_open(&self, fid: u32, mode: u8) -> Result<NinePeeMessage> {
        debug!("Open: fid={}, mode={}", fid, mode);

        let fids = self.fids.read().await;
        let path = fids.get(&fid)
            .ok_or_else(|| anyhow::anyhow!("Unknown fid: {}", fid))?;

        // Check if path exists (real file or synthetic)
        if !path.exists() && !self.is_synthetic_path(&path) {
            return Ok(NinePeeMessage::Error {
                ename: "File not found".to_string(),
                errno: 2,
            });
        }

        // For now, always allow open
        Ok(NinePeeMessage::Open { fid, mode })
    }

    /// Handle read request
    async fn handle_read(&self, fid: u32, offset: u64, count: u32) -> Result<NinePeeMessage> {
        debug!("Read: fid={}, offset={}, count={}", fid, offset, count);

        let fids = self.fids.read().await;
        let path = fids.get(&fid)
            .ok_or_else(|| anyhow::anyhow!("Unknown fid: {}", fid))?
            .clone();
        drop(fids);

        if path.is_dir() {
            // Read directory entries
            let entries = self.read_directory(&path).await?;
            let data = entries.join("\n").into_bytes();

            // Apply offset and count
            let start = (offset as usize).min(data.len());
            let end = (start + count as usize).min(data.len());

            let bytes_read = (end - start) as u64;
            metrics::record_file_op("read", true, Some(bytes_read));

            // Return Write message with the data slice (9P convention: Read response is Write)
            Ok(NinePeeMessage::Write {
                fid,
                offset,
                data: data[start..end].to_vec(),
            })
        } else {
            // Check if this is a synthetic file
            if self.is_synthetic_path(&path) {
                // Generate synthetic content
                let data = self.read_synthetic_file(&path, offset, count).await?;
                let bytes_read = data.len() as u64;
                metrics::record_file_op("read_synthetic", true, Some(bytes_read));

                // Return Write message with the data (9P convention: Read response is Write)
                Ok(NinePeeMessage::Write {
                    fid,
                    offset,
                    data,
                })
            } else {
                // Read real file content
                let data = tokio::fs::read(&path).await?;

                // Apply offset and count
                let start = (offset as usize).min(data.len());
                let end = (start + count as usize).min(data.len());

                let bytes_read = (end - start) as u64;
                metrics::record_file_op("read", true, Some(bytes_read));

                // Return Write message with the data slice (9P convention: Read response is Write)
                Ok(NinePeeMessage::Write {
                    fid,
                    offset,
                    data: data[start..end].to_vec(),
                })
            }
        }
    }

    /// Handle write request
    async fn handle_write(&self, fid: u32, offset: u64, data: Vec<u8>) -> Result<NinePeeMessage> {
        debug!("Write: fid={}, offset={}, len={}", fid, offset, data.len());

        let fids = self.fids.read().await;
        let path = fids.get(&fid)
            .ok_or_else(|| anyhow::anyhow!("Unknown fid: {}", fid))?
            .clone();
        drop(fids);

        if path.is_dir() {
            return Ok(NinePeeMessage::Error {
                ename: "Cannot write to directory".to_string(),
                errno: 21,
            });
        }

        // Prevent writes to synthetic files (they are read-only)
        if self.is_synthetic_path(&path) {
            return Ok(NinePeeMessage::Error {
                ename: "Cannot write to synthetic file".to_string(),
                errno: 30, // EROFS - Read-only file system
            });
        }

        // For simplicity, we'll overwrite at offset
        // In production, would handle append/truncate modes properly
        use tokio::io::{AsyncWriteExt, AsyncSeekExt};
        use tokio::fs::OpenOptions;
        use std::io::SeekFrom;

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&path)
            .await?;

        file.seek(SeekFrom::Start(offset)).await?;
        file.write_all(&data).await?;

        metrics::record_file_op("write", true, Some(data.len() as u64));

        Ok(NinePeeMessage::Write {
            fid,
            offset,
            data: vec![], // Return empty to indicate success
        })
    }

    /// Handle clunk (close) request
    async fn handle_clunk(&self, fid: u32) -> Result<NinePeeMessage> {
        debug!("Clunk: fid={}", fid);

        let mut fids = self.fids.write().await;
        fids.remove(&fid);

        Ok(NinePeeMessage::Clunk { fid })
    }

    /// Handle stat request
    async fn handle_stat(&self, fid: u32) -> Result<NinePeeMessage> {
        debug!("Stat: fid={}", fid);

        let fids = self.fids.read().await;
        let path = fids.get(&fid)
            .ok_or_else(|| anyhow::anyhow!("Unknown fid: {}", fid))?;

        let _metadata = tokio::fs::metadata(path).await?;

        // For now, return a simple stat
        // In production, would build proper Dir structure
        Ok(NinePeeMessage::Stat { fid })
    }

    /// Handle remove request
    async fn handle_remove(&self, fid: u32) -> Result<NinePeeMessage> {
        debug!("Remove: fid={}", fid);

        let fids = self.fids.read().await;
        let path = fids.get(&fid)
            .ok_or_else(|| anyhow::anyhow!("Unknown fid: {}", fid))?
            .clone();
        drop(fids);

        // Remove file or directory
        if path.is_dir() {
            tokio::fs::remove_dir_all(&path).await?;
        } else {
            tokio::fs::remove_file(&path).await?;
        }

        metrics::record_file_op("remove", true, None);

        // Remove from fid map
        let mut fids = self.fids.write().await;
        fids.remove(&fid);

        Ok(NinePeeMessage::Remove { fid })
    }

    /// Check if a path is a synthetic file
    fn is_synthetic_path(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        path_str.starts_with("/sys/") ||
        path_str.ends_with("/sys/cpuinfo") ||
        path_str.ends_with("/sys/meminfo")
    }

    /// Generate synthetic file content
    async fn read_synthetic_file(&self, path: &Path, offset: u64, count: u32) -> Result<Vec<u8>> {
        let path_str = path.to_string_lossy();

        if path_str.ends_with("/sys/cpuinfo") || path_str.ends_with("cpuinfo") {
            self.cpu_info.generate(offset, count).await
        } else if path_str.ends_with("/sys/meminfo") || path_str.ends_with("meminfo") {
            self.mem_info.generate(offset, count).await
        } else {
            Err(anyhow::anyhow!("Unknown synthetic file: {}", path_str))
        }
    }

    /// Read directory entries (includes synthetic /sys/ directory)
    async fn read_directory(&self, path: &Path) -> Result<Vec<String>> {
        let mut entries = Vec::new();

        // Add synthetic /sys directory at root
        if path == self.root {
            entries.push("sys".to_string());
        }

        // If this is /sys, add synthetic files
        let path_str = path.to_string_lossy();
        if path_str.ends_with("/sys") {
            entries.push("cpuinfo".to_string());
            entries.push("meminfo".to_string());
            return Ok(entries);
        }

        // Read real directory entries
        if path.exists() {
            let mut dir = tokio::fs::read_dir(path).await?;
            while let Some(entry) = dir.next_entry().await? {
                if let Some(name) = entry.file_name().to_str() {
                    entries.push(name.to_string());
                }
            }
        }

        entries.sort();
        Ok(entries)
    }
}

/// Handle a client session
pub async fn handle_session(mut session: Session, server: Arc<FileSystemServer>) -> Result<()> {
    loop {
        // Read message from client
        let request = session.read_message().await?;

        // Process with our filesystem server
        let response = server.process_message(request).await?;

        // Send response
        session.write_message(&response).await?;
    }
}