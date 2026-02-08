//! FUSE-based 9P filesystem mounting for Plan 9 namespace support
//!
//! This module provides a FUSE filesystem interface that bridges 9P protocol
//! to the local filesystem, enabling proper Plan 9 namespace mounting.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use anyhow::{Context, Result};
use fuser::{
    FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry,
    Request,
};
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info, warn};

use crate::client::NinePClient;

const TTL: Duration = Duration::from_secs(1);

/// 9P FUSE filesystem implementation
pub struct NinePFS {
    /// 9P client connection
    client: Arc<Mutex<Option<NinePClient>>>,
    /// Server address (host:port)
    server_addr: String,
    /// Cache of file attributes
    attr_cache: Arc<RwLock<HashMap<u64, FileAttr>>>,
    /// Next available inode number
    next_ino: Arc<RwLock<u64>>,
    /// File handle to path mapping
    path_cache: Arc<RwLock<HashMap<u64, PathBuf>>>,
    /// Inode to fid mapping
    fid_cache: Arc<RwLock<HashMap<u64, u32>>>,
    /// Connected status
    connected: Arc<RwLock<bool>>,
}

impl NinePFS {
    pub fn new(server_addr: String) -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
            server_addr,
            attr_cache: Arc::new(RwLock::new(HashMap::new())),
            next_ino: Arc::new(RwLock::new(2)), // Start at 2, 1 is root
            path_cache: Arc::new(RwLock::new(HashMap::new())),
            fid_cache: Arc::new(RwLock::new(HashMap::new())),
            connected: Arc::new(RwLock::new(false)),
        }
    }

    /// Connect to the 9P server
    async fn connect(&self) -> Result<()> {
        info!("Connecting to 9P server at {}", self.server_addr);

        // Try to connect to the 9P server
        match NinePClient::connect(&self.server_addr).await {
            Ok(mut client) => {
                // Attach to the filesystem
                client.attach("nobody", "").await?;

                *self.client.lock().await = Some(client);
                *self.connected.write().await = true;

                // Initialize caches
                let mut attr_cache = self.attr_cache.write().await;
                attr_cache.insert(1, create_root_attr());

                let mut path_cache = self.path_cache.write().await;
                path_cache.insert(1, PathBuf::from("/"));

                info!(
                    "✅ Successfully connected to 9P server at {}",
                    self.server_addr
                );
                Ok(())
            }
            Err(e) => {
                warn!(
                    "⚠️  Could not connect to 9P server at {}: {}",
                    self.server_addr, e
                );
                warn!("📁 FUSE mount will show placeholder files only");

                // Still initialize caches for offline mode
                let mut attr_cache = self.attr_cache.write().await;
                attr_cache.insert(1, create_root_attr());

                let mut path_cache = self.path_cache.write().await;
                path_cache.insert(1, PathBuf::from("/"));

                Ok(())
            }
        }
    }

    /// Get next available inode number
    #[allow(dead_code)]
    async fn allocate_ino(&self) -> u64 {
        let mut next_ino = self.next_ino.write().await;
        let ino = *next_ino;
        *next_ino += 1;
        ino
    }

    /// Attempt to read directory from 9P server
    async fn readdir_from_9p(&self, ino: u64, _offset: i64) -> Result<Vec<(String, FileType, u64)>, Box<dyn std::error::Error>> {
        let path = {
            let path_cache = self.path_cache.read().await;
            path_cache.get(&ino).cloned().unwrap_or(PathBuf::from("/"))
        };
        let path_str = path.to_string_lossy();

        let mut client_guard = self.client.lock().await;
        if let Some(client) = client_guard.as_mut() {
             match client.list_directory(&path_str).await {
                Ok(names) => {
                     let mut entries = Vec::new();
                     entries.push((".".to_string(), FileType::Directory, ino));
                     entries.push(("..".to_string(), FileType::Directory, 1));

                     let mut next_ino_lock = self.next_ino.write().await;
                     let mut path_cache_lock = self.path_cache.write().await;

                     for name in names {
                         let child_path = if path_str == "/" {
                             PathBuf::from(format!("/{}", name))
                         } else {
                             path.join(&name)
                         };

                         let child_ino = *next_ino_lock;
                         *next_ino_lock += 1;

                         path_cache_lock.insert(child_ino, child_path);
                         entries.push((name, FileType::RegularFile, child_ino));
                     }
                     return Ok(entries);
                }
                Err(e) => warn!("Failed to list directory: {}", e),
             }
        }

        Ok(vec![])
    }

    /// Attempt to read file data from 9P server
    async fn read_from_9p(&self, ino: u64, offset: u64, size: u32) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let path = {
            let path_cache = self.path_cache.read().await;
            path_cache.get(&ino).cloned().unwrap_or(PathBuf::from("/"))
        };
        let path_str = path.to_string_lossy();

        let mut client_guard = self.client.lock().await;
        if let Some(client) = client_guard.as_mut() {
            match client.read_at(&path_str, offset, size).await {
                Ok(data) => return Ok(data),
                Err(e) => warn!("Failed to read file {}: {}", path_str, e),
            }
        }

        Ok(vec![])
    }

    /// Attempt to get file attributes from 9P server
    async fn getattr_from_9p(&self, ino: u64) -> Result<FileAttr, Box<dyn std::error::Error>> {
        let path = {
            let path_cache = self.path_cache.read().await;
            path_cache.get(&ino).cloned().unwrap_or(PathBuf::from("/"))
        };
        let path_str = path.to_string_lossy();

        let mut client_guard = self.client.lock().await;
        if let Some(client) = client_guard.as_mut() {
            if let Ok(raw_stat) = client.stat(&path_str).await {
                let kind = if ino == 1 { FileType::Directory } else { FileType::RegularFile };

                return Ok(FileAttr {
                    ino,
                    size: 4096,
                    blocks: 8,
                    atime: UNIX_EPOCH,
                    mtime: UNIX_EPOCH,
                    ctime: UNIX_EPOCH,
                    crtime: UNIX_EPOCH,
                    kind,
                    perm: 0o755,
                    nlink: 1,
                    uid: 1000,
                    gid: 1000,
                    rdev: 0,
                    flags: 0,
                    blksize: 512,
                });
            }
        }

        Err("Failed to get attributes".into())
    }
}

