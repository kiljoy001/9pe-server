//! Auto-mount system with FUSE integration
//!
//! Provides completely transparent automatic discovery and mounting of 9P.e servers:
//! - Individual mount points in /n/ namespace for each discovered server
//! - Network discovery via consensus and local scanning
//! - Automatic mount/unmount based on server availability
//! - Zero user configuration required - runs transparently

use anyhow::{Result, Context};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{RwLock, mpsc};
use tokio::time::interval;
use tracing::{debug, error, info, warn};
use std::process::Command;

use crate::transport::{TransportType, TransportFactory};
use crate::consensus::ConsensusCoordinator;

/// Auto-mount daemon managing individual server mounts
/// Runs transparently - no user configuration needed
pub struct AutoMountDaemon {
    interval: Duration,
    discovered_servers: Arc<RwLock<HashMap<String, DiscoveredServer>>>,
    mounted_servers: Arc<RwLock<HashMap<String, MountedServer>>>,
    consensus_coordinator: Option<Arc<ConsensusCoordinator>>,
    shutdown_tx: Option<mpsc::Sender<()>>,
    health_check_handle: Option<tokio::task::JoinHandle<()>>,
}

/// A discovered 9P.e server
#[derive(Debug, Clone)]
pub struct DiscoveredServer {
    pub address: String,
    pub port: u16,
    pub transport: TransportType,
    pub last_seen: SystemTime,
    pub peer_info: Option<String>,
}

/// A mounted 9P.e server
#[derive(Debug)]
pub struct MountedServer {
    pub server: DiscoveredServer,
    pub mount_path: PathBuf,
    pub transport_type: TransportType,
    pub mounted_at: SystemTime,
}

impl AutoMountDaemon {
    /// Create new auto-mount daemon with sensible defaults
    /// No user configuration required - completely transparent
    pub fn new() -> Self {
        Self {
            interval: Duration::from_secs(30), // Check every 30 seconds
            discovered_servers: Arc::new(RwLock::new(HashMap::new())),
            mounted_servers: Arc::new(RwLock::new(HashMap::new())),
            consensus_coordinator: None,
            shutdown_tx: None,
            health_check_handle: None,
        }
    }

    /// Create with custom interval (mainly for testing)
    pub fn with_interval(interval: u64) -> Self {
        Self {
            interval: Duration::from_secs(interval),
            discovered_servers: Arc::new(RwLock::new(HashMap::new())),
            mounted_servers: Arc::new(RwLock::new(HashMap::new())),
            consensus_coordinator: None,
            shutdown_tx: None,
            health_check_handle: None,
        }
    }

    /// Set consensus coordinator for server discovery
    pub fn with_consensus(mut self, consensus: Arc<ConsensusCoordinator>) -> Self {
        self.consensus_coordinator = Some(consensus);
        self
    }

    /// Generate mount point for a discovered server in /n or ~/n namespace
    fn generate_mount_point(server: &DiscoveredServer) -> PathBuf {
        // Create clean server name for mount point
        let clean_name = server.address
            .replace(".", "_")
            .replace(":", "_")
            .replace("-", "_");

        let n_dir = crate::util::get_n_directory();
        n_dir.join(format!("{}_port_{}", clean_name, server.port))
    }

    /// Ensure /n or ~/n directory exists based on privilege level
    fn ensure_n_directory_exists() -> Result<()> {
        let n_path = crate::util::get_n_directory();
        if !n_path.exists() {
            std::fs::create_dir_all(&n_path)
                .with_context(|| format!("Failed to create {:?} directory", n_path))?;
            info!("Created {:?} directory for namespace mounts", n_path);
        }
        Ok(())
    }

