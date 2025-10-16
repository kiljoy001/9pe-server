//! GHOSTDAG Consensus Implementation for 9P.e
//!
//! This module implements a GHOSTDAG-based consensus system for distributed
//! work coordination. It provides:
//! - DAG-based consensus for work ordering
//! - Cryptographic security for work distribution
//! - Read-only primitives for WASM transformers
//! - Byzantine fault tolerance

pub mod bounded_ghostdag;
pub mod crypto;
pub mod dynamic_scaling;
pub mod ghostdag;
pub mod network;
pub mod work_distribution;

pub use bounded_ghostdag::{Block, BlockId, BlockState, BoundedGhostdag, DagStats, NamespaceOp};
pub use crypto::{CryptoProvider, PublicKey, Signature};
pub use dynamic_scaling::{DynamicScaler, ScaleDecision, ScalingParams};
pub use ghostdag::WorkResult;
pub use ghostdag::{ConsensusState, GhostdagConsensus, WorkBlock};
pub use network::{NetworkConsensus, PeerManager};
pub use work_distribution::{JobRequest, WorkDistributor};

use anyhow::Result;
use crypto::TrustedKeyStore;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Main consensus coordinator for the 9P.e server
pub struct ConsensusCoordinator {
    ghostdag: Arc<RwLock<GhostdagConsensus>>,
    work_distributor: Arc<WorkDistributor>,
    network: Arc<NetworkConsensus>,
    #[allow(dead_code)]
    crypto: Arc<dyn CryptoProvider>,
    trusted_keys: Arc<RwLock<TrustedKeyStore>>,
}

impl ConsensusCoordinator {
    pub fn new(node_id: String, crypto: Arc<dyn CryptoProvider>) -> Self {
        let mut key_store = TrustedKeyStore::new();
        let local_public_key = crypto.get_public_key();
        key_store.add_trusted_key(node_id.clone(), local_public_key);

        let trusted_keys = Arc::new(RwLock::new(key_store));

        let ghostdag = Arc::new(RwLock::new(GhostdagConsensus::new(
            node_id.clone(),
            Arc::clone(&crypto),
            Arc::clone(&trusted_keys),
        )));
        let work_distributor = Arc::new(WorkDistributor::new(node_id.clone()));
        let network = Arc::new(
            NetworkConsensus::new(node_id.clone()).with_trusted_store(Arc::clone(&trusted_keys)),
        );

        Self {
            ghostdag,
            work_distributor,
            network,
            crypto,
            trusted_keys,
        }
    }

    /// Initialize the consensus system
    pub async fn initialize(&self) -> Result<()> {
        self.network.start().await?;
        Ok(())
    }

    /// Get read-only consensus state for WASM transformers
    pub async fn get_consensus_state(&self) -> ConsensusState {
        self.ghostdag.read().await.get_state()
    }

    /// Submit work to the distributed network
    pub async fn submit_work(&self, job: JobRequest) -> Result<String> {
        self.work_distributor.submit_job(job).await
    }

    /// Get work results
    pub async fn get_work_result(&self, job_id: &str) -> Result<Option<WorkResult>> {
        self.work_distributor.get_result(job_id).await
    }

    /// Trust a new node's public key for block and work validation
    pub async fn trust_node(&self, node_id: String, public_key: PublicKey) {
        let mut store = self.trusted_keys.write().await;
        store.add_trusted_key(node_id, public_key);
    }

    /// Submit a transaction to the consensus system (for /srv/consensus/submit)
    pub async fn submit_transaction(&self, tx: serde_json::Value) -> Result<()> {
        // Convert JSON transaction to WorkSubmission
        let work_type = tx["type"].as_str().unwrap_or("transaction").to_string();
        let input_data = serde_json::to_vec(&tx)?;

        let submission = ghostdag::WorkSubmission {
            work_type,
            input_data,
            requirements: ghostdag::WorkRequirements {
                min_nodes: 1,
                required_capabilities: vec![],
                resource_requirements: ghostdag::ResourceRequirements {
                    cpu_cores: None,
                    memory_mb: None,
                    gpu_required: false,
                    storage_mb: None,
                },
            },
            priority: 1,
            max_execution_time_ms: 30000,
        };

        self.ghostdag.write().await.submit_work(submission).await?;
        Ok(())
    }

