//! DHT integration for sovereign node discovery and peer management
//!
//! This module extends the existing KademliaTable with sovereign identity records
//! and integration with the libp2p DHT when available.

use crate::identity::{DhtRecord, NodeId, NodePermissions, SovereignIdentity};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, debug, warn};

/// DHT record storage with sovereign identity support
#[derive(Debug)]
pub struct SovereignDht {
    /// Local node identity
    local_identity: Arc<SovereignIdentity>,
    
    /// In-memory DHT records (when libp2p not available)
    local_records: Arc<RwLock<HashMap<String, DhtRecord>>>,

    /// Persistent storage for DHT records
    storage: Option<sled::Db>,
    
    /// Libp2p Kademlia behaviour (when available)
    #[cfg(feature = "cluster")]
    libp2p_kademlia: Option<libp2p::kad::Behaviour<libp2p::kad::store::MemoryStore>>,
}

impl SovereignDht {
    /// Create a new sovereign DHT instance
    pub fn new(local_identity: Arc<SovereignIdentity>) -> Self {
        info!("Initializing sovereign DHT for node: {}", local_identity.node_id.as_str());
        
        #[cfg(feature = "cluster")]
        let libp2p_kademlia = {
            use libp2p::kad::{Kademlia, KademliaConfig, store::MemoryStore};
            use libp2p::PeerId;
            
            // Create libp2p peer ID from our sovereign identity
            let peer_id = Self::peer_id_from_identity(&local_identity);
            let store = MemoryStore::new(peer_id);
            let mut kademlia = Kademlia::with_config(peer_id, store, KademliaConfig::default());
            
            // Bootstrap with ourselves as initial peer
            let local_key = libp2p::identity::Keypair::ed25519_from_bytes(
                local_identity.ed25519_key.to_bytes()
            ).expect("Failed to create libp2p keypair");
            
            Some(kademlia)
        };
        
        #[cfg(not(feature = "cluster"))]
        let libp2p_kademlia = None;
        
        Self {
            local_identity,
            local_records: Arc::new(RwLock::new(HashMap::new())),
            storage: None,
            libp2p_kademlia,
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

    fn persist_record(&self, record: &DhtRecord) -> Result<(), anyhow::Error> {
        if let Some(db) = self.storage.as_ref() {
            let value = serde_cbor::to_vec(record)?;
            db.insert(record.node_id.as_str().as_bytes(), value)?;
        }
        Ok(())
    }
    
    /// Register our node in the DHT
    pub async fn register_self(&self, listen_addr: SocketAddr) -> Result<(), anyhow::Error> {
        info!("Registering node {} in DHT", self.local_identity.node_id.as_str());
        
        // Create our DHT record
        let record = DhtRecord {
            node_id: self.local_identity.node_id.clone(),
            public_key: self.local_identity.ed25519_public.to_bytes().to_vec(),
            p256_public_key: self.local_identity.p256_public_key_bytes(),
            certificate_der: self.local_identity.certificate.clone(),
            permissions: self.local_identity.permissions.clone(),
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
        
        // Publish to libp2p DHT if available
        #[cfg(feature = "cluster")]
        if let Some(ref kademlia) = self.libp2p_kademlia {
            use libp2p::kad::{Record, store::MemoryStore};
            
            let key = libp2p::kad::RecordKey::new(&self.local_identity.node_id.as_str().as_bytes());
            let value = serde_json::to_vec(&record)?;
            let record = Record::new(key, value);
            
            // Note: This is conceptual - actual integration requires event loop
            debug!("Publishing record to libp2p DHT for node {}", record.key);
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
        let mut records = self.local_records.write().await;
        if let Some(record) = records.get_mut(self.local_identity.node_id.as_str()) {
            let service_info = crate::identity::ServiceInfo {
                mount_point,
                service_type: service_name.clone(),
                capabilities,
            };
            
            record.services.insert(service_name, service_info);
            record.timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs();

            let _ = self.persist_record(record);
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
        
        // If libp2p DHT is available, query it
        #[cfg(feature = "cluster")]
        if let Some(ref _kademlia) = self.libp2p_kademlia {
            // Conceptual lookup - actual implementation would use libp2p events
            debug!("Querying libp2p DHT for node: {}", node_id.as_str());
            // Implementation would require integrating with the libp2p event loop
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
        Ok(())
    }
    
    /// Find nodes providing a specific service
    pub async fn find_nodes_with_service(&self, service_name: &str) -> Vec<DhtRecord> {
        let mut results = Vec::new();
        let records = self.local_records.read().await;
        
        for record in records.values() {
            if record.services.contains_key(service_name) {
                results.push(record.clone());
            }
        }
        
        results
    }
    
    /// Get all known peers
    pub async fn get_all_peers(&self) -> Vec<DhtRecord> {
        let records = self.local_records.read().await;
        records.values().cloned().collect()
    }
    
    /// Bootstrap with known peers
    pub async fn bootstrap(&self, bootstrap_peers: &[SocketAddr]) -> Result<(), anyhow::Error> {
        info!("Bootstrapping DHT with {} peers", bootstrap_peers.len());
        
        #[cfg(feature = "cluster")]
        if let Some(ref _kademlia) = self.libp2p_kademlia {
            // Connect to bootstrap peers conceptually
            for &addr in bootstrap_peers {
                debug!("Connecting to bootstrap peer: {}", addr);
                // Actual libp2p connection would happen through swarm integration
            }
        }
        
        // For local DHT, we don't actively bootstrap since peers register themselves
        
        Ok(())
    }
    
    #[cfg(feature = "cluster")]
    fn peer_id_from_identity(identity: &SovereignIdentity) -> libp2p::PeerId {
        use libp2p::PeerId;
        PeerId::from_public_key(&libp2p::identity::PublicKey::Ed25519(
            identity.ed25519_public.to_bytes().into()
        ))
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
