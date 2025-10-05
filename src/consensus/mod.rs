//! GHOSTDAG Consensus Implementation for 9P.e
//!
//! This module implements a GHOSTDAG-based consensus system for distributed
//! work coordination. It provides:
//! - DAG-based consensus for work ordering
//! - Cryptographic security for work distribution
//! - Read-only primitives for WASM transformers
//! - Byzantine fault tolerance

pub mod ghostdag;
pub mod crypto;
pub mod work_distribution;
pub mod network;
pub mod bounded_ghostdag;
pub mod dynamic_scaling;
// pub mod ollama_worker;  // Disabled - ollama not available
pub mod llama_worker;

pub use ghostdag::{GhostdagConsensus, ConsensusState, WorkBlock};
pub use crypto::{CryptoProvider, Signature, PublicKey};
pub use work_distribution::{WorkDistributor, JobRequest};
pub use ghostdag::WorkResult;
pub use network::{NetworkConsensus, PeerManager};
pub use bounded_ghostdag::{BoundedGhostdag, NamespaceOp, BlockState, DagStats, Block, BlockId};
pub use dynamic_scaling::{DynamicScaler, ScalingParams, ScaleDecision};
// pub use ollama_worker::{OllamaWorker, LLMRequest, LLMResponse, create_llm_job};  // Disabled
pub use llama_worker::{LlamaCppWorker, LLMRequest, LLMResponse, create_llm_job};

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};

/// Main consensus coordinator for the 9P.e server
pub struct ConsensusCoordinator {
    ghostdag: Arc<RwLock<GhostdagConsensus>>,
    work_distributor: Arc<WorkDistributor>,
    network: Arc<NetworkConsensus>,
    crypto: Arc<dyn CryptoProvider>,
}

impl ConsensusCoordinator {
    pub fn new(node_id: String, crypto: Arc<dyn CryptoProvider>) -> Self {
        let ghostdag = Arc::new(RwLock::new(GhostdagConsensus::new(node_id.clone())));
        let work_distributor = Arc::new(WorkDistributor::new(node_id.clone()));
        let network = Arc::new(NetworkConsensus::new(node_id));

        Self {
            ghostdag,
            work_distributor,
            network,
            crypto,
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