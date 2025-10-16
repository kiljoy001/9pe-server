//! Basic 9P operations handler

use crate::consensus::{BoundedGhostdag, NamespaceOp};
use crate::protocol::NinePeeMessage;
use crate::protocol::{Qid, Stat};
use anyhow::Result;
use std::fs::{self, File, Permissions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, warn};

use super::connection_state::{ConnectionState, FileHandle};

/// Handler for basic 9P operations
pub struct BasicOpsHandler {
    /// Root filesystem path
    root: PathBuf,

    /// Connection state
    connection_state: ConnectionState,

    /// Consensus DAG for namespace operations
    consensus_dag: Option<Arc<BoundedGhostdag>>,
}

impl BasicOpsHandler {
    /// Create a new basic operations handler
    pub fn new(root: PathBuf, connection_state: ConnectionState) -> Self {
        Self {
            root,
            connection_state,
            consensus_dag: None,
        }
    }

    /// Set the consensus DAG
    pub fn set_consensus_dag(&mut self, dag: Arc<BoundedGhostdag>) {
        self.consensus_dag = Some(dag);
    }

    /// Handle attach request
    pub async fn handle_attach(
        &self,
        fid: u32,
        _afid: u32,
        uname: String,
        aname: String,
    ) -> Result<NinePeeMessage> {
        debug!("Attach: fid={}, uname={}, aname={}", fid, uname, aname);

        // Create root fid
        let handle = FileHandle {
            fid,
            path: "/".to_string(),
            mode: 0,
            offset: 0,
            synthetic: false,
            translator_id: None,
        };

        self.connection_state.add_fid(fid, handle).await;

        // Get root qid
        let metadata = fs::metadata(&self.root)?;
        let _qid = Qid {
            qtype: if metadata.is_dir() { 0x80 } else { 0 },
            version: 0,
            path: 0,
        };

        Ok(NinePeeMessage::Attach {
            fid,
            afid: 0,
            uname,
            aname,
        })
    }

    /// Handle walk request
    pub async fn handle_walk(
        &self,
        fid: u32,
        newfid: u32,
        wnames: Vec<String>,
    ) -> Result<NinePeeMessage> {
        debug!("Walk: fid={}, newfid={}, wnames={:?}", fid, newfid, wnames);

        let handle = match self.connection_state.get_fid(fid).await {
            Some(h) => h,
            None => {
                return Ok(NinePeeMessage::Error {
                    ename: "Invalid fid".to_string(),
                    errno: 9, // EBADF
                });
            }
        };

        let mut current_path = PathBuf::from(&handle.path);
        let mut qids = Vec::new();

        for name in &wnames {
            current_path.push(name);
            let full_path = self
                .root
                .join(current_path.strip_prefix("/").unwrap_or(&current_path));

            match fs::metadata(&full_path) {
                Ok(metadata) => {
                    qids.push(Qid {
                        qtype: if metadata.is_dir() { 0x80 } else { 0 },
                        version: 0,
                        path: 0,
                    });
                }
                Err(_) => break,
            }
        }

        // Create new fid if walk was successful
        if qids.len() == wnames.len() {
            let new_handle = FileHandle {
                fid: newfid,
                path: current_path.to_string_lossy().to_string(),
                mode: 0,
                offset: 0,
                synthetic: false,
                translator_id: None,
            };

            self.connection_state.add_fid(newfid, new_handle).await;
        }

        Ok(NinePeeMessage::Walk {
            fid,
            newfid,
            wnames,
        })
    }

    /// Handle open request
    pub async fn handle_open(&self, fid: u32, mode: u8) -> Result<NinePeeMessage> {
        debug!("Open: fid={}, mode={}", fid, mode);

        let mut handle = match self.connection_state.get_fid(fid).await {
            Some(h) => h,
            None => {
                return Ok(NinePeeMessage::Error {
                    ename: "Invalid fid".to_string(),
                    errno: 9, // EBADF
                });
            }
        };

        handle.mode = mode;
        self.connection_state.add_fid(fid, handle).await;

        Ok(NinePeeMessage::Open { fid, mode })
    }

