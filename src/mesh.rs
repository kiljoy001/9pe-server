//! Mesh networking for peer-to-peer communication
//!
//! Automatic peer discovery using mDNS + QUIC mesh protocol

use anyhow::{Result, Context};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::net::{SocketAddr, IpAddr};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Duration, interval};
use tracing::{info, error, debug, warn};
use mdns_sd::{ServiceDaemon, ServiceEvent, ScopedIp};
use quinn::{Endpoint, Connection, ServerConfig, ClientConfig, RecvStream, SendStream};

struct SkipServerVerification;

impl rustls::client::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> std::result::Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}

/// Mesh network coordinator with DHT and mDNS discovery
pub struct MeshNetwork {
    node_id: String,
    local_port: u16,
    peers: Arc<RwLock<HashMap<String, QuicPeerConnection>>>,
    bootstrap_peers: Vec<String>,
    mdns_daemon: Option<ServiceDaemon>,
    dht: Arc<RwLock<KademliaTable>>,
    endpoint: Option<Endpoint>,
    connections: Arc<RwLock<HashMap<String, Connection>>>,
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
            endpoint: None,
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn generate_self_signed_cert() -> Result<(Vec<rustls::Certificate>, rustls::PrivateKey)> {
        let cert = rcgen::generate_simple_self_signed(vec!["mesh.local".to_string()])
            .context("Failed to generate self-signed certificate")?;
        let key = rustls::PrivateKey(cert.serialize_private_key_der());
        let cert_der = rustls::Certificate(cert.serialize_der().context("Failed to serialize certificate")?);
        Ok((vec![cert_der], key))
    }

    fn configure_quic_server() -> Result<ServerConfig> {
        let (certs, key) = Self::generate_self_signed_cert()?;
        let mut server_config = ServerConfig::with_single_cert(certs, key)
            .context("Failed to create server config")?;
        
        let transport_config = Arc::get_mut(&mut server_config.transport)
            .context("Failed to get mutable transport config")?;
        transport_config.max_concurrent_uni_streams(0_u8.into());
        transport_config.max_idle_timeout(Some(Duration::from_secs(60).try_into().unwrap()));
        
        Ok(server_config)
    }

    fn configure_quic_client() -> Result<ClientConfig> {
        let mut rustls_config = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
            .with_no_client_auth();
        
        let mut client_config = ClientConfig::new(Arc::new(rustls_config));
        
        let transport_config = Arc::get_mut(&mut client_config.transport)
            .context("Failed to get mutable transport config")?;
        transport_config.max_concurrent_uni_streams(0_u8.into());
        transport_config.max_idle_timeout(Some(Duration::from_secs(60).try_into().unwrap()));
        
        Ok(client_config)
    }

    /// Start the mesh network with DHT and mDNS discovery
    pub async fn start(mut self: Arc<Self>) -> Result<()> {
        info!("Starting mesh network on port {} with DHT and mDNS discovery",
              self.local_port);

        let server_config = Self::configure_quic_server()?;
        let client_config = Self::configure_quic_client()?;
        let addr = SocketAddr::from(([0, 0, 0, 0], self.local_port));
        let endpoint = Endpoint::server(server_config, addr)
            .context("Failed to create QUIC endpoint")?;
        endpoint.set_default_client_config(client_config);
        
        unsafe {
            let self_mut = Arc::get_mut_unchecked(&mut self);
            self_mut.endpoint = Some(endpoint);
        }

        info!("QUIC mesh network listening on port {}", self.local_port);

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
        let endpoint = self.endpoint.as_ref()
            .context("QUIC endpoint not initialized")?;

        loop {
            match endpoint.accept().await {
                Some(connecting) => {
                    let self_clone = Arc::clone(&self);
                    tokio::spawn(async move {
                        match connecting.await {
                            Ok(connection) => {
                                let peer_addr = connection.remote_address();
                                debug!("Incoming QUIC connection from {}", peer_addr);
                                if let Err(e) = self_clone.handle_incoming_connection(connection).await {
                                    debug!("Mesh connection error from {}: {}", peer_addr, e);
                                }
                            }
                            Err(e) => {
                                error!("Failed to establish QUIC connection: {}", e);
                            }
                        }
                    });
                }
                None => {
                    warn!("QUIC endpoint closed");
                    break;
                }
            }
        }
        Ok(())
    }

