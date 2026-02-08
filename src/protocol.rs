//! Core 9P.e Protocol Implementation
//!
//! Implements the enhanced 9P protocol with async streaming, multiplexing,
//! and backward compatibility with 9P2000.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
#[cfg(feature = "testing")]
use arbitrary::Arbitrary;
// use bytes::{Bytes, BytesMut, Buf, BufMut}; // Using Vec<u8> instead
use tokio::sync::{mpsc, RwLock};
use std::sync::Arc;

/// 9P.e protocol version string
pub const NINEP_VERSION: &str = "9P.e";

/// Legacy 9P2000 version string for compatibility
pub const LEGACY_VERSION: &str = "9P2000";

/// Maximum message size (16MB)
pub const MAX_MESSAGE_SIZE: u32 = 16 * 1024 * 1024;

/// Minimum message size (1KB)
pub const MIN_MESSAGE_SIZE: u32 = 1024;

/// Maximum number of walk path components (prevents DoS)
pub const MAX_WNAME_COUNT: u16 = 256;

/// Maximum number of parent hashes in consensus (prevents DoS)
pub const MAX_PARENT_HASHES: u32 = 1024;

/// Maximum string length in protocol messages
pub const MAX_STRING_LENGTH: usize = 65535;

/// Maximum translator code size (16MB)
pub const MAX_TRANSLATOR_CODE_SIZE: u32 = 16 * 1024 * 1024;

/// Maximum synthetic file params size (1MB)
pub const MAX_SYNTHETIC_PARAMS_SIZE: u32 = 1024 * 1024;

/// 9P.e message types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "testing", derive(Arbitrary))]
pub enum NinePMessage {
    // Core 9P2000 messages
    /// Version negotiation message
    Version {
        /// Maximum message size for the connection
        msize: u32,
        /// Protocol version string (9P.e or 9P2000)
        version: String
    },
    /// Authentication request
    Auth {
        /// Authentication file ID
        afid: u32,
        /// User name requesting authentication
        uname: String,
        /// Access name (file tree to access)
        aname: String,
        /// Optional password for authentication
        password: Option<String>
    },
    /// Attach to file tree
    Attach {
        /// File ID for root of attached tree
        fid: u32,
        /// Authentication file ID (or NOFID)
        afid: u32,
        /// User name
        uname: String,
        /// Access name (file tree to access)
        aname: String
    },
    /// Walk file tree
    Walk {
        /// Starting file ID
        fid: u32,
        /// New file ID for destination
        newfid: u32,
        /// Path components to walk
        wnames: Vec<String>
    },
    /// Open a file
    Open {
        /// File ID to open
        fid: u32,
        /// Open mode (read, write, etc.)
        mode: u8
    },
    /// Create a new file
    Create {
        /// File ID of parent directory
        fid: u32,
        /// Name of file to create
        name: String,
        /// Permissions for new file
        perm: u32,
        /// Open mode for new file
        mode: u8
    },
    /// Read from file
    Read {
        /// File ID to read from
        fid: u32,
        /// Offset in file to start reading
        offset: u64,
        /// Number of bytes to read
        count: u32,
        /// Payload returned from read responses
        #[serde(default)]
        data: Vec<u8>
    },
    /// Write to file
    Write {
        /// File ID to write to
        fid: u32,
        /// Offset in file to start writing
        offset: u64,
        /// Data to write
        data: Vec<u8>
    },
    /// Close file (release fid)
    Clunk {
        /// File ID to close
        fid: u32
    },
    /// Remove file
    Remove {
        /// File ID to remove
        fid: u32
    },
    /// Get file statistics
    Stat {
        /// File ID to stat
        fid: u32,
        /// Serialized stat payload when available
        #[serde(default)]
        data: Vec<u8>
    },
    /// Set file statistics
    Wstat {
        /// File ID to modify
        fid: u32,
        /// New stat data
        stat: Vec<u8>
    },
    /// Error response
    Error {
        /// Error message
        ename: String,
        /// Error number
        errno: u32
    },

    // 9P.e extensions
    /// Initialize a new async stream for a file
    StreamInit {
        /// Unique identifier for this stream
        stream_id: u32,
        /// File ID to stream from
        fid: u32,
        /// Stream mode (read/write)
        mode: u8
    },
    /// Data chunk for an active stream
    StreamData {
        /// Stream this data belongs to
        stream_id: u32,
        /// Sequence number of this chunk
        chunk_id: u32,
        /// Chunk data payload
        data: Vec<u8>
    },
    /// Signal end of stream
    StreamEnd {
        /// Stream to terminate
        stream_id: u32,
        /// ID of the final chunk sent
        final_chunk: u32
    },
    /// Create a multiplexed channel for concurrent operations
    MultiplexChannel {
        /// Unique channel identifier
        channel_id: u32,
        /// Channel priority (0-255, higher = more priority)
        priority: u8
    },

    // Capability-based security
    /// Grant a capability for a file
    CapabilityGrant {
        /// Unique capability identifier
        cap_id: u64,
        /// File ID this capability applies to
        fid: u32,
        /// Permission bits granted
        permissions: u32
    },
    /// Revoke a previously granted capability
    CapabilityRevoke {
        /// Capability ID to revoke
        cap_id: u64
    },
    /// Check if a capability is valid
    CapabilityCheck {
        /// Capability ID to verify
        cap_id: u64
    },

    // Synthetic files
    /// Create a synthetic file with dynamic content
    SyntheticCreate {
        /// File ID for the synthetic file
        fid: u32,
        /// Generator function name
        generator: String,
        /// Parameters for the generator
        params: Vec<u8>
    },
    /// Update parameters of a synthetic file
    SyntheticUpdate {
        /// Synthetic file ID to update
        fid: u32,
        /// New generator parameters
        new_params: Vec<u8>
    },
    /// Refresh synthetic file content
    SyntheticRefresh {
        /// Synthetic file ID to refresh
        fid: u32,
        /// Force regeneration even if cached
        force: bool
    },

