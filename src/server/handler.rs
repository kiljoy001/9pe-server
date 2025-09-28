//! Message handler - implements actual 9P.e protocol processing

use anyhow::{Result, Context};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write, Seek, SeekFrom};
use tokio::io::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt};
use tracing::{debug, error, warn};
use ninepee::protocol::{
    NinePeeMessage, ConnectionState, FileHandle, ProtocolError,
    NINEPEE_VERSION, LEGACY_VERSION, MAX_MESSAGE_SIZE
};

/// Handles 9P.e protocol messages with actual filesystem operations
pub struct MessageHandler {
    root: PathBuf,
    max_message_size: u32,
    connection_state: ConnectionState,
    next_fid: u32,
}

impl MessageHandler {
    pub fn new(root: PathBuf, max_message_size: u32) -> Result<Self> {
        let connection_state = ConnectionState::new(
            0, // connection_id will be set by server
            NINEPEE_VERSION,
            max_message_size.min(MAX_MESSAGE_SIZE)
        );

        Ok(Self {
            root,
            max_message_size: max_message_size.min(MAX_MESSAGE_SIZE),
            connection_state,
            next_fid: 1,
        })
    }

    /// Process a 9P.e message and return response
    pub async fn handle_message(&mut self, message: NinePeeMessage) -> Result<NinePeeMessage> {
        debug!("Processing message: {:?}", message);

        match message {
            NinePeeMessage::Version { msize, version } => {
                self.handle_version(msize, version).await
            }
            NinePeeMessage::Auth { afid, uname, aname, password } => {
                self.handle_auth(afid, uname, aname, password).await
            }
            NinePeeMessage::Attach { fid, afid, uname, aname } => {
                self.handle_attach(fid, afid, uname, aname).await
            }
            NinePeeMessage::Walk { fid, newfid, wnames } => {
                self.handle_walk(fid, newfid, wnames).await
            }
            NinePeeMessage::Open { fid, mode } => {
                self.handle_open(fid, mode).await
            }
            NinePeeMessage::Create { fid, name, perm, mode } => {
                self.handle_create(fid, name, perm, mode).await
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
            NinePeeMessage::Remove { fid } => {
                self.handle_remove(fid).await
            }
            NinePeeMessage::Stat { fid } => {
                self.handle_stat(fid).await
            }
            NinePeeMessage::Wstat { fid, stat } => {
                self.handle_wstat(fid, stat).await
            }
            // 9P.e extensions
            NinePeeMessage::StreamInit { stream_id, fid, mode } => {
                self.handle_stream_init(stream_id, fid, mode).await
            }
            _ => {
                error!("Unimplemented message type");
                Ok(NinePeeMessage::Error {
                    ename: "Operation not implemented".to_string(),
                    errno: 95, // EOPNOTSUPP
                })
            }
        }
    }

    /// Handle version negotiation
    async fn handle_version(&mut self, msize: u32, version: String) -> Result<NinePeeMessage> {
        debug!("Version negotiation: msize={}, version={}", msize, version);

        // Negotiate protocol version
        let negotiated_version = if version == NINEPEE_VERSION || version == LEGACY_VERSION {
            version
        } else {
            LEGACY_VERSION.to_string() // Fall back to 9P2000
        };

        // Negotiate message size
        let negotiated_msize = msize.min(self.max_message_size);

        // Update connection state
        self.connection_state.protocol_version = negotiated_version.clone();
        self.connection_state.max_message_size = negotiated_msize;

        Ok(NinePeeMessage::Version {
            msize: negotiated_msize,
            version: negotiated_version,
        })
    }

    /// Handle authentication (simplified for now)
    async fn handle_auth(&mut self, afid: u32, uname: String, aname: String, password: Option<String>) -> Result<NinePeeMessage> {
        debug!("Auth request: afid={}, uname={}, aname={}", afid, uname, aname);

        // For now, just mark as authenticated
        // In real implementation: verify credentials, create auth fid
        self.connection_state.authenticated = true;

        Ok(NinePeeMessage::Version {
            msize: self.connection_state.max_message_size,
            version: self.connection_state.protocol_version.clone(),
        })
    }

    /// Handle attach to filesystem root
    async fn handle_attach(&mut self, fid: u32, afid: u32, uname: String, aname: String) -> Result<NinePeeMessage> {
        debug!("Attach: fid={}, afid={}, uname={}, aname={}", fid, afid, uname, aname);

        // Create file handle for root directory
        let handle = FileHandle {
            fid,
            path: "/".to_string(),
            mode: 0, // Directory
            offset: 0,
            synthetic: false,
            translator_id: None,
        };

        self.connection_state.add_fid(fid, handle);

        // Return stat info for root directory (simplified)
        Ok(NinePeeMessage::Stat { fid })
    }

    /// Handle walk through directory tree
    async fn handle_walk(&mut self, fid: u32, newfid: u32, wnames: Vec<String>) -> Result<NinePeeMessage> {
        debug!("Walk: fid={}, newfid={}, wnames={:?}", fid, newfid, wnames);

        // Get the starting file handle
        let start_handle = match self.connection_state.get_fid(fid) {
            Some(handle) => handle.clone(),
            None => return Ok(NinePeeMessage::Error {
                ename: "Invalid fid".to_string(),
                errno: 9, // EBADF
            }),
        };

        let mut current_path = PathBuf::from(&start_handle.path);

        // Walk through each name component
        for wname in &wnames {
            if wname == ".." {
                current_path.pop();
            } else if wname != "." {
                current_path.push(wname);
            }

            // Check if path exists
            let full_path = self.root.join(current_path.strip_prefix("/").unwrap_or(&current_path));
            if !full_path.exists() {
                return Ok(NinePeeMessage::Error {
                    ename: format!("No such file: {}", wname),
                    errno: 2, // ENOENT
                });
            }
        }

        // Create new file handle
        let new_handle = FileHandle {
            fid: newfid,
            path: current_path.to_string_lossy().to_string(),
            mode: start_handle.mode,
            offset: 0,
            synthetic: false,
            translator_id: None,
        };

        self.connection_state.add_fid(newfid, new_handle);

        Ok(NinePeeMessage::Walk {
            fid,
            newfid,
            wnames,
        })
    }

    /// Handle open file
    async fn handle_open(&mut self, fid: u32, mode: u8) -> Result<NinePeeMessage> {
        debug!("Open: fid={}, mode={}", fid, mode);

        // Update file handle with open mode
        if let Some(mut handle) = self.connection_state.remove_fid(fid) {
            handle.mode = mode;
            self.connection_state.add_fid(fid, handle);

            Ok(NinePeeMessage::Open { fid, mode })
        } else {
            Ok(NinePeeMessage::Error {
                ename: "Invalid fid".to_string(),
                errno: 9, // EBADF
            })
        }
    }

    /// Handle create file
    async fn handle_create(&mut self, fid: u32, name: String, perm: u32, mode: u8) -> Result<NinePeeMessage> {
        debug!("Create: fid={}, name={}, perm={:#o}, mode={}", fid, name, perm, mode);

        let handle = match self.connection_state.get_fid(fid) {
            Some(h) => h.clone(),
            None => return Ok(NinePeeMessage::Error {
                ename: "Invalid fid".to_string(),
                errno: 9, // EBADF
            }),
        };

        let parent_path = self.root.join(handle.path.strip_prefix("/").unwrap_or(&handle.path));
        let new_file_path = parent_path.join(&name);

        // Create the file or directory
        let result = if perm & 0o040000 != 0 { // Directory
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

                self.connection_state.add_fid(fid, new_handle);

                Ok(NinePeeMessage::Create { fid, name, perm, mode })
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

    /// Handle read from file
    async fn handle_read(&mut self, fid: u32, offset: u64, count: u32) -> Result<NinePeeMessage> {
        debug!("Read: fid={}, offset={}, count={}", fid, offset, count);

        let handle = match self.connection_state.get_fid(fid) {
            Some(h) => h.clone(),
            None => return Ok(NinePeeMessage::Error {
                ename: "Invalid fid".to_string(),
                errno: 9, // EBADF
            }),
        };

        let file_path = self.root.join(handle.path.strip_prefix("/").unwrap_or(&handle.path));

        match fs::File::open(&file_path) {
            Ok(mut file) => {
                if let Err(e) = file.seek(SeekFrom::Start(offset)) {
                    return Ok(NinePeeMessage::Error {
                        ename: format!("Seek failed: {}", e),
                        errno: 22, // EINVAL
                    });
                }

                let mut buffer = vec![0u8; count.min(self.max_message_size - 64) as usize];
                match file.read(&mut buffer) {
                    Ok(bytes_read) => {
                        buffer.truncate(bytes_read);
                        Ok(NinePeeMessage::Write {
                            fid,
                            offset,
                            data: buffer,
                        })
                    }
                    Err(e) => Ok(NinePeeMessage::Error {
                        ename: format!("Read failed: {}", e),
                        errno: 5, // EIO
                    })
                }
            }
            Err(e) => Ok(NinePeeMessage::Error {
                ename: format!("Open failed: {}", e),
                errno: 2, // ENOENT
            })
        }
    }

    /// Handle write to file
    async fn handle_write(&mut self, fid: u32, offset: u64, data: Vec<u8>) -> Result<NinePeeMessage> {
        debug!("Write: fid={}, offset={}, len={}", fid, offset, data.len());

        let handle = match self.connection_state.get_fid(fid) {
            Some(h) => h.clone(),
            None => return Ok(NinePeeMessage::Error {
                ename: "Invalid fid".to_string(),
                errno: 9, // EBADF
            }),
        };

        let file_path = self.root.join(handle.path.strip_prefix("/").unwrap_or(&handle.path));

        match OpenOptions::new().write(true).open(&file_path) {
            Ok(mut file) => {
                if let Err(e) = file.seek(SeekFrom::Start(offset)) {
                    return Ok(NinePeeMessage::Error {
                        ename: format!("Seek failed: {}", e),
                        errno: 22, // EINVAL
                    });
                }

                match file.write(&data) {
                    Ok(bytes_written) => {
                        Ok(NinePeeMessage::Write {
                            fid,
                            offset,
                            data: vec![0u8; bytes_written], // Response data not typically needed
                        })
                    }
                    Err(e) => Ok(NinePeeMessage::Error {
                        ename: format!("Write failed: {}", e),
                        errno: 5, // EIO
                    })
                }
            }
            Err(e) => Ok(NinePeeMessage::Error {
                ename: format!("Open for write failed: {}", e),
                errno: 13, // EACCES
            })
        }
    }

    /// Handle close file
    async fn handle_clunk(&mut self, fid: u32) -> Result<NinePeeMessage> {
        debug!("Clunk: fid={}", fid);

        if self.connection_state.remove_fid(fid).is_some() {
            Ok(NinePeeMessage::Clunk { fid })
        } else {
            Ok(NinePeeMessage::Error {
                ename: "Invalid fid".to_string(),
                errno: 9, // EBADF
            })
        }
    }

    /// Handle remove file
    async fn handle_remove(&mut self, fid: u32) -> Result<NinePeeMessage> {
        debug!("Remove: fid={}", fid);

        let handle = match self.connection_state.remove_fid(fid) {
            Some(h) => h,
            None => return Ok(NinePeeMessage::Error {
                ename: "Invalid fid".to_string(),
                errno: 9, // EBADF
            }),
        };

        let file_path = self.root.join(handle.path.strip_prefix("/").unwrap_or(&handle.path));

        let result = if file_path.is_dir() {
            fs::remove_dir(&file_path)
        } else {
            fs::remove_file(&file_path)
        };

        match result {
            Ok(()) => Ok(NinePeeMessage::Remove { fid }),
            Err(e) => Ok(NinePeeMessage::Error {
                ename: format!("Remove failed: {}", e),
                errno: 1, // EPERM
            })
        }
    }

    /// Handle stat file
    async fn handle_stat(&mut self, fid: u32) -> Result<NinePeeMessage> {
        debug!("Stat: fid={}", fid);

        let handle = match self.connection_state.get_fid(fid) {
            Some(h) => h.clone(),
            None => return Ok(NinePeeMessage::Error {
                ename: "Invalid fid".to_string(),
                errno: 9, // EBADF
            }),
        };

        let file_path = self.root.join(handle.path.strip_prefix("/").unwrap_or(&handle.path));

        match fs::metadata(&file_path) {
            Ok(_metadata) => {
                // Return simplified stat data (in real implementation would format properly)
                Ok(NinePeeMessage::Wstat {
                    fid,
                    stat: vec![0u8; 64], // Placeholder stat data
                })
            }
            Err(e) => Ok(NinePeeMessage::Error {
                ename: format!("Stat failed: {}", e),
                errno: 2, // ENOENT
            })
        }
    }

    /// Handle set file attributes
    async fn handle_wstat(&mut self, fid: u32, stat: Vec<u8>) -> Result<NinePeeMessage> {
        debug!("Wstat: fid={}, stat_len={}", fid, stat.len());

        // For now, just acknowledge the wstat operation
        // In real implementation: parse stat data and apply changes
        Ok(NinePeeMessage::Wstat { fid, stat })
    }

    /// Handle stream initialization (9P.e extension)
    async fn handle_stream_init(&mut self, stream_id: u32, fid: u32, mode: u8) -> Result<NinePeeMessage> {
        debug!("StreamInit: stream_id={}, fid={}, mode={}", stream_id, fid, mode);

        if !self.connection_state.supports_extensions() {
            return Ok(NinePeeMessage::Error {
                ename: "Extensions not supported".to_string(),
                errno: 95, // EOPNOTSUPP
            });
        }

        // For now, just acknowledge
        Ok(NinePeeMessage::StreamInit { stream_id, fid, mode })
    }

    /// Serialize message to bytes for transmission
    pub async fn serialize_message(&self, message: &NinePeeMessage) -> Result<Vec<u8>> {
        message.serialize().map_err(|e| anyhow::anyhow!("Serialization failed: {}", e))
    }

    /// Deserialize message from bytes
    pub async fn deserialize_message(&self, data: Vec<u8>) -> Result<NinePeeMessage> {
        NinePeeMessage::deserialize(data).map_err(|e| anyhow::anyhow!("Deserialization failed: {}", e))
    }
}