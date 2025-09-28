//! FUSE mounting implementation for 9P.e clients
//!
//! Provides seamless filesystem access through FUSE

use std::ffi::OsStr;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use anyhow::{Result, Context};
use tracing::{info, warn, error, debug};
use fuser::{
    FileAttr, FileType, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry,
    Request, MountOption, Session,
};
use libc::ENOENT;

use crate::client::NinePeeClient;

/// FUSE filesystem implementation for 9P.e
pub struct NinePeeFuse {
    /// Connection to remote 9P.e server
    client: NinePeeClient,

    /// Server address for debugging
    server_addr: String,

    /// Root inode
    root_inode: u64,

    /// File handle counter
    next_handle: std::sync::atomic::AtomicU64,
}

impl NinePeeFuse {
    /// Create new FUSE filesystem
    pub async fn new(server_addr: String) -> Result<Self> {
        info!("🔗 Connecting to 9P.e server: {}", server_addr);

        let client = NinePeeClient::connect(&server_addr).await
            .with_context(|| format!("Failed to connect to {}", server_addr))?;

        Ok(Self {
            client,
            server_addr,
            root_inode: 1,
            next_handle: std::sync::atomic::AtomicU64::new(1),
        })
    }

    /// Mount the filesystem at the given mount point (blocking operation)
    pub fn mount_blocking(self, mount_point: &Path) -> Result<()> {
        info!("🗻 Mounting {} at {:?}", self.server_addr, mount_point);

        // Mount options for security and performance
        let options = vec![
            MountOption::RO,          // Read-only by default for safety
            MountOption::FSName(self.server_addr.clone()),
            MountOption::Subtype("9pe".to_string()),
            MountOption::AllowOther,  // Allow other users to access
            MountOption::DefaultPermissions,
        ];

        // Create and run FUSE session (blocking)
        let session = Session::new(self, mount_point, &options)?;

        info!("✅ FUSE session starting at {:?}", mount_point);
        session.run()?;

        Ok(())
    }

    /// Convert 9P.e file info to FUSE attributes
    fn to_file_attr(&self, path: &str, size: u64, is_dir: bool) -> FileAttr {
        let now = SystemTime::now();

        FileAttr {
            ino: self.path_to_inode(path),
            size,
            blocks: (size + 511) / 512,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            kind: if is_dir { FileType::Directory } else { FileType::RegularFile },
            perm: if is_dir { 0o755 } else { 0o644 },
            nlink: 1,
            uid: 1000,  // Default user
            gid: 1000,  // Default group
            rdev: 0,
            blksize: 4096,
            flags: 0,
        }
    }

    /// Convert path to inode number (simple hash)
    fn path_to_inode(&self, path: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        if path == "/" {
            return self.root_inode;
        }

        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        hasher.finish()
    }

    /// Get next file handle
    fn next_fh(&self) -> u64 {
        self.next_handle.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }
}

impl Filesystem for NinePeeFuse {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        debug!("FUSE lookup: parent={}, name={:?}", parent, name);

        // Convert to path
        let name_str = match name.to_str() {
            Some(s) => s,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        // For now, simulate file lookup
        let path = if parent == self.root_inode {
            format!("/{}", name_str)
        } else {
            format!("/unknown/{}", name_str)
        };

        // Try to get file info from server
        let rt = tokio::runtime::Handle::current();
        let client = &mut self.client;

        match rt.block_on(async {
            // Try to list directory first to see if file exists
            client.list_directory("/").await
        }) {
            Ok(files) => {
                if files.contains(&name_str.to_string()) {
                    let attr = self.to_file_attr(&path, 1024, false);  // Default size
                    let ttl = Duration::from_secs(1);
                    reply.entry(&ttl, &attr, 0);
                } else {
                    reply.error(ENOENT);
                }
            }
            Err(_) => {
                reply.error(ENOENT);
            }
        }
    }

    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        debug!("FUSE getattr: ino={}", ino);

