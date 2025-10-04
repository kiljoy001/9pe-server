//! 9P.e extension messages
//!
//! Defines extended messages for 9P.e protocol including WASM, consensus, and mesh operations.

use anyhow::Result;
use serde::{Serialize, Deserialize};

/// 9P.e extension message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NinePeeMessage {
    // Basic 9P operations (wrapped)
    Version { msize: u32, version: String },
    Attach { fid: u32, afid: u32, uname: String, aname: String },
    Walk { fid: u32, newfid: u32, wnames: Vec<String> },
    Open { fid: u32, mode: u8 },
    Create { fid: u32, name: String, perm: u32, mode: u8 },
    Read { fid: u32, offset: u64, count: u32 },
    Write { fid: u32, offset: u64, data: Vec<u8> },
    Clunk { fid: u32 },
    Remove { fid: u32 },
    Stat { fid: u32 },
    Wstat { fid: u32, stat: Vec<u8> },

    // 9P.e extensions
    /// WASM translator spawning
    TranslatorSpawn {
        translator_id: u32,
        code: Vec<u8>,
        config: Vec<u8>
    },

    /// WASM translator message
    TranslatorMessage {
        translator_id: u32,
        data: Vec<u8>
    },

    /// Consensus block proposal
    ConsensusPropose {
        block_hash: Vec<u8>,
        parent_hashes: Vec<Vec<u8>>
    },

    /// Synthetic filesystem creation
    SyntheticCreate {
        fid: u32,
        generator: String,
        params: Vec<u8>,
    },

    /// Error response
    Error {
        ename: String,
        errno: u32,
    },

    // Placeholder for future extensions
    Reserved,
}

impl NinePeeMessage {
    /// Convert basic 9P message to NinePee message
    pub fn from_9p_response(fid: u32, success: bool, data: Vec<u8>) -> Self {
        if success {
            Self::Read { fid, offset: 0, count: data.len() as u32 }
        } else {
            Self::Error {
                ename: "Operation failed".to_string(),
                errno: 1,
            }
        }
    }

    /// Check if message is an error
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }

    /// Check if message is a 9P.e extension
    pub fn is_extension(&self) -> bool {
        matches!(self,
            Self::TranslatorSpawn { .. } |
            Self::TranslatorMessage { .. } |
            Self::ConsensusPropose { .. } |
            Self::SyntheticCreate { .. }
        )
    }

    /// Get the FID if this message has one
    pub fn fid(&self) -> Option<u32> {
        match self {
            Self::Attach { fid, .. } |
            Self::Walk { fid, .. } |
            Self::Open { fid, .. } |
            Self::Create { fid, .. } |
            Self::Read { fid, .. } |
            Self::Write { fid, .. } |
            Self::Clunk { fid } |
            Self::Remove { fid } |
            Self::Stat { fid } |
            Self::Wstat { fid, .. } |
            Self::SyntheticCreate { fid, .. } => Some(*fid),
            _ => None,
        }
    }
}

impl Default for NinePeeMessage {
    fn default() -> Self {
        Self::Reserved
    }
}