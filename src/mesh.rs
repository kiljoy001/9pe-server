//! Libp2p Mesh Networking for 9P.e
//!
//! Provides IPFS-like peer discovery and communication using libp2p gossipsub

use libp2p::{
    gossipsub::{self, IdentTopic as Topic, MessageAuthenticity, ValidationMode},
    identify,
    kad,
    mdns,
    noise,
    swarm::{NetworkBehaviour, SwarmEvent, Config},
    tcp,
    yamux,
    Multiaddr,
    PeerId,
    Swarm,
    Transport,
    futures::StreamExt,
};
use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, mpsc};
use anyhow::Result;
use tracing::{info, warn, error, debug};
use serde::{Deserialize, Serialize};

/// 9P.e mesh network topics
pub const TOPIC_9PE_DISCOVERY: &str = "9pe-discovery";
pub const TOPIC_9PE_CONSENSUS: &str = "9pe-consensus";
pub const TOPIC_9PE_FILE_SYNC: &str = "9pe-file-sync";

/// Mesh network message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MeshMessage {
    /// Node announcement with capabilities
    NodeAnnouncement {
        node_id: String,
        listen_addr: String,
        service_addr: String,  // 9P service address (e.g. 192.168.1.116:5641)
        capabilities: Vec<String>,
        version: String,
    },
    /// File system change notification
    FileSystemEvent {
        node_id: String,
        path: String,
        operation: String, // create, modify, delete
        timestamp: u64,
    },
    /// GhostDAG consensus message
    ConsensusMessage {
        node_id: String,
        block_hash: String,
        parent_hashes: Vec<String>,
        blue_score: u64,
    },
    /// Request for file synchronization
    SyncRequest {
        node_id: String,
        path: String,
        hash: String,
    },
    /// Response to sync request
    SyncResponse {
        node_id: String,
        path: String,
        data: Vec<u8>,
    },
}

/// Libp2p network behaviour for 9P.e mesh
#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "MeshEvent")]
pub struct MeshBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
    pub kademlia: kad::Behaviour<kad::store::MemoryStore>,
    pub identify: identify::Behaviour,
}

/// Events from the mesh network
#[derive(Debug)]
pub enum MeshEvent {
    Gossipsub(gossipsub::Event),
    Mdns(mdns::Event),
    Kademlia(kad::Event),
    Identify(identify::Event),
}

impl From<gossipsub::Event> for MeshEvent {
    fn from(event: gossipsub::Event) -> Self {
        MeshEvent::Gossipsub(event)
    }
}

impl From<mdns::Event> for MeshEvent {
    fn from(event: mdns::Event) -> Self {
        MeshEvent::Mdns(event)
    }
}

impl From<kad::Event> for MeshEvent {
    fn from(event: kad::Event) -> Self {
        MeshEvent::Kademlia(event)
    }
}

impl From<identify::Event> for MeshEvent {
    fn from(event: identify::Event) -> Self {
        MeshEvent::Identify(event)
    }
}

/// Information about discovered peers
#[derive(Debug, Clone)]
pub struct DiscoveredPeer {
    pub node_id: String,
    pub listen_addr: String,  // mesh address
    pub service_addr: String, // 9P service address
    pub capabilities: Vec<String>,
    pub version: String,
    pub discovered_at: std::time::SystemTime,
}

/// 9P.e mesh network manager
pub struct MeshNetwork {
    swarm: Swarm<MeshBehaviour>,
    node_id: String,
    listen_addr: String,
    service_addr: Option<String>,  // 9P service address
    message_sender: mpsc::UnboundedSender<MeshMessage>,
    message_receiver: mpsc::UnboundedReceiver<MeshMessage>,
    discovered_peers: Arc<RwLock<HashMap<String, DiscoveredPeer>>>,
}

impl MeshNetwork {
    /// Create a new mesh network instance
    pub async fn new(listen_port: u16) -> Result<Self> {
        // Generate identity
        let local_key = libp2p::identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(local_key.public());

        info!("🌐 Creating mesh network with peer ID: {}", local_peer_id);

        // Create transport
        let transport = tcp::tokio::Transport::default()
            .upgrade(libp2p::core::upgrade::Version::V1)
            .authenticate(noise::Config::new(&local_key)?)
            .multiplex(yamux::Config::default())
            .boxed();

        // Create gossipsub
        let message_id_fn = |message: &gossipsub::Message| {
            let mut s = DefaultHasher::new();
            message.data.hash(&mut s);
            gossipsub::MessageId::from(s.finish().to_string())
        };

        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(10))
            .validation_mode(ValidationMode::Strict)
            .message_id_fn(message_id_fn)
            .build()
            .expect("Valid config");

