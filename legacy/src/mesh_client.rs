//! Mesh-aware client that automatically discovers and connects to peers
//!
//! Provides seamless access to remote filesystems through mesh discovery

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use anyhow::Result;
use tracing::{info, warn, error};

use crate::mesh::MeshMessage;
use crate::client::NinePClient;
use tokio::sync::mpsc;

/// Re-export from mesh module
pub use crate::mesh::DiscoveredPeer;

/// Mesh-aware 9P.e client with automatic discovery
pub struct MeshClient {
    /// Active connections to peers
    connections: Arc<RwLock<HashMap<String, NinePClient>>>,
    /// Sender for mesh messages
    mesh_sender: Option<mpsc::UnboundedSender<MeshMessage>>,
    /// Reference to mesh network's discovered peers
    discovered_peers: Option<Arc<RwLock<HashMap<String, DiscoveredPeer>>>>,
}

impl MeshClient {
    /// Create a new mesh client with mesh sender and discovered peers
    pub async fn new_with_mesh(
        mesh_sender: Option<mpsc::UnboundedSender<MeshMessage>>,
        discovered_peers: Option<Arc<RwLock<HashMap<String, DiscoveredPeer>>>>
    ) -> Result<Self> {
        info!("🌐 Starting mesh client with automatic discovery");

        Ok(Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            mesh_sender,
            discovered_peers,
        })
    }

    /// Create a new mesh client with optional mesh sender (legacy)
    pub async fn new_with_sender(mesh_sender: Option<mpsc::UnboundedSender<MeshMessage>>) -> Result<Self> {
        Self::new_with_mesh(mesh_sender, None).await
    }

    /// Create a new mesh client without mesh support (for backwards compatibility)
    pub async fn new() -> Result<Self> {
        Self::new_with_mesh(None, None).await
    }

    /// Get list of discovered peers from mesh network
    pub async fn list_peers(&self) -> Vec<DiscoveredPeer> {
        if let Some(discovered_peers) = &self.discovered_peers {
            // Return actual discovered peers from mesh network
            discovered_peers.read().await.values().cloned().collect()
        } else {
            // Fallback to empty list if no mesh network available
            warn!("list_peers() called without mesh network - using fallback network scan");
            Vec::new()
        }
    }

    /// List files on any discovered peer or specific server
    pub async fn list(&mut self, target: Option<&str>, path: &str) -> Result<Vec<String>> {
        // If specific server provided, use it
        if let Some(server) = target {
            if server.contains(':') {
                // Direct server address
                return self.list_direct(server, path).await;
            } else {
                // Peer ID - look up in discovered peers
                return self.list_by_peer_id(server, path).await;
            }
        }

        // Otherwise, list from first available peer
        let mut peers = self.list_peers().await;
        if peers.is_empty() {
            // No peers discovered yet - try local network scan
            info!("🔍 No peers discovered yet, scanning local network...");
            self.scan_local_network().await?;

            peers = self.list_peers().await;
            if peers.is_empty() {
                return Err(anyhow::anyhow!("No 9P.e servers found on network"));
            }
        }

        // Use first available peer's 9P service address
        let peer = peers[0].clone();
        info!("📁 Using discovered peer: {} ({})", peer.node_id, peer.service_addr);
        self.list_direct(&peer.service_addr, path).await
    }

    /// Direct connection to a server
    async fn list_direct(&mut self, server: &str, path: &str) -> Result<Vec<String>> {
        // Check if we have an existing connection
        let mut connections = self.connections.write().await;

        if !connections.contains_key(server) {
            info!("🔗 Connecting to {}", server);
            let client = NinePClient::connect(server).await?;
            connections.insert(server.to_string(), client);
        }

        let client = connections.get_mut(server)
            .ok_or_else(|| anyhow::anyhow!("Failed to get connection"))?;

        client.list_directory(path).await
    }

    /// List files by peer ID
    async fn list_by_peer_id(&mut self, peer_id: &str, path: &str) -> Result<Vec<String>> {
        let peers = self.list_peers().await;

        for peer in peers {
            if peer.node_id == peer_id || peer.node_id.starts_with(peer_id) {
                // Use the 9P service address, not the mesh address
                return self.list_direct(&peer.service_addr, path).await;
            }
        }

        Err(anyhow::anyhow!("Peer not found: {}", peer_id))
    }

    /// Scan local network for 9P.e servers (fallback when no mesh)
    pub async fn scan_local_network(&self) -> Result<()> {
        info!("🔍 Mesh discovery not available, using fallback scanning...");

        // In a real implementation, this would trigger mDNS discovery
        // or a quick scan of common ports
        // For now, just log that mesh is preferred

        warn!("Network scanning is deprecated. Please run servers with --mesh-port for automatic discovery.");

        Ok(())
    }

    /// Get local IP address
    fn get_local_ip(&self) -> Result<String> {
        // Simple heuristic - get first non-loopback IPv4
        use std::net::IpAddr;

        for iface in if_addrs::get_if_addrs()? {
            if !iface.is_loopback() {
                if let IpAddr::V4(ipv4) = iface.ip() {
                    return Ok(ipv4.to_string());
                }
            }
        }

        Ok("192.168.1.1".to_string()) // Fallback
    }

    /// Mount remote filesystem (FUSE support needed)
    pub async fn mount(&mut self, target: Option<&str>, mount_point: &str) -> Result<()> {
        let server = if let Some(t) = target {
            t.to_string()
        } else {
            // Auto-discover and use first peer
            let peers = self.list_peers().await;
            if peers.is_empty() {
                self.scan_local_network().await?;
                let peers = self.list_peers().await;
                if peers.is_empty() {
                    return Err(anyhow::anyhow!("No 9P.e servers found"));
                }
            }
            peers[0].listen_addr.clone()
        };

        info!("🗻 Mounting {} at {}", server, mount_point);

        // TODO: Implement actual FUSE mounting
        warn!("FUSE mounting not yet implemented - use list command instead");

        Ok(())
    }

    /// Get file from remote server
    pub async fn get(&mut self, target: Option<&str>, remote: &str, local: &str) -> Result<()> {
        let server = if let Some(t) = target {
            t.to_string()
        } else {
            // Auto-discover
            let peers = self.list_peers().await;
            if peers.is_empty() {
                return Err(anyhow::anyhow!("No peers discovered"));
            }
            peers[0].listen_addr.clone()
        };

        info!("📥 Downloading {} from {} to {}", remote, server, local);

        // Get connection
        let mut connections = self.connections.write().await;
        if !connections.contains_key(&server) {
            let client = NinePClient::connect(&server).await?;
            connections.insert(server.clone(), client);
        }

        let client = connections.get_mut(&server)
            .ok_or_else(|| anyhow::anyhow!("Failed to get connection"))?;

        // Read file
        let data = client.read_file(remote).await?;

        // Write to local file
        tokio::fs::write(local, data).await?;

        info!("✅ Downloaded {} bytes", std::fs::metadata(local)?.len());

        Ok(())
    }
}

