use async_trait::async_trait;
use std::path::Path;
use anyhow::Result;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use crate::ipc::SharedMemoryHandle;

// --- Consensus Trait ---
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStats {
    pub node_count: u32,
    pub connected_peers: u32,
    pub consensus_score: f64,
}

#[async_trait]
pub trait ConsensusProvider: Send + Sync {
    /// Propose a new block with data
    async fn propose_block(&self, data: Vec<u8>) -> Result<String>; // Returns BlockHash
    
    /// Check if a block or transaction is confirmed
    fn is_confirmed(&self, id: &str) -> bool;
    
    /// Get current network statistics
    fn get_network_stats(&self) -> NetworkStats;
}

// --- Storage Trait ---
// Abstracting the filesystem layer (Synthetic or Real)
#[async_trait]
pub trait StorageProvider: Send + Sync {
    /// Read data from a path
    async fn read(&self, path: &Path, offset: u64, size: u32) -> Result<Vec<u8>>;
    
    /// Write data to a path
    async fn write(&self, path: &Path, offset: u64, data: &[u8]) -> Result<u32>;
    
    /// Get file attributes (simplified for trait)
    async fn stat(&self, path: &Path) -> Result<FileAttr>;
    
    /// List directory contents
    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>>;

    /// Create a directory
    async fn create_dir(&self, path: &Path, mode: u32) -> Result<()>;

    /// Create a file
    async fn create_file(&self, path: &Path, mode: u32) -> Result<()>;

    /// Remove a file
    async fn remove_file(&self, path: &Path) -> Result<()>;

    /// Remove a directory
    async fn remove_dir(&self, path: &Path) -> Result<()>;

    /// Rename a file or directory
    async fn rename(&self, from: &Path, to: &Path) -> Result<()>;

    /// Truncate a file to a specific size
    async fn truncate(&self, path: &Path, size: u64) -> Result<()>;

    /// Set file permissions
    async fn set_permissions(&self, path: &Path, mode: u32) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct FileAttr {
    pub size: u64,
    pub mode: u32,
    pub mtime: u64,
    pub is_dir: bool,
}

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
}

// --- Compute Trait ---
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub is_gpu: bool,
    pub memory: u64,
}

#[derive(Clone, Debug)]
pub struct ComputeJob {
    pub id: String,
    pub job_type: String,
    pub operation: String,
    pub params: Vec<u8>,
    pub shm_handle: Option<SharedMemoryHandle>,
}

#[derive(Clone, Debug)]
pub enum JobStatus {
    Pending,
    Running,
    Completed(Vec<u8>),
    Failed(String),
}

#[async_trait]
pub trait ComputeBackend: Send + Sync {
    /// Discover available compute devices
    fn discover_devices(&self) -> Result<Vec<DeviceInfo>>;
    
    /// Submit a job for execution
    async fn submit_job(&self, job: ComputeJob) -> Result<String>; // Returns JobID
    
    /// Get status of a job
    async fn get_job_status(&self, job_id: &str) -> Option<JobStatus>;
}

// --- WASM Trait ---
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmMetadata {
    pub name: String,
    pub version: String,
    pub description: String,
    pub mount_point: String,
    pub status: String,
}

#[async_trait]
pub trait WasmProvider: Send + Sync {
    /// Load a translator from WASM bytecode
    async fn load_translator(&self, name: String, mount_point: &Path, bytecode: Vec<u8>) -> Result<()>;
    
    /// Remove a translator from a mount point
    async fn remove_translator(&self, mount_point: &Path) -> Result<()>;
    
    /// Get a translator for a given path
    async fn get_translator(&self, path: &Path) -> Option<Arc<dyn Translator>>;
    
    /// Set a translator on a specific path (settrans)
    async fn set_translator(&self, path: &str, translator_name: &str, args: Vec<String>) -> Result<()>;
    
    /// List all installed translators
    async fn list_translators(&self) -> Result<Vec<WasmMetadata>>;
}

#[async_trait]
pub trait Translator: Send + Sync {
    /// Invoke a function on the translator
    async fn invoke_function(&self, function: &str, args: Vec<u8>) -> Result<Vec<u8>>;
    
    /// Get translator name
    fn name(&self) -> String;
    
    /// Get mount point
    fn mount_point(&self) -> String;
}