        let mut gossipsub = gossipsub::Behaviour::new(
            MessageAuthenticity::Signed(local_key.clone()),
            gossipsub_config,
        ).map_err(|e| anyhow::anyhow!("Failed to create gossipsub: {}", e))?;

        // Subscribe to topics
        let discovery_topic = Topic::new(TOPIC_9PE_DISCOVERY);
        let consensus_topic = Topic::new(TOPIC_9PE_CONSENSUS);
        let file_sync_topic = Topic::new(TOPIC_9PE_FILE_SYNC);

        gossipsub.subscribe(&discovery_topic)?;
        gossipsub.subscribe(&consensus_topic)?;
        gossipsub.subscribe(&file_sync_topic)?;

        info!("📡 Subscribed to mesh topics: discovery, consensus, file-sync");

        // Create mDNS for local discovery
        let mdns = mdns::tokio::Behaviour::new(mdns::Config::default(), local_peer_id)?;

        // Create Kademlia DHT
        let store = kad::store::MemoryStore::new(local_peer_id);
        let kademlia = kad::Behaviour::new(local_peer_id, store);

        // Create identify protocol
        let identify = identify::Behaviour::new(identify::Config::new(
            "/9pe/1.0.0".to_string(),
            local_key.public(),
        ));

        // Create network behaviour
        let behaviour = MeshBehaviour {
            gossipsub,
            mdns,
            kademlia,
            identify,
        };

        // Create swarm
        let mut swarm = Swarm::new(transport, behaviour, local_peer_id, Config::with_tokio_executor());

        // Listen on specified port
        let listen_addr = format!("/ip4/0.0.0.0/tcp/{}", listen_port);
        swarm.listen_on(listen_addr.parse()?)?;

        // Create message channel
        let (message_sender, message_receiver) = mpsc::unbounded_channel();

        Ok(MeshNetwork {
            swarm,
            node_id: local_peer_id.to_string(),
            listen_addr: listen_addr.clone(),
            service_addr: None,  // Will be set when starting with 9P service info
            message_sender,
            message_receiver,
            discovered_peers: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Start the mesh network event loop
    pub async fn run(&mut self) -> Result<()> {
        info!("🚀 Starting 9P.e mesh network on {}", self.listen_addr);

        // Announce our presence
        self.announce_node().await?;

        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => {
                    self.handle_swarm_event(event).await?;
                }
                message = self.message_receiver.recv() => {
                    if let Some(msg) = message {
                        self.handle_outgoing_message(msg).await?;
                    }
                }
            }
        }
    }

