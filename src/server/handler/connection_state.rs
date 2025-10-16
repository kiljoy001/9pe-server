//! Connection state management

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// File handle information
#[derive(Debug, Clone)]
pub struct FileHandle {
    pub fid: u32,
    pub path: String,
    pub mode: u8,
    pub offset: u64,
    pub synthetic: bool,
    pub translator_id: Option<String>,
}

/// Connection state manager
#[derive(Clone)]
pub struct ConnectionState {
    /// Active file handles
    fids: Arc<RwLock<HashMap<u32, FileHandle>>>,

    /// Next available fid
    next_fid: Arc<RwLock<u32>>,
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionState {
    /// Create a new connection state manager
    pub fn new() -> Self {
        Self {
            fids: Arc::new(RwLock::new(HashMap::new())),
            next_fid: Arc::new(RwLock::new(1)),
        }
    }

    /// Add a new file handle
    pub async fn add_fid(&self, fid: u32, handle: FileHandle) {
        let mut fids = self.fids.write().await;
        fids.insert(fid, handle);
    }

    /// Get a file handle by fid
    pub async fn get_fid(&self, fid: u32) -> Option<FileHandle> {
        let fids = self.fids.read().await;
        fids.get(&fid).cloned()
    }

    /// Remove a file handle
    pub async fn remove_fid(&self, fid: u32) -> Option<FileHandle> {
        let mut fids = self.fids.write().await;
        fids.remove(&fid)
    }

    /// Update file offset
    pub async fn update_offset(&self, fid: u32, offset: u64) {
        let mut fids = self.fids.write().await;
        if let Some(handle) = fids.get_mut(&fid) {
            handle.offset = offset;
        }
    }

    /// Get next available fid
    pub async fn next_fid(&self) -> u32 {
        let mut next = self.next_fid.write().await;
        let fid = *next;
        *next += 1;
        fid
    }
}
