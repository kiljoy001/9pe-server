//! Hypercore Translator Bridge
//!
//! Bridges 9P filesystem operations to Hypercore feeds.
//! - Reads are served from the feed.
//! - Writes are appended to the feed (if writable).
//! - Public keys map to virtual directories.

use anyhow::{Context, Result};
use hypercore::{Hypercore, HypercoreBuilder, Storage};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use async_trait::async_trait;

use crate::traits::{StorageProvider, FileAttr, DirEntry};
use crate::synth::{SyntheticFilesystem, SynthNode, SynthNodeType};

/// Hypercore bridge configuration
pub struct HypercoreConfig {
    pub storage_path: PathBuf,
}

/// Hypercore translator implementing StorageProvider
pub struct HypercoreBridge {
    config: HypercoreConfig,
    /// Active feeds mapped by public key (hex)
    feeds: Arc<RwLock<HashMap<String, Arc<Mutex<Hypercore>>>>>,
    /// Underlying synthetic filesystem for structural nodes
    fs: SyntheticFilesystem,
}

impl HypercoreBridge {
    pub fn new(config: HypercoreConfig) -> Self {
        Self {
            config,
            feeds: Arc::new(RwLock::new(HashMap::new())),
            fs: SyntheticFilesystem::new(),
        }
    }

    /// Open or create a feed
    pub async fn open_feed(&self, key: &str) -> Result<()> {
        let mut feeds = self.feeds.write().await;
        if feeds.contains_key(key) {
            return Ok(());
        }

        let path = self.config.storage_path.join(key);
        tokio::fs::create_dir_all(&path).await?;
        
        let storage = Storage::new_disk(&path, false).await?;

        // Initialize feed using Builder
        let feed = HypercoreBuilder::new(storage).build().await?;
        
        feeds.insert(key.to_string(), Arc::new(Mutex::new(feed)));
        
        // Ensure virtual directory structure exists
        // /<key>/
        let key_path = PathBuf::from("/").join(key);
        self.fs.create_directory(&key_path).await?;
        
        // /<key>/append (virtual file for appending)
        self.fs.create_file(&key_path.join("append"), Vec::new(), true).await?;
        
        // /<key>/info (virtual file for stats)
        self.fs.create_file(&key_path.join("info"), b"Status: Ready\n".to_vec(), false).await?;
        
        Ok(())
    }

    /// Read data from a feed
    pub async fn read_feed_item(&self, key: &str, index: u64) -> Result<Option<Vec<u8>>> {
        let feeds = self.feeds.read().await;
        if let Some(feed_mutex) = feeds.get(key) {
            let mut feed = feed_mutex.lock().await;
            return Ok(feed.get(index).await?);
        }
        Ok(None)
    }

    /// Append data to a feed
    pub async fn append_to_feed(&self, key: &str, data: &[u8]) -> Result<u64> {
        let feeds = self.feeds.read().await;
        if let Some(feed_mutex) = feeds.get(key) {
            let mut feed = feed_mutex.lock().await;
            let outcome = feed.append(data).await?;
            return Ok(outcome.length);
        }
        anyhow::bail!("Feed not found: {}", key)
    }
    
    // Helper to parse path: /<key>/<item>
    // Returns (key, item_type)
    fn parse_path(&self, path: &Path) -> Option<(String, String)> {
        let mut components = path.components();
        // Skip root if absolute
        if path.is_absolute() {
            components.next(); 
        }
        
        let key = components.next()?.as_os_str().to_str()?.to_string();
        let item = components.next().map(|s| s.as_os_str().to_str().unwrap_or("").to_string()).unwrap_or_default();
        
        Some((key, item))
    }
}

#[async_trait]
impl StorageProvider for HypercoreBridge {
    async fn read(&self, path: &Path, offset: u64, size: u32) -> Result<Vec<u8>> {
        // Special case: /<key>/<number> read from feed
        if let Some((key, item)) = self.parse_path(path) {
            if let Ok(index) = item.parse::<u64>() {
                if let Ok(Some(data)) = self.read_feed_item(&key, index).await {
                     // Handle offset/size
                     if offset as usize >= data.len() {
                         return Ok(Vec::new());
                     }
                     let end = std::cmp::min(offset as usize + size as usize, data.len());
                     return Ok(data[offset as usize..end].to_vec());
                }
            }
        }
        
        // Fallback to synthetic FS for "append", "info", or other files
        self.fs.read_file(path).await
    }

