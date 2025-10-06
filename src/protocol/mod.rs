//! 9P.e Protocol Implementation
//!
//! This module implements the complete 9P2000 + 9P.e extensions protocol.
//! It provides message serialization, deserialization, and handling for all
//! standard and extended operations.

use std::io::{Read, Write, Result as IoResult};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub mod messages;
pub mod ninepee_messages;
pub mod client;
pub mod handler;

pub use messages::*;
pub use ninepee_messages::NinePeeMessage;
pub use client::NinePClient;
pub use handler::ProtocolHandler;

/// 9P Protocol Version
pub const VERSION_9P2000: &str = "9P2000";
pub const VERSION_9PE: &str = "9P2000.e";

/// Maximum message size (64KB default, 1MB for 9P.e)
pub const MAX_MSG_SIZE: u32 = 1048576; // 1MB for 9P.e extended operations

/// Message types (9P2000 + 9P.e extensions)
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    // 9P2000 base protocol
    Tversion = 100,
    Rversion = 101,
    Tauth = 102,
    Rauth = 103,
    Tattach = 104,
    Rattach = 105,
    Terror = 106,
    Rerror = 107,
    Tflush = 108,
    Rflush = 109,
    Twalk = 110,
    Rwalk = 111,
    Topen = 112,
    Ropen = 113,
    Tcreate = 114,
    Rcreate = 115,
    Tread = 116,
    Rread = 117,
    Twrite = 118,
    Rwrite = 119,
    Tclunk = 120,
    Rclunk = 121,
    Tremove = 122,
    Rremove = 123,
    Tstat = 124,
    Rstat = 125,
    Twstat = 126,
    Rwstat = 127,

    // 9P.e extensions
    Tstream = 200,
    Rstream = 201,
    Tmux = 202,
    Rmux = 203,
    Tcap = 204,
    Rcap = 205,
    Tsettrans = 206,
    Rsettrans = 207,
    Tconsensus = 208,
    Rconsensus = 209,
}

/// File ID (9P terminology: Qid)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Qid {
    pub qtype: u8,    // File type
    pub version: u32, // Version for caching
    pub path: u64,    // Unique file ID
}

/// File information (9P stat structure)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stat {
    pub size: u16,       // Total size of this structure
    pub typ: u16,        // Server type
    pub dev: u32,        // Server subtype
    pub qid: Qid,        // File ID
    pub mode: u32,       // Permissions and flags
    pub atime: u32,      // Access time
    pub mtime: u32,      // Modification time
    pub length: u64,     // File length
    pub name: String,    // File name
    pub uid: String,     // Owner name
    pub gid: String,     // Group name
    pub muid: String,    // Last modifier
}

/// File identifier (handle for open files)
pub type Fid = u32;

/// Tag for matching requests with responses
pub type Tag = u16;

/// 9P Permission modes
pub mod permissions {
    pub const DMDIR: u32 = 0x80000000;     // Directory
    pub const DMAPPEND: u32 = 0x40000000;  // Append only
    pub const DMEXCL: u32 = 0x20000000;    // Exclusive use
    pub const DMMOUNT: u32 = 0x10000000;   // Mounted channel
    pub const DMAUTH: u32 = 0x08000000;    // Authentication file
    pub const DMTMP: u32 = 0x04000000;     // Temporary (not backed up)

    pub const OREAD: u8 = 0;    // Open for read
    pub const OWRITE: u8 = 1;   // Open for write
    pub const ORDWR: u8 = 2;    // Open for read/write
    pub const OEXEC: u8 = 3;    // Open for execute
    pub const OTRUNC: u8 = 0x10; // Truncate on open
}

/// Wire format encoder/decoder
pub struct WireFormat;

impl WireFormat {
    /// Encode a message to bytes
    pub fn encode<M: Message>(msg: &M) -> IoResult<Vec<u8>> {
        let mut buf = Vec::new();

        // Reserve space for size (4 bytes) and type (1 byte)
        buf.extend_from_slice(&[0u8; 5]);

        // Encode the message content
        msg.encode(&mut buf)?;

        // Write actual size at the beginning
        let size = buf.len() as u32;
        buf[0..4].copy_from_slice(&size.to_le_bytes());

        // Write message type
        buf[4] = msg.msg_type() as u8;

        Ok(buf)
    }

