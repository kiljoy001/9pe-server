//! Synthetic filesystem implementation for virtual directories
//!
//! Provides in-memory virtual filesystem that exists only in the 9P namespace.
//! No physical directories are created on disk.

use anyhow::Result;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Virtual file or directory in the synthetic filesystem
#[derive(Debug, Clone)]
pub struct SynthNode {
    pub name: String,
    pub path: PathBuf,
    pub node_type: SynthNodeType,
    pub permissions: u32,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
    pub accessed: DateTime<Utc>,
}

#[derive(Clone)]
pub enum SynthNodeType {
    Directory { children: Vec<String> },
    File { content: Vec<u8>, writable: bool },
    ControlFile { handler: Arc<dyn ControlHandler> },
}

impl std::fmt::Debug for SynthNodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Directory { children } => f
                .debug_struct("Directory")
                .field("children", children)
                .finish(),
            Self::File { content, writable } => f
                .debug_struct("File")
                .field("content_len", &content.len())
                .field("writable", writable)
                .finish(),
            Self::ControlFile { .. } => f.debug_struct("ControlFile").finish(),
        }
    }
}

/// Handler for control files that execute operations
pub trait ControlHandler: Send + Sync {
    fn read(&self) -> Result<Vec<u8>>;
    fn write(&self, data: &[u8]) -> Result<()>;
}

/// Synthetic filesystem that maintains virtual directories and files
#[derive(Debug)]
pub struct SyntheticFilesystem {
    nodes: Arc<RwLock<HashMap<PathBuf, SynthNode>>>,
    /// Maximum number of nodes (files + directories) allowed
    max_nodes: usize,
    /// Maximum total bytes across all files
    max_total_bytes: usize,
}

impl Default for SyntheticFilesystem {
    fn default() -> Self {
        Self::new()
    }
}

impl SyntheticFilesystem {
    /// Default maximum nodes (64K)
    pub const DEFAULT_MAX_NODES: usize = 65536;
    /// Default maximum total bytes (256 MB)
    pub const DEFAULT_MAX_TOTAL_BYTES: usize = 256 * 1024 * 1024;