    /// Handle create request
    pub async fn handle_create(
        &self,
        fid: u32,
        name: String,
        perm: u32,
        mode: u8,
    ) -> Result<NinePeeMessage> {
        debug!(
            "Create: fid={}, name={}, perm={:o}, mode={}",
            fid, name, perm, mode
        );

        let handle = match self.connection_state.get_fid(fid).await {
            Some(h) => h,
            None => {
                return Ok(NinePeeMessage::Error {
                    ename: "Invalid fid".to_string(),
                    errno: 9, // EBADF
                });
            }
        };

        let parent_path = self
            .root
            .join(handle.path.strip_prefix("/").unwrap_or(&handle.path));
        let new_file_path = parent_path.join(&name);

        // Log operation to consensus DAG if available
        if let Some(ref dag) = self.consensus_dag {
            let op = NamespaceOp::Create {
                path: format!("{}/{}", handle.path.trim_end_matches('/'), name),
                mode: perm,
                is_dir: perm & 0o040000 != 0,
            };
            // Create a simple block for the operation
            let block = crate::consensus::bounded_ghostdag::Block {
                id: format!("create_{}", uuid::Uuid::new_v4()),
                parents: vec![],
                operations: vec![op],
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                creator: "server".to_string(),
                signature: vec![],
                state: crate::consensus::bounded_ghostdag::BlockState::Pending,
                ghost_weight: 1,
                height: 0,
            };
            let _ = dag.add_block(block).await;
        }

        // Create the file or directory
        let result = if perm & 0o040000 != 0 {
            fs::create_dir(&new_file_path)
        } else {
            File::create(&new_file_path).map(|_| ())
        };

        match result {
            Ok(()) => {
                // Update fid to point to new file
                let new_handle = FileHandle {
                    fid,
                    path: format!("{}/{}", handle.path.trim_end_matches('/'), name),
                    mode,
                    offset: 0,
                    synthetic: false,
                    translator_id: None,
                };

                self.connection_state.add_fid(fid, new_handle).await;

                Ok(NinePeeMessage::Create {
                    fid,
                    name,
                    perm,
                    mode,
                })
            }
            Err(e) => {
                warn!("Failed to create file {}: {}", name, e);
                Ok(NinePeeMessage::Error {
                    ename: format!("Create failed: {}", e),
                    errno: 1, // EPERM
                })
            }
        }
    }

    /// Handle read request
    pub async fn handle_read(&self, fid: u32, offset: u64, count: u32) -> Result<NinePeeMessage> {
        debug!("Read: fid={}, offset={}, count={}", fid, offset, count);

        let handle = match self.connection_state.get_fid(fid).await {
            Some(h) => h,
            None => {
                return Ok(NinePeeMessage::Error {
                    ename: "Invalid fid".to_string(),
                    errno: 9, // EBADF
                });
            }
        };

        let file_path = self
            .root
            .join(handle.path.strip_prefix("/").unwrap_or(&handle.path));

        // Handle directory reads
        if file_path.is_dir() {
            let entries = fs::read_dir(&file_path)?;
            let mut data = Vec::new();

            for entry in entries {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().to_string();
                let metadata = entry.metadata()?;

                let stat = Stat {
                    size: 0, // Size of stat structure
                    typ: if metadata.is_dir() { 0x80 } else { 0 },
                    dev: 0,
                    qid: Qid {
                        qtype: if metadata.is_dir() { 0x80 } else { 0 },
                        version: 0,
                        path: 0,
                    },
                    mode: if metadata.is_dir() {
                        0o040755
                    } else {
                        0o100644
                    },
                    atime: 0,
                    mtime: 0,
                    length: metadata.len(),
                    name: name.clone(),
                    uid: "".to_string(),
                    gid: "".to_string(),
                    muid: "".to_string(),
                };

                // Serialize stat (simplified)
                let stat_data = bincode::serialize(&stat)?;
                data.extend_from_slice(&stat_data);
            }

            let start = offset as usize;
            let end = (start + count as usize).min(data.len());
            let slice = if start < data.len() {
                &data[start..end]
            } else {
                &[]
            };

            return Ok(NinePeeMessage::Read {
                fid,
                offset,
                count: slice.len() as u32,
                data: slice.to_vec(),
            });
        }

        // Handle file reads
        let mut file = File::open(&file_path)?;
        file.seek(SeekFrom::Start(offset))?;

        let mut buffer = vec![0u8; count as usize];
        let bytes_read = file.read(&mut buffer)?;
        buffer.truncate(bytes_read);

        Ok(NinePeeMessage::Read {
            fid,
            offset,
            count: bytes_read as u32,
            data: buffer,
        })
    }

