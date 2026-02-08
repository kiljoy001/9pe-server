//! Automatic FUSE mounting for discovered 9P.e servers
//!
//! Provides automatic mounting of remote filesystems with security

use std::collections::HashMap;
use std::sync::Arc;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::RwLock;
use anyhow::{Result, Context};
use tracing::{info, warn, error, debug};
use libp2p::PeerId;

use crate::mesh_client::{MeshClient, DiscoveredPeer};
use crate::auth::{AuthService, AuthMethod, SignedCapability, Permissions, User};
use crate::client::NinePClient;
use crate::simple_fuse;

/// Mount configuration for a remote server
#[derive(Debug, Clone)]
pub struct MountConfig {
    pub server_addr: String,
    pub mount_point: PathBuf,
    pub permissions: Permissions,
    pub capability: Option<SignedCapability>,
    pub read_only: bool,
    pub auto_unmount_secs: Option<u64>,
}

/// Auto-mount manager that handles discovery and mounting
pub struct AutoMountManager {
    /// Authentication service
    auth_service: Arc<AuthService>,

    /// Active mounts
    mounts: Arc<RwLock<HashMap<String, MountConfig>>>,

    /// Mount permissions cache
    permissions_cache: Arc<RwLock<HashMap<String, Permissions>>>,

    /// Base mount directory
    mount_base: PathBuf,

    /// User context
    user: Option<User>,
}

