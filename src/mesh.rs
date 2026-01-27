//! Mesh networking for peer-to-peer communication
//!
//! Automatic peer discovery using mDNS + QUIC mesh protocol with
//! sovereign identity and DHT-based peer discovery.

use anyhow::{anyhow, Context, Result};
use mdns_sd::ServiceDaemon;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::time::Duration;
use tracing::{debug, error, info, warn};

// QUIC imports
use quinn::{Connection as QuinnConnection, Endpoint, RecvStream, SendStream, ServerConfig};
use rustls::{Certificate, PrivateKey, ServerConfig as RustlsServerConfig};

// Sovereign identity imports
use crate::identity::{NodeId, NodePermissions, SovereignIdentity, WorkReceipt};
use crate::dht::SovereignDht;

/// Mesh network coordinator with DHT and mDNS discovery
pub struct MeshNetwork {
    #[allow(dead_code)]
    node_id: String,
    local_port: u16,
    peers: Arc<RwLock<HashMap<String, PeerConnection>>>,
    bootstrap_peers: Vec<String>,
    mdns_daemon: Option<ServiceDaemon>,
    dht: Arc<SovereignDht>, // Use our sovereign DHT instead of KademliaTable
    endpoint: Arc<RwLock<Option<Endpoint>>>, // QUIC endpoint
    namespace_manager: Arc<Mutex<Option<Arc<dyn crate::namespace_manager::MeshMessageHandler>>>>,
    start_time: std::time::Instant,
    sovereign_identity: Arc<SovereignIdentity>, // Our sovereign identity
}

impl MeshNetwork {
    pub fn new(
        sovereign_identity: Arc<SovereignIdentity>,
        dht: Arc<SovereignDht>,
        local_port: u16, 
        bootstrap_peers: Vec<String>
    ) -> Self {
        Self {
            node_id: sovereign_identity.node_id.as_str().to_string(),
            local_port,
            peers: Arc::new(RwLock::new(HashMap::new())),
            bootstrap_peers,
            mdns_daemon: None,
            dht,
            endpoint: Arc::new(RwLock::new(None)),
            namespace_manager: Arc::new(Mutex::new(None)),
            start_time: std::time::Instant::now(),
            sovereign_identity,
        }
    }

    /// Set namespace manager for handling namespace messages
    pub async fn set_namespace_manager(
        &self,
        namespace_manager: Arc<dyn crate::namespace_manager::MeshMessageHandler>,
    ) {
        let mut ns_mgr = self.namespace_manager.lock().await;
        *ns_mgr = Some(namespace_manager);
    }