    // Translator system
    /// Spawn a new Hurd-style translator
    TranslatorSpawn {
        /// Unique translator identifier
        translator_id: u32,
        /// WASM bytecode for the translator
        code: Vec<u8>,
        /// Configuration for the translator
        config: Vec<u8>
    },
    /// Send message to a running translator
    TranslatorMessage {
        /// Target translator ID
        translator_id: u32,
        /// Message data payload
        data: Vec<u8>
    },
    /// Terminate a running translator
    TranslatorKill {
        /// Translator ID to terminate
        translator_id: u32
    },

    // GHOSTDAG consensus
    /// Propose a new block for consensus
    ConsensusPropose {
        /// Hash of the proposed block
        block_hash: [u8; 32],
        /// Parent block hashes
        parent_hashes: Vec<[u8; 32]>
    },
    /// Vote on a proposed block
    ConsensusVote {
        /// Block hash to vote on
        block_hash: [u8; 32],
        /// Vote (true = accept, false = reject)
        vote: bool
    },
    /// Commit a block to the chain
    ConsensusCommit {
        /// Block hash to commit
        block_hash: [u8; 32],
        /// GHOSTDAG blue score
        blue_score: u64
    },

    // Memory management
    /// Allocate shared memory
    MemAlloc {
        /// Size of memory to allocate
        size: u64,
        /// Unique identifier for this region
        id: String,
    },
    /// Borrow shared memory
    MemBorrow {
        /// Region ID to borrow
        id: String,
        /// Whether to request exclusive write access
        write: bool,
    },
    /// Release shared memory borrow
    MemRelease {
        /// Region ID to release
        id: String,
    },
    /// Memory operation response
    MemResponse {
        /// Region ID referenced
        id: String,
        /// Whether the operation succeeded
        success: bool,
    },
}

/// File ID type
pub type Fid = u32;

/// Stream ID type
pub type StreamId = u32;

/// Channel ID type
pub type ChannelId = u32;

/// Unique Identifier for a file
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "testing", derive(Arbitrary))]
pub struct Qid {
    /// File type (directory, etc)
    pub qtype: u8,
    /// Protocol version
    pub version: u32,
    /// Unique path identifier
    pub path: u64,
}

/// Start with modern size/type/dev/qid
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "testing", derive(Arbitrary))]
pub struct Stat {
    /// Size of this stat structure
    pub size: u16,
    /// File type
    pub typ: u16,
    /// Device ID
    pub dev: u32,
    /// Unique ID
    pub qid: Qid,
    /// Permissions and mode
    pub mode: u32,
    /// Last access time
    pub atime: u32,
    /// Last modification time
    pub mtime: u32,
    /// File length
    pub length: u64,
    /// File name
    pub name: String,
    /// Owner name
    pub uid: String,
    /// Group name
    pub gid: String,
    /// Modifier name
    pub muid: String,
}

/// Connection state for a 9P.e session
#[derive(Debug)]
pub struct ConnectionState {
    /// Unique identifier for this connection
    pub connection_id: u32,
    /// Negotiated protocol version (9P2000 or 9P.e)
    pub protocol_version: String,
    /// Maximum message size in bytes
    pub max_message_size: u32,
    /// Active file handles mapped by FID
    pub active_fids: HashMap<Fid, FileHandle>,
    /// Active async streams mapped by stream ID
    pub active_streams: HashMap<StreamId, StreamHandle>,
    /// Active multiplex channels mapped by channel ID
    pub active_channels: HashMap<ChannelId, ChannelHandle>,
    /// Granted capabilities mapped by capability ID to permissions
    pub capabilities: HashMap<u64, u32>,
    /// Whether the connection is authenticated
    pub authenticated: bool,
    /// Shared memory borrows active on this connection
    pub shared_memory_borrows: HashMap<String, crate::ipc::SharedMemoryHandle>,
}

/// File handle state for an open file
#[derive(Debug, Clone)]
pub struct FileHandle {
    /// File identifier
    pub fid: Fid,
    /// Full path to the file
    pub path: String,
    /// Open mode (read/write/execute bits)
    pub mode: u8,
    /// Current read/write offset in the file
    pub offset: u64,
    /// Whether this is a synthetic file
    pub synthetic: bool,
    /// Associated translator ID if any
    pub translator_id: Option<u32>,
}

/// Stream handle for async operations
#[derive(Debug)]
pub struct StreamHandle {
    /// Unique stream identifier
    pub stream_id: StreamId,
    /// File ID this stream is connected to
    pub fid: Fid,
    /// Stream mode (read/write)
    pub mode: u8,
    /// Channel for sending data to the stream
    pub sender: mpsc::UnboundedSender<Vec<u8>>,
    /// Channel for receiving data from the stream
    pub receiver: Arc<RwLock<mpsc::UnboundedReceiver<Vec<u8>>>>,
    /// Whether the stream has been closed
    pub closed: bool,
}

/// Multiplex channel handle
#[derive(Debug)]
pub struct ChannelHandle {
    /// Unique channel identifier
    pub channel_id: ChannelId,
    /// Channel priority (0-255, higher = more priority)
    pub priority: u8,
    /// Queue for sending messages through this channel
    pub message_queue: mpsc::UnboundedSender<NinePMessage>,
    /// Whether the channel is active
    pub active: bool,
}