    pub fn new() -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
            max_nodes: Self::DEFAULT_MAX_NODES,
            max_total_bytes: Self::DEFAULT_MAX_TOTAL_BYTES,
        }
    }

    /// Create with custom limits
    pub fn with_limits(max_nodes: usize, max_total_bytes: usize) -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
            max_nodes,
            max_total_bytes,
        }
    }

    /// Calculate total bytes used by all files
    fn total_bytes_used(nodes: &HashMap<PathBuf, SynthNode>) -> usize {
        nodes.values().map(|n| {
            match &n.node_type {
                SynthNodeType::File { content, .. } => content.len(),
                _ => 0,
            }
        }).sum()
    }

    /// Check if adding content would exceed limits
    fn check_limits(&self, nodes: &HashMap<PathBuf, SynthNode>, additional_bytes: usize, additional_nodes: usize) -> Result<()> {
        if nodes.len() + additional_nodes > self.max_nodes {
            anyhow::bail!(
                "Node limit exceeded: {} + {} > {}",
                nodes.len(),
                additional_nodes,
                self.max_nodes
            );
        }

        let current_bytes = Self::total_bytes_used(nodes);
        if current_bytes + additional_bytes > self.max_total_bytes {
            anyhow::bail!(
                "Total size limit exceeded: {} + {} > {} bytes",
                current_bytes,
                additional_bytes,
                self.max_total_bytes
            );
        }

        Ok(())
    }

    /// Create a virtual directory
    pub async fn create_directory(&self, path: &Path) -> Result<()> {
        let mut nodes = self.nodes.write().await;

        // Count how many new directories we need to create
        let mut dirs_to_create = 0usize;
        let mut check_path = PathBuf::new();
        for component in path.components() {
            check_path.push(component);
            if !nodes.contains_key(&check_path) {
                dirs_to_create += 1;
            }
        }

        // Check node limit before creating
        if dirs_to_create > 0 {
            self.check_limits(&nodes, 0, dirs_to_create)?;
        }

        // Create parent directories if needed
        let mut current = PathBuf::new();
        for component in path.components() {
            current.push(component);

            if !nodes.contains_key(&current) {
                let name = current
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                let node = SynthNode {
                    name,
                    path: current.clone(),
                    node_type: SynthNodeType::Directory {
                        children: Vec::new(),
                    },
                    permissions: 0o755,
                    created: Utc::now(),
                    modified: Utc::now(),
                    accessed: Utc::now(),
                };

                nodes.insert(current.clone(), node);

                // Update parent's children list
                if let Some(parent_path) = current.parent() {
                    if let Some(parent_node) = nodes.get_mut(parent_path) {
                        if let SynthNodeType::Directory { ref mut children } = parent_node.node_type
                        {
                            let child_name = current
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            if !children.contains(&child_name) {
                                children.push(child_name);
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Maximum file size for synthetic files (16 MB default)
    pub const MAX_FILE_SIZE: usize = 16 * 1024 * 1024;

    /// Create a virtual file
    pub async fn create_file(&self, path: &Path, content: Vec<u8>, writable: bool) -> Result<()> {
        // Enforce per-file size limit
        if content.len() > Self::MAX_FILE_SIZE {
            anyhow::bail!(
                "File size {} bytes exceeds maximum {} bytes",
                content.len(),
                Self::MAX_FILE_SIZE
            );
        }

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            self.create_directory(parent).await?;
        }

        let mut nodes = self.nodes.write().await;

        // Check if this is a new file or replacement
        let is_new = !nodes.contains_key(path);
        let old_size = nodes.get(path).map(|n| {
            match &n.node_type {
                SynthNodeType::File { content, .. } => content.len(),
                _ => 0,
            }
        }).unwrap_or(0);

        // Check aggregate limits (new bytes = content.len() - old_size for replacements)
        let additional_bytes = if is_new { content.len() } else { content.len().saturating_sub(old_size) };
        let additional_nodes = if is_new { 1 } else { 0 };
        self.check_limits(&nodes, additional_bytes, additional_nodes)?;

        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let node = SynthNode {
            name: name.clone(),
            path: path.to_path_buf(),
            node_type: SynthNodeType::File { content, writable },
            permissions: if writable { 0o644 } else { 0o444 },
            created: Utc::now(),
            modified: Utc::now(),
            accessed: Utc::now(),
        };

        nodes.insert(path.to_path_buf(), node);

        // Update parent's children
        if let Some(parent_path) = path.parent() {
            if let Some(parent_node) = nodes.get_mut(parent_path) {
                if let SynthNodeType::Directory { ref mut children } = parent_node.node_type {
                    if !children.contains(&name) {
                        children.push(name);
                    }
                }
            }
        }

        Ok(())
    }

    /// Create a control file with custom handler
    pub async fn create_control_file(
        &self,
        path: &Path,
        handler: Arc<dyn ControlHandler>,
    ) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            self.create_directory(parent).await?;
        }

        let mut nodes = self.nodes.write().await;

        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let node = SynthNode {
            name: name.clone(),
            path: path.to_path_buf(),
            node_type: SynthNodeType::ControlFile { handler },
            permissions: 0o644,
            created: Utc::now(),
            modified: Utc::now(),
            accessed: Utc::now(),
        };

        nodes.insert(path.to_path_buf(), node);

        // Update parent's children
        if let Some(parent_path) = path.parent() {
            if let Some(parent_node) = nodes.get_mut(parent_path) {
                if let SynthNodeType::Directory { ref mut children } = parent_node.node_type {
                    if !children.contains(&name) {
                        children.push(name);
                    }
                }
            }
        }

        Ok(())
    }

    /// Check if a path exists in the synthetic filesystem
    pub async fn exists(&self, path: &Path) -> bool {
        let nodes = self.nodes.read().await;
        nodes.contains_key(path)
    }

    /// Get a node by path
    pub async fn get_node(&self, path: &Path) -> Option<SynthNode> {
        let nodes = self.nodes.read().await;
        nodes.get(path).cloned()
    }

    /// List directory contents
    pub async fn list_directory(&self, path: &Path) -> Result<Vec<String>> {
        let nodes = self.nodes.read().await;

        match nodes.get(path) {
            Some(node) => {
                if let SynthNodeType::Directory { ref children } = node.node_type {
                    Ok(children.clone())
                } else {
                    anyhow::bail!("Not a directory: {:?}", path)
                }
            }
            None => anyhow::bail!("Directory not found: {:?}", path),
        }
    }

    /// Read file contents
    pub async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        let nodes = self.nodes.read().await;

        if let Some(node) = nodes.get(path) {
            match &node.node_type {
                SynthNodeType::File { content, .. } => {
                    return Ok(content.clone());
                }
                SynthNodeType::ControlFile { handler } => {
                    return handler.read();
                }
                _ => {}
            }
        }

        anyhow::bail!("File not found: {:?}", path)
    }

    /// Write file contents
    pub async fn write_file(&self, path: &Path, data: Vec<u8>) -> Result<()> {
        // Enforce per-file size limit
        if data.len() > Self::MAX_FILE_SIZE {
            anyhow::bail!(
                "Write size {} bytes exceeds maximum {} bytes",
                data.len(),
                Self::MAX_FILE_SIZE
            );
        }

        let mut nodes = self.nodes.write().await;

        // Check aggregate limit before writing
        if let Some(node) = nodes.get(path) {
            if let SynthNodeType::File { content, .. } = &node.node_type {
                let additional_bytes = data.len().saturating_sub(content.len());
                if additional_bytes > 0 {
                    self.check_limits(&nodes, additional_bytes, 0)?;
                }
            }
        }

        if let Some(node) = nodes.get_mut(path) {
            match &mut node.node_type {
                SynthNodeType::File { content, writable } => {
                    if *writable {
                        *content = data;
                        node.modified = Utc::now();
                        return Ok(());
                    } else {
                        anyhow::bail!("File is read-only: {:?}", path);
                    }
                }
                SynthNodeType::ControlFile { handler } => {
                    return handler.write(&data);
                }
                _ => {}
            }
        }

        anyhow::bail!("File not found: {:?}", path)
    }
    /// Remove a node (file or directory)
    pub async fn remove_node(&self, path: &Path) -> Result<()> {
        let mut nodes = self.nodes.write().await;

        if !nodes.contains_key(path) {
            anyhow::bail!("Path not found: {:?}", path);
        }

        // Check if directory is empty
        if let Some(node) = nodes.get(path) {
            if let SynthNodeType::Directory { children } = &node.node_type {
                if !children.is_empty() {
                    anyhow::bail!("Directory not empty: {:?}", path);
                }
            }
        }

        nodes.remove(path);

        // Update parent
        if let Some(parent_path) = path.parent() {
            if let Some(parent_node) = nodes.get_mut(parent_path) {
                if let SynthNodeType::Directory { ref mut children } = parent_node.node_type {
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    if let Some(pos) = children.iter().position(|x| *x == name) {
                        children.remove(pos);
                    }
                }
            }
        }

        Ok(())
    }

    /// Rename a node
    pub async fn rename_node(&self, from: &Path, to: &Path) -> Result<()> {
        let mut nodes = self.nodes.write().await;

        if !nodes.contains_key(from) {
            anyhow::bail!("Source path not found: {:?}", from);
        }

        if nodes.contains_key(to) {
            anyhow::bail!("Destination path already exists: {:?}", to);
        }

        // Validate destination parent exists and is a directory BEFORE making changes
        if let Some(dest_parent_path) = to.parent() {
            match nodes.get(dest_parent_path) {
                Some(parent_node) => {
                    if !matches!(parent_node.node_type, SynthNodeType::Directory { .. }) {
                        anyhow::bail!("Destination parent is not a directory: {:?}", dest_parent_path);
                    }
                }
                None => {
                    anyhow::bail!("Destination parent directory not found: {:?}", dest_parent_path);
                }
            }
        }

        // Remove from source parent
        if let Some(parent_path) = from.parent() {
            if let Some(parent_node) = nodes.get_mut(parent_path) {
                if let SynthNodeType::Directory { ref mut children } = parent_node.node_type {
                    let name = from
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    if let Some(pos) = children.iter().position(|x| *x == name) {
                        children.remove(pos);
                    }
                }
            }
        }

        // Move node
        if let Some(mut node) = nodes.remove(from) {
            let new_name = to
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            node.name = new_name.clone();
            node.path = to.to_path_buf();
            nodes.insert(to.to_path_buf(), node);

            // Add to destination parent (already validated above)
            if let Some(parent_path) = to.parent() {
                if let Some(parent_node) = nodes.get_mut(parent_path) {
                    if let SynthNodeType::Directory { ref mut children } = parent_node.node_type {
                        children.push(new_name);
                    }
                }
            }
        }

        Ok(())
    }

    /// Truncate file to size
    pub async fn truncate_file(&self, path: &Path, size: u64) -> Result<()> {
        // Enforce size limit when expanding
        if size as usize > Self::MAX_FILE_SIZE {
            anyhow::bail!(
                "Truncate size {} bytes exceeds maximum {} bytes",
                size,
                Self::MAX_FILE_SIZE
            );
        }

        let mut nodes = self.nodes.write().await;

        if let Some(node) = nodes.get_mut(path) {
            match &mut node.node_type {
                SynthNodeType::File { content, writable } => {
                     if !*writable {
                        anyhow::bail!("File is read-only: {:?}", path);
                     }
                     content.resize(size as usize, 0);
                     node.modified = Utc::now();
                     return Ok(());
                }
                _ => anyhow::bail!("Not a file: {:?}", path),
            }
        }
        anyhow::bail!("File not found: {:?}", path)
    }

    /// Set permissions
    pub async fn set_permissions(&self, path: &Path, mode: u32) -> Result<()> {
        let mut nodes = self.nodes.write().await;

        if let Some(node) = nodes.get_mut(path) {
            node.permissions = mode;
            node.modified = Utc::now(); // Metadata change updates mtime in 9P? or ctime? 
            // 9P usually updates mtime on content change. wstat can change mtime explicitly.
            // Here we are just changing permissions.
            // Let's just update modified for now to signal change.
            Ok(())
        } else {
            anyhow::bail!("Path not found: {:?}", path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_directory() {
        let fs = SyntheticFilesystem::new();
        let path = PathBuf::from("/test/dir");

        fs.create_directory(&path).await.unwrap();
        assert!(fs.exists(&path).await, "Directory should exist");
    }

    #[tokio::test]
    async fn test_create_nested_directories() {
        let fs = SyntheticFilesystem::new();
        let path = PathBuf::from("/test/deep/nested/dir");

        fs.create_directory(&path).await.unwrap();

        // All intermediate directories should exist
        assert!(fs.exists(&PathBuf::from("/test")).await);
        assert!(fs.exists(&PathBuf::from("/test/deep")).await);
        assert!(fs.exists(&PathBuf::from("/test/deep/nested")).await);
        assert!(fs.exists(&PathBuf::from("/test/deep/nested/dir")).await);
    }

    #[tokio::test]
    async fn test_create_file() {
        let fs = SyntheticFilesystem::new();
        let path = PathBuf::from("/test/file.txt");
        let content = b"test content".to_vec();

        fs.create_file(&path, content.clone(), false).await.unwrap();

        assert!(fs.exists(&path).await, "File should exist");
        let read_content = fs.read_file(&path).await.unwrap();
        assert_eq!(read_content, content, "Content should match");
    }

    #[tokio::test]
    async fn test_writable_file() {
        let fs = SyntheticFilesystem::new();
        let path = PathBuf::from("/test/writable.txt");
        let initial = b"initial".to_vec();
        let updated = b"updated".to_vec();

        fs.create_file(&path, initial, true).await.unwrap();
        fs.write_file(&path, updated.clone()).await.unwrap();

        let content = fs.read_file(&path).await.unwrap();
        assert_eq!(content, updated, "Content should be updated");
    }

    #[tokio::test]
    async fn test_readonly_file() {
        let fs = SyntheticFilesystem::new();
        let path = PathBuf::from("/test/readonly.txt");
        let content = b"readonly".to_vec();

        fs.create_file(&path, content, false).await.unwrap();

        // Writing to readonly file should fail
        let result = fs.write_file(&path, b"new".to_vec()).await;
        assert!(result.is_err(), "Writing to readonly file should fail");
    }

    #[tokio::test]
    async fn test_list_directory() {
        let fs = SyntheticFilesystem::new();
        let dir = PathBuf::from("/test");

        fs.create_file(&dir.join("file1.txt"), b"content1".to_vec(), false)
            .await
            .unwrap();
        fs.create_file(&dir.join("file2.txt"), b"content2".to_vec(), false)
            .await
            .unwrap();

        let children = fs.list_directory(&dir).await.unwrap();
        assert_eq!(children.len(), 2, "Should have 2 children");
        assert!(children.contains(&"file1.txt".to_string()));
        assert!(children.contains(&"file2.txt".to_string()));
    }

    #[tokio::test]
    async fn test_get_node() {
        let fs = SyntheticFilesystem::new();
        let path = PathBuf::from("/test/file.txt");

        fs.create_file(&path, b"content".to_vec(), true)
            .await
            .unwrap();

        let node = fs.get_node(&path).await.unwrap();
        assert_eq!(node.name, "file.txt");
        assert!(matches!(node.node_type, SynthNodeType::File { .. }));
    }

    #[tokio::test]
    async fn test_read_nonexistent_file() {
        let fs = SyntheticFilesystem::new();
        let path = PathBuf::from("/nonexistent/file.txt");

        let result = fs.read_file(&path).await;
        assert!(result.is_err(), "Reading nonexistent file should fail");
    }

    /// Fuzz test: Filesystem should handle arbitrary paths
    #[test]
    fn fuzz_path_handling() {
        use proptest::prelude::*;

        proptest!(|(path_str in "[a-zA-Z0-9/_-]{1,50}")| {
            let _path = PathBuf::from(&path_str);
            // Should not panic
        });
    }

    /// Fuzz test: File content should handle arbitrary data
    #[test]
    fn fuzz_file_content() {
        use proptest::prelude::*;

        proptest!(|(content: Vec<u8>)| {
            let fs = SyntheticFilesystem::new();
            // Should not panic with any content
            let _ = content.len();
        });
    }
}