    /// Handle write request
    pub async fn handle_write(
        &self,
        fid: u32,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<NinePeeMessage> {
        debug!("Write: fid={}, offset={}, len={}", fid, offset, data.len());

        let handle = match self.connection_state.get_fid(fid).await {
            Some(h) => h,
            None => {
                return Ok(NinePeeMessage::Error {
                    ename: "Invalid fid".to_string(),
                    errno: 9, // EBADF
                });
            }
        };

        let file_path = self
            .root
            .join(handle.path.strip_prefix("/").unwrap_or(&handle.path));

        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(&file_path)?;

        file.seek(SeekFrom::Start(offset))?;
        let bytes_written = file.write(&data)?;

        // Log operation to consensus DAG if available
        if let Some(ref dag) = self.consensus_dag {
            // Create a hash of the written data for the consensus record
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};

            let mut hasher = DefaultHasher::new();
            data[..bytes_written].hash(&mut hasher);
            let data_hash = hasher.finish();

            let mut hash_bytes = [0u8; 32];
            hash_bytes[..8].copy_from_slice(&data_hash.to_le_bytes());

            let op = NamespaceOp::Write {
                path: handle.path.clone(),
                offset,
                hash: hash_bytes,
            };

            let block = crate::consensus::bounded_ghostdag::Block {
                id: format!("write_{}", uuid::Uuid::new_v4()),
                parents: vec![],
                operations: vec![op],
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                creator: "server".to_string(),
                signature: vec![],
                state: crate::consensus::bounded_ghostdag::BlockState::Pending,
                ghost_weight: 1,
                height: 0,
            };
            let _ = dag.add_block(block).await;
        }

        Ok(NinePeeMessage::Write {
            fid,
            offset,
            data: data[..bytes_written].to_vec(),
        })
    }

    /// Handle clunk request
    pub async fn handle_clunk(&self, fid: u32) -> Result<NinePeeMessage> {
        debug!("Clunk: fid={}", fid);

        self.connection_state.remove_fid(fid).await;

        Ok(NinePeeMessage::Clunk { fid })
    }

    /// Handle remove request
    pub async fn handle_remove(&self, fid: u32) -> Result<NinePeeMessage> {
        debug!("Remove: fid={}", fid);

        let handle = match self.connection_state.get_fid(fid).await {
            Some(h) => h,
            None => {
                return Ok(NinePeeMessage::Error {
                    ename: "Invalid fid".to_string(),
                    errno: 9, // EBADF
                });
            }
        };

        let file_path = self
            .root
            .join(handle.path.strip_prefix("/").unwrap_or(&handle.path));

        let result = if file_path.is_dir() {
            fs::remove_dir(&file_path)
        } else {
            fs::remove_file(&file_path)
        };

        match result {
            Ok(()) => {
                // Log operation to consensus DAG if available
                if let Some(ref dag) = self.consensus_dag {
                    let op = NamespaceOp::Delete {
                        path: handle.path.clone(),
                    };

                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs();

                    let block = crate::consensus::Block {
                        id: format!("delete_{}_{}_{}", handle.path.replace("/", "_"), now, fid),
                        parents: vec![],
                        operations: vec![op],
                        timestamp: now,
                        creator: "basic_ops".to_string(),
                        signature: vec![],
                        state: crate::consensus::BlockState::Pending,
                        ghost_weight: 1,
                        height: 0,
                    };

                    if let Err(e) = dag.add_block(block).await {
                        warn!("Failed to log delete operation to consensus DAG: {}", e);
                    } else {
                        debug!(
                            "Logged delete operation to consensus DAG for path: {}",
                            handle.path
                        );
                    }
                }

                self.connection_state.remove_fid(fid).await;
                Ok(NinePeeMessage::Remove { fid })
            }
            Err(e) => Ok(NinePeeMessage::Error {
                ename: format!("Remove failed: {}", e),
                errno: 1, // EPERM
            }),
        }
    }

    /// Handle stat request
    pub async fn handle_stat(&self, fid: u32) -> Result<NinePeeMessage> {
        debug!("Stat: fid={}", fid);

        let handle = match self.connection_state.get_fid(fid).await {
            Some(h) => h,
            None => {
                return Ok(NinePeeMessage::Error {
                    ename: "Invalid fid".to_string(),
                    errno: 9, // EBADF
                });
            }
        };

        let file_path = self
            .root
            .join(handle.path.strip_prefix("/").unwrap_or(&handle.path));
        let metadata = fs::metadata(&file_path)?;

        let stat = Stat {
            size: 0,
            typ: if metadata.is_dir() { 0x80 } else { 0 },
            dev: 0,
            qid: Qid {
                qtype: if metadata.is_dir() { 0x80 } else { 0 },
                version: 0,
                path: 0,
            },
            mode: if metadata.is_dir() {
                0o040755
            } else {
                0o100644
            },
            atime: 0,
            mtime: 0,
            length: metadata.len(),
            name: file_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            uid: "".to_string(),
            gid: "".to_string(),
            muid: "".to_string(),
        };

        let stat_bytes = bincode::serialize(&stat)?;

        Ok(NinePeeMessage::Stat {
            fid,
            data: stat_bytes,
        })
    }