/// Protocol error types
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    /// Message exceeds maximum allowed size
    #[error("Invalid message size: {0}")]
    InvalidMessageSize(u32),

    /// Message format is invalid or malformed
    #[error("Invalid message: {0}")]
    InvalidMessage(String),

    /// Protocol version is not supported
    #[error("Unsupported protocol version: {0}")]
    UnsupportedVersion(String),

    /// Referenced file ID does not exist
    #[error("File ID not found: {0}")]
    FidNotFound(Fid),

    /// Referenced stream ID does not exist
    #[error("Stream not found: {0}")]
    StreamNotFound(StreamId),

    /// Referenced channel ID does not exist
    #[error("Channel not found: {0}")]
    ChannelNotFound(ChannelId),

    /// Operation not permitted
    #[error("Permission denied")]
    PermissionDenied,

    /// Referenced capability does not exist
    #[error("Capability not found: {0}")]
    CapabilityNotFound(u64),

    /// Authentication is required for this operation
    #[error("Authentication required")]
    AuthenticationRequired,

    /// System resource limit exceeded
    #[error("Resource limit exceeded")]
    ResourceLimitExceeded,

    /// Failed to serialize or deserialize data
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// I/O operation failed
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

impl NinePMessage {
    /// Create a Write message with size validation
    pub fn new_write(fid: u32, offset: u64, data: Vec<u8>) -> Result<Self, ProtocolError> {
        // Check size before creating the message
        if data.len() > (MAX_MESSAGE_SIZE as usize - 32) {
            return Err(ProtocolError::InvalidMessageSize(data.len() as u32));
        }
        Ok(NinePMessage::Write { fid, offset, data })
    }

    /// Create a Write message from a data source without allocating huge buffers
    pub fn new_write_safe(fid: u32, offset: u64, data_len: usize) -> Result<Self, ProtocolError> {
        // Validate size before any allocation
        if data_len > (MAX_MESSAGE_SIZE as usize - 32) {
            return Err(ProtocolError::InvalidMessageSize(data_len as u32));
        }
        // For testing, create with zeros - in real use would stream from source
        Ok(NinePMessage::Write {
            fid,
            offset,
            data: vec![0; data_len.min(MAX_MESSAGE_SIZE as usize - 32)]
        })
    }

    // Helper functions for writing to Vec<u8>
    fn write_u8(buf: &mut Vec<u8>, value: u8) {
        buf.push(value);
    }

    fn write_u16(buf: &mut Vec<u8>, value: u16) {
        buf.extend_from_slice(&value.to_le_bytes());
    }

    fn write_u32(buf: &mut Vec<u8>, value: u32) {
        buf.extend_from_slice(&value.to_le_bytes());
    }

    fn write_u64(buf: &mut Vec<u8>, value: u64) {
        buf.extend_from_slice(&value.to_le_bytes());
    }

    // Removed unused write_bytes function

    fn write_string_impl(buf: &mut Vec<u8>, s: &str) {
        let bytes = s.as_bytes();
        Self::write_u16(buf, bytes.len() as u16);
        buf.extend_from_slice(bytes);
    }