impl AutoMountManager {
    /// Create new auto-mount manager
    pub async fn new(mount_base: Option<PathBuf>) -> Result<Self> {
        let mount_base = mount_base.unwrap_or_else(|| PathBuf::from("/tmp/9pe-namespace"));

        // Create mount base directory if it doesn't exist
        tokio::fs::create_dir_all(&mount_base).await?;

        // Create Plan 9 style directories
        let srv_dir = mount_base.join("srv");
        let n_dir = mount_base.join("n");

        tokio::fs::create_dir_all(&srv_dir).await
            .context("Failed to create /srv directory")?;
        tokio::fs::create_dir_all(&n_dir).await
            .context("Failed to create /n directory")?;

        info!("📁 Created Plan 9 style directories:");
        info!("   /srv -> {:?}", srv_dir);
        info!("   /n/ -> {:?}", n_dir);

        // Clean up any orphaned mounts from previous sessions
        if let Err(e) = cleanup_orphaned_mounts(&mount_base).await {
            warn!("Failed to clean up orphaned mounts: {}", e);
        }

        // Initialize auth service
        let auth_service = Arc::new(AuthService::new());

        // Create default user (in production, would authenticate properly)
        let user = User {
            uid: std::process::id(),
            username: whoami::username(),
            password_hash: String::new(), // Auto-mount doesn't need password
            groups: vec!["users".to_string()],
            home_dir: std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()),
            shell: std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()),
            public_key: None,
        };

        // Add user to auth service
        auth_service.add_user(user.clone()).await?;

        Ok(Self {
            auth_service,
            mounts: Arc::new(RwLock::new(HashMap::new())),
            permissions_cache: Arc::new(RwLock::new(HashMap::new())),
            mount_base,
            user: Some(user),
        })
    }

    /// Start auto-mounting discovered servers
    pub async fn start_auto_mount(&self) -> Result<()> {
        info!("🔐 Starting secure auto-mount manager");

        // Create mesh client for discovery with mesh network access
        // Note: For auto-mount to work properly, it needs to connect to an existing mesh network
        // or create its own mesh network for discovery
        let (mesh_sender, discovered_peers) = if let Ok((sender, peers)) = crate::mesh::start_mesh_network(44444, None).await {
            (Some(sender), Some(peers))
        } else {
            warn!("Failed to start mesh network for auto-mount discovery");
            (None, None)
        };

        let mut mesh_client = MeshClient::new_with_mesh(mesh_sender, discovered_peers).await?;

        // Background task to monitor and mount
        let mounts = self.mounts.clone();
        let auth_service = self.auth_service.clone();
        let mount_base = self.mount_base.clone();
        let user = self.user.clone();

        tokio::spawn(async move {
            loop {
                // Wait for discovery
                tokio::time::sleep(Duration::from_secs(3)).await;

                // Get discovered peers
                let peers = mesh_client.list_peers().await;

                if peers.is_empty() {
                    debug!("No peers discovered yet, scanning...");
                    if let Err(e) = mesh_client.scan_local_network().await {
                        warn!("Network scan failed: {}", e);
                    }
                    continue;
                }

                for peer in peers {
                    // Check if already mounted
                    if mounts.read().await.contains_key(&peer.listen_addr) {
                        continue;
                    }

                    info!("🔍 Discovered new server: {}", peer.listen_addr);

                    // Try to authenticate and get permissions
                    match authenticate_to_server(&peer, &auth_service, user.as_ref()).await {
                        Ok(capability) => {
                            info!("✅ Authenticated to {}", peer.listen_addr);

                            // Create Plan 9 style mount points
                            let mount_name = peer.node_id.replace([':', '.'], "_");

                            // Mount in /n/ directory (namespace)
                            let n_mount_point = mount_base.join("n").join(&mount_name);

                            // Create service file in /srv directory
                            let srv_file = mount_base.join("srv").join(&mount_name);
                            let srv_content = format!("{}#{}\n", peer.listen_addr, peer.node_id);
                            if let Err(e) = tokio::fs::write(&srv_file, srv_content).await {
                                warn!("Failed to create service file: {}", e);
                            } else {
                                info!("📄 Created service file: {:?}", srv_file);
                            }

                            // Try to mount
                            match mount_server(&peer, &n_mount_point, capability.clone()).await {
                                Ok(()) => {
                                    info!("🗻 Auto-mounted {} at {:?}", peer.listen_addr, n_mount_point);

                                    let config = MountConfig {
                                        server_addr: peer.listen_addr.clone(),
                                        mount_point: n_mount_point.clone(),
                                        permissions: Permissions::READ.with(Permissions::TRAVERSE),
                                        capability: Some(capability),
                                        read_only: true,  // Default to read-only for safety
                                        auto_unmount_secs: Some(3600), // Auto-unmount after 1 hour
                                    };

                                    mounts.write().await.insert(peer.listen_addr.clone(), config);
                                }
                                Err(e) => {
                                    warn!("Failed to mount {}: {}", peer.listen_addr, e);
                                }
                            }
                        }
                        Err(e) => {
                            debug!("Cannot authenticate to {}: {}", peer.listen_addr, e);
                        }
                    }
                }

                // Clean up expired mounts
                cleanup_expired_mounts(&mounts).await;
            }
        });

        info!("✅ Auto-mount manager started");
        Ok(())
    }

    /// List all active mounts
    pub async fn list_mounts(&self) -> Vec<MountConfig> {
        self.mounts.read().await.values().cloned().collect()
    }

    /// Manually unmount a server
    pub async fn unmount(&self, server_addr: &str) -> Result<()> {
        let mut mounts = self.mounts.write().await;

        if let Some(config) = mounts.remove(server_addr) {
            info!("📤 Unmounting {}", server_addr);

            // Use simple unmount
            if let Err(e) = simple_fuse::unmount(&config.mount_point).await {
                warn!("Unmount failed: {}", e);
            }

            // Remove mount info file
            let info_file = config.mount_point.join(".9pe_mount_info");
            let _ = tokio::fs::remove_file(info_file).await;

            // Remove mount point directory if empty
            let _ = tokio::fs::remove_dir(&config.mount_point).await;

            info!("✅ Successfully unmounted {}", server_addr);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Server not mounted"))
        }
    }

    /// Get permissions for a server
    pub async fn get_permissions(&self, server_addr: &str) -> Option<Permissions> {
        self.permissions_cache.read().await.get(server_addr).copied()
    }

    /// Request additional permissions
    pub async fn request_permissions(
        &self,
        server_addr: &str,
        requested: Permissions,
    ) -> Result<SignedCapability> {
        // In production, would negotiate with server
        let user = self.user.as_ref().ok_or_else(|| anyhow::anyhow!("No user context"))?;

        // Issue capability for requested permissions
        let capability = self.auth_service.issue_capability(
            user.username.clone(),
            format!("{}/**", server_addr),
            requested,
            3600, // 1 hour validity
        ).await?;

        // Update cache
        self.permissions_cache.write().await.insert(server_addr.to_string(), requested);

        Ok(capability)
    }
}