    /// Handle wstat request
    pub async fn handle_wstat(&self, fid: u32, stat_data: Vec<u8>) -> Result<NinePeeMessage> {
        debug!("Wstat: fid={}, data_len={}", fid, stat_data.len());

        let handle = match self.connection_state.get_fid(fid).await {
            Some(h) => h,
            None => {
                return Ok(NinePeeMessage::Error {
                    ename: "Invalid fid".to_string(),
                    errno: 9, // EBADF
                });
            }
        };

        let file_path = self
            .root
            .join(handle.path.strip_prefix("/").unwrap_or(&handle.path));

        // Parse the stat structure from the data
        match self.parse_stat_changes(&stat_data).await {
            Ok(changes) => {
                // Apply the changes to the file
                if let Err(e) = self
                    .apply_stat_changes(&file_path, &handle.path, &changes)
                    .await
                {
                    warn!("Failed to apply stat changes: {}", e);
                    return Ok(NinePeeMessage::Error {
                        ename: format!("Wstat failed: {}", e),
                        errno: 1, // EPERM
                    });
                }

                // Log operation to consensus DAG if available
                if let Some(ref dag) = self.consensus_dag {
                    // Create a rename operation if the name changed
                    if let Some(ref new_name) = changes.name {
                        let parent_path = std::path::Path::new(&handle.path)
                            .parent()
                            .unwrap_or(std::path::Path::new("/"))
                            .to_string_lossy()
                            .to_string();

                        let new_path = if parent_path == "/" {
                            format!("/{}", new_name)
                        } else {
                            format!("{}/{}", parent_path.trim_end_matches('/'), new_name)
                        };

                        let op = NamespaceOp::Rename {
                            from: handle.path.clone(),
                            to: new_path,
                        };

                        let block = crate::consensus::bounded_ghostdag::Block {
                            id: format!("wstat_{}", uuid::Uuid::new_v4()),
                            parents: vec![],
                            operations: vec![op],
                            timestamp: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_secs(),
                            creator: "server".to_string(),
                            signature: vec![],
                            state: crate::consensus::bounded_ghostdag::BlockState::Pending,
                            ghost_weight: 1,
                            height: 0,
                        };
                        let _ = dag.add_block(block).await;
                    }
                }

                Ok(NinePeeMessage::Wstat {
                    fid,
                    stat: stat_data,
                })
            }
            Err(e) => {
                warn!("Failed to parse stat data: {}", e);
                Ok(NinePeeMessage::Error {
                    ename: format!("Invalid stat data: {}", e),
                    errno: 22, // EINVAL
                })
            }
        }
    }

    /// Parse stat changes from the wstat data
    async fn parse_stat_changes(&self, data: &[u8]) -> Result<StatChanges> {
        if data.len() < 2 {
            return Err(anyhow::anyhow!("Stat data too short"));
        }

        // Parse the stat structure according to 9P protocol
        // For now, implement basic support for name changes and mode changes
        let mut changes = StatChanges::default();

        // In a real implementation, you would parse the full stat structure
        // Here we'll implement a simplified version that handles the most common changes

        // Try to deserialize as a Stat structure
        match bincode::deserialize::<Stat>(data) {
            Ok(stat) => {
                // Check what fields have meaningful values (non-default)
                if !stat.name.is_empty() && stat.name != "~" {
                    changes.name = Some(stat.name);
                }

                // Mode changes (permissions)
                if stat.mode != u32::MAX {
                    changes.mode = Some(stat.mode);
                }

                // Length changes (truncation)
                if stat.length != u64::MAX {
                    changes.length = Some(stat.length);
                }
            }
            Err(_) => {
                // If deserialization fails, try to extract minimal info
                // This is a fallback for clients that send partial stat data
                debug!("Could not deserialize full stat, using minimal parsing");
            }
        }

        Ok(changes)
    }

    /// Apply stat changes to the file
    async fn apply_stat_changes(
        &self,
        file_path: &std::path::Path,
        _current_path: &str,
        changes: &StatChanges,
    ) -> Result<()> {
        // Apply mode changes (permissions)
        if let Some(mode) = changes.mode {
            let permissions = Permissions::from_mode(mode & 0o777);
            fs::set_permissions(file_path, permissions)?;
            debug!(
                "Changed permissions for {:?} to {:o}",
                file_path,
                mode & 0o777
            );
        }

        // Apply length changes (truncation)
        if let Some(length) = changes.length {
            if file_path.is_file() {
                let file = fs::OpenOptions::new().write(true).open(file_path)?;
                file.set_len(length)?;
                debug!("Truncated file {:?} to {} bytes", file_path, length);
            }
        }

        // Apply name changes (rename)
        if let Some(ref new_name) = changes.name {
            let parent_dir = file_path.parent().unwrap_or(std::path::Path::new("/"));
            let new_path = parent_dir.join(new_name);

            fs::rename(file_path, &new_path)?;
            debug!("Renamed {:?} to {:?}", file_path, new_path);
        }

        Ok(())
    }
}

/// Represents changes to be applied via wstat
#[derive(Debug, Default)]
struct StatChanges {
    /// New file name (for rename operations)
    name: Option<String>,
    /// New file mode/permissions
    mode: Option<u32>,
    /// New file length (for truncation)
    length: Option<u64>,
}