    async fn write(&self, path: &Path, _offset: u64, data: &[u8]) -> Result<u32> {
        // Check for specific writes
         if let Some((key, item)) = self.parse_path(path) {
             if item == "append" {
                 // Open feed if not opened?
                 if !self.feeds.read().await.contains_key(&key) {
                     self.open_feed(&key).await?;
                 }
                 
                 self.append_to_feed(&key, data).await?;
                 return Ok(data.len() as u32);
             }
         }
         
         // Synthetic FS write
         self.fs.write_file(path, data.to_vec()).await?;
         Ok(data.len() as u32)
    }

    async fn stat(&self, path: &Path) -> Result<FileAttr> {
        // Dynamic stat for feed items
        if let Some((key, item)) = self.parse_path(path) {
            if let Ok(index) = item.parse::<u64>() {
                // Check if index exists
                if let Ok(Some(data)) = self.read_feed_item(&key, index).await {
                     return Ok(FileAttr {
                         size: data.len() as u64,
                         mode: 0o444, // Read only
                         mtime: 0,
                         is_dir: false,
                     });
                }
            }
        }
        
        // Populate "info" with real stats on stat()
        if let Some((key, item)) = self.parse_path(path) {
            if item == "info" {
                 if let Some(feed_mutex) = self.feeds.read().await.get(&key) {
                     let feed = feed_mutex.lock().await;
                     // Try accessing info.length
                     let info = format!("Length: {}\nByteLength: {}\n", feed.info().length, feed.info().byte_length);
                     // Update synth file
                     let _ = self.fs.write_file(path, info.into_bytes()).await;
                 }
            }
        }

        // Manual stat from SynthNode
        let node = self.fs.get_node(path).await.ok_or_else(|| anyhow::anyhow!("File not found"))?;
        let is_dir = matches!(node.node_type, SynthNodeType::Directory { .. });
        Ok(FileAttr {
            size: if is_dir { 0 } else { 
                match node.node_type {
                    SynthNodeType::File { content, .. } => content.len() as u64,
                    _ => 0,
                }
            },
            mode: node.permissions,
            mtime: node.modified.timestamp() as u64,
            is_dir,
        })
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let names = self.fs.list_directory(path).await.unwrap_or_default();
        let mut entries = Vec::new();
        
        for name in names {
             let child_path = path.join(&name);
             let is_dir = if let Some(node) = self.fs.get_node(&child_path).await {
                  matches!(node.node_type, SynthNodeType::Directory { .. })
             } else {
                 false
             };
             entries.push(DirEntry { name, is_dir });
        }
        
        // If listing a key directory, add feed items
        if let Some((key, item)) = self.parse_path(path) {
            if item.is_empty() { // listing /<key>/
                 // Ensure feed is open/loaded
                 if !self.feeds.read().await.contains_key(&key) {
                     // Attempt to open? Or we assume it's opened explicitly.
                     // For now, assume explicit open or previously opened.
                 }
                 
                 if let Some(feed_mutex) = self.feeds.read().await.get(&key) {
                     let feed = feed_mutex.lock().await;
                     let len = feed.info().length;
                     for i in 0..len {
                         entries.push(DirEntry {
                             name: i.to_string(),
                             is_dir: false,
                         });
                     }
                 }
            }
        }
        
        Ok(entries)
    }

    async fn create_dir(&self, path: &Path, mode: u32) -> Result<()> {
        // If creating a top level directory, it means "open this feed"
        // e.g. mkdir /key
        if path.components().count() == 2 {
            let key = path.file_name().unwrap().to_string_lossy().to_string();
            self.open_feed(&key).await?;
        }
        self.fs.create_directory(path).await
    }

    async fn create_file(&self, path: &Path, mode: u32) -> Result<()> {
        self.fs.create_file(path, Vec::new(), true).await
    }

    async fn remove_file(&self, path: &Path) -> Result<()> {
        self.fs.remove_node(path).await
    }

    async fn remove_dir(&self, path: &Path) -> Result<()> {
        self.fs.remove_node(path).await
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        self.fs.rename_node(from, to).await
    }

    async fn truncate(&self, path: &Path, size: u64) -> Result<()> {
         // Cannot truncate feed items
        self.fs.truncate_file(path, size).await
    }

    async fn set_permissions(&self, path: &Path, mode: u32) -> Result<()> {
        self.fs.set_permissions(path, mode).await
    }
}
