//! Mesh networking for peer-to-peer communication
//!
//! Simple TCP-based mesh protocol for node discovery and communication

use anyhow::{Result, Context};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tokio::time::{Duration, interval};
use tracing::{info, error, debug, warn};

/// Mesh network coordinator
pub struct MeshNetwork {
    node_id: String,
    local_port: u16,
    peers: Arc<RwLock<HashMap<String, PeerConnection>>>,
    bootstrap_peers: Vec<String>,
}

impl MeshNetwork {
    pub fn new(node_id: String, local_port: u16, bootstrap_peers: Vec<String>) -> Self {
        Self {
            node_id,
            local_port,
            peers: Arc::new(RwLock::new(HashMap::new())),
            bootstrap_peers,
        }
    }

    /// Start the mesh network
    pub async fn start(self: Arc<Self>) -> Result<()> {
        info!("Starting mesh network on port {} with {} bootstrap peers",
              self.local_port, self.bootstrap_peers.len());

        // Start listener for incoming connections
        let listener_self = Arc::clone(&self);
        tokio::spawn(async move {
            if let Err(e) = listener_self.run_listener().await {
                error!("Mesh listener error: {}", e);
            }
        });

        // Connect to bootstrap peers
        let connector_self = Arc::clone(&self);
        tokio::spawn(async move {
            connector_self.connect_to_bootstrap_peers().await;
        });

        // Start periodic heartbeat
        let heartbeat_self = Arc::clone(&self);
        tokio::spawn(async move {
            heartbeat_self.run_heartbeat().await;
        });

        Ok(())
    }