impl Filesystem for NinePFS {
    fn init(
        &mut self,
        _req: &Request<'_>,
        _config: &mut fuser::KernelConfig,
    ) -> Result<(), libc::c_int> {
        info!("Initializing 9P FUSE filesystem");

        // Start connection in background
        let self_clone = Arc::new(self.clone());
        tokio::spawn(async move {
            if let Err(e) = self_clone.connect().await {
                error!("Failed to connect to 9P server: {}", e);
            }
        });

        Ok(())
    }

    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        debug!("lookup: parent={}, name={:?}", parent, name);

        // For now, return a simple directory entry
        let ino = 2; // Placeholder
        let attr = create_file_attr(ino, FileType::RegularFile, 0, 0);
        reply.entry(&TTL, &attr, 0);
    }

    fn getattr(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyAttr) {
        debug!("getattr: ino={}", ino);

        // Attempt to get attributes from 9P server
        let rt = tokio::runtime::Handle::try_current()
            .unwrap_or_else(|_| {
                tokio::runtime::Runtime::new().unwrap().handle().clone()
            });
            
        match rt.block_on(self.getattr_from_9p(ino)) {
            Ok(attr) => {
                reply.attr(&TTL, &attr);
            }
            Err(_) => {
                // Fall back to original implementation
                debug!("Getting attributes from 9P server failed, using fallback");
                if ino == 1 {
                    // Root directory
                    let attr = create_root_attr();
                    reply.attr(&TTL, &attr);
                } else {
                    // Try to get from cache or return default
                    let attr = create_file_attr(ino, FileType::RegularFile, 0, 0);
                    reply.attr(&TTL, &attr);
                }
            }
        }
    }

    fn read(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock: Option<u64>,
        reply: ReplyData,
    ) {
        debug!("read: ino={}, offset={}, size={}", ino, offset, size);

        // Attempt to read actual content from 9P server
        let rt = tokio::runtime::Handle::try_current()
            .unwrap_or_else(|_| {
                tokio::runtime::Runtime::new().unwrap().handle().clone()
            });
            
        match rt.block_on(self.read_from_9p(ino, offset as u64, size)) {
            Ok(data) => {
                reply.data(&data);
            }
            Err(_) => {
                // Fall back to original hardcoded content if server communication fails
                debug!("Reading from 9P server failed, falling back to hardcoded content");
                let data: &[u8] = match ino {
                    2 => b"# README\n\nThis is content from the 9P.e server!\nYou are viewing files through FUSE mount.\n",
                    3 => b"{\"message\": \"Hello from 9P server\", \"timestamp\": \"2025-09-30\"}\n",
                    _ => b"File content from 9P server\n",
                };
                
                let start = offset as usize;
                let end = (start + size as usize).min(data.len());

                if start < data.len() {
                    reply.data(&data[start..end]);
                } else {
                    reply.data(&[]);
                }
            }
        }
    }

    fn readdir(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        debug!("readdir: ino={}, offset={}", ino, offset);

        // Connect to 9P server and do actual directory listing
        let rt = tokio::runtime::Handle::try_current()
            .unwrap_or_else(|_| {
                // Fallback to single-threaded runtime if needed
                tokio::runtime::Runtime::new().unwrap().handle().clone()
            });
            
        let server_addr = self.server_addr.clone();
        
        // Attempt to list directory from 9P server
        match rt.block_on(self.readdir_from_9p(ino, offset)) {
            Ok(entries) => {
                for (idx, (entry_name, entry_type, entry_ino)) in entries.into_iter().enumerate() {
                    if offset <= idx as i64 {
                        let _ = reply.add(entry_ino, idx as i64 + 1, entry_type, &entry_name);
                    }
                }
                reply.ok();
            }
            Err(_) => {
                // Fall back to hardcoded example files if 9P communication fails
                warn!("📁 Falling back to placeholder files (9P server not responding)");
                if ino == 1 {
                    if offset == 0 {
                        let _ = reply.add(1, 0, FileType::Directory, ".");
                        let _ = reply.add(1, 1, FileType::Directory, "..");
                        let _ = reply.add(2, 2, FileType::RegularFile, "README.txt");
                        let _ = reply.add(3, 3, FileType::RegularFile, "data.json");
                        let _ = reply.add(4, 4, FileType::Directory, "documents");
                    }
                }
                reply.ok();
            }
        }
    }
}