/// CLI commands for mesh operations
pub mod commands {
    use super::*;

    /// List discovered peers
    pub async fn mesh_list() -> Result<()> {
        let client = MeshClient::new().await?;

        // Wait a bit for discovery
        tokio::time::sleep(Duration::from_secs(2)).await;

        let peers = client.list_peers().await;

        if peers.is_empty() {
            info!("No peers discovered yet. Scanning network...");
            client.scan_local_network().await?;
            let peers = client.list_peers().await;

            if peers.is_empty() {
                info!("No 9P.e servers found on network");
                return Ok(());
            }
        }

        info!("📡 Discovered {} peers:", peers.len());
        for peer in peers {
            info!("  {} - service at {}",
                  peer.node_id,
                  peer.service_addr
            );
        }

        Ok(())
    }

    /// Auto-connect and list files
    pub async fn auto_list(path: &str) -> Result<()> {
        let mut client = MeshClient::new().await?;

        // Wait for discovery
        tokio::time::sleep(Duration::from_secs(1)).await;

        match client.list(None, path).await {
            Ok(files) => {
                if files.is_empty() {
                    info!("📭 Directory is empty");
                } else {
                    info!("📁 Files in {}:", path);
                    for file in files {
                        info!("  📄 {}", file);
                    }
                }
            }
            Err(e) => {
                error!("❌ Failed to list files: {}", e);
            }
        }

        Ok(())
    }

    /// Auto-connect and get file
    pub async fn auto_get(remote: &str, local: &str) -> Result<()> {
        let mut client = MeshClient::new().await?;

        // Wait for discovery
        tokio::time::sleep(Duration::from_secs(1)).await;

        client.get(None, remote, local).await?;

        Ok(())
    }
}