    /// Handle swarm events
    async fn handle_swarm_event<THandlerErr>(&mut self, event: SwarmEvent<MeshEvent, THandlerErr>) -> Result<()> {
        match event {
            SwarmEvent::Behaviour(MeshEvent::Gossipsub(gossipsub::Event::Message {
                propagation_source: _,
                message_id: _,
                message,
            })) => {
                self.handle_gossip_message(message).await?;
            }
            SwarmEvent::Behaviour(MeshEvent::Mdns(mdns::Event::Discovered(list))) => {
                for (peer_id, multiaddr) in list {
                    info!("🔍 Discovered peer via mDNS: {} at {}", peer_id, multiaddr);
                    self.swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                    self.swarm.behaviour_mut().kademlia.add_address(&peer_id, multiaddr);
                }
            }
            SwarmEvent::Behaviour(MeshEvent::Mdns(mdns::Event::Expired(list))) => {
                for (peer_id, _) in list {
                    debug!("📤 Peer expired via mDNS: {}", peer_id);
                    self.swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
                }
            }
            SwarmEvent::Behaviour(MeshEvent::Identify(identify::Event::Received {
                peer_id,
                info,
            })) => {
                info!("🆔 Identified peer: {} - {}", peer_id, info.agent_version);
                for addr in info.listen_addrs {
                    self.swarm.behaviour_mut().kademlia.add_address(&peer_id, addr);
                }
            }
            SwarmEvent::NewListenAddr { address, .. } => {
                info!("🎧 Listening on {}", address);
            }
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                info!("🤝 Connected to peer: {}", peer_id);
            }
            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                info!("👋 Disconnected from peer: {}", peer_id);
            }
            _ => {}
        }

        Ok(())
    }

    /// Handle incoming gossip messages
    async fn handle_gossip_message(&mut self, message: gossipsub::Message) -> Result<()> {
        let topic = message.topic.to_string();

        match serde_json::from_slice::<MeshMessage>(&message.data) {
            Ok(mesh_msg) => {
                debug!("📨 Received mesh message on topic {}: {:?}", topic, mesh_msg);

                match mesh_msg {
                    MeshMessage::NodeAnnouncement { node_id, listen_addr, service_addr, capabilities, version } => {
                        info!("📢 Node announcement: {} at {} serving 9P on {} (v{}) - {:?}",
                              node_id, listen_addr, service_addr, version, capabilities);

                        // Store discovered peer
                        let peer = DiscoveredPeer {
                            node_id: node_id.clone(),
                            listen_addr: listen_addr.clone(),
                            service_addr: service_addr.clone(),
                            capabilities: capabilities.clone(),
                            version: version.clone(),
                            discovered_at: std::time::SystemTime::now(),
                        };

                        self.discovered_peers.write().await.insert(node_id.clone(), peer.clone());

                        // Notify synthetic filesystem about new service
                        if let Err(e) = self.notify_service_discovery(peer).await {
                            warn!("Failed to notify service discovery: {}", e);
                        }
                    }
                    MeshMessage::FileSystemEvent { node_id, path, operation, timestamp } => {
                        info!("📁 File system event from {}: {} {} at {}",
                              node_id, operation, path, timestamp);
                        // TODO: Handle file sync requests
                    }
                    MeshMessage::ConsensusMessage { node_id, block_hash, parent_hashes, blue_score } => {
                        info!("🔗 Consensus message from {}: block {} (blue_score: {}, parents: {})",
                              node_id, block_hash, blue_score, parent_hashes.len());

                        // Forward to GhostDAG consensus handler
                        if let Err(e) = self.handle_consensus_block(node_id, block_hash, parent_hashes).await {
                            warn!("Failed to process consensus block: {}", e);
                        }
                    }
                    MeshMessage::SyncRequest { node_id, path, hash } => {
                        info!("🔄 Sync request from {} for {}: {}", node_id, path, hash);
                        // TODO: Respond with file data if we have it
                    }
                    MeshMessage::SyncResponse { node_id, path, data } => {
                        info!("📥 Sync response from {} for {}: {} bytes", node_id, path, data.len());
                        // TODO: Store received file data
                    }
                }
            }
            Err(e) => {
                warn!("Failed to parse mesh message: {}", e);
            }
        }

        Ok(())
    }

    /// Handle outgoing messages
    async fn handle_outgoing_message(&mut self, message: MeshMessage) -> Result<()> {
        let topic = match message {
            MeshMessage::NodeAnnouncement { .. } => TOPIC_9PE_DISCOVERY,
            MeshMessage::FileSystemEvent { .. } => TOPIC_9PE_FILE_SYNC,
            MeshMessage::ConsensusMessage { .. } => TOPIC_9PE_CONSENSUS,
            MeshMessage::SyncRequest { .. } | MeshMessage::SyncResponse { .. } => TOPIC_9PE_FILE_SYNC,
        };

        let data = serde_json::to_vec(&message)?;
        let topic = Topic::new(topic);

        if let Err(e) = self.swarm.behaviour_mut().gossipsub.publish(topic, data) {
            error!("Failed to publish message: {}", e);
        }

        Ok(())
    }

    /// Set the 9P service address
    pub fn set_service_addr(&mut self, addr: String) {
        self.service_addr = Some(addr);
    }

    /// Announce this node to the network
    async fn announce_node(&mut self) -> Result<()> {
        let service_addr = self.service_addr.clone()
            .unwrap_or_else(|| "not-configured".to_string());

        let announcement = MeshMessage::NodeAnnouncement {
            node_id: self.node_id.clone(),
            listen_addr: self.listen_addr.clone(),
            service_addr,
            capabilities: vec![
                "9pe-filesystem".to_string(),
                "ghostdag-consensus".to_string(),
                "file-sync".to_string(),
            ],
            version: "1.0.0".to_string(),
        };

        self.message_sender.send(announcement)?;
        info!("📢 Announced node to mesh network with service at {}",
              self.service_addr.as_ref().unwrap_or(&"not-configured".to_string()));
        Ok(())
    }

    /// Get a message sender for external use
    pub fn message_sender(&self) -> mpsc::UnboundedSender<MeshMessage> {
        self.message_sender.clone()
    }

    /// Get node ID
    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    /// Get listen address
    pub fn listen_addr(&self) -> &str {
        &self.listen_addr
    }

    /// Get discovered peers
    pub async fn get_discovered_peers(&self) -> Vec<DiscoveredPeer> {
        self.discovered_peers.read().await.values().cloned().collect()
    }

    /// Notify synthetic filesystem about service discovery
    async fn notify_service_discovery(&self, peer: DiscoveredPeer) -> Result<()> {
        // This will be connected to the synthetic filesystem
        // For now, just log it
        debug!("Service discovery notification for: {}", peer.service_addr);
        Ok(())
    }

    /// Get a discovered peer by node ID
    pub async fn get_peer(&self, node_id: &str) -> Option<DiscoveredPeer> {
        self.discovered_peers.read().await.get(node_id).cloned()
    }

    /// Handle incoming consensus block
    async fn handle_consensus_block(&self, node_id: String, block_hash: String, parent_hashes: Vec<String>) -> Result<()> {
        // This will be called when we receive consensus messages from peers
        // For now, we'll prepare the integration point for GhostDAG

        debug!("Processing consensus block from {}: {} with {} parents",
               node_id, block_hash, parent_hashes.len());

        // Convert string hashes to BlockHash type
        // In a real implementation, this would forward to the GhostDAG instance

        // TODO: Get global GhostDAG instance and add block
        // if let Some(ghostdag) = get_ghostdag_instance().await {
        //     let block = create_block_from_message(node_id, block_hash, parent_hashes);
        //     ghostdag.add_block(block).await?;
        // }

        Ok(())
    }
}