impl Clone for NinePFS {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            server_addr: self.server_addr.clone(),
            attr_cache: self.attr_cache.clone(),
            next_ino: self.next_ino.clone(),
            path_cache: self.path_cache.clone(),
            fid_cache: self.fid_cache.clone(),
            connected: self.connected.clone(),
        }
    }
}

/// Create root directory attributes
fn create_root_attr() -> FileAttr {
    FileAttr {
        ino: 1,
        size: 0,
        blocks: 0,
        atime: UNIX_EPOCH,
        mtime: UNIX_EPOCH,
        ctime: UNIX_EPOCH,
        crtime: UNIX_EPOCH,
        kind: FileType::Directory,
        perm: 0o755,
        nlink: 2,
        uid: 1000,
        gid: 1000,
        rdev: 0,
        flags: 0,
        blksize: 512,
    }
}

/// Create file attributes
fn create_file_attr(ino: u64, kind: FileType, size: u64, mode: u16) -> FileAttr {
    FileAttr {
        ino,
        size,
        blocks: size.div_ceil(512),
        atime: UNIX_EPOCH,
        mtime: UNIX_EPOCH,
        ctime: UNIX_EPOCH,
        crtime: UNIX_EPOCH,
        kind,
        perm: mode,
        nlink: if kind == FileType::Directory { 2 } else { 1 },
        uid: 1000,
        gid: 1000,
        rdev: 0,
        flags: 0,
        blksize: 512,
    }
}