    /// Serialize message to bytes
    pub fn serialize(&self) -> Result<Vec<u8>, ProtocolError> {
        // Early check for Write message data size to avoid allocating huge buffers
        match self {
            NinePMessage::Write { data, .. } | NinePMessage::Read { data, .. } => {
                // Check if data alone exceeds max message size
                // Message overhead is at least 1 (type) + fields (~32 bytes)
                if data.len() > (MAX_MESSAGE_SIZE as usize - 32) {
                    return Err(ProtocolError::InvalidMessageSize(data.len() as u32 + 32));
                }
            }
            _ => {}
        }

        let mut buf = Vec::new();

        // Write message type
        let msg_type = self.message_type();
        Self::write_u8(&mut buf, msg_type);

        // Serialize message data
        match self {
            NinePMessage::Version { msize, version } => {
                Self::write_u32(&mut buf, *msize);
                Self::write_string_impl(&mut buf, version);
            }
            NinePMessage::Auth { afid, uname, aname, password } => {
                Self::write_u32(&mut buf, *afid);
                Self::write_string_impl(&mut buf, uname);
                Self::write_string_impl(&mut buf, aname);
                // Serialize password as optional string
                if let Some(pass) = password {
                    Self::write_u8(&mut buf, 1); // Has password
                    Self::write_string_impl(&mut buf, pass);
                } else {
                    Self::write_u8(&mut buf, 0); // No password
                }
            }
            NinePMessage::Attach { fid, afid, uname, aname } => {
                Self::write_u32(&mut buf, *fid);
                Self::write_u32(&mut buf, *afid);
                Self::write_string_impl(&mut buf, uname);
                Self::write_string_impl(&mut buf, aname);
            }
            NinePMessage::Walk { fid, newfid, wnames } => {
                Self::write_u32(&mut buf, *fid);
                Self::write_u32(&mut buf, *newfid);
                Self::write_u16(&mut buf, wnames.len() as u16);
                for wname in wnames {
                    Self::write_string_impl(&mut buf, wname);
                }
            }
            NinePMessage::Open { fid, mode } => {
                Self::write_u32(&mut buf, *fid);
                Self::write_u8(&mut buf, *mode);
            }
            NinePMessage::Create { fid, name, perm, mode } => {
                Self::write_u32(&mut buf, *fid);
                Self::write_string_impl(&mut buf, name);
                Self::write_u32(&mut buf, *perm);
                Self::write_u8(&mut buf, *mode);
            }
            NinePMessage::Read { fid, offset, count, data } => {
                Self::write_u32(&mut buf, *fid);
                Self::write_u64(&mut buf, *offset);
                Self::write_u32(&mut buf, *count);
                Self::write_u32(&mut buf, data.len() as u32);
                buf.extend_from_slice(data);
            }
            NinePMessage::Write { fid, offset, data } => {
                Self::write_u32(&mut buf, *fid);
                Self::write_u64(&mut buf, *offset);
                Self::write_u32(&mut buf, data.len() as u32);
                buf.extend_from_slice(data);
            }
            NinePMessage::Clunk { fid } => {
                Self::write_u32(&mut buf, *fid);
            }
            NinePMessage::Remove { fid } => {
                Self::write_u32(&mut buf, *fid);
            }
            NinePMessage::Stat { fid, data } => {
                Self::write_u32(&mut buf, *fid);
                Self::write_u32(&mut buf, data.len() as u32);
                buf.extend_from_slice(data);
            }
            NinePMessage::Wstat { fid, stat } => {
                Self::write_u32(&mut buf, *fid);
                Self::write_u32(&mut buf, stat.len() as u32);
                buf.extend_from_slice(stat);
            }
            NinePMessage::Error { ename, errno } => {
                Self::write_string_impl(&mut buf, ename);
                Self::write_u32(&mut buf, *errno);
            }

            // 9P.e extensions
            NinePMessage::StreamInit { stream_id, fid, mode } => {
                Self::write_u32(&mut buf, *stream_id);
                Self::write_u32(&mut buf, *fid);
                Self::write_u8(&mut buf, *mode);
            }
            NinePMessage::StreamData { stream_id, chunk_id, data } => {
                Self::write_u32(&mut buf, *stream_id);
                Self::write_u32(&mut buf, *chunk_id);
                Self::write_u32(&mut buf, data.len() as u32);
                buf.extend_from_slice(data);
            }
            NinePMessage::StreamEnd { stream_id, final_chunk } => {
                Self::write_u32(&mut buf, *stream_id);
                Self::write_u32(&mut buf, *final_chunk);
            }
            NinePMessage::MultiplexChannel { channel_id, priority } => {
                Self::write_u32(&mut buf, *channel_id);
                Self::write_u8(&mut buf, *priority);
            }
            NinePMessage::CapabilityGrant { cap_id, fid, permissions } => {
                Self::write_u64(&mut buf, *cap_id);
                Self::write_u32(&mut buf, *fid);
                Self::write_u32(&mut buf, *permissions);
            }
            NinePMessage::CapabilityRevoke { cap_id } => {
                Self::write_u64(&mut buf, *cap_id);
            }
            NinePMessage::CapabilityCheck { cap_id } => {
                Self::write_u64(&mut buf, *cap_id);
            }
            NinePMessage::SyntheticCreate { fid, generator, params } => {
                Self::write_u32(&mut buf, *fid);
                Self::write_string_impl(&mut buf, generator);
                Self::write_u32(&mut buf, params.len() as u32);
                buf.extend_from_slice(params);
            }
            NinePMessage::SyntheticUpdate { fid, new_params } => {
                Self::write_u32(&mut buf, *fid);
                Self::write_u32(&mut buf, new_params.len() as u32);
                buf.extend_from_slice(new_params);
            }
            NinePMessage::SyntheticRefresh { fid, force } => {
                Self::write_u32(&mut buf, *fid);
                Self::write_u8(&mut buf, if *force { 1 } else { 0 });
            }
            NinePMessage::TranslatorSpawn { translator_id, code, config } => {
                Self::write_u32(&mut buf, *translator_id);
                Self::write_u32(&mut buf, code.len() as u32);
                buf.extend_from_slice(code);
                Self::write_u32(&mut buf, config.len() as u32);
                buf.extend_from_slice(config);
            }
            NinePMessage::TranslatorMessage { translator_id, data } => {
                Self::write_u32(&mut buf, *translator_id);
                Self::write_u32(&mut buf, data.len() as u32);
                buf.extend_from_slice(data);
            }
            NinePMessage::TranslatorKill { translator_id } => {
                Self::write_u32(&mut buf, *translator_id);
            }
            NinePMessage::ConsensusPropose { block_hash, parent_hashes } => {
                buf.extend_from_slice(block_hash);
                Self::write_u32(&mut buf, parent_hashes.len() as u32);
                for parent in parent_hashes {
                    buf.extend_from_slice(parent);
                }
            }
            NinePMessage::ConsensusVote { block_hash, vote } => {
                buf.extend_from_slice(block_hash);
                Self::write_u8(&mut buf, if *vote { 1 } else { 0 });
            }
            NinePMessage::ConsensusCommit { block_hash, blue_score } => {
                buf.extend_from_slice(block_hash);
                Self::write_u64(&mut buf, *blue_score);
            }
            NinePMessage::MemAlloc { size, id } => {
                Self::write_u64(&mut buf, *size);
                Self::write_string_impl(&mut buf, id);
            }
            NinePMessage::MemBorrow { id, write } => {
                Self::write_string_impl(&mut buf, id);
                Self::write_u8(&mut buf, if *write { 1 } else { 0 });
            }
            NinePMessage::MemRelease { id } => {
                Self::write_string_impl(&mut buf, id);
            }
            NinePMessage::MemResponse { id, success } => {
                Self::write_string_impl(&mut buf, id);
                Self::write_u8(&mut buf, if *success { 1 } else { 0 });
            }
        }

        // Check message size bounds
        let total_size = buf.len() as u32;
        if total_size > MAX_MESSAGE_SIZE {
            return Err(ProtocolError::InvalidMessageSize(total_size));
        }

        Ok(buf)
    }

    // Helper functions for reading from byte slices
    fn read_u8(data: &mut &[u8]) -> Result<u8, ProtocolError> {
        if data.is_empty() {
            return Err(ProtocolError::InvalidMessage("Not enough bytes for u8".to_string()));
        }
        let val = data[0];
        *data = &data[1..];
        Ok(val)
    }

    fn read_u16(data: &mut &[u8]) -> Result<u16, ProtocolError> {
        if data.len() < 2 {
            return Err(ProtocolError::InvalidMessage("Not enough bytes for u16".to_string()));
        }
        let bytes = [data[0], data[1]];
        *data = &data[2..];
        Ok(u16::from_le_bytes(bytes))
    }