    async fn run_listener(self: Arc<Self>) -> Result<()> {
        let addr = SocketAddr::from(([0, 0, 0, 0], self.local_port));
        let listener = TcpListener::bind(addr).await
            .context("Failed to bind mesh listener")?;

        info!("Mesh network listening on port {}", self.local_port);

        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    debug!("Incoming mesh connection from {}", peer_addr);
                    let self_clone = Arc::clone(&self);
                    tokio::spawn(async move {
                        if let Err(e) = self_clone.handle_incoming_connection(stream, peer_addr).await {
                            debug!("Mesh connection error from {}: {}", peer_addr, e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept mesh connection: {}", e);
                }
            }
        }
    }

    async fn handle_incoming_connection(&self, mut stream: TcpStream, peer_addr: SocketAddr) -> Result<()> {
        // Read handshake message
        let mut size_buf = [0u8; 4];
        stream.read_exact(&mut size_buf).await?;
        let msg_size = u32::from_le_bytes(size_buf);

        if msg_size > 1024 * 1024 {
            anyhow::bail!("Message too large: {}", msg_size);
        }

        let mut msg_buf = vec![0u8; msg_size as usize];
        stream.read_exact(&mut msg_buf).await?;

        let message: MeshMessage = bincode::deserialize(&msg_buf)?;

        match message {
            MeshMessage::Handshake { node_id, version } => {
                info!("Peer {} connected from {} (version {})", node_id, peer_addr, version);

                // Send handshake response
                let response = MeshMessage::HandshakeAck {
                    node_id: self.node_id.clone(),
                    version: 1,
                };
                self.send_message(&mut stream, &response).await?;

                // Store peer connection
                let mut peers = self.peers.write().await;
                peers.insert(node_id.clone(), PeerConnection {
                    node_id: node_id.clone(),
                    address: peer_addr,
                    last_seen: std::time::Instant::now(),
                });
                drop(peers);

                // Handle messages from this peer
                loop {
                    match self.receive_message(&mut stream).await {
                        Ok(msg) => {
                            self.handle_message(&node_id, msg).await?;
                        }
                        Err(e) => {
                            debug!("Peer {} disconnected: {}", node_id, e);
                            break;
                        }
                    }
                }

                // Remove peer on disconnect
                let mut peers = self.peers.write().await;
                peers.remove(&node_id);
                info!("Peer {} disconnected", node_id);
            }
            _ => {
                warn!("Expected handshake, got {:?}", message);
            }
        }

        Ok(())
    }

    async fn connect_to_bootstrap_peers(&self) {
        for peer_addr_str in &self.bootstrap_peers {
            let peer_addr_str = peer_addr_str.clone();
            let self_clone = self.node_id.clone();
            let peers_clone = Arc::clone(&self.peers);

            tokio::spawn(async move {
                // Parse address
                let addr: SocketAddr = match peer_addr_str.parse() {
                    Ok(a) => a,
                    Err(e) => {
                        error!("Invalid peer address {}: {}", peer_addr_str, e);
                        return;
                    }
                };

                // Retry connection with backoff
                let mut backoff = Duration::from_secs(1);
                loop {
                    match TcpStream::connect(addr).await {
                        Ok(mut stream) => {
                            info!("Connected to peer at {}", addr);

                            // Send handshake
                            let handshake = MeshMessage::Handshake {
                                node_id: self_clone.clone(),
                                version: 1,
                            };

                            if let Err(e) = Self::send_message_static(&mut stream, &handshake).await {
                                error!("Failed to send handshake to {}: {}", addr, e);
                                tokio::time::sleep(backoff).await;
                                backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
                                continue;
                            }

                            // Wait for handshake ack
                            match Self::receive_message_static(&mut stream).await {
                                Ok(MeshMessage::HandshakeAck { node_id, .. }) => {
                                    info!("Handshake complete with peer {} at {}", node_id, addr);

                                    // Store connection
                                    let mut peers = peers_clone.write().await;
                                    peers.insert(node_id.clone(), PeerConnection {
                                        node_id: node_id.clone(),
                                        address: addr,
                                        last_seen: std::time::Instant::now(),
                                    });
                                    drop(peers);

                                    // Keep connection alive - read messages
                                    loop {
                                        match Self::receive_message_static(&mut stream).await {
                                            Ok(msg) => {
                                                debug!("Received message from {}: {:?}", node_id, msg);
                                                // Update last_seen
                                                let mut peers = peers_clone.write().await;
                                                if let Some(peer) = peers.get_mut(&node_id) {
                                                    peer.last_seen = std::time::Instant::now();
                                                }
                                            }
                                            Err(e) => {
                                                info!("Peer {} disconnected: {}", node_id, e);
                                                break;
                                            }
                                        }
                                    }

                                    // Remove peer
                                    let mut peers = peers_clone.write().await;
                                    peers.remove(&node_id);
                                }
                                Ok(msg) => {
                                    warn!("Expected HandshakeAck, got {:?}", msg);
                                }
                                Err(e) => {
                                    error!("Failed to receive handshake ack from {}: {}", addr, e);
                                }
                            }

                            // Reconnect after delay
                            tokio::time::sleep(backoff).await;
                            backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
                        }
                        Err(e) => {
                            debug!("Failed to connect to peer {}: {} (retrying in {:?})", addr, e, backoff);
                            tokio::time::sleep(backoff).await;
                            backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
                        }
                    }
                }
            });
        }
    }

    async fn run_heartbeat(&self) {
        let mut ticker = interval(Duration::from_secs(30));
        loop {
            ticker.tick().await;

            let peers = self.peers.read().await;
            let peer_count = peers.len();
            if peer_count > 0 {
                debug!("Mesh network: {} connected peers", peer_count);
                for (peer_id, peer) in peers.iter() {
                    let elapsed = peer.last_seen.elapsed();
                    debug!("  - {} at {} (last seen {:?} ago)", peer_id, peer.address, elapsed);
                }
            }
        }
    }

    async fn handle_message(&self, from_peer: &str, message: MeshMessage) -> Result<()> {
        match message {
            MeshMessage::Ping => {
                debug!("Received ping from {}", from_peer);
                // Update last_seen
                let mut peers = self.peers.write().await;
                if let Some(peer) = peers.get_mut(from_peer) {
                    peer.last_seen = std::time::Instant::now();
                }
            }
            MeshMessage::PeerList { peers: peer_list } => {
                info!("Received peer list from {}: {} peers", from_peer, peer_list.len());
                // TODO: Add new peers to bootstrap list
            }
            _ => {
                debug!("Received message from {}: {:?}", from_peer, message);
            }
        }
        Ok(())
    }

    async fn send_message(&self, stream: &mut TcpStream, message: &MeshMessage) -> Result<()> {
        Self::send_message_static(stream, message).await
    }

    async fn send_message_static(stream: &mut TcpStream, message: &MeshMessage) -> Result<()> {
        let data = bincode::serialize(message)?;
        let size = data.len() as u32;
        stream.write_all(&size.to_le_bytes()).await?;
        stream.write_all(&data).await?;
        stream.flush().await?;
        Ok(())
    }

    async fn receive_message(&self, stream: &mut TcpStream) -> Result<MeshMessage> {
        Self::receive_message_static(stream).await
    }

    async fn receive_message_static(stream: &mut TcpStream) -> Result<MeshMessage> {
        let mut size_buf = [0u8; 4];
        stream.read_exact(&mut size_buf).await?;
        let msg_size = u32::from_le_bytes(size_buf);

        if msg_size > 1024 * 1024 {
            anyhow::bail!("Message too large: {}", msg_size);
        }

        let mut msg_buf = vec![0u8; msg_size as usize];
        stream.read_exact(&mut msg_buf).await?;

        let message: MeshMessage = bincode::deserialize(&msg_buf)?;
        Ok(message)
    }

    pub async fn get_peer_count(&self) -> usize {
        self.peers.read().await.len()
    }

    pub async fn get_connected_peers(&self) -> Vec<String> {
        self.peers.read().await.keys().cloned().collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum MeshMessage {
    Handshake {
        node_id: String,
        version: u32,
    },
    HandshakeAck {
        node_id: String,
        version: u32,
    },
    Ping,
    Pong,
    PeerList {
        peers: Vec<String>,
    },
    ConsensusBlock {
        block_data: Vec<u8>,
    },
}

struct PeerConnection {
    node_id: String,
    address: SocketAddr,
    last_seen: std::time::Instant,
}
