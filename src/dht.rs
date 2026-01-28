//! DHT integration for sovereign node discovery and peer management
//!
//! This module extends the existing KademliaTable with sovereign identity records
//! and integration with the libp2p DHT when available.

use crate::identity::{DhtRecord, NodeId, NodePermissions, ServiceInfo, ServiceCapabilities, SovereignIdentity};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, RwLock};
use tracing::{info, debug, warn};
use sha2::Digest;
use serde::{Serialize, Deserialize};

use futures::StreamExt;
use libp2p::{
    core::Multiaddr,
    identity,
    kad::{
        record::Key as RecordKey,
        store::MemoryStore,
        Behaviour as Kademlia,
        Event as KademliaEvent,
        GetRecordOk,
        PutRecordOk,
        QueryId,
        QueryResult,
        Record,
        Quorum,
    },
    swarm::{Swarm, SwarmBuilder, SwarmEvent},
    multiaddr::Protocol,
    PeerId,
};
/// DHT record storage with sovereign identity support
#[derive(Debug)]
pub struct SovereignDht {
    /// Local node identity
    local_identity: Arc<SovereignIdentity>,
    
    /// In-memory DHT records (when libp2p not available)
    local_records: Arc<RwLock<HashMap<String, DhtRecord>>>,

    /// Persistent storage for DHT records
    storage: Option<sled::Db>,

    network: Arc<RwLock<Option<NetworkHandle>>>,

    /// Local service registry for periodic re-advertisement
    local_services: Arc<RwLock<HashMap<String, ServiceInfo>>>,
}

#[derive(Clone)]
struct NetworkHandle {
    sender: mpsc::Sender<DhtCommand>,
}

enum DhtCommand {
    PutRecord {
        key: Vec<u8>,
        value: Vec<u8>,
        reply: oneshot::Sender<Result<(), anyhow::Error>>,
    },
    GetRecord {
        key: Vec<u8>,
        reply: oneshot::Sender<Result<Option<Vec<u8>>, anyhow::Error>>,
    },
    AddAddress {
        peer: PeerId,
        addr: Multiaddr,
    },
    Bootstrap,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceRecord {
    pub node_id: NodeId,
    pub service_name: String,
    pub mount_point: String,
    pub capabilities: ServiceCapabilities,
    pub network_addr: SocketAddr,
    pub timestamp: u64,
}

impl SovereignDht {
    /// Create a new sovereign DHT instance
    pub fn new(local_identity: Arc<SovereignIdentity>) -> Self {
        info!("Initializing sovereign DHT for node: {}", local_identity.node_id.as_str());
        
        Self {
            local_identity,
            local_records: Arc::new(RwLock::new(HashMap::new())),
            storage: None,
            network: Arc::new(RwLock::new(None)),
            local_services: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new sovereign DHT instance with sled-backed storage.
    pub async fn new_with_store<P: AsRef<Path>>(
        local_identity: Arc<SovereignIdentity>,
        path: P,
    ) -> Result<Self, anyhow::Error> {
        let mut dht = Self::new(local_identity);
        let db = sled::open(path)?;
        dht.storage = Some(db);
        dht.load_from_store().await?;
        Ok(dht)
    }

    pub fn start_maintenance(&self, interval: std::time::Duration) {
        let dht = Arc::new(self.clone_for_tasks());
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(interval);
            loop {
                timer.tick().await;
                if let Err(e) = dht.publish_local_state().await {
                    warn!("DHT maintenance publish failed: {}", e);
                }
            }
        });
    }

    fn clone_for_tasks(&self) -> Self {
        Self {
            local_identity: Arc::clone(&self.local_identity),
            local_records: Arc::clone(&self.local_records),
            storage: self.storage.clone(),
            network: Arc::clone(&self.network),
            local_services: Arc::clone(&self.local_services),
        }
    }