/// Global mesh network instance for service discovery
// TODO: Fix thread safety issues with Swarm type
// static MESH_NETWORK: tokio::sync::OnceCell<Arc<RwLock<Option<Arc<MeshNetwork>>>>> = tokio::sync::OnceCell::const_new();

/// Get the global mesh network instance
pub async fn get_mesh_network() -> Option<Arc<MeshNetwork>> {
    // Temporarily return None until thread safety is fixed
    None
    // let cell = MESH_NETWORK.get_or_init(|| async {
    //     Arc::new(RwLock::new(None))
    // }).await;
    // cell.read().await.clone()
}

/// Start mesh networking in background with optional service address
pub async fn start_mesh_network(listen_port: u16, service_addr: Option<String>) -> Result<mpsc::UnboundedSender<MeshMessage>> {
    // Create mesh network
    let mut mesh = MeshNetwork::new(listen_port).await?;

    // Set the 9P service address if provided
    if let Some(addr) = service_addr {
        mesh.set_service_addr(addr);
    }

    let sender = mesh.message_sender();

    // Run the mesh network in background
    tokio::spawn(async move {
        if let Err(e) = mesh.run().await {
            error!("Mesh network error: {}", e);
        }
    });

    Ok(sender)
}

/// Utility functions for common mesh operations
pub mod utils {
    use super::*;

    /// Broadcast file system change to mesh
    pub async fn broadcast_file_change(
        sender: &mpsc::UnboundedSender<MeshMessage>,
        node_id: String,
        path: String,
        operation: String,
    ) -> Result<()> {
        let message = MeshMessage::FileSystemEvent {
            node_id,
            path,
            operation,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
        };

        sender.send(message)?;
        Ok(())
    }

    /// Broadcast consensus message to mesh
    pub async fn broadcast_consensus(
        sender: &mpsc::UnboundedSender<MeshMessage>,
        node_id: String,
        block_hash: String,
        parent_hashes: Vec<String>,
        blue_score: u64,
    ) -> Result<()> {
        let message = MeshMessage::ConsensusMessage {
            node_id,
            block_hash,
            parent_hashes,
            blue_score,
        };

        sender.send(message)?;
        Ok(())
    }

    /// Request file synchronization from mesh
    pub async fn request_file_sync(
        sender: &mpsc::UnboundedSender<MeshMessage>,
        node_id: String,
        path: String,
        hash: String,
    ) -> Result<()> {
        let message = MeshMessage::SyncRequest {
            node_id,
            path,
            hash,
        };

        sender.send(message)?;
        Ok(())
    }
}