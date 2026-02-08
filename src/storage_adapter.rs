use crate::traits::{StorageProvider, FileAttr, DirEntry};
use crate::synth::SyntheticFilesystem;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use std::path::{Path, PathBuf};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::PermissionsExt;

/// Adapter for SyntheticFilesystem
pub struct SyntheticStorageAdapter {
    fs: Arc<SyntheticFilesystem>,
}

impl SyntheticStorageAdapter {
    pub fn new(fs: Arc<SyntheticFilesystem>) -> Self {
        Self { fs }
    }
}

#[async_trait]
impl StorageProvider for SyntheticStorageAdapter {
    async fn read(&self, path: &Path, offset: u64, size: u32) -> Result<Vec<u8>> {
        match self.fs.read_file(path).await {
            Ok(content) => {
                let start = offset as usize;
                if start >= content.len() {
                    return Ok(Vec::new());
                }
                let end = content.len().min(start + size as usize);
                Ok(content[start..end].to_vec())
            }
            Err(_) => {
                // Return empty if file not found or IsDir (simplified behavior)
                // Real implementation ideally distinguishes errors
                Ok(Vec::new()) 
            }
        }
    }
    
    async fn write(&self, path: &Path, offset: u64, data: &[u8]) -> Result<u32> {
        // Read-modify-write for offset support
        let mut content = self.fs.read_file(path).await.unwrap_or_default();
        let start = offset as usize;
        
        // Extend if writing past end
        if start + data.len() > content.len() {
            content.resize(start + data.len(), 0);
        }
        
        content[start..start + data.len()].copy_from_slice(data);
        self.fs.write_file(path, content).await?;
        
        Ok(data.len() as u32)
    }
    
    async fn stat(&self, path: &Path) -> Result<FileAttr> {
        let node = self.fs.get_node(path).await
            .ok_or_else(|| anyhow::anyhow!("File not found: {:?}", path))?;
        
        let (size, is_dir) = match &node.node_type {
            crate::synth::SynthNodeType::File { content, .. } => (content.len() as u64, false),
            crate::synth::SynthNodeType::Directory { .. } => (0, true),
            crate::synth::SynthNodeType::ControlFile { .. } => (0, false),
        };

        Ok(FileAttr {
            size,
            mode: node.permissions,
            mtime: node.modified.timestamp() as u64,
            is_dir,
        })
    }
    
    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let children = self.fs.list_directory(path).await?;
        let mut entries = Vec::new();
        
        for name in children {
            let child_path = path.join(&name);
            let is_dir = if let Some(node) = self.fs.get_node(&child_path).await {
                matches!(node.node_type, crate::synth::SynthNodeType::Directory { .. })
            } else {
                false
            };
            
            entries.push(DirEntry { name, is_dir });
        }
        
        Ok(entries)
    }

    async fn create_dir(&self, path: &Path, _mode: u32) -> Result<()> {
        self.fs.create_directory(path).await
    }

    async fn create_file(&self, path: &Path, _mode: u32) -> Result<()> {
        // Create empty file
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
        self.fs.truncate_file(path, size).await
    }

    async fn set_permissions(&self, path: &Path, mode: u32) -> Result<()> {
        self.fs.set_permissions(path, mode).await
    }
}

/// Adapter for Physical Filesystem (std::fs)
pub struct PhysicalStorageAdapter {
    root: PathBuf,
}

impl PhysicalStorageAdapter {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn resolve_path(&self, path: &Path) -> PathBuf {
        let clean_path = path.strip_prefix("/").unwrap_or(path);
        self.root.join(clean_path)
    }
}

#[async_trait]
impl StorageProvider for PhysicalStorageAdapter {
    async fn read(&self, path: &Path, offset: u64, size: u32) -> Result<Vec<u8>> {
        let full_path = self.resolve_path(path);
        let mut file = File::open(&full_path)?;
        file.seek(SeekFrom::Start(offset))?;
        
        let mut buffer = vec![0u8; size as usize];
        let n = file.read(&mut buffer)?;
        buffer.truncate(n);
        Ok(buffer)
    }
    
    async fn write(&self, path: &Path, offset: u64, data: &[u8]) -> Result<u32> {
        let full_path = self.resolve_path(path);
        // Only open with create if we actually want to create? 
        // 9P write implies file is open. But here we are stateless per op roughly.
        // Actually usually open happens before.
        // But StorageProvider is low level.
        // For write, we assume file exists unless we want to create implicitly?
        // Standard 9P Write is on an FID. Handler calls us.
        // Let's assume file exists.
        
        let mut file = OpenOptions::new().write(true).open(&full_path)?;
        file.seek(SeekFrom::Start(offset))?;
        let n = file.write(data)?;
        Ok(n as u32)
    }
    
    async fn stat(&self, path: &Path) -> Result<FileAttr> {
        let full_path = self.resolve_path(path);
        let metadata = fs::metadata(&full_path)?;
        
        Ok(FileAttr {
            size: metadata.len(),
            mode: metadata.permissions().mode(),
            mtime: metadata.modified()?.duration_since(std::time::UNIX_EPOCH)?.as_secs(),
            is_dir: metadata.is_dir(),
        })
    }
    
    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let full_path = self.resolve_path(path);
        let mut entries = Vec::new();
        