    async fn publish_local_state(&self) -> Result<(), anyhow::Error> {
        if let Some(record) = self
            .local_records
            .read()
            .await
            .get(self.local_identity.node_id.as_str())
            .cloned()
        {
            let _ = self
                .send_put_record(record.node_id.as_str().as_bytes().to_vec(), &record)
                .await;
            if !record.node_name_hash.is_empty() {
                let key = Self::name_record_key(&record.node_name_hash);
                let _ = self.send_put_record(key, &record).await;
            }
        }

        let services = self.local_services.read().await.clone();
        for (service_name, info) in services {
            let record = ServiceRecord {
                node_id: self.local_identity.node_id.clone(),
                service_name: service_name.clone(),
                mount_point: info.mount_point.clone(),
                capabilities: info.capabilities.clone(),
                network_addr: self
                    .local_records
                    .read()
                    .await
                    .get(self.local_identity.node_id.as_str())
                    .map(|r| r.network_addr)
                    .unwrap_or_else(|| "0.0.0.0:0".parse().unwrap()),
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs(),
            };

            let service_key = Self::service_record_key(&service_name, &record.node_id);
            let _ = self.send_put_record(service_key, &record).await;

            let index_key = Self::service_index_key(&service_name);
            let mut index: Vec<String> = match self.send_get_record(index_key.clone()).await {
                Ok(Some(bytes)) => serde_cbor::from_slice(&bytes).unwrap_or_default(),
                _ => Vec::new(),
            };
            if !index.contains(&record.node_id.as_str().to_string()) {
                index.push(record.node_id.as_str().to_string());
                if let Ok(value) = serde_cbor::to_vec(&index) {
                    let _ = self.send_put_value(index_key, value).await;
                }
            }
        }

        Ok(())
    }

