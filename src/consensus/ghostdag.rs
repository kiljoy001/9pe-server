//! GHOSTDAG consensus implementation
//!
//! This implements the GHOSTDAG consensus algorithm for ordering work
//! in a distributed environment with Byzantine fault tolerance.

use anyhow::{Result, Context};
use serde::{Serialize, Deserialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use super::crypto::{CryptoProvider, Signature, PublicKey, WorkProof};

/// Unique identifier for blocks in the DAG
pub type BlockId = String;

/// GHOSTDAG consensus state machine
pub struct GhostdagConsensus {
    node_id: String,
    dag: HashMap<BlockId, WorkBlock>,
    tips: HashSet<BlockId>,
    confirmed_blocks: HashSet<BlockId>,
    pending_work: HashMap<String, PendingWork>,
    ghost_score: HashMap<BlockId, u64>,
    confirmation_depth: u64,
}

impl GhostdagConsensus {
    pub fn new(node_id: String) -> Self {
        Self {
            node_id,
            dag: HashMap::new(),
            tips: HashSet::new(),
            confirmed_blocks: HashSet::new(),
            pending_work: HashMap::new(),
            ghost_score: HashMap::new(),
            confirmation_depth: 6, // Blocks deep for confirmation
        }
    }

    /// Add a new work block to the DAG
    pub async fn add_block(&mut self, block: WorkBlock) -> Result<()> {
        let block_id = block.id.clone();

        // Validate block structure
        self.validate_block(&block).await?;

        // Update DAG structure
        self.dag.insert(block_id.clone(), block.clone());

        // Update tips
        for parent in &block.parents {
            self.tips.remove(parent);
        }
        self.tips.insert(block_id.clone());

        // Recalculate GHOST scores
        self.update_ghost_scores().await?;

        // Check for confirmed blocks
        self.update_confirmed_blocks().await?;

        Ok(())
    }

    /// Get the current main chain using GHOST algorithm
    pub async fn get_main_chain(&self) -> Vec<BlockId> {
        let mut chain = Vec::new();

        if let Some(genesis_id) = self.find_genesis_block() {
            let mut current = genesis_id;
            chain.push(current.clone());

            while let Some(next) = self.find_heaviest_child(&current) {
                chain.push(next.clone());
                current = next;
            }
        }

        chain
    }

    /// Get read-only consensus state for WASM transformers
    pub fn get_state(&self) -> ConsensusState {
        let main_chain = futures::executor::block_on(self.get_main_chain());

        ConsensusState {
            dag_height: self.dag.len() as u64,
            confirmed_blocks: self.confirmed_blocks.iter().cloned().collect(),
            pending_work: self.pending_work.keys().cloned().collect(),
            tips: self.tips.iter().cloned().collect(),
            main_chain,
            ghost_scores: self.ghost_score.clone(),
            node_id: self.node_id.clone(),
        }
    }

    /// Submit work to be processed by the network
    pub async fn submit_work(&mut self, work: WorkSubmission) -> Result<String> {
        let work_id = format!("work_{}", uuid::Uuid::new_v4());

        let pending = PendingWork {
            id: work_id.clone(),
            submission: work,
            submitted_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
            assigned_nodes: HashSet::new(),
            status: WorkStatus::Pending,
        };

        self.pending_work.insert(work_id.clone(), pending);
        Ok(work_id)
    }

    /// Create a new work block
    pub async fn create_work_block(
        &self,
        work_results: Vec<WorkResult>,
        crypto: &dyn CryptoProvider,
    ) -> Result<WorkBlock> {
        let block_id = format!("block_{}", uuid::Uuid::new_v4());
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        // Select parents (current tips)
        let parents: Vec<BlockId> = self.tips.iter().cloned().collect();

        // Create block data
        let mut block_data = Vec::new();
        block_data.extend_from_slice(block_id.as_bytes());
        block_data.extend_from_slice(&timestamp.to_le_bytes());
        for parent in &parents {
            block_data.extend_from_slice(parent.as_bytes());
        }

        // Sign the block
        let signature = crypto.sign(&block_data).await?;

        let block = WorkBlock {
            id: block_id,
            parents,
            work_results,
            timestamp,
            creator: self.node_id.clone(),
            signature,
            ghost_weight: 1, // Initial weight
        };

        Ok(block)
    }

    // Private helper methods

    async fn validate_block(&self, block: &WorkBlock) -> Result<()> {
        // Validate parents exist
        for parent in &block.parents {
            if !self.dag.contains_key(parent) {
                anyhow::bail!("Parent block {} not found", parent);
            }
        }

        // Validate timestamp
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        if block.timestamp > now + 300 {
            anyhow::bail!("Block timestamp too far in future");
        }

        // TODO: Validate signature and work results

        Ok(())
    }

    async fn update_ghost_scores(&mut self) -> Result<()> {
        // Reset scores
        self.ghost_score.clear();

        // Calculate GHOST scores for each block
        for block_id in self.dag.keys() {
            let score = self.calculate_ghost_score(block_id).await?;
            self.ghost_score.insert(block_id.clone(), score);
        }

        Ok(())
    }

    async fn calculate_ghost_score(&self, block_id: &str) -> Result<u64> {
        let mut score = 1; // Base score
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        queue.push_back(block_id.to_string());
        visited.insert(block_id.to_string());

        // Count all blocks in the subtree
        while let Some(current) = queue.pop_front() {
            // Find children of current block
            for (child_id, child_block) in &self.dag {
                if !visited.contains(child_id) && child_block.parents.contains(&current) {
                    score += child_block.ghost_weight;
                    queue.push_back(child_id.clone());
                    visited.insert(child_id.clone());
                }
            }
        }

        Ok(score)
    }

    async fn update_confirmed_blocks(&mut self) -> Result<()> {
        let main_chain = self.get_main_chain().await;

        // Confirm blocks that are deep enough in the main chain
        if main_chain.len() > self.confirmation_depth as usize {
            let confirm_until = main_chain.len() - self.confirmation_depth as usize;
            for block_id in &main_chain[..confirm_until] {
                self.confirmed_blocks.insert(block_id.clone());
            }
        }

        Ok(())
    }

    fn find_genesis_block(&self) -> Option<BlockId> {
        // Find block with no parents
        for (block_id, block) in &self.dag {
            if block.parents.is_empty() {
                return Some(block_id.clone());
            }
        }
        None
    }

    fn find_heaviest_child(&self, parent_id: &str) -> Option<BlockId> {
        let mut heaviest_child = None;
        let mut max_score = 0;

        for (block_id, block) in &self.dag {
            if block.parents.contains(&parent_id.to_string()) {
                if let Some(score) = self.ghost_score.get(block_id) {
                    if *score > max_score {
                        max_score = *score;
                        heaviest_child = Some(block_id.clone());
                    }
                }
            }
        }

        heaviest_child
    }
}

/// A block in the GHOSTDAG
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkBlock {
    pub id: BlockId,
    pub parents: Vec<BlockId>,
    pub work_results: Vec<WorkResult>,
    pub timestamp: u64,
    pub creator: String,
    pub signature: Signature,
    pub ghost_weight: u64,
}

/// Work result from a computation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkResult {
    pub work_id: String,
    pub result_data: Vec<u8>,
    pub computation_proof: WorkProof,
    pub executor_node: String,
    pub execution_time_ms: u64,
}

