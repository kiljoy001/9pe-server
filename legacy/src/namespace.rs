//! Plan 9-style Namespace Management with /srv and /n/ directories
//!
//! Implements virtual filesystem for service discovery and namespace mounting

use std::collections::HashMap;
use std::sync::Arc;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use std::ffi::OsStr;
use tokio::sync::RwLock;
use fuser::{
    FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyData, ReplyDirectory,
    ReplyEntry, Request, ReplyOpen, ReplyWrite, ReplyCreate,
};
use libc::{ENOENT, ENOTDIR, EISDIR, EACCES, EEXIST, EIO};
use anyhow::{Result, Context};
use tracing::{info, debug, warn, error};

use crate::mesh::DiscoveredPeer;
use crate::client::NinePeeClient;

const TTL: Duration = Duration::from_secs(1);
const ROOT_INODE: u64 = 1;
const SRV_INODE: u64 = 2;
const N_INODE: u64 = 3;
const SRV_BASE: u64 = 1000;
const N_BASE: u64 = 2000;
const NS_FILE_BASE: u64 = 10000;

/// Connection pool for multiplexed 9P connections
pub struct ConnectionPool {
    connections: Arc<RwLock<HashMap<String, Arc<RwLock<NinePeeClient>>>>>,
}

impl ConnectionPool {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get or create a multiplexed connection to a peer
    pub async fn get_connection(&self, service_addr: &str) -> Result<Arc<RwLock<NinePeeClient>>> {
        let mut conns = self.connections.write().await;

        if let Some(conn) = conns.get(service_addr) {
            return Ok(conn.clone());
        }

        // Create new multiplexed connection
        debug!("Creating new multiplexed connection to {}", service_addr);
        let client = NinePeeClient::connect(service_addr).await
            .context(format!("Failed to connect to {}", service_addr))?;

        let conn = Arc::new(RwLock::new(client));
        conns.insert(service_addr.to_string(), conn.clone());

        Ok(conn)
    }

    /// Remove a failed connection
    pub async fn remove_connection(&self, service_addr: &str) {
        self.connections.write().await.remove(service_addr);
    }
}

/// Namespace filesystem implementing /srv and /n/
pub struct NamespaceFS {
    /// Discovered peers from mesh network
    mesh_peers: Option<Arc<RwLock<HashMap<String, DiscoveredPeer>>>>,
    connection_pool: Arc<ConnectionPool>,

    /// Inode to path mapping
    inode_map: Arc<RwLock<HashMap<u64, PathBuf>>>,

    /// Path to inode mapping
    path_map: Arc<RwLock<HashMap<PathBuf, u64>>>,

    /// Next available inode
    next_inode: Arc<RwLock<u64>>,

    /// Mounted namespaces under /n/
    namespaces: Arc<RwLock<HashMap<String, NamespaceMount>>>,

    /// Cached peer list for /srv
    srv_cache: Arc<RwLock<Vec<DiscoveredPeer>>>,
    last_srv_update: Arc<RwLock<SystemTime>>,
}

/// A mounted namespace in /n/
#[derive(Clone)]
struct NamespaceMount {
    peer_name: String,
    service_addr: String,
    mount_point: String,
    mounted_at: SystemTime,
}

impl NamespaceFS {
    pub fn new(mesh_peers: Option<Arc<RwLock<HashMap<String, DiscoveredPeer>>>>) -> Self {
        let mut inode_map = HashMap::new();
        let mut path_map = HashMap::new();

        // Pre-populate root directories
        inode_map.insert(ROOT_INODE, PathBuf::from("/"));
        inode_map.insert(SRV_INODE, PathBuf::from("/srv"));
        inode_map.insert(N_INODE, PathBuf::from("/n"));

        path_map.insert(PathBuf::from("/"), ROOT_INODE);
        path_map.insert(PathBuf::from("/srv"), SRV_INODE);
        path_map.insert(PathBuf::from("/n"), N_INODE);

        Self {
            mesh_peers,
            connection_pool: Arc::new(ConnectionPool::new()),
            inode_map: Arc::new(RwLock::new(inode_map)),
            path_map: Arc::new(RwLock::new(path_map)),
            next_inode: Arc::new(RwLock::new(NS_FILE_BASE)),
            namespaces: Arc::new(RwLock::new(HashMap::new())),
            srv_cache: Arc::new(RwLock::new(Vec::new())),
            last_srv_update: Arc::new(RwLock::new(UNIX_EPOCH)),
        }
    }