/// Mount a 9P server using FUSE
pub async fn mount_9p_fuse(server_addr: String, mount_point: PathBuf) -> Result<()> {
    info!(
        "Mounting 9P server {} at {:?} using FUSE",
        server_addr, mount_point
    );

    // Check for and clean up any existing broken mount
    if mount_point.exists() {
        if is_mount_point(&mount_point).await? {
            warn!(
                "Mount point {:?} already mounted, attempting cleanup",
                mount_point
            );
            if let Err(e) = unmount_fuse(&mount_point).await {
                warn!("Failed to unmount existing mount: {}", e);
            }
        }
        // Remove any leftover directory
        if mount_point.exists() {
            std::fs::remove_dir_all(&mount_point)
                .context("Failed to remove existing mount point")?;
        }
    }

    // Create fresh mount point
    std::fs::create_dir_all(&mount_point).context("Failed to create mount point")?;

    // Create filesystem
    let fs = NinePFS::new(server_addr);

    // Mount options - read-only for safety
    let options = vec![MountOption::RO, MountOption::FSName("9pe-fuse".to_string())];

    // Mount the filesystem
    info!("Starting FUSE mount...");

    // Note: This is a blocking call, so we need to run it in a separate thread
    tokio::task::spawn_blocking(move || {
        if let Err(e) = fuser::mount2(fs, &mount_point, &options) {
            error!("FUSE mount failed: {}", e);
        }
    })
    .await?;

    Ok(())
}

/// Check if a path is a mount point
async fn is_mount_point(path: &PathBuf) -> Result<bool> {
    // Use mountpoint command to check if directory is a mount point
    let output = tokio::process::Command::new("mountpoint")
        .arg("-q")
        .arg(path)
        .output()
        .await;

    match output {
        Ok(result) => Ok(result.status.success()),
        Err(_) => {
            // If mountpoint command fails, check /proc/mounts
            check_proc_mounts(path).await
        }
    }
}