/// Work submission from clients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkSubmission {
    pub work_type: String,
    pub input_data: Vec<u8>,
    pub requirements: WorkRequirements,
    pub priority: u32,
    pub max_execution_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkRequirements {
    pub min_nodes: u32,
    pub required_capabilities: Vec<String>,
    pub resource_requirements: ResourceRequirements,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRequirements {
    pub cpu_cores: Option<u32>,
    pub memory_mb: Option<u32>,
    pub gpu_required: bool,
    pub storage_mb: Option<u32>,
}

/// Pending work in the system
#[derive(Debug, Clone)]
struct PendingWork {
    id: String,
    submission: WorkSubmission,
    submitted_at: u64,
    assigned_nodes: HashSet<String>,
    status: WorkStatus,
}

#[derive(Debug, Clone)]
enum WorkStatus {
    Pending,
    Assigned,
    InProgress,
    Completed,
    Failed,
}

/// Read-only consensus state exposed to WASM transformers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusState {
    pub dag_height: u64,
    pub confirmed_blocks: Vec<BlockId>,
    pub pending_work: Vec<String>,
    pub tips: Vec<BlockId>,
    pub main_chain: Vec<BlockId>,
    pub ghost_scores: HashMap<BlockId, u64>,
    pub node_id: String,
}

impl ConsensusState {
    /// Get consensus confidence score (0.0 to 1.0)
    pub fn confidence_score(&self) -> f64 {
        if self.dag_height == 0 {
            return 0.0;
        }

        let confirmed_ratio = self.confirmed_blocks.len() as f64 / self.dag_height as f64;
        let tip_convergence = 1.0 / (1.0 + self.tips.len() as f64);

        (confirmed_ratio + tip_convergence) / 2.0
    }

    /// Check if a block is confirmed
    pub fn is_block_confirmed(&self, block_id: &str) -> bool {
        self.confirmed_blocks.contains(&block_id.to_string())
    }

    /// Get the current main chain tip
    pub fn main_chain_tip(&self) -> Option<&BlockId> {
        self.main_chain.last()
    }

    /// Get GHOST score for a block
    pub fn get_ghost_score(&self, block_id: &str) -> Option<u64> {
        self.ghost_scores.get(block_id).copied()
    }
}

/// GHOSTDAG network statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStats {
    pub total_blocks: u64,
    pub confirmed_blocks: u64,
    pub pending_work_items: u64,
    pub active_nodes: u64,
    pub consensus_confidence: f64,
    pub average_block_time_ms: u64,
    pub network_hashrate: f64,
    pub fork_rate: f64,
}

impl NetworkStats {
    pub fn from_consensus_state(state: &ConsensusState) -> Self {
        Self {
            total_blocks: state.dag_height,
            confirmed_blocks: state.confirmed_blocks.len() as u64,
            pending_work_items: state.pending_work.len() as u64,
            active_nodes: 1, // TODO: Get from network layer
            consensus_confidence: state.confidence_score(),
            average_block_time_ms: 10000, // TODO: Calculate from actual data
            network_hashrate: 0.0, // TODO: Calculate from work proofs
            fork_rate: state.tips.len() as f64 / state.dag_height as f64,
        }
    }
}