    /// Update /srv cache if needed
    async fn update_srv_cache(&self) {
        if let Some(mesh_peers) = &self.mesh_peers {
            let now = SystemTime::now();
            let last_update = *self.last_srv_update.read().await;

            // Update cache every 5 seconds
            if now.duration_since(last_update).unwrap_or(Duration::MAX) > Duration::from_secs(5) {
                let peers = mesh_peers.read().await;
                let peer_list: Vec<DiscoveredPeer> = peers.values().cloned().collect();
                *self.srv_cache.write().await = peer_list;
                *self.last_srv_update.write().await = now;
            }
        }
    }

    /// Get inode for a peer in /srv
    fn peer_to_srv_inode(&self, index: usize) -> u64 {
        SRV_BASE + index as u64
    }

    /// Get inode for a namespace in /n/
    fn namespace_to_n_inode(&self, name: &str) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(name, &mut hasher);
        N_BASE + (std::hash::Hasher::finish(&hasher) % 1000)
    }

    fn get_file_attr(&self, ino: u64, ftype: FileType) -> FileAttr {
        FileAttr {
            ino,
            size: 0,
            blocks: 0,
            atime: SystemTime::now(),
            mtime: SystemTime::now(),
            ctime: SystemTime::now(),
            crtime: SystemTime::now(),
            kind: ftype,
            perm: if ftype == FileType::Directory { 0o755 } else { 0o644 },
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
            flags: 0,
            blksize: 4096,
        }
    }

    /// Mount a namespace under /n/
    pub async fn mount_namespace(&self, peer_name: String, service_addr: String) -> Result<()> {
        let mount = NamespaceMount {
            peer_name: peer_name.clone(),
            service_addr,
            mount_point: format!("/n/{}", peer_name),
            mounted_at: SystemTime::now(),
        };

        self.namespaces.write().await.insert(peer_name.clone(), mount);
        info!("📁 Mounted namespace {} at /n/{}", peer_name, peer_name);

        Ok(())
    }
}

impl Filesystem for NamespaceFS {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let name = name.to_str().unwrap_or("");