    fn read_u32(data: &mut &[u8]) -> Result<u32, ProtocolError> {
        if data.len() < 4 {
            return Err(ProtocolError::InvalidMessage("Not enough bytes for u32".to_string()));
        }
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&data[..4]);
        *data = &data[4..];
        Ok(u32::from_le_bytes(bytes))
    }

    fn read_u64(data: &mut &[u8]) -> Result<u64, ProtocolError> {
        if data.len() < 8 {
            return Err(ProtocolError::InvalidMessage("Not enough bytes for u64".to_string()));
        }
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&data[..8]);
        *data = &data[8..];
        Ok(u64::from_le_bytes(bytes))
    }

    fn read_bytes(data: &mut &[u8], len: usize) -> Result<Vec<u8>, ProtocolError> {
        // Defense #1: Validate size before allocation
        if len > MAX_MESSAGE_SIZE as usize {
            return Err(ProtocolError::InvalidMessageSize(len as u32));
        }
        if data.len() < len {
            return Err(ProtocolError::InvalidMessage("Not enough bytes".to_string()));
        }
        let result = data[..len].to_vec();
        *data = &data[len..];
        Ok(result)
    }

    /// Deserialize message from bytes
    pub fn deserialize(data: Vec<u8>) -> Result<Self, ProtocolError> {
        let mut data_slice = data.as_slice();
        if data_slice.is_empty() {
            return Err(ProtocolError::SerializationError("Empty message".to_string()));
        }

        let msg_type = Self::read_u8(&mut data_slice)?;

        match msg_type {
            0 => {
                let msize = Self::read_u32(&mut data_slice)?;
                let version = Self::read_string(&mut data_slice)?;
                Ok(NinePMessage::Version { msize, version })
            }
            1 => {
                let afid = Self::read_u32(&mut data_slice)?;
                let uname = Self::read_string(&mut data_slice)?;
                let aname = Self::read_string(&mut data_slice)?;
                // Deserialize password as optional string
                let password = if Self::read_u8(&mut data_slice)? == 1 {
                    Some(Self::read_string(&mut data_slice)?)
                } else {
                    None
                };
                Ok(NinePMessage::Auth { afid, uname, aname, password })
            }
            2 => {
                let fid = Self::read_u32(&mut data_slice)?;
                let afid = Self::read_u32(&mut data_slice)?;
                let uname = Self::read_string(&mut data_slice)?;
                let aname = Self::read_string(&mut data_slice)?;
                Ok(NinePMessage::Attach { fid, afid, uname, aname })
            }
            3 => {
                let fid = Self::read_u32(&mut data_slice)?;
                let newfid = Self::read_u32(&mut data_slice)?;
                let wname_count = Self::read_u16(&mut data_slice)?;

                // Validate wname count to prevent DoS
                if wname_count > MAX_WNAME_COUNT {
                    return Err(ProtocolError::SerializationError(
                        format!("Walk wname count {} exceeds maximum {}", wname_count, MAX_WNAME_COUNT)
                    ));
                }

                let mut wnames = Vec::with_capacity(wname_count as usize);
                for _ in 0..wname_count {
                    wnames.push(Self::read_string(&mut data_slice)?);
                }
                Ok(NinePMessage::Walk { fid, newfid, wnames })
            }
            4 => {
                let fid = Self::read_u32(&mut data_slice)?;
                let mode = Self::read_u8(&mut data_slice)?;
                Ok(NinePMessage::Open { fid, mode })
            }
            5 => {
                let fid = Self::read_u32(&mut data_slice)?;
                let name = Self::read_string(&mut data_slice)?;
                let perm = Self::read_u32(&mut data_slice)?;
                let mode = Self::read_u8(&mut data_slice)?;
                Ok(NinePMessage::Create { fid, name, perm, mode })
            }
            6 => {
                let fid = Self::read_u32(&mut data_slice)?;
                let offset = Self::read_u64(&mut data_slice)?;
                let count = Self::read_u32(&mut data_slice)?;
                let read_data = if data_slice.len() >= 4 {
                    let data_len = Self::read_u32(&mut data_slice)?;
                    if data_slice.len() < data_len as usize {
                        return Err(ProtocolError::SerializationError("Insufficient read payload".to_string()));
                    }
                    Self::read_bytes(&mut data_slice, data_len as usize)?
                } else {
                    Vec::new()
                };
                Ok(NinePMessage::Read {
                    fid,
                    offset,
                    count,
                    data: read_data,
                })
            }
            7 => {
                let fid = Self::read_u32(&mut data_slice)?;
                let offset = Self::read_u64(&mut data_slice)?;
                let data_len = Self::read_u32(&mut data_slice)?;

                // CRITICAL: Validate size BEFORE attempting to read/allocate
                if data_len > MAX_MESSAGE_SIZE - 32 {
                    return Err(ProtocolError::InvalidMessageSize(data_len));
                }

                if data_slice.len() < data_len as usize {
                    return Err(ProtocolError::SerializationError("Insufficient data".to_string()));
                }
                let write_data = Self::read_bytes(&mut data_slice, data_len as usize)?;
                Ok(NinePMessage::Write { fid, offset, data: write_data })
            }
            8 => {
                let fid = Self::read_u32(&mut data_slice)?;
                Ok(NinePMessage::Clunk { fid })
            }
            9 => {
                let fid = Self::read_u32(&mut data_slice)?;
                Ok(NinePMessage::Remove { fid })
            }
            10 => {
                let fid = Self::read_u32(&mut data_slice)?;
                let stat_data = if data_slice.len() >= 4 {
                    let stat_len = Self::read_u32(&mut data_slice)?;
                    if data_slice.len() < stat_len as usize {
                        return Err(ProtocolError::SerializationError("Insufficient stat payload".to_string()));
                    }
                    Self::read_bytes(&mut data_slice, stat_len as usize)?
                } else {
                    Vec::new()
                };
                Ok(NinePMessage::Stat { fid, data: stat_data })
            }
            11 => {
                let fid = Self::read_u32(&mut data_slice)?;
                let stat_len = Self::read_u32(&mut data_slice)?;
                if data_slice.len() <stat_len as usize {
                    return Err(ProtocolError::SerializationError("Insufficient stat data".to_string()));
                }
                let stat = Self::read_bytes(&mut data_slice, stat_len as usize)?;
                Ok(NinePMessage::Wstat { fid, stat })
            }
            12 => {
                let ename = Self::read_string(&mut data_slice)?;
                let errno = Self::read_u32(&mut data_slice)?;
                Ok(NinePMessage::Error { ename, errno })
            }

            // 9P.e extensions (starting from 100)
            100 => {
                let stream_id = Self::read_u32(&mut data_slice)?;
                let fid = Self::read_u32(&mut data_slice)?;
                let mode = Self::read_u8(&mut data_slice)?;
                Ok(NinePMessage::StreamInit { stream_id, fid, mode })
            }
            101 => {
                let stream_id = Self::read_u32(&mut data_slice)?;
                let chunk_id = Self::read_u32(&mut data_slice)?;
                let data_len = Self::read_u32(&mut data_slice)?;
                if data_slice.len() <data_len as usize {
                    return Err(ProtocolError::SerializationError("Insufficient stream data".to_string()));
                }
                let stream_data = Self::read_bytes(&mut data_slice, data_len as usize)?;
                Ok(NinePMessage::StreamData { stream_id, chunk_id, data: stream_data })
            }
            102 => {
                let stream_id = Self::read_u32(&mut data_slice)?;
                let final_chunk = Self::read_u32(&mut data_slice)?;
                Ok(NinePMessage::StreamEnd { stream_id, final_chunk })
            }
            103 => {
                let channel_id = Self::read_u32(&mut data_slice)?;
                let priority = Self::read_u8(&mut data_slice)?;
                Ok(NinePMessage::MultiplexChannel { channel_id, priority })
            }
            110 => {
                let cap_id = Self::read_u64(&mut data_slice)?;
                let fid = Self::read_u32(&mut data_slice)?;
                let permissions = Self::read_u32(&mut data_slice)?;
                Ok(NinePMessage::CapabilityGrant { cap_id, fid, permissions })
            }
            111 => {
                let cap_id = Self::read_u64(&mut data_slice)?;
                Ok(NinePMessage::CapabilityRevoke { cap_id })
            }
            112 => {
                let cap_id = Self::read_u64(&mut data_slice)?;
                Ok(NinePMessage::CapabilityCheck { cap_id })
            }
            120 => {
                let fid = Self::read_u32(&mut data_slice)?;
                let generator = Self::read_string(&mut data_slice)?;
                let params_len = Self::read_u32(&mut data_slice)?;

                // Validate params size to prevent DoS
                if params_len > MAX_SYNTHETIC_PARAMS_SIZE {
                    return Err(ProtocolError::SerializationError(
                        format!("Synthetic params size {} exceeds maximum {}", params_len, MAX_SYNTHETIC_PARAMS_SIZE)
                    ));
                }

                if data_slice.len() < params_len as usize {
                    return Err(ProtocolError::SerializationError("Insufficient params data".to_string()));
                }
                let params = Self::read_bytes(&mut data_slice, params_len as usize)?;
                Ok(NinePMessage::SyntheticCreate { fid, generator, params })
            }
            121 => {
                let fid = Self::read_u32(&mut data_slice)?;
                let params_len = Self::read_u32(&mut data_slice)?;

                // Validate params size to prevent DoS
                if params_len > MAX_SYNTHETIC_PARAMS_SIZE {
                    return Err(ProtocolError::SerializationError(
                        format!("Synthetic params size {} exceeds maximum {}", params_len, MAX_SYNTHETIC_PARAMS_SIZE)
                    ));
                }

                if data_slice.len() < params_len as usize {
                    return Err(ProtocolError::SerializationError("Insufficient params data".to_string()));
                }
                let new_params = Self::read_bytes(&mut data_slice, params_len as usize)?;
                Ok(NinePMessage::SyntheticUpdate { fid, new_params })
            }
            122 => {
                let fid = Self::read_u32(&mut data_slice)?;
                let force = Self::read_u8(&mut data_slice)? != 0;
                Ok(NinePMessage::SyntheticRefresh { fid, force })
            }
            130 => {
                let translator_id = Self::read_u32(&mut data_slice)?;
                let code_len = Self::read_u32(&mut data_slice)?;

                // Validate code size to prevent DoS
                if code_len > MAX_TRANSLATOR_CODE_SIZE {
                    return Err(ProtocolError::SerializationError(
                        format!("Translator code size {} exceeds maximum {}", code_len, MAX_TRANSLATOR_CODE_SIZE)
                    ));
                }

                if data_slice.len() < code_len as usize {
                    return Err(ProtocolError::SerializationError("Insufficient code data".to_string()));
                }
                let code = Self::read_bytes(&mut data_slice, code_len as usize)?;
                let config_len = Self::read_u32(&mut data_slice)?;

                // Validate config size (same limit as code)
                if config_len > MAX_TRANSLATOR_CODE_SIZE {
                    return Err(ProtocolError::SerializationError(
                        format!("Translator config size {} exceeds maximum {}", config_len, MAX_TRANSLATOR_CODE_SIZE)
                    ));
                }

                if data_slice.len() < config_len as usize {
                    return Err(ProtocolError::SerializationError("Insufficient config data".to_string()));
                }
                let config = Self::read_bytes(&mut data_slice, config_len as usize)?;
                Ok(NinePMessage::TranslatorSpawn { translator_id, code, config })
            }
            131 => {
                let translator_id = Self::read_u32(&mut data_slice)?;
                let data_len = Self::read_u32(&mut data_slice)?;
                if data_slice.len() <data_len as usize {
                    return Err(ProtocolError::SerializationError("Insufficient translator data".to_string()));
                }
                let translator_data = Self::read_bytes(&mut data_slice, data_len as usize)?;
                Ok(NinePMessage::TranslatorMessage { translator_id, data: translator_data })
            }
            132 => {
                let translator_id = Self::read_u32(&mut data_slice)?;
                Ok(NinePMessage::TranslatorKill { translator_id })
            }
            140 => {
                if data_slice.len() < 32 {
                    return Err(ProtocolError::SerializationError("Insufficient hash data".to_string()));
                }
                let block_hash_vec = Self::read_bytes(&mut data_slice, 32)?;
                let mut block_hash = [0u8; 32];
                block_hash.copy_from_slice(&block_hash_vec);
                let parent_count = Self::read_u32(&mut data_slice)?;

                // Validate parent count to prevent DoS
                if parent_count > MAX_PARENT_HASHES {
                    return Err(ProtocolError::SerializationError(
                        format!("Parent hash count {} exceeds maximum {}", parent_count, MAX_PARENT_HASHES)
                    ));
                }

                let mut parent_hashes = Vec::with_capacity(parent_count as usize);
                for _ in 0..parent_count {
                    if data_slice.len() < 32 {
                        return Err(ProtocolError::SerializationError("Insufficient parent hash data".to_string()));
                    }
                    let parent_hash_vec = Self::read_bytes(&mut data_slice, 32)?;
                    let mut parent_hash = [0u8; 32];
                    parent_hash.copy_from_slice(&parent_hash_vec);
                    parent_hashes.push(parent_hash);
                }
                Ok(NinePMessage::ConsensusPropose { block_hash, parent_hashes })
            }
            141 => {
                if data_slice.len() <32 {
                    return Err(ProtocolError::SerializationError("Insufficient hash data".to_string()));
                }
                let block_hash_vec = Self::read_bytes(&mut data_slice, 32)?;
                let mut block_hash = [0u8; 32];
                block_hash.copy_from_slice(&block_hash_vec);
                let vote = Self::read_u8(&mut data_slice)? != 0;
                Ok(NinePMessage::ConsensusVote { block_hash, vote })
            }
            142 => {
                if data_slice.len() <32 {
                    return Err(ProtocolError::SerializationError("Insufficient hash data".to_string()));
                }
                let block_hash_vec = Self::read_bytes(&mut data_slice, 32)?;
                let mut block_hash = [0u8; 32];
                block_hash.copy_from_slice(&block_hash_vec);
                let blue_score = Self::read_u64(&mut data_slice)?;
                Ok(NinePMessage::ConsensusCommit { block_hash, blue_score })
            }
            150 => {
                let size = Self::read_u64(&mut data_slice)?;
                let id = Self::read_string(&mut data_slice)?;
                Ok(NinePMessage::MemAlloc { size, id })
            }
            151 => {
                let id = Self::read_string(&mut data_slice)?;
                let write = Self::read_u8(&mut data_slice)? != 0;
                Ok(NinePMessage::MemBorrow { id, write })
            }
            152 => {
                let id = Self::read_string(&mut data_slice)?;
                Ok(NinePMessage::MemRelease { id })
            }
            153 => {
                let id = Self::read_string(&mut data_slice)?;
                let success = Self::read_u8(&mut data_slice)? != 0;
                Ok(NinePMessage::MemResponse { id, success })
            }
            _ => Err(ProtocolError::SerializationError(format!("Unknown message type: {}", msg_type))),
        }
    }

    /// Get message type ID
    fn message_type(&self) -> u8 {
        match self {
            NinePMessage::Version { .. } => 0,
            NinePMessage::Auth { .. } => 1,
            NinePMessage::Attach { .. } => 2,
            NinePMessage::Walk { .. } => 3,
            NinePMessage::Open { .. } => 4,
            NinePMessage::Create { .. } => 5,
            NinePMessage::Read { .. } => 6,
            NinePMessage::Write { .. } => 7,
            NinePMessage::Clunk { .. } => 8,
            NinePMessage::Remove { .. } => 9,
            NinePMessage::Stat { .. } => 10,
            NinePMessage::Wstat { .. } => 11,
            NinePMessage::Error { .. } => 12,

            NinePMessage::StreamInit { .. } => 100,
            NinePMessage::StreamData { .. } => 101,
            NinePMessage::StreamEnd { .. } => 102,
            NinePMessage::MultiplexChannel { .. } => 103,

            NinePMessage::CapabilityGrant { .. } => 110,
            NinePMessage::CapabilityRevoke { .. } => 111,
            NinePMessage::CapabilityCheck { .. } => 112,

            NinePMessage::SyntheticCreate { .. } => 120,
            NinePMessage::SyntheticUpdate { .. } => 121,
            NinePMessage::SyntheticRefresh { .. } => 122,

            NinePMessage::TranslatorSpawn { .. } => 130,
            NinePMessage::TranslatorMessage { .. } => 131,
            NinePMessage::TranslatorKill { .. } => 132,

            NinePMessage::ConsensusCommit { .. } => 142,

            NinePMessage::MemAlloc { .. } => 150,
            NinePMessage::MemBorrow { .. } => 151,
            NinePMessage::MemRelease { .. } => 152,
            NinePMessage::MemResponse { .. } => 153,

            NinePMessage::ConsensusPropose { .. } => 140,
            NinePMessage::ConsensusVote { .. } => 141,
        }
    }

    // write_string is replaced by write_string_impl above

    /// Read string from buffer with length validation
    fn read_string(data: &mut &[u8]) -> Result<String, ProtocolError> {
        if data.len() < 2 {
            return Err(ProtocolError::SerializationError("Insufficient string length data".to_string()));
        }

        let len = Self::read_u16(data)? as usize;

        // Validate string length against maximum
        if len > MAX_STRING_LENGTH {
            return Err(ProtocolError::SerializationError(
                format!("String length {} exceeds maximum {}", len, MAX_STRING_LENGTH)
            ));
        }

        if data.len() < len {
            return Err(ProtocolError::SerializationError("Insufficient string data".to_string()));
        }

        let string_bytes = Self::read_bytes(data, len)?;
        String::from_utf8(string_bytes)
            .map_err(|e| ProtocolError::SerializationError(format!("Invalid UTF-8: {}", e)))
    }

    /// Check if message is legacy 9P2000 compatible
    pub fn is_legacy_compatible(&self) -> bool {
        matches!(self,
            NinePMessage::Version { .. } |
            NinePMessage::Auth { .. } |
            NinePMessage::Attach { .. } |
            NinePMessage::Walk { .. } |
            NinePMessage::Open { .. } |
            NinePMessage::Create { .. } |
            NinePMessage::Read { .. } |
            NinePMessage::Write { .. } |
            NinePMessage::Clunk { .. } |
            NinePMessage::Remove { .. } |
            NinePMessage::Stat { .. } |
            NinePMessage::Wstat { .. } |
            NinePMessage::Error { .. }
        )
    }
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self {
            connection_id: 0,
            protocol_version: NINEP_VERSION.to_string(),
            max_message_size: MAX_MESSAGE_SIZE,
            active_fids: HashMap::new(),
            active_streams: HashMap::new(),
            active_channels: HashMap::new(),
            capabilities: HashMap::new(),
            authenticated: false,
            shared_memory_borrows: HashMap::new(),
        }
    }
}