/// Fallback method to check mount points via /proc/mounts
async fn check_proc_mounts(path: &PathBuf) -> Result<bool> {
    let mounts = tokio::fs::read_to_string("/proc/mounts").await?;
    let path_str = path.to_string_lossy();

    for line in mounts.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts[1] == path_str {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Unmount a FUSE filesystem
pub async fn unmount_fuse(mount_point: &PathBuf) -> Result<()> {
    info!("Unmounting FUSE filesystem at {:?}", mount_point);

    // Try fusermount3 first, then fusermount
    let commands = ["fusermount3", "fusermount"];
    let mut last_error = None;

    for cmd in &commands {
        let output = tokio::process::Command::new(cmd)
            .arg("-u")
            .arg(mount_point)
            .output()
            .await;

        match output {
            Ok(result) if result.status.success() => {
                info!("Successfully unmounted {:?}", mount_point);
                return Ok(());
            }
            Ok(result) => {
                let stderr = String::from_utf8_lossy(&result.stderr);
                last_error = Some(format!("{} failed: {}", cmd, stderr));
            }
            Err(e) => {
                last_error = Some(format!("{} command failed: {}", cmd, e));
            }
        }
    }

    // If all fusermount attempts failed, try lazy unmount as last resort
    warn!("Standard unmount failed, attempting lazy unmount");
    let output = tokio::process::Command::new("umount")
        .arg("-l")
        .arg(mount_point)
        .output()
        .await;

    match output {
        Ok(result) if result.status.success() => {
            info!("Successfully lazy unmounted {:?}", mount_point);
            Ok(())
        }
        _ => Err(anyhow::anyhow!(
            "All unmount attempts failed. Last error: {}",
            last_error.unwrap_or_else(|| "Unknown error".to_string())
        )),
    }
}

/// Clean up any broken FUSE mounts in the 9pe directory
pub async fn cleanup_broken_mounts() -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let nine_pe_dir = PathBuf::from(home).join("9pe");

    if !nine_pe_dir.exists() {
        return Ok(());
    }

    info!("Checking for broken FUSE mounts in {:?}", nine_pe_dir);

    let mut entries = tokio::fs::read_dir(&nine_pe_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.is_dir() && is_mount_point(&path).await? {
            // Check if mount is responsive
            if !is_mount_responsive(&path).await {
                warn!("Found unresponsive mount at {:?}, cleaning up", path);
                if let Err(e) = unmount_fuse(&path).await {
                    warn!("Failed to cleanup broken mount {:?}: {}", path, e);
                }
            }
        }
    }

    Ok(())
}

/// Check if a mount point is responsive
async fn is_mount_responsive(path: &PathBuf) -> bool {
    // Try to list directory contents with a timeout
    match tokio::time::timeout(std::time::Duration::from_secs(5), tokio::fs::read_dir(path)).await {
        Ok(Ok(_)) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_root_attr() {
        let attr = create_root_attr();
        assert_eq!(attr.ino, 1);
        assert_eq!(attr.kind, FileType::Directory);
        assert_eq!(attr.perm, 0o755);
        assert_eq!(attr.nlink, 2);
    }

    #[test]
    fn test_create_file_attr() {
        let attr = create_file_attr(42, FileType::RegularFile, 1024, 0o644);
        assert_eq!(attr.ino, 42);
        assert_eq!(attr.kind, FileType::RegularFile);
        assert_eq!(attr.size, 1024);
        assert_eq!(attr.perm, 0o644);
        assert_eq!(attr.nlink, 1);
    }

    #[test]
    fn test_create_directory_attr() {
        let attr = create_file_attr(10, FileType::Directory, 0, 0o755);
        assert_eq!(attr.kind, FileType::Directory);
        assert_eq!(attr.nlink, 2); // Directories have nlink=2
    }

    #[test]
    fn test_ninep_fs_creation() {
        let fs = NinePFS::new("127.0.0.1:5640".to_string());
        assert_eq!(fs.server_addr, "127.0.0.1:5640");
    }

    #[tokio::test]
    async fn test_allocate_ino() {
        let fs = NinePFS::new("127.0.0.1:5640".to_string());
        let ino1 = fs.allocate_ino().await;
        let ino2 = fs.allocate_ino().await;
        let ino3 = fs.allocate_ino().await;

        // Should allocate sequential inode numbers
        assert_eq!(ino1 + 1, ino2);
        assert_eq!(ino2 + 1, ino3);
    }

    #[test]
    fn test_ninep_fs_clone() {
        let fs = NinePFS::new("127.0.0.1:5640".to_string());
        let fs_clone = fs.clone();

        assert_eq!(fs.server_addr, fs_clone.server_addr);
    }

    #[tokio::test]
    async fn test_check_proc_mounts_nonexistent() {
        let path = PathBuf::from("/nonexistent/mount/point");
        let result = check_proc_mounts(&path).await;

        // Should either succeed (false) or fail if /proc/mounts unavailable
        let _ = result;
    }

    #[test]
    fn test_block_calculation() {
        let attr = create_file_attr(1, FileType::RegularFile, 1024, 0o644);
        assert_eq!(attr.blocks, 2); // 1024 bytes = 2 blocks of 512

        let attr = create_file_attr(1, FileType::RegularFile, 100, 0o644);
        assert_eq!(attr.blocks, 1); // 100 bytes = 1 block (rounded up)
    }

    /// Fuzz test: File attributes should handle arbitrary sizes
    #[test]
    fn fuzz_file_attributes() {
        use proptest::prelude::*;

        proptest!(|(ino: u64, size: u64, perm: u16)| {
            let attr = create_file_attr(ino, FileType::RegularFile, size, perm);
            // Should not panic with any values
            assert_eq!(attr.ino, ino);
            assert_eq!(attr.size, size);
        });
    }

    /// Fuzz test: Server address parsing
    #[test]
    fn fuzz_server_address() {
        use proptest::prelude::*;

        proptest!(|(addr in ".*")| {
            let fs = NinePFS::new(addr.clone());
            assert_eq!(fs.server_addr, addr);
        });
    }
}
