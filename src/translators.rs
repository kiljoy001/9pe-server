//! Hurd-style Translators for 9P.e
//!
//! Translators are programs that present filesystem interfaces for non-filesystem resources

use std::collections::HashMap;
use std::sync::Arc;
use std::path::PathBuf;
use tokio::sync::RwLock;
use tokio::process::Command;
use anyhow::{Result, Context};
use async_trait::async_trait;
use serde::{Serialize, Deserialize};

/// Capability for translator access control
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    pub issuer: String,           // Public key ID
    pub subject: String,          // Who can use this
    pub resource: String,         // What resource
    pub permissions: Vec<String>, // read, write, execute
    pub valid_until: u64,         // Unix timestamp
    pub signature: Vec<u8>,       // Ed25519 signature
}

/// Translator isolation level
#[derive(Debug, Clone, Copy)]
pub enum IsolationLevel {
    None,        // Run in same process (unsafe)
    Process,     // Separate process
    Container,   // Docker/podman container
    VM,          // Full VM isolation
    WASM,        // WebAssembly sandbox
}

/// Base trait for all translators
#[async_trait]
pub trait Translator: Send + Sync {
    /// Get translator name
    fn name(&self) -> &str;

    /// Get translator type (http, sql, git, etc.)
    fn translator_type(&self) -> &str;

    /// Get isolation level
    fn isolation(&self) -> IsolationLevel;

    /// Check if translator supports operation
    fn supports(&self, operation: &str) -> bool;

    /// Initialize translator
    async fn init(&mut self) -> Result<()>;

    /// Shutdown translator
    async fn shutdown(&mut self) -> Result<()>;

    /// Read data through translator
    async fn read(&self, path: &str, offset: u64, count: u32) -> Result<Vec<u8>>;

    /// Write data through translator
    async fn write(&self, path: &str, offset: u64, data: Vec<u8>) -> Result<u32>;

    /// List directory through translator
    async fn list(&self, path: &str) -> Result<Vec<String>>;

    /// Get metadata
    async fn stat(&self, path: &str) -> Result<FileInfo>;
}

/// File information returned by translators
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    pub modified: u64,
    pub permissions: u32,
}