impl ConnectionState {
    /// Create new connection with specified version
    pub fn new(connection_id: u32, version: &str, msize: u32) -> Self {
        Self {
            connection_id,
            protocol_version: version.to_string(),
            max_message_size: msize.min(MAX_MESSAGE_SIZE).max(MIN_MESSAGE_SIZE),
            active_fids: HashMap::new(),
            active_streams: HashMap::new(),
            active_channels: HashMap::new(),
            capabilities: HashMap::new(),
            authenticated: false,
            shared_memory_borrows: HashMap::new(),
        }
    }

    /// Check if connection supports 9P.e extensions
    pub fn supports_extensions(&self) -> bool {
        self.protocol_version == NINEP_VERSION
    }

    /// Add file handle
    pub fn add_fid(&mut self, fid: Fid, handle: FileHandle) {
        self.active_fids.insert(fid, handle);
    }

    /// Remove file handle
    pub fn remove_fid(&mut self, fid: Fid) -> Option<FileHandle> {
        self.active_fids.remove(&fid)
    }

    /// Get file handle
    pub fn get_fid(&self, fid: Fid) -> Option<&FileHandle> {
        self.active_fids.get(&fid)
    }

    /// Add stream handle
    pub fn add_stream(&mut self, stream_id: StreamId, handle: StreamHandle) {
        self.active_streams.insert(stream_id, handle);
    }

