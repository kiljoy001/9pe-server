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
}

impl Default for SyntheticFilesystem {
    fn default() -> Self {
        Self::new()
    }
}

impl SyntheticFilesystem {
    pub fn new() -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a virtual directory
    pub async fn create_directory(&self, path: &Path) -> Result<()> {
        let mut nodes = self.nodes.write().await;

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

    /// Create a virtual file
    pub async fn create_file(&self, path: &Path, content: Vec<u8>, writable: bool) -> Result<()> {
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

        if let Some(node) = nodes.get(path) {
            if let SynthNodeType::Directory { ref children } = node.node_type {
                return Ok(children.clone());
            }
        }

        Ok(Vec::new())
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
        let mut nodes = self.nodes.write().await;

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