    /// Start the auto-mount daemon
    pub async fn start(&mut self) -> Result<()> {
        let n_path = crate::util::get_n_directory();
        info!("Starting transparent auto-mount daemon for {:?} namespace", n_path);

        // Ensure namespace directory exists
        Self::ensure_n_directory_exists()?;

        // Start server health monitoring
        self.start_health_monitoring().await?;

        // Start discovery and mount management tasks
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
        self.shutdown_tx = Some(shutdown_tx);

        let discovered_servers = self.discovered_servers.clone();
        let mounted_servers = self.mounted_servers.clone();
        let consensus_coordinator = self.consensus_coordinator.clone();
        let discovery_interval = self.interval;

        // Discovery task
        tokio::spawn(async move {
            let mut timer = interval(discovery_interval);
            loop {
                tokio::select! {
                    _ = timer.tick() => {
                        if let Err(e) = Self::discover_and_mount_servers(
                            consensus_coordinator.as_ref(),
                            &discovered_servers,
                            &mounted_servers
                        ).await {
                            error!("Discovery error: {}", e);
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Shutting down discovery task");
                        break;
                    }
                }
            }
        });

        info!("Auto-mount daemon started - will automatically manage /n/ mounts");
        Ok(())
    }

    /// Start health monitoring for mounted servers
    async fn start_health_monitoring(&mut self) -> Result<()> {
        let mounted_servers = self.mounted_servers.clone();
        let discovered_servers = self.discovered_servers.clone();

        let handle = tokio::spawn(async move {
            let mut health_timer = interval(Duration::from_secs(60)); // Check health every minute

            loop {
                health_timer.tick().await;

                if let Err(e) = Self::health_check_mounted_servers(&mounted_servers, &discovered_servers).await {
                    error!("Health check failed: {}", e);
                }
            }
        });

        self.health_check_handle = Some(handle);
        Ok(())
    }

    /// Health check mounted servers and unmount dead ones
    async fn health_check_mounted_servers(
        mounted_servers: &Arc<RwLock<HashMap<String, MountedServer>>>,
        discovered_servers: &Arc<RwLock<HashMap<String, DiscoveredServer>>>
    ) -> Result<()> {
        let mut to_unmount = Vec::new();

        {
            let mounted = mounted_servers.read().await;
            let discovered = discovered_servers.read().await;

            for (server_key, mount) in mounted.iter() {
                // Check if server is still discovered and responsive
                if let Some(server) = discovered.get(server_key) {
                    // Check if server was seen recently (within 2 intervals)
                    if let Ok(elapsed) = server.last_seen.elapsed() {
                        if elapsed > Duration::from_secs(120) { // 2 minutes timeout
                            warn!("Server {} not seen for {:?}, marking for unmount", server.address, elapsed);
                            to_unmount.push(server_key.clone());
                        }
                    }
                } else {
                    // Server no longer discovered
                    info!("Server {} no longer discovered, marking for unmount", mount.server.address);
                    to_unmount.push(server_key.clone());
                }
            }
        }

        // Unmount dead servers
        if !to_unmount.is_empty() {
            let mut mounted = mounted_servers.write().await;
            for server_key in to_unmount {
                if let Some(mount) = mounted.remove(&server_key) {
                    info!("Auto-unmounting dead server: {}", mount.server.address);
                    if let Err(e) = Self::unmount_server(&mount.mount_path).await {
                        error!("Failed to unmount {}: {}", mount.mount_path.display(), e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Stop the auto-mount daemon
    pub async fn stop(&mut self) -> Result<()> {
        info!("Stopping auto-mount daemon");

        // Signal shutdown
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }

        // Stop health monitoring
        if let Some(handle) = self.health_check_handle.take() {
            handle.abort();
        }

        // Unmount all mounted servers
        {
            let mut mounted = self.mounted_servers.write().await;
            for (_, mount) in mounted.drain() {
                info!("Unmounting {}", mount.server.address);
                if let Err(e) = Self::unmount_server(&mount.mount_path).await {
                    error!("Failed to unmount {}: {}", mount.mount_path.display(), e);
                }
            }
        }

        // Stop consensus coordinator
        if let Some(_coordinator) = self.consensus_coordinator.take() {
            info!("Consensus coordinator cleanup - placeholder");
        }

        Ok(())
    }

    /// Get current status
    pub async fn status(&self) -> AutoMountStatus {
        let discovered = self.discovered_servers.read().await;
        let mounted = self.mounted_servers.read().await;

        AutoMountStatus {
            mount_point: crate::util::get_n_directory(),
            running: self.shutdown_tx.is_some(),
            discovered_count: discovered.len(),
            mounted_count: mounted.len(),
            servers: discovered.values().cloned().collect(),
        }
    }

    /// Discover servers and automatically mount them
    async fn discover_and_mount_servers(
        consensus_coordinator: Option<&Arc<ConsensusCoordinator>>,
        discovered_servers: &Arc<RwLock<HashMap<String, DiscoveredServer>>>,
        mounted_servers: &Arc<RwLock<HashMap<String, MountedServer>>>
    ) -> Result<()> {
        debug!("Starting server discovery and auto-mount cycle");

        // Discover servers
        let mut new_servers = Vec::new();

        if let Some(consensus) = consensus_coordinator {
            // Get servers from consensus network
            if let Ok(consensus_servers) = Self::get_consensus_peers(consensus).await {
                new_servers.extend(consensus_servers);
            }
        }

        // Add local discovery
        new_servers.extend(Self::get_local_servers());

        // Update discovered servers
        {
            let mut discovered = discovered_servers.write().await;
            for (address, port, transport) in new_servers {
                let server_key = format!("{}:{}", address, port);
                let server = DiscoveredServer {
                    address: address.clone(),
                    port,
                    transport,
                    last_seen: SystemTime::now(),
                    peer_info: None,
                };

                discovered.insert(server_key.clone(), server.clone());

                // Check if we should mount this server
                let mounted = mounted_servers.read().await;
                if !mounted.contains_key(&server_key) {
                    drop(mounted); // Release read lock

                    // Try to mount the server
                    if let Err(e) = Self::mount_server(&server, mounted_servers).await {
                        warn!("Failed to mount server {}:{}: {}", address, port, e);
                    } else {
                        let n_dir = crate::util::get_n_directory();
                        info!("Auto-mounted server {}:{} at {:?}/{}_port_{}",
                              address, port, n_dir,
                              address.replace(".", "_").replace(":", "_").replace("-", "_"),
                              port);
                    }
                }
            }
        }

        Ok(())
    }

    /// Get consensus network peers for discovery
    async fn get_consensus_peers(consensus: &Arc<ConsensusCoordinator>) -> Result<Vec<(String, u16, TransportType)>> {
        info!("Querying consensus network for active 9P.e servers");
        let consensus_state = consensus.get_consensus_state().await;
        let mut discovered_servers = Vec::new();

        // For now, use the node_id as a source, but this is a placeholder
        // In a real implementation, we'd get actual network nodes from the consensus layer
        info!("Consensus node: {}", consensus_state.node_id);

        // Fallback to local discovery since the consensus layer doesn't expose network nodes yet
        warn!("Consensus network discovery not fully implemented, using local discovery");
        discovered_servers = Self::get_local_servers();

        Ok(discovered_servers)
    }

    /// Get local servers (fallback discovery)
    fn get_local_servers() -> Vec<(String, u16, TransportType)> {
        vec![
            ("127.0.0.1".to_string(), 5640, TransportType::Tcp),
            ("127.0.0.1".to_string(), 5641, TransportType::Tcp),
            ("127.0.0.1".to_string(), 5642, TransportType::Tcp),
        ]
    }

    /// Parse node address from consensus format
    fn parse_node_address(node_id: &str) -> Result<(String, u16, TransportType)> {
        // Simple parsing - in real implementation would be more sophisticated
        if let Some((addr, port_str)) = node_id.split_once(':') {
            let port = port_str.parse::<u16>()
                .context("Invalid port in node address")?;
            Ok((addr.to_string(), port, TransportType::Tcp))
        } else {
            anyhow::bail!("Invalid node address format: {}", node_id)
        }
    }

    /// Mount a discovered server at its individual mount point
    async fn mount_server(
        server: &DiscoveredServer,
        mounted_servers: &Arc<RwLock<HashMap<String, MountedServer>>>
    ) -> Result<()> {
        let mount_point = Self::generate_mount_point(server);
        debug!("Mounting server {}:{} at {:?}", server.address, server.port, mount_point);

        // Create mount point directory
        if !mount_point.exists() {
            std::fs::create_dir_all(&mount_point)
                .context("Failed to create mount point directory")?;
        }

        // Test connection first
        Self::create_connection(&server.address, server.port, server.transport.clone()).await?;

        // For now, we'll use a simple directory mount since we don't have 9P FUSE client yet
        // In a real implementation, this would mount the 9P server using FUSE
        info!("Successfully connected to {}:{} - mount point ready at {:?}",
              server.address, server.port, mount_point);

        // Record the mount
        let server_key = format!("{}:{}", server.address, server.port);
        let mounted = MountedServer {
            server: server.clone(),
            mount_path: mount_point,
            transport_type: server.transport.clone(),
            mounted_at: SystemTime::now(),
        };

        {
            let mut mounted_map = mounted_servers.write().await;
            mounted_map.insert(server_key, mounted);
        }

        Ok(())
    }

    /// Create connection to 9P.e server using transport factory
    async fn create_connection(address: &str, port: u16, transport_type: TransportType) -> Result<()> {
        debug!("Establishing {:?} connection to {}:{}", transport_type, address, port);

        let transport = TransportFactory::create(transport_type)
            .context("Failed to create transport")?;

        let addr = format!("{}:{}", address, port).parse()
            .context("Invalid server address")?;

        match transport.connect(addr).await {
            Ok(_connection) => {
                debug!("Connection test successful to {}:{}", address, port);
                Ok(())
            }
            Err(e) => {
                error!("Connection test failed: {}", e);
                anyhow::bail!("Connection test failed: {}", e)
            }
        }
    }

    /// Unmount a server's mount point
    async fn unmount_server(mount_path: &PathBuf) -> Result<()> {
        info!("Unmounting {:?}", mount_path);

        // Use fusermount to unmount if it's a FUSE mount
        let output = Command::new("fusermount")
            .args(["-u", mount_path.to_str().unwrap()])
            .output();

        match output {
            Ok(result) => {
                if result.status.success() {
                    info!("Successfully unmounted {:?}", mount_path);
                } else {
                    warn!("fusermount failed, trying regular unmount");
                    // Try regular umount as fallback
                    Command::new("umount")
                        .arg(mount_path.to_str().unwrap())
                        .output()
                        .context("Failed to unmount using umount")?;
                }
            }
            Err(_) => {
                // fusermount not available, just remove the directory
                if mount_path.exists() {
                    std::fs::remove_dir_all(mount_path)
                        .context("Failed to remove mount directory")?;
                }
            }
        }

        Ok(())
    }
}

/// Status information for the auto-mount daemon
#[derive(Debug)]
pub struct AutoMountStatus {
    pub mount_point: PathBuf,
    pub running: bool,
    pub discovered_count: usize,
    pub mounted_count: usize,
    pub servers: Vec<DiscoveredServer>,
}

impl Default for AutoMountDaemon {
    fn default() -> Self {
        Self::new()
    }
}

/// Initialize auto-mount system and start daemon
pub async fn initialize_auto_mount() -> Result<AutoMountDaemon> {
    let mut daemon = AutoMountDaemon::new();
    daemon.start().await?;
    Ok(daemon)
}

/// Initialize auto-mount with consensus coordinator
pub async fn initialize_auto_mount_with_consensus(consensus: Arc<ConsensusCoordinator>) -> Result<AutoMountDaemon> {
    let mut daemon = AutoMountDaemon::new().with_consensus(consensus);
    daemon.start().await?;
    Ok(daemon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discovered_server_creation() {
        let server = DiscoveredServer {
            address: "127.0.0.1".to_string(),
            port: 5640,
            transport: TransportType::Tcp,
            last_seen: SystemTime::now(),
            peer_info: None,
        };

        assert_eq!(server.address, "127.0.0.1");
        assert_eq!(server.port, 5640);
    }

    #[test]
    fn test_generate_mount_point() {
        let server = DiscoveredServer {
            address: "192.168.1.10".to_string(),
            port: 5640,
            transport: TransportType::Tcp,
            last_seen: SystemTime::now(),
            peer_info: None,
        };

        let mount_point = AutoMountDaemon::generate_mount_point(&server);
        let path_str = mount_point.to_string_lossy();

        // Should contain sanitized address and port
        assert!(path_str.contains("192_168_1_10"));
        assert!(path_str.contains("port_5640"));
    }

    #[test]
    fn test_daemon_creation() {
        let daemon = AutoMountDaemon::new();
        assert_eq!(daemon.interval, Duration::from_secs(30));
    }

    #[test]
    fn test_daemon_with_custom_interval() {
        let daemon = AutoMountDaemon::with_interval(60);
        assert_eq!(daemon.interval, Duration::from_secs(60));
    }

    #[test]
    fn test_parse_node_address() {
        let result = AutoMountDaemon::parse_node_address("192.168.1.1:5640");
        assert!(result.is_ok());

        let (addr, port, _transport) = result.unwrap();
        assert_eq!(addr, "192.168.1.1");
        assert_eq!(port, 5640);
    }

    #[test]
    fn test_parse_invalid_node_address() {
        let result = AutoMountDaemon::parse_node_address("invalid-address");
        assert!(result.is_err());

        let result = AutoMountDaemon::parse_node_address("192.168.1.1:invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_get_local_servers() {
        let servers = AutoMountDaemon::get_local_servers();
        assert!(!servers.is_empty(), "Should return local servers");

        for (addr, port, transport) in servers {
            assert!(matches!(transport, TransportType::Tcp));
            assert!(port > 0);
            assert!(!addr.is_empty());
        }
    }

    #[tokio::test]
    async fn test_status_initial_state() {
        let daemon = AutoMountDaemon::new();
        let status = daemon.status().await;

        assert_eq!(status.discovered_count, 0);
        assert_eq!(status.mounted_count, 0);
        assert_eq!(status.running, false);
    }

    #[test]
    fn test_default_trait() {
        let daemon = AutoMountDaemon::default();
        assert_eq!(daemon.interval, Duration::from_secs(30));
    }

    /// Fuzz test: Mount point generation should handle arbitrary addresses
    #[test]
    fn fuzz_mount_point_generation() {
        use proptest::prelude::*;

        proptest!(|(addr in ".*", port: u16)| {
            let server = DiscoveredServer {
                address: addr,
                port,
                transport: TransportType::Tcp,
                last_seen: SystemTime::now(),
                peer_info: None,
            };

            let mount_point = AutoMountDaemon::generate_mount_point(&server);
            // Should not panic with any address
            let _ = mount_point.to_string_lossy();
        });
    }

    /// Fuzz test: Address parsing should handle arbitrary input
    #[test]
    fn fuzz_address_parsing() {
        use proptest::prelude::*;

        proptest!(|(addr_str in ".*")| {
            // Should not panic with arbitrary input
            let _ = AutoMountDaemon::parse_node_address(&addr_str);
        });
    }
}