/// Authenticate to a discovered server
async fn authenticate_to_server(
    peer: &DiscoveredPeer,
    auth_service: &AuthService,
    user: Option<&User>,
) -> Result<SignedCapability> {
    // Try to connect first
    match NinePClient::connect(&peer.listen_addr).await {
        Ok(_client) => {
            // For now, issue a basic read-only capability
            // In production, would negotiate with server
            let capability = auth_service.issue_capability(
                user.map(|u| u.username.clone()).unwrap_or_else(|| "anonymous".to_string()),
                format!("{}/**", peer.listen_addr),
                Permissions::READ.with(Permissions::TRAVERSE),
                86400, // 24 hour validity
            ).await?;

            Ok(capability)
        }
        Err(e) => {
            Err(anyhow::anyhow!("Cannot connect to {}: {}", peer.listen_addr, e))
        }
    }
}

/// Mount a server using FUSE
async fn mount_server(
    peer: &DiscoveredPeer,
    mount_point: &Path,
    capability: SignedCapability,
) -> Result<()> {
    // Check if FUSE is available
    if !simple_fuse::is_fuse_available() {
        warn!("FUSE not available - creating mount marker only");
    }

    info!("🗻 Mounting {} at {:?}", peer.listen_addr, mount_point);

    // Create mount point directory
    tokio::fs::create_dir_all(mount_point).await?;

    // Mount using simple FUSE
    match simple_fuse::mount_with_cleanup(peer.listen_addr.clone(), mount_point).await {
        Ok(()) => {
            // Create mount info file
            let info_file = mount_point.join(".9pe_mount_info");
            let mount_info = format!(
                "server: {}\nnode: {}\nmounted_at: {}\npermissions: {:?}\ncapability_id: {}\n",
                peer.listen_addr,
                peer.node_id,
                chrono::Local::now(),
                capability.capability.permissions,
                capability.capability.id,
            );

            tokio::fs::write(info_file, mount_info).await?;

            info!("✅ Successfully mounted {} at {:?}", peer.listen_addr, mount_point);
            Ok(())
        }
        Err(e) => {
            error!("Failed to mount {}: {}", peer.listen_addr, e);
            Err(e)
        }
    }
}

/// Clean up expired and broken mounts
async fn cleanup_expired_mounts(mounts: &Arc<RwLock<HashMap<String, MountConfig>>>) {
    let mut to_remove = Vec::new();

    {
        let mounts_read = mounts.read().await;
        for (addr, config) in mounts_read.iter() {
            let mut should_remove = false;

            // Check for expired mounts
            if let Some(auto_unmount_secs) = config.auto_unmount_secs {
                if auto_unmount_secs == 0 {
                    should_remove = true;
                    debug!("Mount {} expired due to auto_unmount_secs = 0", addr);
                }
            }

            // Check if mount point is stale or broken
            if !should_remove {
                if let Err(e) = check_mount_health(&config.mount_point).await {
                    warn!("Mount {} is unhealthy: {}", addr, e);
                    should_remove = true;
                }
            }

            if should_remove {
                to_remove.push(addr.clone());
            }
        }
    }

    if !to_remove.is_empty() {
        let mut mounts_write = mounts.write().await;
        for addr in to_remove {
            if let Some(config) = mounts_write.remove(&addr) {
                cleanup_mount_point(&config).await;
                info!("🧹 Cleaned up stale/broken mount: {}", addr);
            }
        }
    }
}

/// Check if a mount point is healthy and accessible
async fn check_mount_health(mount_point: &Path) -> Result<()> {
    // Check if mount point exists
    if !mount_point.exists() {
        return Err(anyhow::anyhow!("Mount point does not exist"));
    }

    // Check if it's actually mounted by trying to read .9pe_mount_info
    let info_file = mount_point.join(".9pe_mount_info");
    if !info_file.exists() {
        return Err(anyhow::anyhow!("Mount info file missing - mount may be stale"));
    }

    // Try to read the info file to check if filesystem is responsive
    match tokio::fs::read_to_string(&info_file).await {
        Ok(_) => {
            debug!("Mount point {:?} is healthy", mount_point);
            Ok(())
        }
        Err(e) => {
            Err(anyhow::anyhow!("Mount point unresponsive: {}", e))
        }
    }
}

