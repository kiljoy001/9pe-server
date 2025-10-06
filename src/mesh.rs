//! Mesh networking for peer-to-peer communication
//!
//! Automatic peer discovery using mDNS + TCP mesh protocol

use anyhow::{Result, Context};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::net::{SocketAddr, IpAddr};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tokio::time::{Duration, interval};
use tracing::{info, error, debug, warn};
use mdns_sd::{ServiceDaemon, ServiceEvent, ScopedIp};

/// Mesh network coordinator with DHT and mDNS discovery
pub struct MeshNetwork {
    node_id: String,
    local_port: u16,
    peers: Arc<RwLock<HashMap<String, PeerConnection>>>,
    bootstrap_peers: Vec<String>,
    mdns_daemon: Option<ServiceDaemon>,
    dht: Arc<RwLock<KademliaTable>>,
}

impl MeshNetwork {
    pub fn new(node_id: String, local_port: u16, bootstrap_peers: Vec<String>) -> Self {
        Self {
            node_id: node_id.clone(),
            local_port,
            peers: Arc::new(RwLock::new(HashMap::new())),
            bootstrap_peers,
            mdns_daemon: None,
            dht: Arc::new(RwLock::new(KademliaTable::new(&node_id))),
        }
    }

    /// Start the mesh network with DHT and mDNS discovery
    pub async fn start(self: Arc<Self>) -> Result<()> {
        info!("Starting mesh network on port {} with DHT and mDNS discovery",
              self.local_port);

        // Start listener for incoming connections
        let listener_self = Arc::clone(&self);
        tokio::spawn(async move {
            if let Err(e) = listener_self.run_listener().await {
                error!("Mesh listener error: {}", e);
            }
        });

        // Start mDNS discovery
        let mdns_self = Arc::clone(&self);
        tokio::spawn(async move {
            mdns_self.run_mdns_discovery().await;
        });

        // Start DHT discovery (use bootstrap peers as initial DHT seeds)
        let dht_self = Arc::clone(&self);
        tokio::spawn(async move {
            dht_self.run_dht_discovery().await;
        });

        // Connect to bootstrap peers (for DHT seeding)
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

    async fn run_mdns_discovery(&self) {
        info!("Starting mDNS service discovery on local network");

        match ServiceDaemon::new() {
            Ok(mdns) => {
                let service_type = "_9pe._tcp.local.";
                let instance_name = format!("9pe-{}", self.node_id);
                let host_name = format!("{}.local.", self.node_id);

                info!("Advertising mDNS service: {}", instance_name);

                // Get local IP addresses
                let local_addrs: Vec<IpAddr> = if_addrs::get_if_addrs()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|iface| iface.ip())
                    .collect();

                // Register our service
                match mdns_sd::ServiceInfo::new(
                    service_type,
                    &instance_name,
                    &host_name,
                    &local_addrs[..],
                    self.local_port,
                    &[("node_id", self.node_id.as_str())][..],
                ) {
                    Ok(service_info) => {
                        if let Err(e) = mdns.register(service_info) {
                            warn!("Failed to register mDNS service: {}", e);
                        } else {
                            info!("mDNS service registered successfully");
                        }
                    }
                    Err(e) => {
                        warn!("Failed to create mDNS service info: {}", e);
                    }
                }

                // Browse for other 9P.e nodes
                let receiver = match mdns.browse(service_type) {
                    Ok(rx) => rx,
                    Err(e) => {
                        error!("Failed to browse mDNS services: {}", e);
                        return;
                    }
                };

                while let Ok(event) = receiver.recv() {
                    match event {
                        ServiceEvent::ServiceResolved(info) => {
                            info!("mDNS discovered peer: {} at {:?}", info.get_fullname(), info.get_addresses());

                            // Connect to discovered peers
                            for scoped_ip in info.get_addresses() {
                                let ip_addr = match scoped_ip {
                                    ScopedIp::V4(v4) => IpAddr::V4(*v4.addr()),
                                    ScopedIp::V6(v6) => IpAddr::V6(*v6.addr()),
                                    _ => continue,  // Unknown variant, skip
                                };

                                let peer_addr = SocketAddr::new(ip_addr, self.local_port);
                                info!("Auto-connecting to mDNS peer at {}", peer_addr);

                                // TODO: Trigger connection to discovered peer
                                // For now, peers will connect when they see us via their own mDNS
                            }
                        }
                        ServiceEvent::ServiceRemoved(_, fullname) => {
                            debug!("mDNS peer removed: {}", fullname);
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                warn!("mDNS not available: {} (continuing with DHT only)", e);
            }
        }
    }

    async fn run_dht_discovery(&self) {
        info!("Starting DHT peer discovery");

        let mut ticker = interval(Duration::from_secs(60));
        loop {
            ticker.tick().await;

            // Query DHT for nearby peers
            use sha2::{Sha256, Digest};
            let mut hasher = Sha256::new();
            hasher.update(self.node_id.as_bytes());
            let our_id: [u8; 32] = hasher.finalize().into();

            let dht = self.dht.read().await;
            let nearby_peers = dht.find_closest(&our_id, 20);
            drop(dht);

            debug!("DHT has {} total peers", nearby_peers.len());

            // Refresh DHT by querying random nodes
            for peer in nearby_peers.iter().take(3) {
                debug!("Refreshing DHT via {}", peer.address);
                // TODO: Send FindNode RPC to peer
            }
        }
    }

    async fn run_heartbeat(&self) {
        let mut ticker = interval(Duration::from_secs(30));
        loop {
            ticker.tick().await;

            let peers = self.peers.read().await;
            let peer_count = peers.len();

            let dht = self.dht.read().await;
            let dht_count = dht.get_all_peers().len();
            drop(dht);

            if peer_count > 0 || dht_count > 0 {
                debug!("Mesh network: {} active connections, {} DHT peers", peer_count, dht_count);
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
            MeshMessage::FindNode { target } => {
                debug!("Received DHT FindNode query from {} for {:?}", from_peer, &target[..8]);

                // Find closest peers from our DHT
                let dht = self.dht.read().await;
                let closest = dht.find_closest(&target, 20);
                drop(dht);

                let peer_list: Vec<(SocketAddr, [u8; 32])> = closest
                    .iter()
                    .map(|p| (p.address, p.id))
                    .collect();

                debug!("Replying with {} DHT peers", peer_list.len());
                // TODO: Send FindNodeReply back to from_peer
            }
            MeshMessage::FindNodeReply { peers } => {
                debug!("Received DHT FindNodeReply from {} with {} peers", from_peer, peers.len());

                let peer_count = peers.len();

                // Add all returned peers to our DHT
                let mut dht = self.dht.write().await;
                for (addr, id) in peers {
                    dht.add_peer(id, addr);
                }
                drop(dht);

                info!("Added {} peers to DHT from {}", peer_count, from_peer);
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
    // DHT messages
    FindNode {
        target: [u8; 32],
    },
    FindNodeReply {
        peers: Vec<(SocketAddr, [u8; 32])>,
    },
}

struct PeerConnection {
    node_id: String,
    address: SocketAddr,
    last_seen: std::time::Instant,
}

/// Kademlia DHT for distributed peer discovery
struct KademliaTable {
    local_id: [u8; 32],  // SHA-256 of node_id
    buckets: Vec<Vec<KademliaPeer>>,  // 256 k-buckets
}

#[derive(Clone, Debug)]
struct KademliaPeer {
    id: [u8; 32],
    address: SocketAddr,
    last_seen: std::time::Instant,
}

impl KademliaTable {
    fn new(node_id: &str) -> Self {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(node_id.as_bytes());
        let local_id: [u8; 32] = hasher.finalize().into();

        Self {
            local_id,
            buckets: vec![Vec::new(); 256],
        }
    }

    fn add_peer(&mut self, id: [u8; 32], address: SocketAddr) {
        let bucket_idx = self.bucket_index(&id);
        let bucket = &mut self.buckets[bucket_idx];

        // Check if peer already exists
        if let Some(peer) = bucket.iter_mut().find(|p| p.id == id) {
            peer.last_seen = std::time::Instant::now();
            peer.address = address;
            return;
        }

        // Add new peer (keep bucket size limited to 20)
        if bucket.len() < 20 {
            bucket.push(KademliaPeer {
                id,
                address,
                last_seen: std::time::Instant::now(),
            });
        }
    }

    fn bucket_index(&self, target: &[u8; 32]) -> usize {
        // XOR distance and find first differing bit
        for i in 0..32 {
            let xor = self.local_id[i] ^ target[i];
            if xor != 0 {
                return (i * 8) + (7 - xor.leading_zeros() as usize);
            }
        }
        0
    }

    fn find_closest(&self, target: &[u8; 32], count: usize) -> Vec<KademliaPeer> {
        let mut all_peers: Vec<_> = self.buckets
            .iter()
            .flat_map(|bucket| bucket.iter().cloned())
            .collect();

        // Sort by XOR distance
        all_peers.sort_by_key(|peer| {
            let mut distance = [0u8; 32];
            for i in 0..32 {
                distance[i] = peer.id[i] ^ target[i];
            }
            distance
        });

        all_peers.into_iter().take(count).collect()
    }

    fn get_all_peers(&self) -> Vec<KademliaPeer> {
        self.buckets
            .iter()
            .flat_map(|bucket| bucket.iter().cloned())
            .collect()
    }
}
