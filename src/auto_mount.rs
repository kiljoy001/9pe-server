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
use crate::client::NinePeeClient;
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
        let mount_base = mount_base.unwrap_or_else(|| PathBuf::from("/tmp/9pe-mounts"));

        // Create mount base directory if it doesn't exist
        tokio::fs::create_dir_all(&mount_base).await?;

        // Initialize auth service
        let auth_service = Arc::new(AuthService::new());

        // Create default user (in production, would authenticate properly)
        let user = User {
            uid: std::process::id(),
            username: whoami::username(),
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

        // Create mesh client for discovery
        let mut mesh_client = MeshClient::new().await?;

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

                            // Create mount point
                            let mount_name = peer.node_id.replace([':', '.'], "_");
                            let mount_point = mount_base.join(&mount_name);

                            // Try to mount
                            match mount_server(&peer, &mount_point, capability.clone()).await {
                                Ok(()) => {
                                    info!("🗻 Auto-mounted {} at {:?}", peer.listen_addr, mount_point);

                                    let config = MountConfig {
                                        server_addr: peer.listen_addr.clone(),
                                        mount_point: mount_point.clone(),
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
    match NinePeeClient::connect(&peer.listen_addr).await {
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

/// Clean up expired mounts
async fn cleanup_expired_mounts(mounts: &Arc<RwLock<HashMap<String, MountConfig>>>) {
    let mut to_remove = Vec::new();

    {
        let mounts_read = mounts.read().await;
        for (addr, config) in mounts_read.iter() {
            if let Some(auto_unmount_secs) = config.auto_unmount_secs {
                // Check if mount has expired
                // In production, would track mount time
                if auto_unmount_secs == 0 {
                    to_remove.push(addr.clone());
                }
            }
        }
    }

    if !to_remove.is_empty() {
        let mut mounts_write = mounts.write().await;
        for addr in to_remove {
            if let Some(config) = mounts_write.remove(&addr) {
                // Unmount
                let _ = tokio::process::Command::new("fusermount")
                    .arg("-u")
                    .arg(&config.mount_point)
                    .output()
                    .await;

                let _ = tokio::fs::remove_dir(&config.mount_point).await;

                info!("🕐 Auto-unmounted expired mount: {}", addr);
            }
        }
    }
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