    pub async fn start_networking(
        &self,
        listen_addr: SocketAddr,
        bootstrap_addrs: Vec<Multiaddr>,
    ) -> Result<(), anyhow::Error> {
        info!("Starting libp2p DHT networking on {}", listen_addr);
        {
            let guard = self.network.read().await;
            if guard.is_some() {
                return Ok(());
            }
        }

        let keypair = identity::Keypair::ed25519_from_bytes(self.local_identity.ed25519_key.to_bytes())
            .map_err(|e| anyhow::anyhow!("Failed to create libp2p keypair: {e}"))?;
        let peer_id = PeerId::from(keypair.public());

        let transport = libp2p::tokio_development_transport(keypair.clone())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to build libp2p transport: {e}"))?;

        let store = MemoryStore::new(peer_id);
        let mut behaviour = Kademlia::new(peer_id, store);
        behaviour.set_mode(Some(libp2p::kad::Mode::Server));

        let mut swarm = SwarmBuilder::with_tokio_executor(transport, behaviour, peer_id).build();
        let listen_multiaddr = Self::socket_addr_to_multiaddr(listen_addr);
        swarm.listen_on(listen_multiaddr)?;

        for addr in bootstrap_addrs {
            if let Some(peer) = Self::peer_id_from_multiaddr(&addr) {
                swarm.behaviour_mut().add_address(&peer, addr);
            } else {
                warn!("Bootstrap addr missing peer id: {}", addr);
            }
        }

        let (sender, mut receiver) = mpsc::channel(64);
        let handle = NetworkHandle { sender: sender.clone() };

        {
            let mut guard = self.network.write().await;
            *guard = Some(handle);
        }

        let local_records = Arc::clone(&self.local_records);
        let storage = self.storage.clone();

        tokio::spawn(async move {
            let mut pending_get: HashMap<QueryId, oneshot::Sender<Result<Option<Vec<u8>>, anyhow::Error>>> =
                HashMap::new();
            let mut pending_put: HashMap<QueryId, oneshot::Sender<Result<(), anyhow::Error>>> =
                HashMap::new();

            loop {
                tokio::select! {
                    event = swarm.select_next_some() => {
                        match event {
                            SwarmEvent::Behaviour(KademliaEvent::OutboundQueryCompleted { id, result, .. }) => {
                                match result {
                                    QueryResult::GetRecord(Ok(GetRecordOk::FoundRecord(found))) => {
                                        if let Some(reply) = pending_get.remove(&id) {
                                            let _ = reply.send(Ok(Some(found.record.value)));
                                        }
                                    }
                                    QueryResult::GetRecord(Ok(GetRecordOk::FinishedWithNoAdditionalRecord { .. })) => {
                                        if let Some(reply) = pending_get.remove(&id) {
                                            let _ = reply.send(Ok(None));
                                        }
                                    }
                                    QueryResult::GetRecord(Err(e)) => {
                                        if let Some(reply) = pending_get.remove(&id) {
                                            let _ = reply.send(Err(anyhow::anyhow!("GetRecord failed: {e:?}")));
                                        }
                                    }
                                    QueryResult::PutRecord(Ok(PutRecordOk { .. })) => {
                                        if let Some(reply) = pending_put.remove(&id) {
                                            let _ = reply.send(Ok(()));
                                        }
                                    }
                                    QueryResult::PutRecord(Err(e)) => {
                                        if let Some(reply) = pending_put.remove(&id) {
                                            let _ = reply.send(Err(anyhow::anyhow!("PutRecord failed: {e:?}")));
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            SwarmEvent::Behaviour(KademliaEvent::InboundRequest { request }) => {
                                if let libp2p::kad::InboundRequest::PutRecord { source, record, .. } = request {
                                    if let Ok(parsed) = serde_cbor::from_slice::<DhtRecord>(&record.value) {
                                        let mut records = local_records.write().await;
                                        records.insert(parsed.node_id.as_str().to_string(), parsed.clone());
                                        if let Some(db) = storage.as_ref() {
                                            if let Ok(value) = serde_cbor::to_vec(&parsed) {
                                                let _ = db.insert(parsed.node_id.as_str().as_bytes(), value);
                                            }
                                        }
                                        debug!("Stored DHT record from {}", source);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    cmd = receiver.recv() => {
                        let Some(cmd) = cmd else { break; };
                        match cmd {
                            DhtCommand::PutRecord { key, value, reply } => {
                                let record = Record::new(RecordKey::new(&key), value);
                                match swarm.behaviour_mut().put_record(record, Quorum::One) {
                                    Ok(id) => { pending_put.insert(id, reply); }
                                    Err(e) => { let _ = reply.send(Err(anyhow::anyhow!("PutRecord failed: {e}"))); }
                                }
                            }
                            DhtCommand::GetRecord { key, reply } => {
                                let id = swarm.behaviour_mut().get_record(RecordKey::new(&key));
                                pending_get.insert(id, reply);
                            }
                            DhtCommand::AddAddress { peer, addr } => {
                                swarm.behaviour_mut().add_address(&peer, addr);
                            }
                            DhtCommand::Bootstrap => {
                                let _ = swarm.behaviour_mut().bootstrap();
                            }
                        }
                    }
                }
            }
        });

        if !bootstrap_addrs.is_empty() {
            let _ = self.send_bootstrap().await;
        }

        Ok(())
    }

    async fn load_from_store(&mut self) -> Result<(), anyhow::Error> {
        let db = match self.storage.as_ref() {
            Some(db) => db,
            None => return Ok(()),
        };

        let mut records = HashMap::new();
        for entry in db.iter() {
            let (_, value) = entry?;
            let record: DhtRecord = serde_cbor::from_slice(&value)?;
            records.insert(record.node_id.as_str().to_string(), record);
        }

        let mut guard = self.local_records.write().await;
        *guard = records;
        Ok(())
    }

    async fn network_handle(&self) -> Option<NetworkHandle> {
        let guard = self.network.read().await;
        guard.clone()
    }

    async fn send_put_record<T: Serialize>(
        &self,
        key: Vec<u8>,
        record: &T,
    ) -> Result<(), anyhow::Error> {
        let value = serde_cbor::to_vec(record)?;
        self.send_put_value(key, value).await
    }

    async fn send_put_value(&self, key: Vec<u8>, value: Vec<u8>) -> Result<(), anyhow::Error> {
        if let Some(handle) = self.network_handle().await {
            let (tx, rx) = oneshot::channel();
            handle
                .sender
                .send(DhtCommand::PutRecord { key, value, reply: tx })
                .await
                .map_err(|_| anyhow::anyhow!("DHT network task unavailable"))?;
            return rx.await.map_err(|_| anyhow::anyhow!("DHT put cancelled"))?;
        }
        Ok(())
    }

    async fn send_get_record(&self, key: Vec<u8>) -> Result<Option<Vec<u8>>, anyhow::Error> {
        if let Some(handle) = self.network_handle().await {
            let (tx, rx) = oneshot::channel();
            handle
                .sender
                .send(DhtCommand::GetRecord { key, reply: tx })
                .await
                .map_err(|_| anyhow::anyhow!("DHT network task unavailable"))?;
            return rx.await.map_err(|_| anyhow::anyhow!("DHT get cancelled"))?;
        }
        Ok(None)
    }

    async fn send_add_address(&self, peer: PeerId, addr: Multiaddr) -> Result<(), anyhow::Error> {
        if let Some(handle) = self.network_handle().await {
            handle
                .sender
                .send(DhtCommand::AddAddress { peer, addr })
                .await
                .map_err(|_| anyhow::anyhow!("DHT network task unavailable"))?;
        }
        Ok(())
    }

    async fn send_bootstrap(&self) -> Result<(), anyhow::Error> {
        if let Some(handle) = self.network_handle().await {
            handle
                .sender
                .send(DhtCommand::Bootstrap)
                .await
                .map_err(|_| anyhow::anyhow!("DHT network task unavailable"))?;
        }
        Ok(())
    }

    fn socket_addr_to_multiaddr(addr: SocketAddr) -> Multiaddr {
        match addr.ip() {
            std::net::IpAddr::V4(ip) => {
                Multiaddr::from(Protocol::Ip4(ip)).with(Protocol::Tcp(addr.port()))
            }
            std::net::IpAddr::V6(ip) => {
                Multiaddr::from(Protocol::Ip6(ip)).with(Protocol::Tcp(addr.port()))
            }
        }
    }

    fn peer_id_from_multiaddr(addr: &Multiaddr) -> Option<PeerId> {
        addr.iter().last().and_then(|proto| {
            if let Protocol::P2p(multihash) = proto {
                PeerId::from_multihash(multihash).ok()
            } else {
                None
            }
        })
    }

    fn persist_record(&self, record: &DhtRecord) -> Result<(), anyhow::Error> {
        if let Some(db) = self.storage.as_ref() {
            let value = serde_cbor::to_vec(record)?;
            db.insert(record.node_id.as_str().as_bytes(), value)?;
        }
        Ok(())
    }

    fn name_record_key(name_hash: &[u8]) -> Vec<u8> {
        let mut key = b"name:".to_vec();
        key.extend_from_slice(name_hash);
        key
    }

    fn service_record_key(service_name: &str, node_id: &NodeId) -> Vec<u8> {
        format!("service:{}:{}", service_name, node_id.as_str()).into_bytes()
    }

    fn service_index_key(service_name: &str) -> Vec<u8> {
        format!("service-index:{}", service_name).into_bytes()
    }
    
    /// Register our node in the DHT
    pub async fn register_self(&self, listen_addr: SocketAddr) -> Result<(), anyhow::Error> {
        self.register_self_with_name(listen_addr, None).await
    }

    /// Register our node in the DHT with a friendly name.
    pub async fn register_self_with_name(
        &self,
        listen_addr: SocketAddr,
        node_name: Option<String>,
    ) -> Result<(), anyhow::Error> {
        info!("Registering node {} in DHT", self.local_identity.node_id.as_str());

        let name_hash = if let Some(ref name) = node_name {
            Self::name_hash_for_addr(&listen_addr, name)
        } else {
            Vec::new()
        };

        // Create our DHT record
        let record = DhtRecord {
            node_id: self.local_identity.node_id.clone(),
            public_key: self.local_identity.ed25519_public.to_bytes().to_vec(),
            p256_public_key: self.local_identity.p256_public_key_bytes(),
            certificate_der: self.local_identity.certificate.clone(),
            permissions: self.local_identity.permissions.clone(),
            node_name,
            node_name_hash: name_hash,
            network_addr: listen_addr,
            services: HashMap::new(),
            capabilities: Default::default(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
        };
        
        // Store in local records
        {
            let mut records = self.local_records.write().await;
            records.insert(self.local_identity.node_id.as_str().to_string(), record.clone());
        }

        self.persist_record(&record)?;

        let _ = self
            .send_put_record(record.node_id.as_str().as_bytes().to_vec(), &record)
            .await;
        if !record.node_name_hash.is_empty() {
            let key = Self::name_record_key(&record.node_name_hash);
            let _ = self.send_put_record(key, &record).await;
        }
        
        Ok(())
    }
    
    /// Advertise a service provided by this node
    pub async fn advertise_service(
        &self, 
        service_name: String, 
        mount_point: String,
        capabilities: crate::identity::ServiceCapabilities
    ) -> Result<(), anyhow::Error> {
        info!("Advertising service '{}' at mount point '{}'", service_name, mount_point);
        
        // Update our local record with the new service
        let updated_record = {
            let mut records = self.local_records.write().await;
            if let Some(record) = records.get_mut(self.local_identity.node_id.as_str()) {
                let service_info = crate::identity::ServiceInfo {
                    mount_point: mount_point.clone(),
                    service_type: service_name.clone(),
                    capabilities: capabilities.clone(),
                };

                record.services.insert(service_name.clone(), service_info);
                record.timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs();

                let _ = self.persist_record(record);
                Some(record.clone())
            } else {
                None
            }
        };

        if let Some(record) = updated_record {
            let _ = self
                .send_put_record(record.node_id.as_str().as_bytes().to_vec(), &record)
                .await;
            if !record.node_name_hash.is_empty() {
                let key = Self::name_record_key(&record.node_name_hash);
                let _ = self.send_put_record(key, &record).await;
            }
        }

        // Track locally for refresh
        {
            let mut services = self.local_services.write().await;
            services.insert(
                service_name.clone(),
                ServiceInfo {
                    mount_point: mount_point.clone(),
                    service_type: service_name.clone(),
                    capabilities: capabilities.clone(),
                },
            );
        }

        // Publish service record + update index
        let record = ServiceRecord {
            node_id: self.local_identity.node_id.clone(),
            service_name: service_name.clone(),
            mount_point,
            capabilities,
            network_addr: self
                .local_records
                .read()
                .await
                .get(self.local_identity.node_id.as_str())
                .map(|r| r.network_addr)
                .unwrap_or_else(|| "0.0.0.0:0".parse().unwrap()),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
        };

        let service_key = Self::service_record_key(&service_name, &record.node_id);
        let _ = self.send_put_record(service_key, &record).await;

        let index_key = Self::service_index_key(&service_name);
        let mut index: Vec<String> = match self.send_get_record(index_key.clone()).await {
            Ok(Some(bytes)) => serde_cbor::from_slice(&bytes).unwrap_or_default(),
            _ => Vec::new(),
        };
        if !index.contains(&record.node_id.as_str().to_string()) {
            index.push(record.node_id.as_str().to_string());
            let index_record = serde_cbor::to_vec(&index)?;
            let _ = self.send_put_value(index_key, index_record).await;
        }

        Ok(())
    }
    
    /// Lookup a node by ID
    pub async fn lookup_node(&self, node_id: &NodeId) -> Option<DhtRecord> {
        debug!("Looking up node: {}", node_id.as_str());
        
        // First check local records
        {
            let records = self.local_records.read().await;
            if let Some(record) = records.get(node_id.as_str()) {
                return Some(record.clone());
            }
        }

        let key = node_id.as_str().as_bytes().to_vec();
        if let Ok(Ok(Some(value))) = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            self.send_get_record(key),
        )
        .await
        {
            if let Ok(record) = serde_cbor::from_slice::<DhtRecord>(&value) {
                let mut records = self.local_records.write().await;
                records.insert(record.node_id.as_str().to_string(), record.clone());
                let _ = self.persist_record(&record);
                return Some(record);
            }
        }

        None
    }

    /// Lookup a node by friendly name + address hash.
    pub async fn lookup_by_name_hash(&self, name_hash: &[u8]) -> Option<DhtRecord> {
        {
            let records = self.local_records.read().await;
            if let Some(record) = records
                .values()
                .find(|record| record.node_name_hash.as_slice() == name_hash)
            {
                return Some(record.clone());
            }
        }

        let key = Self::name_record_key(name_hash);
        if let Ok(Ok(Some(value))) = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            self.send_get_record(key),
        )
        .await
        {
            if let Ok(record) = serde_cbor::from_slice::<DhtRecord>(&value) {
                let mut records = self.local_records.write().await;
                records.insert(record.node_id.as_str().to_string(), record.clone());
                let _ = self.persist_record(&record);
                return Some(record);
            }
        }

        None
    }

    /// Insert or update a peer record from a verified auth response
    pub async fn upsert_peer_record(
        &self,
        node_id: NodeId,
        ed25519_public_key: Vec<u8>,
        p256_public_key: Vec<u8>,
        certificate_der: Vec<u8>,
        network_addr: Option<SocketAddr>,
        permissions: NodePermissions,
    ) -> Result<(), anyhow::Error> {
        let record = DhtRecord {
            node_id: node_id.clone(),
            public_key: ed25519_public_key,
            p256_public_key,
            certificate_der,
            permissions,
            node_name: None,
            node_name_hash: Vec::new(),
            network_addr: network_addr.unwrap_or_else(|| "0.0.0.0:0".parse().unwrap()),
            services: HashMap::new(),
            capabilities: Default::default(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
        };

        let mut records = self.local_records.write().await;
        records.insert(node_id.as_str().to_string(), record.clone());
        self.persist_record(&record)?;

        let _ = self
            .send_put_record(record.node_id.as_str().as_bytes().to_vec(), &record)
            .await;
        if !record.node_name_hash.is_empty() {
            let key = Self::name_record_key(&record.node_name_hash);
            let _ = self.send_put_record(key, &record).await;
        }
        if let (Ok(peer_id), Some(addr)) = (
            Self::peer_id_from_ed25519_public(&record.public_key),
            network_addr,
        ) {
            let multiaddr = Self::socket_addr_to_multiaddr(addr);
            let _ = self.send_add_address(peer_id, multiaddr).await;
        }
        Ok(())
    }

    pub fn name_hash_for_addr(addr: &SocketAddr, node_name: &str) -> Vec<u8> {
        let mut hasher = sha2::Sha256::new();
        hasher.update(addr.to_string().as_bytes());
        hasher.update(b"@");
        hasher.update(node_name.as_bytes());
        hasher.finalize().to_vec()
    }
    
    /// Find nodes providing a specific service
    pub async fn find_nodes_with_service(&self, service_name: &str) -> Vec<DhtRecord> {
        let mut results = Vec::new();
        {
            let records = self.local_records.read().await;
            for record in records.values() {
                if record.services.contains_key(service_name) {
                    results.push(record.clone());
                }
            }
        }

        let index_key = Self::service_index_key(service_name);
        if let Ok(Some(bytes)) = self.send_get_record(index_key).await {
            if let Ok(node_ids) = serde_cbor::from_slice::<Vec<String>>(&bytes) {
                for node_id in node_ids {
                    if results.iter().any(|r| r.node_id.as_str() == node_id) {
                        continue;
                    }
                    if let Some(record) = self.lookup_node(&NodeId::new(node_id)).await {
                        results.push(record);
                    }
                }
            }
        }

        results
    }
    
    /// Get all known peers
    pub async fn get_all_peers(&self) -> Vec<DhtRecord> {
        let records = self.local_records.read().await;
        records.values().cloned().collect()
    }

    /// Update a peer's network address and add it to the DHT routing table when available.
    pub async fn update_peer_address(
        &self,
        node_id: &NodeId,
        addr: SocketAddr,
    ) -> Result<(), anyhow::Error> {
        let record = {
            let mut records = self.local_records.write().await;
            if let Some(record) = records.get_mut(node_id.as_str()) {
                record.network_addr = addr;
                record.timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs();
                self.persist_record(record)?;
                Some(record.clone())
            } else {
                None
            }
        };

        if let Some(record) = record {
            let _ = self
                .send_put_record(record.node_id.as_str().as_bytes().to_vec(), &record)
                .await;
            if !record.node_name_hash.is_empty() {
                let key = Self::name_record_key(&record.node_name_hash);
                let _ = self.send_put_record(key, &record).await;
            }
            if let Ok(peer_id) = Self::peer_id_from_ed25519_public(&record.public_key) {
                let multiaddr = Self::socket_addr_to_multiaddr(addr);
                let _ = self.send_add_address(peer_id, multiaddr).await;
            }
        } else {
            warn!("Cannot update address for unknown peer {}", node_id.as_str());
        }
        Ok(())
    }
    
    /// Bootstrap with known peers
    pub async fn bootstrap(&self, bootstrap_peers: &[SocketAddr]) -> Result<(), anyhow::Error> {
        info!("Bootstrapping DHT with {} peers", bootstrap_peers.len());

        let records = self.local_records.read().await;
        for &addr in bootstrap_peers {
            if let Some(record) = records.values().find(|r| r.network_addr == addr) {
                if let Ok(peer_id) = Self::peer_id_from_ed25519_public(&record.public_key) {
                    let multiaddr = Self::socket_addr_to_multiaddr(addr);
                    let _ = self.send_add_address(peer_id, multiaddr).await;
                }
            } else {
                debug!("Bootstrap peer {} not in local records", addr);
            }
        }
        let _ = self.send_bootstrap().await;

        Ok(())
    }
    
    fn peer_id_from_ed25519_public(public_key: &[u8]) -> Result<PeerId, anyhow::Error> {
        let pubkey = identity::ed25519::PublicKey::try_from_bytes(public_key)
            .map_err(|e| anyhow::anyhow!("Invalid ed25519 public key: {e}"))?;
        Ok(PeerId::from_public_key(&identity::PublicKey::Ed25519(pubkey)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use tempfile::tempdir;
    
    #[tokio::test]
    async fn test_sovereign_dht_registration() {
        let identity = Arc::new(SovereignIdentity::generate().expect("Failed to generate identity"));
        let dht = SovereignDht::new(identity.clone());
        
        let addr: SocketAddr = "127.0.0.1:9000".parse().unwrap();
        dht.register_self(addr).await.expect("Failed to register node");
        
        // Should be able to lookup our own record
        let record = dht.lookup_node(&identity.node_id).await;
        assert!(record.is_some());
        let record = record.unwrap();
        assert_eq!(record.network_addr, addr);
        assert_eq!(record.public_key, identity.ed25519_public.to_bytes().to_vec());
        assert_eq!(record.p256_public_key, identity.p256_public_key_bytes());
        assert_eq!(record.certificate_der, identity.certificate);
        assert_eq!(record.permissions.max_concurrent_jobs, identity.permissions.max_concurrent_jobs);
    }
    
    #[tokio::test]
    async fn test_service_advertisement() {
        let identity = Arc::new(SovereignIdentity::generate().expect("Failed to generate identity"));
        let dht = SovereignDht::new(identity.clone());
        
        let addr: SocketAddr = "127.0.0.1:9000".parse().unwrap();
        dht.register_self(addr).await.expect("Failed to register");
        
        let capabilities = crate::identity::ServiceCapabilities::default();
        dht.advertise_service("compute".to_string(), "/srv/compute".to_string(), capabilities)
            .await
            .expect("Failed to advertise service");
            
        let compute_nodes = dht.find_nodes_with_service("compute").await;
        assert_eq!(compute_nodes.len(), 1);
        assert_eq!(compute_nodes[0].node_id, identity.node_id);
    }

    #[tokio::test]
    async fn test_dht_persistence_roundtrip() {
        let temp_dir = tempdir().expect("tempdir");
        let identity = Arc::new(SovereignIdentity::generate().expect("Failed to generate identity"));
        let dht = SovereignDht::new_with_store(identity.clone(), temp_dir.path())
            .await
            .expect("create dht");

        let addr: SocketAddr = "127.0.0.1:9001".parse().unwrap();
        dht.register_self(addr).await.expect("register self");

        let reloaded = SovereignDht::new_with_store(identity.clone(), temp_dir.path())
            .await
            .expect("reload dht");

        let record = reloaded.lookup_node(&identity.node_id).await;
        assert!(record.is_some());
        let record = record.unwrap();
        assert_eq!(record.public_key, identity.ed25519_public.to_bytes().to_vec());
        assert_eq!(record.p256_public_key, identity.p256_public_key_bytes());
        assert_eq!(record.certificate_der, identity.certificate);
    }
}