    /// Remove stream handle
    pub fn remove_stream(&mut self, stream_id: StreamId) -> Option<StreamHandle> {
        self.active_streams.remove(&stream_id)
    }

    /// Add capability
    pub fn grant_capability(&mut self, cap_id: u64, permissions: u32) {
        self.capabilities.insert(cap_id, permissions);
    }

    /// Remove capability
    pub fn revoke_capability(&mut self, cap_id: u64) -> Option<u32> {
        self.capabilities.remove(&cap_id)
    }

    /// Check capability
    pub fn check_capability(&self, cap_id: u64) -> Option<u32> {
        self.capabilities.get(&cap_id).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_serialization_roundtrip() {
        let msg = NinePMessage::Version {
            msize: 8192,
            version: "9P.e".to_string(),
        };

        let serialized = msg.serialize().unwrap();
        let deserialized = NinePMessage::deserialize(serialized).unwrap();

        assert_eq!(msg, deserialized);
    }

    #[test]
    fn test_write_read_message() {
        let data = Vec::from("hello world");
        let msg = NinePMessage::Write {
            fid: 42,
            offset: 1024,
            data: data.clone(),
        };

        let serialized = msg.serialize().unwrap();
        let deserialized = NinePMessage::deserialize(serialized).unwrap();

        match deserialized {
            NinePMessage::Write { fid, offset, data: deserialized_data } => {
                assert_eq!(fid, 42);
                assert_eq!(offset, 1024);
                assert_eq!(deserialized_data, data);
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_stream_messages() {
        let stream_data = Vec::from("stream content");
        let msg = NinePMessage::StreamData {
            stream_id: 100,
            chunk_id: 5,
            data: stream_data.clone(),
        };

        let serialized = msg.serialize().unwrap();
        let deserialized = NinePMessage::deserialize(serialized).unwrap();

        match deserialized {
            NinePMessage::StreamData { stream_id, chunk_id, data } => {
                assert_eq!(stream_id, 100);
                assert_eq!(chunk_id, 5);
                assert_eq!(data, stream_data);
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_legacy_compatibility() {
        let legacy_msg = NinePMessage::Read {
            fid: 1,
            offset: 0,
            count: 1024,
            data: Vec::new(),
        };

        assert!(legacy_msg.is_legacy_compatible());

        let extension_msg = NinePMessage::StreamInit {
            stream_id: 1,
            fid: 2,
            mode: 0,
        };

        assert!(!extension_msg.is_legacy_compatible());
    }

    #[test]
    fn test_connection_state() {
        let mut conn = ConnectionState::new(1, NINEP_VERSION, 8192);

        assert!(conn.supports_extensions());
        assert!(!conn.authenticated);

        let handle = FileHandle {
            fid: 42,
            path: "/test".to_string(),
            mode: 0,
            offset: 0,
            synthetic: false,
            translator_id: None,
        };

        conn.add_fid(42, handle);
        assert!(conn.get_fid(42).is_some());

        conn.grant_capability(12345, 0b111);
        assert_eq!(conn.check_capability(12345), Some(0b111));

        conn.revoke_capability(12345);
        assert_eq!(conn.check_capability(12345), None);
    }
}