    /// Decode a message from bytes
    pub fn decode(buf: &[u8]) -> IoResult<Box<dyn Message>> {
        if buf.len() < 5 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Message too short"
            ));
        }

        let size = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let msg_type = buf[4];

        // Decode based on message type
        let msg = match msg_type {
            100 => Box::new(Tversion::decode(&buf[5..])?) as Box<dyn Message>,
            101 => Box::new(Rversion::decode(&buf[5..])?) as Box<dyn Message>,
            102 => Box::new(Tauth::decode(&buf[5..])?) as Box<dyn Message>,
            103 => Box::new(Rauth::decode(&buf[5..])?) as Box<dyn Message>,
            104 => Box::new(Tattach::decode(&buf[5..])?) as Box<dyn Message>,
            105 => Box::new(Rattach::decode(&buf[5..])?) as Box<dyn Message>,
            110 => Box::new(Twalk::decode(&buf[5..])?) as Box<dyn Message>,
            111 => Box::new(Rwalk::decode(&buf[5..])?) as Box<dyn Message>,
            112 => Box::new(Topen::decode(&buf[5..])?) as Box<dyn Message>,
            113 => Box::new(Ropen::decode(&buf[5..])?) as Box<dyn Message>,
            116 => Box::new(Tread::decode(&buf[5..])?) as Box<dyn Message>,
            117 => Box::new(Rread::decode(&buf[5..])?) as Box<dyn Message>,
            118 => Box::new(Twrite::decode(&buf[5..])?) as Box<dyn Message>,
            119 => Box::new(Rwrite::decode(&buf[5..])?) as Box<dyn Message>,
            120 => Box::new(Tclunk::decode(&buf[5..])?) as Box<dyn Message>,
            121 => Box::new(Rclunk::decode(&buf[5..])?) as Box<dyn Message>,
            124 => Box::new(Tstat::decode(&buf[5..])?) as Box<dyn Message>,
            125 => Box::new(Rstat::decode(&buf[5..])?) as Box<dyn Message>,
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Unknown message type: {}", msg_type)
                ))
            }
        };

        Ok(msg)
    }
}

/// Base trait for all 9P messages
pub trait Message: Send + Sync {
    fn msg_type(&self) -> MessageType;
    fn tag(&self) -> Tag;
    fn encode(&self, buf: &mut Vec<u8>) -> IoResult<()>;
}

/// File operations supported by the protocol
#[derive(Debug, Clone)]
pub struct FileOps {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
    pub append: bool,
    pub exclusive: bool,
}

impl FileOps {
    pub fn from_mode(mode: u8) -> Self {
        use permissions::*;
        Self {
            read: mode & 0x3 == OREAD || mode & 0x3 == ORDWR,
            write: mode & 0x3 == OWRITE || mode & 0x3 == ORDWR,
            execute: mode & 0x3 == OEXEC,
            append: false, // Set separately via file mode
            exclusive: false,
        }
    }
}

/// Create a root Qid
pub fn root_qid() -> Qid {
    Qid {
        qtype: permissions::DMDIR as u8,
        version: 0,
        path: 0,
    }
}

/// Create a file Qid
pub fn file_qid(path_id: u64, version: u32) -> Qid {
    Qid {
        qtype: 0,
        version,
        path: path_id,
    }
}

/// Create a directory Qid
pub fn dir_qid(path_id: u64, version: u32) -> Qid {
    Qid {
        qtype: permissions::DMDIR as u8,
        version,
        path: path_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qid_creation() {
        let root = root_qid();
        assert_eq!(root.path, 0);
        assert_eq!(root.qtype, permissions::DMDIR as u8);

        let file = file_qid(42, 1);
        assert_eq!(file.path, 42);
        assert_eq!(file.qtype, 0);

        let dir = dir_qid(100, 2);
        assert_eq!(dir.path, 100);
        assert_eq!(dir.qtype, permissions::DMDIR as u8);
    }
}