        match parent {
            ROOT_INODE => {
                match name {
                    "srv" => reply.entry(&TTL, &self.get_file_attr(SRV_INODE, FileType::Directory), 0),
                    "n" => reply.entry(&TTL, &self.get_file_attr(N_INODE, FileType::Directory), 0),
                    _ => reply.error(ENOENT),
                }
            }
            SRV_INODE => {
                // Block to handle async operation
                let rt = match tokio::runtime::Runtime::new() {
                    Ok(rt) => rt,
                    Err(e) => {
                        error!("Failed to create runtime for SRV lookup: {}", e);
                        reply.error(EIO);
                        return;
                    }
                };
                rt.block_on(async {
                    self.update_srv_cache().await;
                    let peers = self.srv_cache.read().await;

                    for (i, peer) in peers.iter().enumerate() {
                        if peer.node_id == name {
                            let ino = self.peer_to_srv_inode(i);
                            reply.entry(&TTL, &self.get_file_attr(ino, FileType::RegularFile), 0);
                            return;
                        }
                    }
                    reply.error(ENOENT);
                })
            }
            N_INODE => {
                // Check if namespace is mounted
                let rt = match tokio::runtime::Runtime::new() {
                    Ok(rt) => rt,
                    Err(e) => {
                        error!("Failed to create runtime for namespace lookup: {}", e);
                        reply.error(EIO);
                        return;
                    }
                };
                rt.block_on(async {
                    let namespaces = self.namespaces.read().await;
                    if namespaces.contains_key(name) {
                        let ino = self.namespace_to_n_inode(name);
                        reply.entry(&TTL, &self.get_file_attr(ino, FileType::Directory), 0);
                    } else {
                        reply.error(ENOENT);
                    }
                })
            }
            _ => reply.error(ENOENT),
        }
    }

    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        match ino {
            ROOT_INODE => reply.attr(&TTL, &self.get_file_attr(ROOT_INODE, FileType::Directory)),
            SRV_INODE => reply.attr(&TTL, &self.get_file_attr(SRV_INODE, FileType::Directory)),
            N_INODE => reply.attr(&TTL, &self.get_file_attr(N_INODE, FileType::Directory)),
            _ => {
                if ino >= SRV_BASE && ino < N_BASE {
                    reply.attr(&TTL, &self.get_file_attr(ino, FileType::RegularFile));
                } else if ino >= N_BASE && ino < NS_FILE_BASE {
                    reply.attr(&TTL, &self.get_file_attr(ino, FileType::Directory));
                } else {
                    reply.error(ENOENT);
                }
            }
        }
    }

    fn readdir(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, mut reply: ReplyDirectory) {
        match ino {
            ROOT_INODE => {
                if offset == 0 {
                    reply.add(ROOT_INODE, 1, FileType::Directory, ".");
                    reply.add(ROOT_INODE, 2, FileType::Directory, "..");
                    reply.add(SRV_INODE, 3, FileType::Directory, "srv");
                    reply.add(N_INODE, 4, FileType::Directory, "n");
                }
                reply.ok();
            }
            SRV_INODE => {
                let rt = match tokio::runtime::Runtime::new() {
                    Ok(rt) => rt,
                    Err(e) => {
                        error!("Failed to create runtime for SRV readdir: {}", e);
                        reply.error(EIO);
                        return;
                    }
                };
                rt.block_on(async {
                    self.update_srv_cache().await;
                    let peers = self.srv_cache.read().await;

                    let mut entries = vec![
                        (SRV_INODE, FileType::Directory, "."),
                        (ROOT_INODE, FileType::Directory, ".."),
                    ];

                    for (i, peer) in peers.iter().enumerate() {
                        entries.push((self.peer_to_srv_inode(i), FileType::RegularFile, peer.node_id.as_str()));
                    }

                    for (i, (ino, ftype, name)) in entries.iter().enumerate().skip(offset as usize) {
                        if reply.add(*ino, (i + 1) as i64, *ftype, name) {
                            break;
                        }
                    }
                    reply.ok();
                })
            }
            N_INODE => {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let namespaces = self.namespaces.read().await;

                    let mut entries = vec![
                        (N_INODE, FileType::Directory, ".".to_string()),
                        (ROOT_INODE, FileType::Directory, "..".to_string()),
                    ];

                    for (name, _mount) in namespaces.iter() {
                        entries.push((self.namespace_to_n_inode(name), FileType::Directory, name.clone()));
                    }

                    for (i, (ino, ftype, name)) in entries.iter().enumerate().skip(offset as usize) {
                        if reply.add(*ino, (i + 1) as i64, *ftype, name) {
                            break;
                        }
                    }
                    reply.ok();
                })
            }
            _ => reply.error(ENOTDIR),
        }
    }

    fn read(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, size: u32, _flags: i32, _lock_owner: Option<u64>, reply: ReplyData) {
        // Reading from /srv files gives connection info
        if ino >= SRV_BASE && ino < N_BASE {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let peers = self.srv_cache.read().await;
                let index = (ino - SRV_BASE) as usize;

                if let Some(peer) = peers.get(index) {
                    let info = format!(
                        "node_id: {}\nservice_addr: {}\nversion: {}\ncapabilities: {}\n",
                        peer.node_id,
                        peer.service_addr,
                        peer.version,
                        peer.capabilities.join(", ")
                    );

                    let data = info.as_bytes();
                    let end = std::cmp::min(offset as usize + size as usize, data.len());
                    let start = std::cmp::min(offset as usize, end);
                    reply.data(&data[start..end]);
                } else {
                    reply.error(ENOENT);
                }
            })
        } else {
            reply.error(EISDIR);
        }
    }

    fn write(&mut self, _req: &Request, ino: u64, _fh: u64, _offset: i64, data: &[u8], _write_flags: u32, _flags: i32, _lock_owner: Option<u64>, reply: ReplyWrite) {
        // Writing to /srv files triggers connection/mount
        if ino >= SRV_BASE && ino < N_BASE {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let peers = self.srv_cache.read().await;
                let index = (ino - SRV_BASE) as usize;

                if let Some(peer) = peers.get(index) {
                    // Writing "mount" to /srv/peer mounts it to /n/peer
                    if data == b"mount\n" || data == b"mount" {
                        match self.mount_namespace(peer.node_id.clone(), peer.service_addr.clone()).await {
                            Ok(_) => {
                                info!("✅ Mounted {} to /n/{}", peer.service_addr, peer.node_id);
                                reply.written(data.len() as u32);
                            }
                            Err(e) => {
                                error!("Failed to mount namespace: {}", e);
                                reply.error(EACCES);
                            }
                        }
                    } else {
                        reply.error(EACCES);
                    }
                } else {
                    reply.error(ENOENT);
                }
            })
        } else {
            reply.error(EISDIR);
        }
    }
}

