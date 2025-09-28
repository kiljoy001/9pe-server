//! Namespace Consensus - Public network ordering with namespace isolation
//!
//! Orders transactions across public internet while maintaining namespace boundaries.
//! Designed for future GPU/CPU compute pooling across trusted namespace members.

use std::collections::{HashMap, VecDeque, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use anyhow::Result;
use tracing::{info, debug, warn};
use serde::{Serialize, Deserialize};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey, Verifier};

/// Maximum events per namespace
const MAX_EVENTS_PER_NS: usize = 1000;

/// Time before events are forgotten
const FORGET_AFTER_SECS: u64 = 600; // 10 minutes

/// Namespace-scoped event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceEvent {
    /// Sequence within namespace
    pub sequence: u64,

    /// Which namespace this belongs to
    pub namespace: String,

    /// Event details
    pub event: EventType,

    /// Timestamp
    pub timestamp: u64,

    /// Who submitted it (public key)
    pub submitter: Vec<u8>,

    /// Signature (proves namespace membership)
    pub signature: Vec<u8>,
}

/// Event types for namespace operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventType {
    /// File operation in namespace
    FileOp {
        path: String,
        op: FileOperation,
    },

    /// Compute resource offered
    ComputeOffer {
        gpu_count: u32,
        cpu_cores: u32,
        memory_gb: u32,
        price_per_hour: f64,
    },

    /// Compute job submitted
    ComputeJob {
        job_id: String,
        required_gpus: u32,
        transformer_code: Vec<u8>,
    },

    /// Namespace membership change
    MembershipChange {
        member_key: Vec<u8>,
        action: MemberAction,
    },

    /// Resource allocation
    ResourceGrant {
        from_member: Vec<u8>,
        to_member: Vec<u8>,
        resource: ResourceType,
        duration_secs: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileOperation {
    Read,
    Write,
    Delete,
    Execute,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemberAction {
    Join,
    Leave,
    Promote,  // Can approve compute jobs
    Demote,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResourceType {
    Gpu { count: u32 },
    Cpu { cores: u32 },
    Storage { gb: u32 },
}

/// Namespace membership and permissions
#[derive(Debug, Clone)]
pub struct NamespaceMember {
    pub public_key: VerifyingKey,
    pub joined_at: u64,
    pub can_approve_compute: bool,
    pub trust_score: u32,  // 0-100, earned over time
}

/// Namespace state (ephemeral)
pub struct NamespaceState {
    /// Namespace identifier
    pub namespace_id: String,

    /// Members with permissions
    pub members: Arc<RwLock<HashMap<Vec<u8>, NamespaceMember>>>,

    /// Recent events (rolling window)
    pub events: Arc<RwLock<VecDeque<NamespaceEvent>>>,

    /// Current sequence number
    pub sequence: Arc<RwLock<u64>>,

    /// Available compute resources
    pub compute_pool: Arc<RwLock<ComputePool>>,
}

/// Available compute resources in namespace
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComputePool {
    pub total_gpus: u32,
    pub total_cpu_cores: u32,
    pub total_memory_gb: u32,
    pub providers: HashMap<Vec<u8>, ComputeProvider>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeProvider {
    pub gpus: u32,
    pub cpu_cores: u32,
    pub memory_gb: u32,
    pub available: bool,
    pub last_heartbeat: u64,
}

/// Public Network Consensus - manages multiple namespaces
pub struct NamespaceConsensus {
    /// All namespace states
    namespaces: Arc<RwLock<HashMap<String, Arc<NamespaceState>>>>,

    /// Our signing key (for this node)
    node_key: SigningKey,

    /// Our namespaces (ones we're members of)
    our_namespaces: Arc<RwLock<HashSet<String>>>,
}

impl NamespaceConsensus {
    /// Create new namespace consensus
    pub async fn new() -> Result<Self> {
        info!("🌐 Initializing Namespace Consensus for PUBLIC network");

        let node_key = SigningKey::from_bytes(&rand::random());
        let public_key = VerifyingKey::from(&node_key);

        info!("🔑 Node public key: {:?}", hex::encode(public_key.to_bytes()));

        Ok(Self {
            namespaces: Arc::new(RwLock::new(HashMap::new())),
            node_key,
            our_namespaces: Arc::new(RwLock::new(HashSet::new())),
        })
    }

    /// Join a namespace
    pub async fn join_namespace(&self, namespace_id: String) -> Result<()> {
        info!("📁 Joining namespace: {}", namespace_id);

        // Create or get namespace
        let mut namespaces = self.namespaces.write().await;
        let ns = namespaces.entry(namespace_id.clone())
            .or_insert_with(|| Arc::new(NamespaceState {
                namespace_id: namespace_id.clone(),
                members: Arc::new(RwLock::new(HashMap::new())),
                events: Arc::new(RwLock::new(VecDeque::with_capacity(MAX_EVENTS_PER_NS))),
                sequence: Arc::new(RwLock::new(0)),
                compute_pool: Arc::new(RwLock::new(ComputePool::default())),
            }));

        // Add ourselves as member
        let public_key = VerifyingKey::from(&self.node_key);
        let member = NamespaceMember {
            public_key,
            joined_at: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            can_approve_compute: false,  // Start with no privileges
            trust_score: 0,
        };

        ns.members.write().await.insert(
            public_key.to_bytes().to_vec(),
            member
        );

        self.our_namespaces.write().await.insert(namespace_id);

        Ok(())
    }

    /// Submit event to namespace
    pub async fn submit_event(
        &self,
        namespace_id: &str,
        event: EventType,
    ) -> Result<u64> {
        // Check we're in this namespace
        if !self.our_namespaces.read().await.contains(namespace_id) {
            return Err(anyhow::anyhow!("Not a member of namespace"));
        }

        let namespaces = self.namespaces.read().await;
        let ns = namespaces.get(namespace_id)
            .ok_or_else(|| anyhow::anyhow!("Namespace not found"))?;

        // Get next sequence
        let sequence = {
            let mut seq = ns.sequence.write().await;
            *seq += 1;
            *seq
        };

        // Create event
        let public_key = VerifyingKey::from(&self.node_key);
        let mut ns_event = NamespaceEvent {
            sequence,
            namespace: namespace_id.to_string(),
            event: event.clone(),
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            submitter: public_key.to_bytes().to_vec(),
            signature: vec![],
        };

        // Sign it (proves we're a member)
        let event_bytes = bincode::serialize(&(
            &ns_event.sequence,
            &ns_event.namespace,
            &ns_event.event,
            &ns_event.timestamp,
        ))?;

        let signature = self.node_key.sign(&event_bytes);
        ns_event.signature = signature.to_bytes().to_vec();

        // Add to namespace events
        let mut events = ns.events.write().await;
        events.push_back(ns_event.clone());

        // Maintain size limit
        while events.len() > MAX_EVENTS_PER_NS {
            events.pop_front();
        }

        // Update compute pool if needed
        if let EventType::ComputeOffer { gpu_count, cpu_cores, memory_gb, .. } = event {
            let mut pool = ns.compute_pool.write().await;
            pool.providers.insert(
                public_key.to_bytes().to_vec(),
                ComputeProvider {
                    gpus: gpu_count,
                    cpu_cores,
                    memory_gb,
                    available: true,
                    last_heartbeat: ns_event.timestamp,
                }
            );
            pool.total_gpus += gpu_count;
            pool.total_cpu_cores += cpu_cores;
            pool.total_memory_gb += memory_gb;

            info!("🖥️ Compute resources added to {}: {} GPUs, {} CPU cores",
                  namespace_id, gpu_count, cpu_cores);
        }

        debug!("Event {} submitted to namespace {}", sequence, namespace_id);

        Ok(sequence)
    }

    /// Verify and process event from network
    pub async fn handle_network_event(&self, event: NamespaceEvent) -> Result<()> {
        // Get namespace
        let namespaces = self.namespaces.read().await;
        let ns = namespaces.get(&event.namespace)
            .ok_or_else(|| anyhow::anyhow!("Unknown namespace"))?;

        // Verify signature (proves membership)
        let public_key_bytes: [u8; 32] = event.submitter.as_slice().try_into()?;
        let public_key = VerifyingKey::from_bytes(&public_key_bytes)?;

        let event_bytes = bincode::serialize(&(
            &event.sequence,
            &event.namespace,
            &event.event,
            &event.timestamp,
        ))?;

        let signature = Signature::from_slice(&event.signature)?;

        public_key.verify(&event_bytes, &signature)
            .map_err(|_| anyhow::anyhow!("Invalid signature - not a namespace member"))?;

        // Verify sender is actually a member
        if !ns.members.read().await.contains_key(&event.submitter) {
            warn!("Event from non-member, rejecting");
            return Err(anyhow::anyhow!("Not a member"));
        }

        // Add to events
        let mut events = ns.events.write().await;

        // Insert in order
        let pos = events.iter().position(|e| e.sequence > event.sequence)
            .unwrap_or(events.len());

        events.insert(pos, event);

        // Maintain size
        while events.len() > MAX_EVENTS_PER_NS {
            events.pop_front();
        }

        Ok(())
    }

    /// Get namespace state for synchronization
    pub async fn get_namespace_snapshot(&self, namespace_id: &str) -> Result<NamespaceSnapshot> {
        let namespaces = self.namespaces.read().await;
        let ns = namespaces.get(namespace_id)
            .ok_or_else(|| anyhow::anyhow!("Namespace not found"))?;

        // Clone the namespace reference to avoid borrow issues
        let ns = ns.clone();
        drop(namespaces);

        let members = ns.members.read().await
            .keys()
            .cloned()
            .collect();
        let recent_events = ns.events.read().await
            .iter()
            .cloned()
            .collect();
        let compute_pool = ns.compute_pool.read().await.clone();

        Ok(NamespaceSnapshot {
            namespace_id: namespace_id.to_string(),
            members,
            recent_events,
            compute_pool,
        })
    }

    /// Check available compute in namespace
    pub async fn get_compute_availability(&self, namespace_id: &str) -> Result<ComputePool> {
        let namespaces = self.namespaces.read().await;
        let ns = namespaces.get(namespace_id)
            .ok_or_else(|| anyhow::anyhow!("Namespace not found"))?;

        let ns = ns.clone();
        drop(namespaces);

        let compute_pool = ns.compute_pool.read().await.clone();
        Ok(compute_pool)
    }

    /// Submit GPU compute job to namespace
    pub async fn submit_compute_job(
        &self,
        namespace_id: &str,
        transformer_code: Vec<u8>,
        required_gpus: u32,
    ) -> Result<String> {
        let job_id = format!("job_{}", uuid::Uuid::new_v4());

        let event = EventType::ComputeJob {
            job_id: job_id.clone(),
            required_gpus,
            transformer_code,
        };

        self.submit_event(namespace_id, event).await?;

        info!("🎯 Compute job {} submitted to namespace {}", job_id, namespace_id);

        Ok(job_id)
    }
}

/// Snapshot for synchronization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceSnapshot {
    pub namespace_id: String,
    pub members: Vec<Vec<u8>>,
    pub recent_events: Vec<NamespaceEvent>,
    pub compute_pool: ComputePool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_namespace_consensus() {
        let consensus = NamespaceConsensus::new().await.unwrap();

        // Join namespace
        consensus.join_namespace("test_ns".to_string()).await.unwrap();

        // Submit event
        let event = EventType::FileOp {
            path: "/test/file.txt".to_string(),
            op: FileOperation::Write,
        };

        let seq = consensus.submit_event("test_ns", event).await.unwrap();
        assert_eq!(seq, 1);
    }

    #[tokio::test]
    async fn test_compute_pool() {
        let consensus = NamespaceConsensus::new().await.unwrap();

        consensus.join_namespace("gpu_ns".to_string()).await.unwrap();

        // Offer compute resources
        let offer = EventType::ComputeOffer {
            gpu_count: 4,
            cpu_cores: 32,
            memory_gb: 128,
            price_per_hour: 0.5,
        };

        consensus.submit_event("gpu_ns", offer).await.unwrap();

        // Check pool
        let pool = consensus.get_compute_availability("gpu_ns").await.unwrap();
        assert_eq!(pool.total_gpus, 4);
    }
}