    /// Get recent blocks from the DAG (for /srv/consensus/blocks)
    pub async fn get_recent_blocks(&self, count: usize) -> Vec<BlockInfo> {
        let state = self.ghostdag.read().await.get_state();
        let main_chain = state.main_chain;

        main_chain
            .iter()
            .rev()
            .take(count)
            .map(|block_id| {
                let parent = if let Some(idx) = main_chain.iter().position(|id| id == block_id) {
                    if idx > 0 {
                        Some(main_chain[idx - 1].clone())
                    } else {
                        None
                    }
                } else {
                    None
                };

                BlockInfo {
                    block_id: block_id.clone(),
                    height: main_chain.iter().position(|id| id == block_id).unwrap_or(0) as u64,
                    parent_id: parent,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                }
            })
            .collect()
    }

    /// Get DAG structure information (for /srv/consensus/dag)
    pub async fn get_dag_structure(&self) -> DagInfo {
        let state = self.ghostdag.read().await.get_state();

        DagInfo {
            total_vertices: state.dag_height,
            total_edges: state.main_chain.len() as u64, // Simplified
            tips: state.tips,
            orphans: 0, // TODO: Calculate actual orphans
            max_depth: state.main_chain.len() as u64,
        }
    }

    /// Get network peers participating in consensus (for /srv/consensus/peers)
    pub async fn get_network_peers(&self) -> Vec<ConsensusPeerInfo> {
        // For now, return placeholder data
        // TODO: Integrate with actual network layer
        vec![ConsensusPeerInfo {
            peer_id: "peer-1".to_string(),
            address: "192.168.1.100:5640".to_string(),
            blocks_ahead: 0,
            latency_ms: 15,
        }]
    }

    /// Get consensus metrics (for /srv/consensus/metrics)
    pub async fn get_metrics(&self) -> ConsensusMetrics {
        let state = self.ghostdag.read().await.get_state();

        ConsensusMetrics {
            tip_height: state.main_chain.len() as u64,
            total_blocks: state.dag_height,
            pending_tx_count: state.pending_work.len(),
            network_hashrate: 0.0, // TODO: Calculate from work proofs
            active_peers: 1,       // TODO: Get from network layer
            consensus_reached: state.tips.len() <= 3, // Consider consensus reached if ≤3 tips
        }
    }
}

/// Block information for /srv/consensus/blocks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockInfo {
    pub block_id: String,
    pub height: u64,
    pub parent_id: Option<String>,
    pub timestamp: u64,
}

/// DAG structure information for /srv/consensus/dag
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagInfo {
    pub total_vertices: u64,
    pub total_edges: u64,
    pub tips: Vec<String>,
    pub orphans: u64,
    pub max_depth: u64,
}

/// Consensus peer information for /srv/consensus/peers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusPeerInfo {
    pub peer_id: String,
    pub address: String,
    pub blocks_ahead: i64,
    pub latency_ms: u64,
}

/// Consensus metrics for /srv/consensus/metrics and /srv/consensus/status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusMetrics {
    pub tip_height: u64,
    pub total_blocks: u64,
    pub pending_tx_count: usize,
    pub network_hashrate: f64,
    pub active_peers: usize,
    pub consensus_reached: bool,
}

/// Read-only consensus primitives exposed to WASM transformers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusReadOnlyState {
    pub current_dag_height: u64,
    pub confirmed_blocks: Vec<BlockId>,
    pub pending_work: Vec<String>,
    pub network_nodes: Vec<String>,
    pub consensus_score: f64,
    pub last_block_timestamp: u64,
}

/// Work request structure for distributed computation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributedWorkRequest {
    pub job_id: String,
    pub work_type: WorkType,
    pub priority: WorkPriority,
    pub requirements: WorkRequirements,
    pub payload: Vec<u8>,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkType {
    Compute,
    Storage,
    Network,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkPriority {
    Low,
    Normal,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkRequirements {
    pub min_nodes: u32,
    pub required_capabilities: Vec<String>,
    pub geographic_constraints: Option<String>,
    pub hardware_requirements: Option<HardwareSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareSpec {
    pub min_cpu_cores: Option<u32>,
    pub min_memory_gb: Option<u32>,
    pub requires_gpu: bool,
    pub gpu_compute_capability: Option<f32>,
}