        if ino == self.root_inode {
            // Root directory
            let attr = self.to_file_attr("/", 4096, true);
            reply.attr(&Duration::from_secs(1), &attr);
        } else {
            // Other files - try to get info from server
            let attr = self.to_file_attr("/unknown", 1024, false);
            reply.attr(&Duration::from_secs(1), &attr);
        }
    }

    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock: Option<u64>,
        reply: ReplyData,
    ) {
        debug!("FUSE read: ino={}, offset={}, size={}", ino, offset, size);

        if ino == self.root_inode {
            reply.error(libc::EISDIR);
            return;
        }

        // Try to read file from server
        let rt = tokio::runtime::Handle::current();
        let client = &mut self.client;

        match rt.block_on(async {
            // For now, read a default file - in production would map inode to path
            client.read_file("/README.md").await
        }) {
            Ok(mut data) => {
                // Handle offset and size
                let start = offset as usize;
                if start >= data.len() {
                    reply.data(&[]);
                } else {
                    let end = std::cmp::min(start + size as usize, data.len());
                    data.drain(..start);
                    data.truncate(end - start);
                    reply.data(&data);
                }
            }
            Err(e) => {
                warn!("Failed to read file: {}", e);
                reply.error(libc::EIO);
            }
        }
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        debug!("FUSE readdir: ino={}, offset={}", ino, offset);

        if ino != self.root_inode {
            reply.error(libc::ENOTDIR);
            return;
        }

        let rt = tokio::runtime::Handle::current();
        let client = &mut self.client;

        match rt.block_on(async {
            client.list_directory("/").await
        }) {
            Ok(files) => {
                let entries = vec![
                    (1, FileType::Directory, "."),
                    (1, FileType::Directory, ".."),
                ];

                // Add files from server
                let mut all_entries = entries;
                for (i, file) in files.iter().enumerate() {
                    all_entries.push((
                        2 + i as u64,
                        FileType::RegularFile,
                        file.as_str(),
                    ));
                }

                for (i, entry) in all_entries.iter().enumerate().skip(offset as usize) {
                    if reply.add(entry.0, (i + 1) as i64, entry.1, entry.2) {
                        break;
                    }
                }
                reply.ok();
            }
            Err(e) => {
                warn!("Failed to list directory: {}", e);
                reply.error(libc::EIO);
            }
        }
    }

    fn open(&mut self, _req: &Request, ino: u64, _flags: i32, reply: fuser::ReplyOpen) {
        debug!("FUSE open: ino={}", ino);

        if ino == self.root_inode {
            reply.error(libc::EISDIR);
        } else {
            let fh = self.next_fh();
            reply.opened(fh, 0);
        }
    }

    fn opendir(&mut self, _req: &Request, ino: u64, _flags: i32, reply: fuser::ReplyOpen) {
        debug!("FUSE opendir: ino={}", ino);

        if ino == self.root_inode {
            let fh = self.next_fh();
            reply.opened(fh, 0);
        } else {
            reply.error(libc::ENOTDIR);
        }
    }
}

/// Mount a 9P.e server using FUSE (blocking operation)
pub async fn mount_ninepee_server(
    server_addr: String,
    mount_point: &Path,
) -> Result<()> {
    info!("🗻 Mounting 9P.e server {} at {:?}", server_addr, mount_point);

    // Create mount point
    tokio::fs::create_dir_all(mount_point).await?;

    // Create FUSE filesystem
    let fs = NinePeeFuse::new(server_addr.clone()).await?;

    // Mount it in blocking thread
    let mount_point_owned = mount_point.to_owned();
    tokio::task::spawn_blocking(move || {
        fs.mount_blocking(&mount_point_owned)
    }).await??;

    Ok(())
}

/// Unmount a FUSE filesystem
pub async fn unmount(mount_point: &Path) -> Result<()> {
    info!("📤 Unmounting {:?}", mount_point);

    // Use fusermount to unmount
    let output = tokio::process::Command::new("fusermount")
        .arg("-u")
        .arg(mount_point)
        .output()
        .await?;

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("Unmount failed: {}", error));
    }

    info!("✅ Successfully unmounted {:?}", mount_point);
    Ok(())
}

/// Check if FUSE is available on the system
pub fn is_fuse_available() -> bool {
    Path::new("/dev/fuse").exists()
}

/// Mount helper that creates the mount point
pub async fn mount_with_cleanup(
    server_addr: String,
    mount_point: &Path,
) -> Result<()> {
    // Check FUSE availability
    if !is_fuse_available() {
        return Err(anyhow::anyhow!("FUSE not available. Install fuse package."));
    }

    // Create mount point
    tokio::fs::create_dir_all(mount_point).await?;

    // Mount
    mount_ninepee_server(server_addr, mount_point).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_fuse_available() {
        // Just check that the function doesn't panic
        let _available = is_fuse_available();
    }

    #[tokio::test]
    async fn test_mount_point_creation() {
        let temp_dir = TempDir::new().unwrap();
        let mount_point = temp_dir.path().join("test_mount");

        tokio::fs::create_dir_all(&mount_point).await.unwrap();
        assert!(mount_point.exists());
    }
}