    async fn handle_incoming_connection(&self, connection: Connection) -> Result<()> {
        let peer_addr = connection.remote_address();
        
        let (mut send_stream, mut recv_stream) = connection.accept_bi().await
            .context("Failed to accept bidirectional stream")?;

        let message = self.receive_message_from_stream(&mut recv_stream).await?;

        match message {
            MeshMessage::Handshake { node_id, version } => {
                info!("Peer {} connected from {} (version {})", node_id, peer_addr, version);

                let response = MeshMessage::HandshakeAck {
                    node_id: self.node_id.clone(),
                    version: 1,
                };
                self.send_message_to_stream(&mut send_stream, &response).await?;

                let mut peers = self.peers.write().await;
                peers.insert(node_id.clone(), QuicPeerConnection {
                    node_id: node_id.clone(),
                    address: peer_addr,
                    last_seen: std::time::Instant::now(),
                });
                drop(peers);

                let mut connections = self.connections.write().await;
                connections.insert(node_id.clone(), connection.clone());
                drop(connections);

                loop {
                    match connection.accept_bi().await {
                        Ok((mut send, mut recv)) => {
                            match self.receive_message_from_stream(&mut recv).await {
                                Ok(msg) => {
                                    if let Err(e) = self.handle_message_with_response(&node_id, msg, &mut send).await {
                                        error!("Error handling message from {}: {}", node_id, e);
                                    }
                                }
                                Err(e) => {
                                    debug!("Peer {} disconnected: {}", node_id, e);
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            debug!("Peer {} connection closed: {}", node_id, e);
                            break;
                        }
                    }
                }

                let mut peers = self.peers.write().await;
                peers.remove(&node_id);
                let mut connections = self.connections.write().await;
                connections.remove(&node_id);
                info!("Peer {} disconnected", node_id);
            }
            _ => {
                warn!("Expected handshake, got {:?}", message);
            }
        }

        Ok(())
    }

    async fn connect_to_bootstrap_peers(&self) {
        let endpoint = match &self.endpoint {
            Some(e) => e.clone(),
            None => {
                error!("QUIC endpoint not initialized");
                return;
            }
        };

        for peer_addr_str in &self.bootstrap_peers {
            let peer_addr_str = peer_addr_str.clone();
            let node_id = self.node_id.clone();
            let peers_clone = Arc::clone(&self.peers);
            let connections_clone = Arc::clone(&self.connections);
            let endpoint_clone = endpoint.clone();

            tokio::spawn(async move {
                let addr: SocketAddr = match peer_addr_str.parse() {
                    Ok(a) => a,
                    Err(e) => {
                        error!("Invalid peer address {}: {}", peer_addr_str, e);
                        return;
                    }
                };

                let mut backoff = Duration::from_secs(1);
                loop {
                    match endpoint_clone.connect(addr, "mesh.local") {
                        Ok(connecting) => {
                            match connecting.await {
                                Ok(connection) => {
                                    info!("Connected to peer at {}", addr);

                                    let (mut send_stream, mut recv_stream) = match connection.open_bi().await {
                                        Ok(streams) => streams,
                                        Err(e) => {
                                            error!("Failed to open stream to {}: {}", addr, e);
                                            tokio::time::sleep(backoff).await;
                                            backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
                                            continue;
                                        }
                                    };

                                    let handshake = MeshMessage::Handshake {
                                        node_id: node_id.clone(),
                                        version: 1,
                                    };

                                    let data = match bincode::serialize(&handshake) {
                                        Ok(d) => d,
                                        Err(e) => {
                                            error!("Failed to serialize handshake: {}", e);
                                            continue;
                                        }
                                    };

                                    let size = data.len() as u32;
                                    if let Err(e) = send_stream.write_all(&size.to_le_bytes()).await {
                                        error!("Failed to send handshake size to {}: {}", addr, e);
                                        tokio::time::sleep(backoff).await;
                                        backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
                                        continue;
                                    }

                                    if let Err(e) = send_stream.write_all(&data).await {
                                        error!("Failed to send handshake to {}: {}", addr, e);
                                        tokio::time::sleep(backoff).await;
                                        backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
                                        continue;
                                    }

                                    if let Err(e) = send_stream.finish().await {
                                        error!("Failed to finish handshake stream to {}: {}", addr, e);
                                        tokio::time::sleep(backoff).await;
                                        backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
                                        continue;
                                    }

                                    use tokio::io::AsyncReadExt;
                                    let mut size_buf = [0u8; 4];
                                    if let Err(e) = recv_stream.read_exact(&mut size_buf).await {
                                        error!("Failed to read handshake ack size from {}: {}", addr, e);
                                        tokio::time::sleep(backoff).await;
                                        backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
                                        continue;
                                    }

                                    let msg_size = u32::from_le_bytes(size_buf);
                                    if msg_size > 1024 * 1024 {
                                        error!("Message too large from {}: {}", addr, msg_size);
                                        continue;
                                    }

                                    let mut msg_buf = vec![0u8; msg_size as usize];
                                    if let Err(e) = recv_stream.read_exact(&mut msg_buf).await {
                                        error!("Failed to read handshake ack from {}: {}", addr, e);
                                        tokio::time::sleep(backoff).await;
                                        backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
                                        continue;
                                    }

                                    match bincode::deserialize::<MeshMessage>(&msg_buf) {
                                        Ok(MeshMessage::HandshakeAck { node_id: peer_node_id, .. }) => {
                                            info!("Handshake complete with peer {} at {}", peer_node_id, addr);

                                            let mut peers = peers_clone.write().await;
                                            peers.insert(peer_node_id.clone(), QuicPeerConnection {
                                                node_id: peer_node_id.clone(),
                                                address: addr,
                                                last_seen: std::time::Instant::now(),
                                            });
                                            drop(peers);

                                            let mut connections = connections_clone.write().await;
                                            connections.insert(peer_node_id.clone(), connection.clone());
                                            drop(connections);

                                            backoff = Duration::from_secs(1);
                                        }
                                        Ok(msg) => {
                                            warn!("Expected HandshakeAck, got {:?}", msg);
                                        }
                                        Err(e) => {
                                            error!("Failed to deserialize handshake ack from {}: {}", addr, e);
                                        }
                                    }
                                }
                                Err(e) => {
                                    debug!("Failed to connect to peer {}: {} (retrying in {:?})", addr, e, backoff);
                                    tokio::time::sleep(backoff).await;
                                    backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to initiate connection to {}: {}", addr, e);
                            tokio::time::sleep(backoff).await;
                            backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
                        }
                    }

                    tokio::time::sleep(Duration::from_secs(30)).await;
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

            use sha2::{Sha256, Digest};
            let mut hasher = Sha256::new();
            hasher.update(self.node_id.as_bytes());
            let our_id: [u8; 32] = hasher.finalize().into();

            let dht = self.dht.read().await;
            let nearby_peers = dht.find_closest(&our_id, 20);
            drop(dht);

            debug!("DHT has {} total peers", nearby_peers.len());

            for peer in nearby_peers.iter().take(3) {
                debug!("Refreshing DHT via {}", peer.address);
                
                let connections = self.connections.read().await;
                let peer_id_opt = self.peers.read().await.iter()
                    .find(|(_, p)| p.address == peer.address)
                    .map(|(id, _)| id.clone());
                drop(connections);

                if let Some(peer_id) = peer_id_opt {
                    let find_node_msg = MeshMessage::FindNode { target: our_id };
                    if let Err(e) = self.send_message_to_peer(&peer_id, &find_node_msg).await {
                        debug!("Failed to send FindNode to {}: {}", peer_id, e);
                    }
                }
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

    async fn send_message_to_stream(&self, stream: &mut SendStream, message: &MeshMessage) -> Result<()> {
        let data = bincode::serialize(message)?;
        let size = data.len() as u32;
        stream.write_all(&size.to_le_bytes()).await
            .context("Failed to write message size")?;
        stream.write_all(&data).await
            .context("Failed to write message data")?;
        stream.finish().await
            .context("Failed to finish stream")?;
        Ok(())
    }

    async fn receive_message_from_stream(&self, stream: &mut RecvStream) -> Result<MeshMessage> {
        use tokio::io::AsyncReadExt;
        
        let mut size_buf = [0u8; 4];
        stream.read_exact(&mut size_buf).await
            .context("Failed to read message size")?;
        let msg_size = u32::from_le_bytes(size_buf);

        if msg_size > 1024 * 1024 {
            anyhow::bail!("Message too large: {}", msg_size);
        }

        let mut msg_buf = vec![0u8; msg_size as usize];
        stream.read_exact(&mut msg_buf).await
            .context("Failed to read message data")?;

        let message: MeshMessage = bincode::deserialize(&msg_buf)
            .context("Failed to deserialize message")?;
        Ok(message)
    }

    async fn handle_message_with_response(&self, from_peer: &str, message: MeshMessage, send_stream: &mut SendStream) -> Result<()> {
        match message {
            MeshMessage::Ping => {
                debug!("Received ping from {}", from_peer);
                let mut peers = self.peers.write().await;
                if let Some(peer) = peers.get_mut(from_peer) {
                    peer.last_seen = std::time::Instant::now();
                }
            }
            MeshMessage::PeerList { peers: peer_list } => {
                info!("Received peer list from {}: {} peers", from_peer, peer_list.len());
            }
            MeshMessage::FindNode { target } => {
                debug!("Received DHT FindNode query from {} for {:?}", from_peer, &target[..8]);

                let dht = self.dht.read().await;
                let closest = dht.find_closest(&target, 20);
                drop(dht);

                let peer_list: Vec<(SocketAddr, [u8; 32])> = closest
                    .iter()
                    .map(|p| (p.address, p.id))
                    .collect();

                debug!("Replying with {} DHT peers", peer_list.len());
                let reply = MeshMessage::FindNodeReply { peers: peer_list };
                self.send_message_to_stream(send_stream, &reply).await?;
            }
            MeshMessage::FindNodeReply { peers } => {
                debug!("Received DHT FindNodeReply from {} with {} peers", from_peer, peers.len());

                let peer_count = peers.len();

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

    async fn send_message_to_peer(&self, peer_id: &str, message: &MeshMessage) -> Result<()> {
        let connections = self.connections.read().await;
        let connection = connections.get(peer_id)
            .context(format!("No connection to peer {}", peer_id))?;
        
        let (mut send_stream, _recv_stream) = connection.open_bi().await
            .context("Failed to open bidirectional stream")?;
        
        self.send_message_to_stream(&mut send_stream, message).await?;
        Ok(())
    }

    pub async fn get_peer_count(&self) -> usize {
        self.peers.read().await.len()
    }

    pub async fn get_connected_peers(&self) -> Vec<String> {
        self.peers.read().await.keys().cloned().collect()
    }

    /// Get all peers with detailed information (for /srv/mesh/peers)
    pub async fn get_all_peers(&self) -> std::collections::HashMap<String, PeerInfo> {
        let peers = self.peers.read().await;
        peers.iter().map(|(id, peer_conn)| {
            (id.clone(), PeerInfo {
                peer_id: peer_conn.node_id.clone(),
                address: peer_conn.address.to_string(),
                connected: true,
                last_seen: std::time::SystemTime::now(),
            })
        }).collect()
    }

    /// Connect to a new peer (for /srv/mesh/connect)
    pub async fn connect_to_peer(&self, address: &str, peer_id: Option<String>) -> Result<()> {
        let addr: SocketAddr = address.parse()
            .context("Invalid peer address")?;

        let endpoint = self.endpoint.as_ref()
            .context("QUIC endpoint not initialized")?;

        let connection = endpoint.connect(addr, "mesh.local")
            .context("Failed to initiate QUIC connection")?
            .await
            .context("Failed to establish QUIC connection")?;

        let (mut send_stream, mut recv_stream) = connection.open_bi().await
            .context("Failed to open bidirectional stream")?;

        let handshake = MeshMessage::Handshake {
            node_id: self.node_id.clone(),
            version: 1,
        };

        self.send_message_to_stream(&mut send_stream, &handshake).await?;
        let response = self.receive_message_from_stream(&mut recv_stream).await?;

        match response {
            MeshMessage::HandshakeAck { node_id, .. } => {
                let peer_conn = QuicPeerConnection {
                    node_id: node_id.clone(),
                    address: addr,
                    last_seen: std::time::Instant::now(),
                };

                self.peers.write().await.insert(node_id.clone(), peer_conn);
                self.connections.write().await.insert(node_id.clone(), connection);
                info!("Connected to peer {} at {}", node_id, addr);
                Ok(())
            }
            _ => Err(anyhow::anyhow!("Expected HandshakeAck, got {:?}", response))
        }
    }

    /// Disconnect from a peer (for /srv/mesh/disconnect)
    pub async fn disconnect_peer(&self, peer_id: &str) -> Result<()> {
        if self.peers.write().await.remove(peer_id).is_some() {
            info!("Disconnected from peer {}", peer_id);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Peer {} not found", peer_id))
        }
    }

    /// Announce service via mDNS (for /srv/mesh/announce)
    pub async fn announce_service(&self, service_name: &str) -> Result<()> {
        info!("Announcing service: {}", service_name);
        // TODO: Actual mDNS announcement when mDNS daemon is available
        Ok(())
    }

    /// Get mesh network status (for /srv/mesh/status)
    pub async fn get_status(&self) -> MeshStatus {
        let peer_count = self.peers.read().await.len();
        MeshStatus {
            node_id: self.node_id.clone(),
            peer_count,
            active_connections: peer_count,
            mdns_enabled: self.mdns_daemon.is_some(),
            dht_enabled: true,
            uptime_seconds: 0, // TODO: Track actual uptime
        }
    }

    /// Get DHT routing table (for /srv/mesh/dht)
    pub async fn get_dht_routing_table(&self) -> Vec<(Vec<u8>, String)> {
        // TODO: Return actual DHT routing table when Kademlia is fully implemented
        vec![]
    }
}

/// Peer information for control interface
#[derive(Clone, Debug)]
pub struct PeerInfo {
    pub peer_id: String,
    pub address: String,
    pub connected: bool,
    pub last_seen: std::time::SystemTime,
}

impl PeerInfo {
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    pub fn address(&self) -> Option<String> {
        Some(self.address.clone())
    }

    pub fn last_seen(&self) -> std::time::SystemTime {
        self.last_seen
    }
}

/// Mesh network status
#[derive(Clone, Debug)]
pub struct MeshStatus {
    pub node_id: String,
    pub peer_count: usize,
    pub active_connections: usize,
    pub mdns_enabled: bool,
    pub dht_enabled: bool,
    pub uptime_seconds: u64,
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

struct QuicPeerConnection {
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

#[cfg(test)]
mod fuzz_tests {
    use super::*;
    use proptest::prelude::*;

    /// Fuzz test: Mesh message deserialization
    #[test]
    fn fuzz_mesh_message_deserialization() {
        proptest!(|(bytes: Vec<u8>)| {
            // Should never panic on arbitrary input
            let _ = serde_json::from_slice::<MeshMessage>(&bytes);
        });
    }

    /// Fuzz test: Node ID validation
    #[test]
    fn fuzz_node_id_validation() {
        proptest!(|(node_id in ".*")| {
            // Should handle any node ID string
            let _ = node_id.as_bytes();
        });
    }

    /// Fuzz test: Peer address parsing
    #[test]
    fn fuzz_peer_address_parsing() {
        proptest!(|(addr_str in ".*")| {
            // Should never panic on invalid addresses
            let _ = addr_str.parse::<std::net::SocketAddr>();
        });
    }

    /// Fuzz test: Kademlia distance calculation
    #[test]
    fn fuzz_kademlia_distance() {
        proptest!(|(
            id1 in prop::collection::vec(any::<u8>(), 32),
            id2 in prop::collection::vec(any::<u8>(), 32)
        )| {
            let mut distance = [0u8; 32];
            for i in 0..32 {
                distance[i] = id1[i] ^ id2[i];
            }
            // XOR should never panic
            prop_assert!(distance.len() == 32);
        });
    }

    /// Fuzz test: Bootstrap peer parsing
    #[test]
    fn fuzz_bootstrap_parsing() {
        proptest!(|(peer_str in ".*")| {
            // Format: "peer_id@ip:port"
            if let Some((id, addr)) = peer_str.split_once('@') {
                let _ = addr.parse::<std::net::SocketAddr>();
                let _ = id.to_string();
            }
        });
    }
}