/// HTTP translator - presents HTTP resources as files
pub struct HttpTranslator {
    base_url: String,
    client: reqwest::Client,
    cache: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl HttpTranslator {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: reqwest::Client::new(),
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl Translator for HttpTranslator {
    fn name(&self) -> &str { "http_translator" }
    fn translator_type(&self) -> &str { "http" }
    fn isolation(&self) -> IsolationLevel { IsolationLevel::Process }

    fn supports(&self, operation: &str) -> bool {
        matches!(operation, "read" | "list" | "stat")
    }

    async fn init(&mut self) -> Result<()> {
        // Test connection
        self.client.get(&self.base_url).send().await?;
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        self.cache.write().await.clear();
        Ok(())
    }

    async fn read(&self, path: &str, offset: u64, count: u32) -> Result<Vec<u8>> {
        // Check cache first
        if let Some(data) = self.cache.read().await.get(path) {
            let start = offset.min(data.len() as u64) as usize;
            let end = (start + count as usize).min(data.len());
            return Ok(data[start..end].to_vec());
        }

        // Fetch from HTTP
        let url = format!("{}/{}", self.base_url, path);
        let response = self.client.get(&url).send().await?;
        let data = response.bytes().await?.to_vec();

        // Cache it
        self.cache.write().await.insert(path.to_string(), data.clone());

        // Return requested range
        let start = offset.min(data.len() as u64) as usize;
        let end = (start + count as usize).min(data.len());
        Ok(data[start..end].to_vec())
    }

    async fn write(&self, _path: &str, _offset: u64, _data: Vec<u8>) -> Result<u32> {
        Err(anyhow::anyhow!("HTTP translator is read-only"))
    }

    async fn list(&self, path: &str) -> Result<Vec<String>> {
        // Would parse HTML or use REST API
        Ok(vec![])
    }

    async fn stat(&self, path: &str) -> Result<FileInfo> {
        let url = format!("{}/{}", self.base_url, path);
        let response = self.client.head(&url).send().await?;

        Ok(FileInfo {
            name: path.split('/').last().unwrap_or("").to_string(),
            size: response.headers()
                .get("content-length")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            is_dir: false,
            modified: 0,
            permissions: 0o444, // Read-only
        })
    }
}

/// SQL translator - presents database tables as directories
pub struct SqlTranslator {
    connection_string: String,
    isolation: IsolationLevel,
}

impl SqlTranslator {
    pub fn new(connection_string: String) -> Self {
        Self {
            connection_string,
            isolation: IsolationLevel::Process,
        }
    }
}

#[async_trait]
impl Translator for SqlTranslator {
    fn name(&self) -> &str { "sql_translator" }
    fn translator_type(&self) -> &str { "sql" }
    fn isolation(&self) -> IsolationLevel { self.isolation }

    fn supports(&self, operation: &str) -> bool {
        matches!(operation, "read" | "write" | "list" | "stat")
    }

    async fn init(&mut self) -> Result<()> {
        // Would connect to database
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        // Would disconnect
        Ok(())
    }

    async fn read(&self, path: &str, offset: u64, count: u32) -> Result<Vec<u8>> {
        // Parse path as table/row/column
        // Execute SELECT query
        // Return as CSV or JSON
        Ok(b"table_data".to_vec())
    }

    async fn write(&self, path: &str, _offset: u64, data: Vec<u8>) -> Result<u32> {
        // Parse path and data
        // Execute INSERT/UPDATE query
        Ok(data.len() as u32)
    }

    async fn list(&self, path: &str) -> Result<Vec<String>> {
        // List tables or rows
        Ok(vec!["users".to_string(), "posts".to_string()])
    }

    async fn stat(&self, path: &str) -> Result<FileInfo> {
        Ok(FileInfo {
            name: path.to_string(),
            size: 0,
            is_dir: true, // Tables are directories
            modified: 0,
            permissions: 0o755,
        })
    }
}

/// Git translator - presents git repositories as filesystems
pub struct GitTranslator {
    repo_path: PathBuf,
    branch: String,
}

impl GitTranslator {
    pub fn new(repo_path: PathBuf, branch: String) -> Self {
        Self { repo_path, branch }
    }
}

#[async_trait]
impl Translator for GitTranslator {
    fn name(&self) -> &str { "git_translator" }
    fn translator_type(&self) -> &str { "git" }
    fn isolation(&self) -> IsolationLevel { IsolationLevel::Process }

    fn supports(&self, operation: &str) -> bool {
        matches!(operation, "read" | "write" | "list" | "stat")
    }

    async fn init(&mut self) -> Result<()> {
        // Verify git repo exists
        Command::new("git")
            .args(&["rev-parse", "--git-dir"])
            .current_dir(&self.repo_path)
            .output()
            .await
            .context("Not a git repository")?;
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }

    async fn read(&self, path: &str, offset: u64, count: u32) -> Result<Vec<u8>> {
        // Use git show to read file from branch
        let output = Command::new("git")
            .args(&["show", &format!("{}:{}", self.branch, path)])
            .current_dir(&self.repo_path)
            .output()
            .await?;

        if !output.status.success() {
            return Err(anyhow::anyhow!("File not found in git"));
        }

        let data = output.stdout;
        let start = offset.min(data.len() as u64) as usize;
        let end = (start + count as usize).min(data.len());
        Ok(data[start..end].to_vec())
    }

    async fn write(&self, path: &str, _offset: u64, data: Vec<u8>) -> Result<u32> {
        // Write to working directory
        let file_path = self.repo_path.join(path);
        tokio::fs::write(&file_path, &data).await?;

        // Stage the change
        Command::new("git")
            .args(&["add", path])
            .current_dir(&self.repo_path)
            .output()
            .await?;

        Ok(data.len() as u32)
    }

    async fn list(&self, path: &str) -> Result<Vec<String>> {
        let output = Command::new("git")
            .args(&["ls-tree", "--name-only", &self.branch, path])
            .current_dir(&self.repo_path)
            .output()
            .await?;

        let files = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(String::from)
            .collect();

        Ok(files)
    }

    async fn stat(&self, path: &str) -> Result<FileInfo> {
        let output = Command::new("git")
            .args(&["ls-tree", "-l", &self.branch, path])
            .current_dir(&self.repo_path)
            .output()
            .await?;

        let line = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<&str> = line.split_whitespace().collect();

        Ok(FileInfo {
            name: path.split('/').last().unwrap_or("").to_string(),
            size: parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(0),
            is_dir: parts.get(1).map(|&s| s == "tree").unwrap_or(false),
            modified: 0,
            permissions: 0o644,
        })
    }
}

/// WASM translator - runs WebAssembly modules as filesystem handlers
pub struct WasmTranslator {
    module_path: PathBuf,
    runtime: Option<wasmtime::Engine>,
}

impl WasmTranslator {
    pub fn new(module_path: PathBuf) -> Self {
        Self {
            module_path,
            runtime: None,
        }
    }
}

#[async_trait]
impl Translator for WasmTranslator {
    fn name(&self) -> &str { "wasm_translator" }
    fn translator_type(&self) -> &str { "wasm" }
    fn isolation(&self) -> IsolationLevel { IsolationLevel::WASM }

    fn supports(&self, operation: &str) -> bool {
        true // WASM can implement any operation
    }

    async fn init(&mut self) -> Result<()> {
        // Initialize WASM runtime
        self.runtime = Some(wasmtime::Engine::default());
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        self.runtime = None;
        Ok(())
    }

    async fn read(&self, path: &str, offset: u64, count: u32) -> Result<Vec<u8>> {
        // Call WASM module's read function
        Ok(vec![])
    }

    async fn write(&self, path: &str, offset: u64, data: Vec<u8>) -> Result<u32> {
        // Call WASM module's write function
        Ok(data.len() as u32)
    }

    async fn list(&self, path: &str) -> Result<Vec<String>> {
        // Call WASM module's list function
        Ok(vec![])
    }

    async fn stat(&self, path: &str) -> Result<FileInfo> {
        // Call WASM module's stat function
        Ok(FileInfo {
            name: path.to_string(),
            size: 0,
            is_dir: false,
            modified: 0,
            permissions: 0o644,
        })
    }
}

/// Manages all translators in the system
pub struct TranslatorManager {
    translators: Arc<RwLock<HashMap<String, Arc<dyn Translator>>>>,
    mount_points: Arc<RwLock<HashMap<PathBuf, String>>>,
}

impl TranslatorManager {
    pub fn new() -> Self {
        Self {
            translators: Arc::new(RwLock::new(HashMap::new())),
            mount_points: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a translator
    pub async fn register(&self, name: String, translator: Arc<dyn Translator>) -> Result<()> {
        self.translators.write().await.insert(name, translator);
        Ok(())
    }

    /// Mount a translator at a path
    pub async fn mount(&self, path: PathBuf, translator_name: String) -> Result<()> {
        if !self.translators.read().await.contains_key(&translator_name) {
            return Err(anyhow::anyhow!("Translator not found: {}", translator_name));
        }

        self.mount_points.write().await.insert(path, translator_name);
        Ok(())
    }

    /// Get translator for a path
    pub async fn get_translator(&self, path: &PathBuf) -> Option<Arc<dyn Translator>> {
        // Find longest matching mount point
        let mounts = self.mount_points.read().await;
        let mut best_match = None;
        let mut best_len = 0;

        for (mount_path, trans_name) in mounts.iter() {
            if path.starts_with(mount_path) && mount_path.as_os_str().len() > best_len {
                best_match = Some(trans_name.clone());
                best_len = mount_path.as_os_str().len();
            }
        }

        if let Some(name) = best_match {
            self.translators.read().await.get(&name).cloned()
        } else {
            None
        }
    }

    /// List all mount points
    pub async fn list_mounts(&self) -> Vec<(PathBuf, String)> {
        self.mount_points.read().await
            .iter()
            .map(|(p, t)| (p.clone(), t.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_translator_manager() {
        let manager = TranslatorManager::new();

        let http_trans = Arc::new(HttpTranslator::new("http://example.com".to_string()));
        manager.register("http".to_string(), http_trans).await.unwrap();

        manager.mount(PathBuf::from("/http"), "http".to_string()).await.unwrap();

        let trans = manager.get_translator(&PathBuf::from("/http/index.html")).await;
        assert!(trans.is_some());
    }
}