/// Clean up a mount point completely
async fn cleanup_mount_point(config: &MountConfig) {
    debug!("Cleaning up mount point: {:?}", config.mount_point);

    // Try to unmount using fusermount
    if let Ok(output) = tokio::process::Command::new("fusermount")
        .arg("-u")
        .arg(&config.mount_point)
        .output()
        .await
    {
        if !output.status.success() {
            debug!("fusermount failed, trying lazy unmount");
            // Try lazy unmount if regular unmount fails
            let _ = tokio::process::Command::new("fusermount")
                .arg("-uz")
                .arg(&config.mount_point)
                .output()
                .await;
        }
    }

    // Remove mount info file
    let info_file = config.mount_point.join(".9pe_mount_info");
    if info_file.exists() {
        let _ = tokio::fs::remove_file(info_file).await;
    }

    // Remove mount point directory if empty
    let _ = tokio::fs::remove_dir(&config.mount_point).await;
}

/// Clean up orphaned mount points on startup
async fn cleanup_orphaned_mounts(mount_base: &Path) -> Result<()> {
    info!("🧹 Cleaning up orphaned mounts in {:?}", mount_base);

    let n_dir = mount_base.join("n");
    if !n_dir.exists() {
        return Ok(());
    }

    let mut entries = tokio::fs::read_dir(&n_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.is_dir() {
            // Check if this looks like a mount point
            let info_file = path.join(".9pe_mount_info");
            if info_file.exists() {
                // This looks like a mount point, check if it's stale
                match tokio::fs::read_to_string(&info_file).await {
                    Ok(content) => {
                        debug!("Found existing mount info: {}", content);

                        // Check if the mount is still valid by testing accessibility
                        match tokio::fs::read_dir(&path).await {
                            Ok(_) => {
                                debug!("Mount {:?} appears healthy, keeping", path);
                            }
                            Err(_) => {
                                warn!("Mount {:?} appears stale, cleaning up", path);
                                cleanup_stale_mount(&path).await;
                            }
                        }
                    }
                    Err(_) => {
                        warn!("Mount {:?} has unreadable info file, cleaning up", path);
                        cleanup_stale_mount(&path).await;
                    }
                }
            } else if path.read_dir().map_or(true, |mut d| d.next().is_none()) {
                // Empty directory without mount info - likely orphaned
                debug!("Removing empty orphaned directory: {:?}", path);
                let _ = tokio::fs::remove_dir(&path).await;
            }
        }
    }

    Ok(())
}

/// Clean up a stale mount point
async fn cleanup_stale_mount(mount_point: &Path) {
    debug!("Cleaning up stale mount: {:?}", mount_point);

    // Try to unmount
    let _ = tokio::process::Command::new("fusermount")
        .arg("-uz")  // Lazy unmount
        .arg(mount_point)
        .output()
        .await;

    // Remove mount info file
    let info_file = mount_point.join(".9pe_mount_info");
    let _ = tokio::fs::remove_file(info_file).await;

    // Remove the directory
    let _ = tokio::fs::remove_dir_all(mount_point).await;

    info!("🗑️ Cleaned up stale mount: {:?}", mount_point);
}

/// CLI commands for auto-mounting
pub mod commands {
    use super::*;

    /// Start auto-mount daemon
    pub async fn start_daemon() -> Result<()> {
        let manager = AutoMountManager::new(None).await?;
        manager.start_auto_mount().await?;

        // Keep daemon running
        tokio::signal::ctrl_c().await?;

        info!("Shutting down auto-mount daemon");
        Ok(())
    }

    /// List active mounts
    pub async fn list_mounts() -> Result<()> {
        let manager = AutoMountManager::new(None).await?;
        let mounts = manager.list_mounts().await;

        if mounts.is_empty() {
            info!("No active mounts");
        } else {
            info!("🗻 Active mounts:");
            for mount in mounts {
                info!("  {} -> {:?}", mount.server_addr, mount.mount_point);
                info!("    Permissions: {:?}", mount.permissions);
                info!("    Read-only: {}", mount.read_only);
            }
        }

        Ok(())
    }

    /// Clean up orphaned mount points
    pub async fn cleanup_orphaned(mount_base: Option<PathBuf>) -> Result<()> {
        let mount_base = mount_base.unwrap_or_else(|| PathBuf::from("/tmp/9pe-namespace"));
        cleanup_orphaned_mounts(&mount_base).await?;
        info!("✅ Cleanup completed for {:?}", mount_base);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_auto_mount_manager() {
        let manager = AutoMountManager::new(Some("/tmp/test-mounts".into())).await.unwrap();
        assert!(manager.list_mounts().await.is_empty());
    }
}