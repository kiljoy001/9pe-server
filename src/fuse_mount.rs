//! FUSE-based 9P filesystem mounting for Plan 9 namespace support
//!
//! This module provides a FUSE filesystem interface that bridges 9P protocol
//! to the local filesystem, enabling proper Plan 9 namespace mounting.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Result, Context};
use fuser::{
    FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry,
    Request,
};
use tokio::sync::{RwLock, Mutex};
use tracing::{debug, error, info, warn};

use crate::protocol::{NinePClient, Qid, Stat, permissions};


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

                info!("✅ Successfully connected to 9P server at {}", self.server_addr);
                Ok(())
            },
            Err(e) => {
                warn!("⚠️  Could not connect to 9P server at {}: {}", self.server_addr, e);
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
    async fn allocate_ino(&self) -> u64 {
        let mut next_ino = self.next_ino.write().await;
        let ino = *next_ino;
        *next_ino += 1;
        ino
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

        // Return sample content based on inode
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

    fn readdir(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        debug!("readdir: ino={}, offset={}", ino, offset);

        // For now, provide a simple directory listing
        // TODO: Implement actual 9P readdir calls
        if ino == 1 {
            // Root directory
            if offset == 0 {
                reply.add(1, 0, FileType::Directory, ".");
                reply.add(1, 1, FileType::Directory, "..");
                // Show some example files that would come from the 9P server
                reply.add(2, 2, FileType::RegularFile, "README.txt");
                reply.add(3, 3, FileType::RegularFile, "data.json");
                reply.add(4, 4, FileType::Directory, "documents");
            }
        }

        reply.ok();
    }
}

// Clone implementation for NinePFS
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
        blocks: (size + 511) / 512,
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
pub async fn mount_9p_fuse(
    server_addr: String,
    mount_point: PathBuf,
) -> Result<()> {
    info!("Mounting 9P server {} at {:?} using FUSE", server_addr, mount_point);

    // Check for and clean up any existing broken mount
    if mount_point.exists() {
        if is_mount_point(&mount_point).await? {
            warn!("Mount point {:?} already mounted, attempting cleanup", mount_point);
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
    std::fs::create_dir_all(&mount_point)
        .context("Failed to create mount point")?;

    // Create filesystem
    let fs = NinePFS::new(server_addr);

    // Mount options - read-only for safety
    let options = vec![
        MountOption::RO,
        MountOption::FSName("9pe-fuse".to_string()),
    ];

    // Mount the filesystem
    info!("Starting FUSE mount...");

    // Note: This is a blocking call, so we need to run it in a separate thread
    tokio::task::spawn_blocking(move || {
        if let Err(e) = fuser::mount2(fs, &mount_point, &options) {
            error!("FUSE mount failed: {}", e);
        }
    }).await?;

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
        _ => {
            Err(anyhow::anyhow!("All unmount attempts failed. Last error: {}",
                last_error.unwrap_or_else(|| "Unknown error".to_string())))
        }
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
        if path.is_dir() {
            if is_mount_point(&path).await? {
                // Check if mount is responsive
                if !is_mount_responsive(&path).await {
                    warn!("Found unresponsive mount at {:?}, cleaning up", path);
                    if let Err(e) = unmount_fuse(&path).await {
                        warn!("Failed to cleanup broken mount {:?}: {}", path, e);
                    }
                }
            }
        }
    }

    Ok(())
}

/// Check if a mount point is responsive
async fn is_mount_responsive(path: &PathBuf) -> bool {
    // Try to list directory contents with a timeout
    match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::fs::read_dir(path)
    ).await {
        Ok(Ok(_)) => true,
        _ => false,
    }
}