    /// Start the mesh network with DHT and mDNS discovery using QUIC
    pub async fn start(self: Arc<Self>) -> Result<()> {
        info!(
            "Starting QUIC mesh network on port {} with DHT and mDNS discovery",
            self.local_port
        );

        // Ensure we have a QUIC endpoint available
        let endpoint_clone = {
            let mut guard = self.endpoint.write().await;
            if guard.is_none() {
                let addr = SocketAddr::from(([0, 0, 0, 0], self.local_port));
                let endpoint = self
                    .create_quic_endpoint(addr)
                    .await
                    .context("Failed to create QUIC endpoint")?;
                *guard = Some(endpoint);
            }
            guard.as_ref().unwrap().clone()
        };

        // Start listener for incoming connections
        let listener_self = Arc::clone(&self);
        tokio::spawn(async move {
            if let Err(e) = listener_self
                .run_listener_with_endpoint(endpoint_clone)
                .await
            {
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

    /// Create QUIC endpoint with sovereign identity certificate
    async fn create_quic_endpoint(&self, addr: SocketAddr) -> Result<Endpoint> {
        // Use our sovereign identity's certificate
        let cert_der = self.sovereign_identity.certificate.clone();
        let private_key_der = self.sovereign_identity.private_key_der.clone();
        
        // Create rustls certificate and private key
        let cert_chain = vec![Certificate(cert_der)];
        let private_key = PrivateKey(private_key_der);

        // Create rustls server config
        let rustls_config = RustlsServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(cert_chain, private_key)
            .map_err(|e| anyhow::anyhow!("Failed to create rustls config: {}", e))?;

        // Create QUIC endpoint
        let mut server_config = ServerConfig::with_crypto(Arc::new(rustls_config));
        server_config.transport = Arc::new(quinn::TransportConfig::default());

        let endpoint = Endpoint::server(server_config, addr)
            .map_err(|e| anyhow::anyhow!("Failed to create QUIC endpoint: {}", e))?;

        Ok(endpoint)
    }

    /// Generate self-signed certificate for QUIC (deprecated - use sovereign identity)
    #[deprecated(note = "Use sovereign identity certificate instead")]
    fn generate_certificate(&self) -> Result<rcgen::Certificate> {
        let mut params = rcgen::CertificateParams::new(vec![self.node_id.clone()]);
        params.distinguished_name = rcgen::DistinguishedName::new();
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, &self.node_id);

        let cert = rcgen::Certificate::from_params(params)
            .map_err(|e| anyhow::anyhow!("Failed to generate certificate: {}", e))?;

        Ok(cert)
    }

    async fn run_listener_with_endpoint(self: Arc<Self>, endpoint: Endpoint) -> Result<()> {
        info!("QUIC mesh network listening on port {}", self.local_port);

        loop {
            match endpoint.accept().await {
                Some(conn) => {
                    let connecting = match conn.await {
                        Ok(c) => c,
                        Err(e) => {
                            error!("Failed to establish QUIC connection: {}", e);
                            continue;
                        }
                    };

                    let peer_addr = connecting.remote_address();
                    debug!("Incoming QUIC mesh connection from {}", peer_addr);

                    let self_clone = Arc::clone(&self);
                    tokio::spawn(async move {
                        if let Err(e) = self_clone.handle_incoming_connection(connecting).await {
                            debug!("QUIC mesh connection error from {}: {}", peer_addr, e);
                        }
                    });
                }
                None => {
                    // Endpoint closed
                    break;
                }
            }
        }

        Ok(())
    }

    async fn handle_incoming_connection(&self, connecting: QuinnConnection) -> Result<()> {
        let peer_addr = connecting.remote_address();

        // Accept bi-directional stream for mesh communication
        let (_send_stream, recv_stream) = connecting
            .accept_bi()
            .await
            .context("Failed to accept QUIC stream")?;

        // Read handshake message
        let message = self.receive_message_quic(recv_stream).await?;

        match message {
            MeshMessage::Handshake {
                node_id,
                version,
                ed25519_public_key,
                p256_public_key,
                certificate_der,
                permissions,
            } => {
                info!(
                    "Peer {} connected from {} (version {})",
                    node_id, peer_addr, version
                );

                let node_id_wrapped = NodeId::new(node_id.clone());
                if let Some(record) = self.dht.lookup_node(&node_id_wrapped).await {
                    if record.public_key != ed25519_public_key
                        || record.p256_public_key != p256_public_key
                        || record.certificate_der != certificate_der
                        || record.permissions != permissions
                    {
                        connecting.close(0u32.into(), b"dht-mismatch");
                        return Err(anyhow!("Handshake keys mismatch for peer {}", node_id));
                    }
                } else {
                    self.dht
                        .upsert_peer_record(
                            node_id_wrapped,
                            ed25519_public_key.clone(),
                            p256_public_key.clone(),
                            certificate_der.clone(),
                            Some(peer_addr),
                            permissions.clone(),
                        )
                        .await?;
                }

                // Send handshake response
                let response = MeshMessage::HandshakeAck {
                    node_id: self.node_id.clone(),
                    version: 1,
                    ed25519_public_key: self.sovereign_identity.ed25519_public.to_bytes().to_vec(),
                    p256_public_key: self.sovereign_identity.p256_public_key_bytes(),
                    certificate_der: self.sovereign_identity.certificate.clone(),
                    permissions: self.sovereign_identity.permissions.clone(),
                };

                // Create new stream for sending response
                let (response_send, _) = connecting
                    .open_bi()
                    .await
                    .context("Failed to open QUIC stream for response")?;

                self.send_message_quic(response_send, &response).await?;

                self.register_peer_connection(node_id.clone(), peer_addr, connecting)
                    .await;

                info!("Peer {} connected successfully via QUIC", node_id);
            }
            _ => {
                warn!("Expected handshake, got {:?}", message);
            }
        }

        Ok(())
    }

    async fn connect_to_bootstrap_peers(self: Arc<Self>) {
        let bootstrap_peers = self.bootstrap_peers.clone();
        for peer_addr_str in bootstrap_peers {
            let network = Arc::clone(&self);
            tokio::spawn(async move {
                let addr: SocketAddr = match peer_addr_str.parse() {
                    Ok(addr) => addr,
                    Err(e) => {
                        error!("Invalid bootstrap address {}: {}", peer_addr_str, e);
                        return;
                    }
                };

                let mut backoff = Duration::from_secs(1);
                loop {
                    match network.try_connect(addr, None).await {
                        Ok(_) => return,
                        Err(e) => {
                            debug!(
                                "Failed to connect to bootstrap peer {}: {} (retrying in {:?})",
                                addr, e, backoff
                            );
                        }
                    }

                    tokio::time::sleep(backoff).await;
                    backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
                }
            });
        }
    }

    async fn run_mdns_discovery(&self) {
        info!("Starting mDNS peer discovery");

        // Create mDNS service daemon
        let mdns = match ServiceDaemon::new() {
            Ok(daemon) => daemon,
            Err(e) => {
                error!("Failed to create mDNS daemon: {}", e);
                return;
            }
        };

        // Service type for 9pe mesh nodes
        let service_type = "_9pe-mesh._udp.local.";
        let instance_name = format!("9pe-{}", &self.node_id[..8]);

        // Register/advertise this node's service
        let service_hostname = format!("{}.local.", instance_name);
        let properties = [("node_id", self.node_id.as_str())];

        match mdns_sd::ServiceInfo::new(
            service_type,
            &instance_name,
            &service_hostname,
            "",  // No specific IP, let mDNS resolve
            self.local_port,
            &properties[..],
        ) {
            Ok(service_info) => {
                if let Err(e) = mdns.register(service_info) {
                    error!("Failed to register mDNS service: {}", e);
                } else {
                    info!("Registered mDNS service: {} on port {}", instance_name, self.local_port);
                }
            }
            Err(e) => {
                error!("Failed to create mDNS service info: {}", e);
            }
        }

        // Browse for peers advertising the service
        let receiver = match mdns.browse(service_type) {
            Ok(recv) => recv,
            Err(e) => {
                error!("Failed to browse mDNS services: {}", e);
                return;
            }
        };

        info!("mDNS discovery active, browsing for {}", service_type);

        // Process discovered services
        loop {
            match receiver.recv_async().await {
                Ok(event) => {
                    match event {
                        mdns_sd::ServiceEvent::ServiceResolved(info) => {
                            debug!("mDNS service resolved: {:?}", info.get_fullname());

                            // Get peer address from service info
                            let addresses = info.get_addresses();
                            for addr in addresses.iter() {
                                let peer_addr = format!("{}:{}", addr, info.get_port());
                                info!("Discovered peer via mDNS: {}", peer_addr);

                                // Extract peer ID from service properties if available
                                let peer_id_hint = info.get_properties()
                                    .iter()
                                    .find_map(|prop| {
                                        if prop.key() == "node_id" {
                                            Some(prop.val_str().to_string())
                                        } else {
                                            None
                                        }
                                    });

                                // Attempt connection
                                if let Err(e) = self.connect_to_peer(&peer_addr, peer_id_hint).await {
                                    debug!("Failed to connect to mDNS peer {}: {}", peer_addr, e);
                                }
                            }
                        }
                        mdns_sd::ServiceEvent::ServiceRemoved(_, fullname) => {
                            debug!("mDNS service removed: {}", fullname);
                        }
                        _ => {}
                    }
                }
                Err(e) => {
                    error!("mDNS receiver error: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }

    async fn run_dht_discovery(&self) {
        info!("Starting DHT-based peer discovery");

        // Initial delay to let the network start up
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Bootstrap phase: connect to bootstrap peers first
        for bootstrap_peer in &self.bootstrap_peers {
            info!("Bootstrapping DHT via peer: {}", bootstrap_peer);
            if let Err(e) = self.connect_to_peer(bootstrap_peer, None).await {
                warn!("Failed to connect to bootstrap peer {}: {}", bootstrap_peer, e);
            }
        }

        // Wait for bootstrap connections to establish
        tokio::time::sleep(Duration::from_secs(1)).await;

        // Periodic DHT maintenance and peer discovery
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;

            // Get current DHT state
            let dht = self.dht.read().await;
            let all_peers = dht.get_all_peers();
            drop(dht);

            if all_peers.is_empty() {
                debug!("DHT empty, waiting for bootstrap peers");
                continue;
            }

            debug!("DHT has {} known peers", all_peers.len());

            // Perform iterative lookup to discover new peers
            // Use our own node ID as lookup target to find closest peers
            let dht_read = self.dht.read().await;
            let target = dht_read.local_id;
            let closest = dht_read.find_closest(&target, 8);
            drop(dht_read);

            // Try to connect to peers we know about but aren't connected to
            for peer in closest {
                let peer_addr = peer.address.to_string();

                // Check if already connected
                let peers = self.peers.read().await;
                let already_connected = peers
                    .values()
                    .any(|p| p.address == peer.address && p.is_connected());
                drop(peers);

                if !already_connected {
                    debug!("DHT: Attempting to connect to peer {}", peer_addr);
                    if let Err(e) = self.connect_to_peer(&peer_addr, None).await {
                        debug!("DHT connection attempt failed for {}: {}", peer_addr, e);
                    }
                }
            }

            // Refresh DHT by requesting peer lists from connected peers
            let connected_peers: Vec<SocketAddr> = {
                let peers = self.peers.read().await;
                peers
                    .values()
                    .filter(|p| p.is_connected())
                    .map(|p| p.address)
                    .collect()
            };

            for peer_addr in connected_peers {
                debug!("DHT: Requesting peer list from {}", peer_addr);
                // The peer list exchange happens via the consensus layer
                // which already handles network messages
            }

            debug!("DHT discovery cycle complete");
        }
    }

    async fn run_heartbeat(&self) {
        // Implementation would go here
    }

    #[allow(dead_code)]
    async fn handle_message(&self, from_peer: &str, message: MeshMessage) -> Result<()> {
        match message {
            MeshMessage::NamespaceAccessRequest {
                namespace_path,
                requester_pubkey,
                requested_role,
                message,
            } => {
                debug!(
                    "Received namespace access request from {} for namespace {}",
                    from_peer, namespace_path
                );
                // Forward to namespace manager if available
                let ns_manager = self.namespace_manager.lock().await;
                if let Some(ref ns_manager) = *ns_manager {
                    let ns_manager_clone = Arc::clone(ns_manager);
                    let from_peer_clone = from_peer.to_string();
                    let _ = ns_manager; // Release the lock before spawning async task
                    tokio::spawn(async move {
                        if let Err(e) = ns_manager_clone
                            .handle_namespace_access_request(
                                from_peer_clone,
                                namespace_path,
                                requester_pubkey,
                                requested_role,
                                message,
                            )
                            .await
                        {
                            warn!("Failed to handle namespace access request: {}", e);
                        }
                    });
                }
            }
            MeshMessage::NamespaceAccessResponse {
                namespace_path,
                requester_pubkey,
                approved,
                message,
            } => {
                debug!(
                    "Received namespace access response from {} for namespace {}: {}",
                    from_peer, namespace_path, approved
                );
                // Forward to namespace manager if available
                let ns_manager = self.namespace_manager.lock().await;
                if let Some(ref ns_manager) = *ns_manager {
                    let ns_manager_clone = Arc::clone(ns_manager);
                    let from_peer_clone = from_peer.to_string();
                    let _ = ns_manager; // Release the lock before spawning async task
                    tokio::spawn(async move {
                        if let Err(e) = ns_manager_clone
                            .handle_namespace_access_response(
                                from_peer_clone,
                                namespace_path,
                                requester_pubkey,
                                approved,
                                message,
                            )
                            .await
                        {
                            warn!("Failed to handle namespace access response: {}", e);
                        }
                    });
                }
            }
            _ => {
                debug!("Received message from {}: {:?}", from_peer, message);
            }
        }
        Ok(())
    }

    async fn send_message_quic(&self, stream: SendStream, message: &MeshMessage) -> Result<()> {
        Self::send_message_quic_static(stream, message).await
    }

    async fn send_message_quic_static(mut stream: SendStream, message: &MeshMessage) -> Result<()> {
        let data = bincode::serialize(message)?;
        let size = data.len() as u32;
        stream.write_all(&size.to_le_bytes()).await?;
        stream.write_all(&data).await?;
        stream.finish().await?;
        Ok(())
    }

    async fn receive_message_quic(&self, stream: RecvStream) -> Result<MeshMessage> {
        Self::receive_message_quic_static(stream).await
    }

    async fn receive_message_quic_static(mut stream: RecvStream) -> Result<MeshMessage> {
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

    /// Send a message to a specific peer
    pub async fn send_message_to_peer(&self, peer_id: &str, message: MeshMessage) -> Result<()> {
        let peers = self.peers.read().await;
        let peer = peers
            .get(peer_id)
            .ok_or_else(|| anyhow::anyhow!("Peer {} not found", peer_id))?;

        let quic_conn = peer
            .quic_connection
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No QUIC connection to peer {}", peer_id))?;

        // Open a new bidirectional stream
        let (send_stream, _recv_stream) = quic_conn
            .open_bi()
            .await
            .context("Failed to open QUIC stream")?;

        // Send the message
        self.send_message_quic(send_stream, &message).await
    }

    async fn register_peer_connection(
        &self,
        node_id: String,
        addr: SocketAddr,
        connection: QuinnConnection,
    ) {
        {
            let mut peers = self.peers.write().await;
            let mut peer_conn = PeerConnection::new(node_id.clone(), addr);
            peer_conn.set_connection(connection);
            peers.insert(node_id.clone(), peer_conn);
        }

        let mut dht = self.dht.write().await;
        dht.add_peer(KademliaTable::hash_node(&node_id), addr);
    }

    /// Broadcast a message to all connected peers
    pub async fn broadcast_message(&self, message: MeshMessage) -> Result<()> {
        let peers = self.peers.read().await;
        let peer_ids: Vec<String> = peers.keys().cloned().collect();
        drop(peers);

        for peer_id in peer_ids {
            if let Err(e) = self.send_message_to_peer(&peer_id, message.clone()).await {
                warn!("Failed to send message to peer {}: {}", peer_id, e);
            }
        }

        Ok(())
    }

    pub async fn get_peer_count(&self) -> usize {
        self.peers.read().await.len()
    }

    pub async fn get_connected_peers(&self) -> Vec<String> {
        self.peers.read().await.keys().cloned().collect()
    }

    pub async fn get_all_peers(&self) -> Vec<(String, PeerState)> {
        let peers = self.peers.read().await;
        peers
            .iter()
            .map(|(peer_id, peer)| {
                (
                    peer_id.clone(),
                    PeerState {
                        address: peer.address,
                        connected: peer.is_connected(),
                        last_seen: peer.last_seen,
                    },
                )
            })
            .collect()
    }

    pub async fn connect_to_peer(&self, address: &str, peer_id_hint: Option<String>) -> Result<()> {
        let addr: SocketAddr = address
            .trim()
            .parse()
            .map_err(|e| anyhow!("Invalid peer address {}: {}", address, e))?;
        self.try_connect(addr, peer_id_hint).await
    }

    pub async fn disconnect_peer(&self, peer_id: &str) -> Result<()> {
        let mut peers = self.peers.write().await;
        if let Some(mut peer) = peers.remove(peer_id) {
            if let Some(conn) = peer.quic_connection.take() {
                conn.close(0u32.into(), b"disconnect");
            }
            info!("Disconnected mesh peer {}", peer_id);
            Ok(())
        } else {
            Err(anyhow!("Peer {} not found", peer_id))
        }
    }

    pub async fn announce_service(&self, service_name: &str) -> Result<()> {
        if self.mdns_daemon.is_some() {
            info!("Announcing service '{}' via mDNS (stub)", service_name);
        } else {
            info!(
                "mDNS service announcement requested for '{}' but daemon is not initialized",
                service_name
            );
        }
        Ok(())
    }

    pub async fn get_status(&self) -> MeshStatus {
        let peers = self.peers.read().await;
        let active_connections = peers.values().filter(|peer| peer.is_connected()).count();

        MeshStatus {
            node_id: self.node_id.clone(),
            peer_count: peers.len(),
            active_connections,
            mdns_enabled: self.mdns_daemon.is_some(),
            dht_enabled: true,
            uptime_seconds: self.start_time.elapsed().as_secs(),
        }
    }

    pub async fn get_dht_routing_table(&self) -> Vec<([u8; 32], String)> {
        self.dht
            .read()
            .await
            .get_all_peers()
            .into_iter()
            .map(|peer| (peer.id, peer.address.to_string()))
            .collect()
    }

    async fn try_connect(&self, addr: SocketAddr, expected_peer: Option<String>) -> Result<()> {
        let endpoint = {
            let guard = self.endpoint.read().await;
            guard
                .clone()
                .ok_or_else(|| anyhow!("QUIC endpoint not initialized"))?
        };

        let connecting = endpoint
            .connect(addr, &self.node_id)
            .map_err(|e| anyhow!("Failed to initiate QUIC connection: {}", e))?;

        let connection = connecting
            .await
            .context("Failed to establish QUIC connection")?;

        let (send_stream, recv_stream) = connection
            .open_bi()
            .await
            .context("Failed to open QUIC stream for handshake")?;

        let handshake = MeshMessage::Handshake {
            node_id: self.node_id.clone(),
            version: 1,
            ed25519_public_key: self.sovereign_identity.ed25519_public.to_bytes().to_vec(),
            p256_public_key: self.sovereign_identity.p256_public_key_bytes(),
            certificate_der: self.sovereign_identity.certificate.clone(),
            permissions: self.sovereign_identity.permissions.clone(),
        };

        Self::send_message_quic_static(send_stream, &handshake).await?;

        match Self::receive_message_quic_static(recv_stream).await? {
            MeshMessage::HandshakeAck {
                node_id,
                ed25519_public_key,
                p256_public_key,
                certificate_der,
                permissions,
                ..
            } => {
                if let Some(expected) = expected_peer {
                    if expected != node_id {
                        connection.close(0u32.into(), b"peer-id-mismatch");
                        return Err(anyhow!(
                            "Peer ID mismatch: expected {}, received {}",
                            expected,
                            node_id
                        ));
                    }
                }

                let node_id_wrapped = NodeId::new(node_id.clone());
                if let Some(record) = self.dht.lookup_node(&node_id_wrapped).await {
                    if record.public_key != ed25519_public_key
                        || record.p256_public_key != p256_public_key
                        || record.certificate_der != certificate_der
                        || record.permissions != permissions
                    {
                        connection.close(0u32.into(), b"dht-mismatch");
                        return Err(anyhow!("Handshake keys mismatch for peer {}", node_id));
                    }
                } else {
                    self.dht
                        .upsert_peer_record(
                            node_id_wrapped,
                            ed25519_public_key,
                            p256_public_key,
                            certificate_der,
                            Some(addr),
                            permissions,
                        )
                        .await?;
                }

                self.register_peer_connection(node_id.clone(), addr, connection)
                    .await;

                info!("Handshake complete with peer {} at {}", node_id, addr);
                Ok(())
            }
            other => {
                connection.close(0u32.into(), b"invalid-handshake");
                Err(anyhow!("Unexpected handshake response: {:?}", other))
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MeshMessage {
    Handshake {
        node_id: String,
        version: u32,
        ed25519_public_key: Vec<u8>,
        p256_public_key: Vec<u8>,
        certificate_der: Vec<u8>,
        permissions: NodePermissions,
    },
    HandshakeAck {
        node_id: String,
        version: u32,
        ed25519_public_key: Vec<u8>,
        p256_public_key: Vec<u8>,
        certificate_der: Vec<u8>,
        permissions: NodePermissions,
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
    // Namespace messages
    NamespaceAccessRequest {
        namespace_path: String,
        requester_pubkey: [u8; 32],
        requested_role: String,
        message: String,
    },
    NamespaceAccessResponse {
        namespace_path: String,
        requester_pubkey: [u8; 32],
        approved: bool,
        message: String,
    },
}

struct PeerConnection {
    #[allow(dead_code)]
    node_id: String,
    address: SocketAddr,
    last_seen: std::time::Instant,
    quic_connection: Option<QuinnConnection>,
}

impl PeerConnection {
    fn new(node_id: String, address: SocketAddr) -> Self {
        Self {
            node_id,
            address,
            last_seen: std::time::Instant::now(),
            quic_connection: None,
        }
    }

    fn set_connection(&mut self, connection: QuinnConnection) {
        self.quic_connection = Some(connection);
        self.mark_seen();
    }

    fn mark_seen(&mut self) {
        self.last_seen = std::time::Instant::now();
    }

    fn is_connected(&self) -> bool {
        self.quic_connection.is_some()
    }

    #[allow(dead_code)]
    fn address_string(&self) -> String {
        self.address.to_string()
    }
}

#[derive(Debug, Clone)]
pub struct PeerState {
    address: SocketAddr,
    connected: bool,
    last_seen: std::time::Instant,
}

impl PeerState {
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    pub fn address(&self) -> Option<String> {
        Some(self.address.to_string())
    }

    pub fn last_seen(&self) -> std::time::Instant {
        self.last_seen
    }
}

#[derive(Debug, Clone)]
pub struct MeshStatus {
    pub node_id: String,
    pub peer_count: usize,
    pub active_connections: usize,
    pub mdns_enabled: bool,
    pub dht_enabled: bool,
    pub uptime_seconds: u64,
}

/// Kademlia DHT for distributed peer discovery
struct KademliaTable {
    local_id: [u8; 32],              // SHA-256 of node_id
    buckets: Vec<Vec<KademliaPeer>>, // 256 k-buckets
}

#[derive(Clone, Debug)]
struct KademliaPeer {
    id: [u8; 32],
    address: SocketAddr,
    last_seen: std::time::Instant,
}

impl KademliaTable {
    fn new(node_id: &str) -> Self {
        Self {
            local_id: Self::hash_node(node_id),
            buckets: vec![Vec::new(); 256],
        }
    }

    fn add_peer(&mut self, id: [u8; 32], address: SocketAddr) {
        let index = self.bucket_index(&id);
        let bucket = &mut self.buckets[index];

        if let Some(existing) = bucket.iter_mut().find(|peer| peer.id == id) {
            existing.address = address;
            existing.last_seen = std::time::Instant::now();
        } else {
            bucket.push(KademliaPeer {
                id,
                address,
                last_seen: std::time::Instant::now(),
            });
        }
    }

    fn bucket_index(&self, target: &[u8; 32]) -> usize {
        for (byte_index, (l, r)) in self.local_id.iter().zip(target.iter()).enumerate() {
            let diff = l ^ r;
            if diff != 0 {
                let leading = diff.leading_zeros() as usize;
                return (byte_index * 8 + leading).min(255);
            }
        }
        255
    }

    #[allow(dead_code)]
    fn find_closest(&self, target: &[u8; 32], count: usize) -> Vec<KademliaPeer> {
        let mut peers = self.get_all_peers();
        peers.sort_by(|a, b| xor_distance(&a.id, target).cmp(&xor_distance(&b.id, target)));
        peers.truncate(count);
        peers
    }

    fn get_all_peers(&self) -> Vec<KademliaPeer> {
        self.buckets
            .iter()
            .flat_map(|bucket| bucket.clone())
            .collect()
    }

    fn hash_node(node_id: &str) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(node_id.as_bytes());
        hasher.finalize().into()
    }
}

#[allow(dead_code)]
fn xor_distance(id: &[u8; 32], target: &[u8; 32]) -> [u8; 32] {
    let mut distance = [0u8; 32];
    for i in 0..32 {
        distance[i] = id[i] ^ target[i];
    }
    distance
}