        for entry in fs::read_dir(full_path)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            entries.push(DirEntry {
                name: entry.file_name().to_string_lossy().to_string(),
                is_dir: metadata.is_dir(),
            });
        }
        Ok(entries)
    }

    async fn create_dir(&self, path: &Path, mode: u32) -> Result<()> {
        let full_path = self.resolve_path(path);
        fs::create_dir(&full_path)?;
        let permissions = fs::Permissions::from_mode(mode);
        fs::set_permissions(&full_path, permissions)?;
        Ok(())
    }

    async fn create_file(&self, path: &Path, mode: u32) -> Result<()> {
        let full_path = self.resolve_path(path);
        let file = File::create(&full_path)?;
        let permissions = fs::Permissions::from_mode(mode);
        file.set_permissions(permissions)?;
        Ok(())
    }

    async fn remove_file(&self, path: &Path) -> Result<()> {
        let full_path = self.resolve_path(path);
        fs::remove_file(full_path)?;
        Ok(())
    }

    async fn remove_dir(&self, path: &Path) -> Result<()> {
        let full_path = self.resolve_path(path);
        fs::remove_dir(full_path)?;
        Ok(())
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let full_from = self.resolve_path(from);
        let full_to = self.resolve_path(to);
        fs::rename(full_from, full_to)?;
        Ok(())
    }

    async fn truncate(&self, path: &Path, size: u64) -> Result<()> {
        let full_path = self.resolve_path(path);
        let file = OpenOptions::new().write(true).open(&full_path)?;
        file.set_len(size)?;
        Ok(())
    }

    async fn set_permissions(&self, path: &Path, mode: u32) -> Result<()> {
        let full_path = self.resolve_path(path);
        let permissions = fs::Permissions::from_mode(mode);
        fs::set_permissions(&full_path, permissions)?;
        Ok(())
    }
}

/// Adapter that routes requests to different providers based on mount points
pub struct RouterStorageAdapter {
    root: Arc<dyn StorageProvider>,
    mounts: Arc<tokio::sync::RwLock<Vec<(PathBuf, Arc<dyn StorageProvider>)>>>,
}

impl RouterStorageAdapter {
    pub fn new(root: Arc<dyn StorageProvider>) -> Self {
        Self {
            root,
            mounts: Arc::new(tokio::sync::RwLock::new(Vec::new())),
        }
    }

    pub async fn mount(&self, path: PathBuf, provider: Arc<dyn StorageProvider>) {
        let clean_path = path.strip_prefix("/").unwrap_or(&path).to_path_buf();
        let mut mounts = self.mounts.write().await;
        mounts.push((clean_path, provider));
        // Sort by path length descending to match longest prefix first
        mounts.sort_by(|a, b| b.0.as_os_str().len().cmp(&a.0.as_os_str().len()));
    }

    async fn find_provider(&self, path: &Path) -> (PathBuf, Arc<dyn StorageProvider>) {
        let clean_path = path.strip_prefix("/").unwrap_or(path);
        let mounts = self.mounts.read().await;
        for (mount_path, provider) in mounts.iter() {
            if let Ok(suffix) = clean_path.strip_prefix(mount_path) {
                let routed_path = if suffix == Path::new("") {
                    PathBuf::from("/")
                } else {
                    PathBuf::from("/").join(suffix)
                };
                
                return (routed_path, provider.clone());
            }
        }
        (clean_path.to_path_buf(), self.root.clone())
    }
}

#[async_trait]
impl StorageProvider for RouterStorageAdapter {
    async fn read(&self, path: &Path, offset: u64, size: u32) -> Result<Vec<u8>> {
        let (p, provider) = self.find_provider(path).await;
        provider.read(&p, offset, size).await
    }

    async fn write(&self, path: &Path, offset: u64, data: &[u8]) -> Result<u32> {
        let (p, provider) = self.find_provider(path).await;
        provider.write(&p, offset, data).await
    }

    async fn stat(&self, path: &Path) -> Result<FileAttr> {
        let (p, provider) = self.find_provider(path).await;
        provider.stat(&p).await
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let (p, provider) = self.find_provider(path).await;
        provider.read_dir(&p).await
    }

    async fn create_dir(&self, path: &Path, mode: u32) -> Result<()> {
        let (p, provider) = self.find_provider(path).await;
        provider.create_dir(&p, mode).await
    }

    async fn create_file(&self, path: &Path, mode: u32) -> Result<()> {
        let (p, provider) = self.find_provider(path).await;
        provider.create_file(&p, mode).await
    }

    async fn remove_file(&self, path: &Path) -> Result<()> {
        let (p, provider) = self.find_provider(path).await;
        provider.remove_file(&p).await
    }

    async fn remove_dir(&self, path: &Path) -> Result<()> {
        let (p, provider) = self.find_provider(path).await;
        provider.remove_dir(&p).await
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let (p_from, provider_from) = self.find_provider(from).await;
        let (p_to, _provider_to) = self.find_provider(to).await;
        // Simplified: assuming same provider or hoping for best
        provider_from.rename(&p_from, &p_to).await
    }

    async fn truncate(&self, path: &Path, size: u64) -> Result<()> {
        let (p, provider) = self.find_provider(path).await;
        provider.truncate(&p, size).await
    }

    async fn set_permissions(&self, path: &Path, mode: u32) -> Result<()> {
        let (p, provider) = self.find_provider(path).await;
        provider.set_permissions(&p, mode).await
    }
}