/// Mount the namespace filesystem with optional read-only mode
pub async fn mount_namespace_fs(
    mount_point: &str,
    mesh_peers: Option<Arc<RwLock<HashMap<String, DiscoveredPeer>>>>,
    read_only: bool
) -> Result<()> {
    let mode = if read_only { "read-only" } else { "read-write" };
    info!("🔧 Mounting namespace filesystem at {} ({})", mount_point, mode);

    // First, try to unmount if already mounted (clean up from previous runs)
    if is_mounted(mount_point) {
        info!("⚠️ Mount point {} already in use, unmounting first", mount_point);
        if let Err(e) = unmount_namespace_fs(mount_point) {
            warn!("Failed to unmount existing mount: {}. Trying lazy unmount...", e);
            // Try lazy unmount as fallback
            let _ = std::process::Command::new("fusermount")
                .arg("-uz")  // -z for lazy unmount
                .arg(mount_point)
                .output();
        }
        // Give the system a moment to clean up
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    // Create mount points
    std::fs::create_dir_all(mount_point)?;

    let fs = NamespaceFS::new(mesh_peers);

    // Spawn FUSE filesystem in background
    let mount_point = mount_point.to_string();
    tokio::task::spawn_blocking(move || {
        let mut options = vec![
            MountOption::FSName("9pe-namespace".to_string()),
        ];

        // Add RW or RO based on configuration (default is RW)
        if read_only {
            options.push(MountOption::RO);
            info!("📝 Namespace filesystem mounted as READ-ONLY");
        } else {
            options.push(MountOption::RW);
            info!("✏️ Namespace filesystem mounted as READ-WRITE");
        }

        match fuser::mount2(fs, &mount_point, &options) {
            Ok(_) => info!("✅ Namespace filesystem mounted successfully"),
            Err(e) => error!("❌ Failed to mount namespace filesystem: {}", e),
        }
    });

    Ok(())
}

/// Check if a mount point is currently mounted
pub fn is_mounted(mount_point: &str) -> bool {
    // Check /proc/mounts for the mount point
    if let Ok(contents) = std::fs::read_to_string("/proc/mounts") {
        return contents.lines().any(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            parts.len() >= 2 && parts[1] == mount_point
        });
    }

    // Fallback: check if directory is accessible
    if let Ok(entries) = std::fs::read_dir(mount_point) {
        // If we can list it but find FUSE-specific behavior
        if entries.count() == 0 {
            // Try to create a test file to see if it's a FUSE mount
            let test_file = format!("{}/.mount_test", mount_point);
            if std::fs::write(&test_file, b"test").is_err() {
                // Can't write = likely a FUSE mount
                return true;
            } else {
                // Clean up test file
                let _ = std::fs::remove_file(test_file);
            }
        }
    }

    false
}

/// Unmount the namespace filesystem with retries and force options
pub fn unmount_namespace_fs(mount_point: &str) -> Result<()> {
    info!("🔧 Unmounting namespace filesystem at {}", mount_point);

    // First attempt: normal unmount
    let output = std::process::Command::new("fusermount")
        .arg("-u")
        .arg(mount_point)
        .output()?;

    if output.status.success() {
        info!("✅ Successfully unmounted {}", mount_point);
        return Ok(());
    }

    // Second attempt: quiet unmount (suppresses some errors)
    warn!("Normal unmount failed, trying quiet unmount: {}",
          String::from_utf8_lossy(&output.stderr));

    let output = std::process::Command::new("fusermount")
        .arg("-uq")  // -q for quiet
        .arg(mount_point)
        .output()?;

    if output.status.success() {
        info!("✅ Successfully unmounted {} with quiet flag", mount_point);
        return Ok(());
    }

    // Third attempt: lazy unmount (detaches immediately but cleans up when not in use)
    warn!("Quiet unmount failed, trying lazy unmount: {}",
          String::from_utf8_lossy(&output.stderr));

    let output = std::process::Command::new("fusermount")
        .arg("-uz")  // -z for lazy unmount
        .arg(mount_point)
        .output()?;

    if !output.status.success() {
        error!("Failed all unmount attempts for {}: {}",
               mount_point, String::from_utf8_lossy(&output.stderr));
        return Err(anyhow::anyhow!("Failed to unmount {}: all attempts failed", mount_point));
    }

    info!("✅ Successfully performed lazy unmount of {}", mount_point);
    Ok(())
}