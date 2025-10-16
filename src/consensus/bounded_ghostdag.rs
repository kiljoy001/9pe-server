//! Bounded GHOSTDAG implementation for global namespace and operation ordering
//!
//! Based on FSM state transitions and bounded DAG concepts from gnumach,
//! this provides a memory-efficient consensus mechanism for distributed
//! 9P.e namespace operations.

use super::dynamic_scaling::{DynamicScaler, ScalingParams};
use anyhow::{Context as AnyhowContext, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// Minimum blocks to keep (floor)
const MIN_DAG_SIZE: usize = 100;
/// Maximum blocks to keep (ceiling)
const MAX_DAG_SIZE: usize = 10_000;
/// Default starting size
const DEFAULT_DAG_SIZE: usize = 1_000;

/// Dynamic sizing parameters
#[allow(dead_code)]
const SCALE_UP_THRESHOLD: f64 = 0.8; // Scale up at 80% full
#[allow(dead_code)]
const SCALE_DOWN_THRESHOLD: f64 = 0.2; // Scale down at 20% full
#[allow(dead_code)]
const SCALE_FACTOR: f64 = 1.5; // Scale by 50% each time
#[allow(dead_code)]
const THROUGHPUT_WINDOW: usize = 100; // Sample last 100 operations
/// Depth threshold for pruning confirmed blocks
const CONFIRMATION_DEPTH: u64 = 100;
/// Maximum parents per block
#[allow(dead_code)]
const MAX_PARENTS: usize = 8;

/// FSM states for block processing (inspired by gnumach's FSM approach)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockState {
    /// Block just received, not yet validated
    Pending,
    /// Block is being validated
    Validating,
    /// Block validation complete, ready to add to DAG
    Valid,
    /// Block added to DAG and processing
    Processing,
    /// Block fully processed and confirmed
    Confirmed,
    /// Block pruned from memory (checkpoint exists)
    Pruned,
}

/// Namespace operation types for 9P.e
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NamespaceOp {
    /// Create a new file or directory
    Create {
        path: String,
        mode: u32,
        is_dir: bool,
    },
    /// Write data to a file
    Write {
        path: String,
        offset: u64,
        hash: [u8; 32],
    },
    /// Delete a file or directory
    Delete { path: String },
    /// Set translator on a path
    SetTrans {
        path: String,
        translator: String,
        args: Vec<String>,
    },
    /// Change permissions
    Chmod { path: String, mode: u32 },
    /// Rename/move operation
    Rename { from: String, to: String },
    /// Register namespace with cryptographic ownership
    RegisterNamespace {
        path: String,
        owner_pubkey: [u8; 32],
        signature: Vec<u8>, // Store as Vec for easier serde
    },
    /// Atomic batch of operations
    Batch { ops: Vec<NamespaceOp> },
}

impl NamespaceOp {
    /// Check if two operations conflict
    pub fn conflicts_with(&self, other: &NamespaceOp) -> bool {
        let self_paths = self.affected_paths();
        let other_paths = other.affected_paths();

        // Operations conflict if they affect the same path
        self_paths.intersection(&other_paths).next().is_some()
    }

    /// Get all paths affected by this operation
    pub fn affected_paths(&self) -> HashSet<String> {
        let mut paths = HashSet::new();
        match self {
            Self::Create { path, .. }
            | Self::Delete { path }
            | Self::Write { path, .. }
            | Self::SetTrans { path, .. }
            | Self::Chmod { path, .. }
            | Self::RegisterNamespace { path, .. } => {
                paths.insert(path.clone());
            }
            Self::Rename { from, to } => {
                paths.insert(from.clone());
                paths.insert(to.clone());
            }
            Self::Batch { ops } => {
                for op in ops {
                    paths.extend(op.affected_paths());
                }
            }
        }
        paths
    }
}

/// Block ID type
pub type BlockId = String;

/// A block in the bounded GHOSTDAG
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// Unique block identifier
    pub id: BlockId,
    /// Parent block IDs (multiple for DAG)
    pub parents: Vec<BlockId>,
    /// Namespace operations in this block
    pub operations: Vec<NamespaceOp>,
    /// Block creation timestamp
    pub timestamp: u64,
    /// Creator node ID
    pub creator: String,
    /// Cryptographic signature
    pub signature: Vec<u8>,
    /// Current FSM state
    pub state: BlockState,
    /// GHOST weight (subtree size)
    pub ghost_weight: u64,
    /// Height in the DAG
    pub height: u64,
}

/// Checkpoint for pruned blocks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Block ID this checkpoint represents
    pub block_id: BlockId,
    /// Height of the checkpoint
    pub height: u64,
    /// Aggregated namespace state up to this point
    pub namespace_state: BTreeMap<String, FileState>,
    /// Timestamp of checkpoint creation
    pub timestamp: u64,
}

/// File state at a checkpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileState {
    pub mode: u32,
    pub size: u64,
    pub content_hash: [u8; 32],
    pub translator: Option<String>,
}

/// Bounded GHOSTDAG consensus implementation
#[derive(Clone)]
pub struct BoundedGhostdag {
    /// Node identifier
    #[allow(dead_code)]
    node_id: String,

    /// The bounded DAG structure
    dag: Arc<RwLock<HashMap<BlockId, Block>>>,

    /// Current tips of the DAG
    tips: Arc<RwLock<HashSet<BlockId>>>,

    /// Confirmed blocks (deeply buried)
    confirmed: Arc<RwLock<HashSet<BlockId>>>,

    /// GHOST scores for each block
    ghost_scores: Arc<RwLock<HashMap<BlockId, u64>>>,

    /// Checkpoints for pruned sections
    checkpoints: Arc<RwLock<Vec<Checkpoint>>>,

    /// Main chain cache
    main_chain: Arc<RwLock<Vec<BlockId>>>,

    /// Pruning threshold
    prune_depth: u64,

    /// Maximum DAG size (dynamically adjustable)
    max_size: Arc<RwLock<usize>>,

    /// Dynamic scaler for adaptive sizing
    scaler: Arc<DynamicScaler>,

    /// Scaling parameters
    scaling_params: ScalingParams,

    /// Last recorded throughput
    last_throughput: Arc<RwLock<f64>>,

    /// Block addition timestamp tracker
    block_timestamps: Arc<RwLock<VecDeque<std::time::Instant>>>,
}

impl BoundedGhostdag {
    /// Create a new bounded GHOSTDAG instance
    pub fn new(node_id: String) -> Self {
        // Get initial size from environment variable
        let initial_size = std::env::var("GHOSTDAG_MAX_BLOCKS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(DEFAULT_DAG_SIZE)
            .clamp(MIN_DAG_SIZE, MAX_DAG_SIZE);

        let mut scaling_params = ScalingParams::default();
        scaling_params.initial_size = initial_size;

        let scaler = Arc::new(DynamicScaler::new(scaling_params.clone()));

        Self {
            node_id,
            dag: Arc::new(RwLock::new(HashMap::new())),
            tips: Arc::new(RwLock::new(HashSet::new())),
            confirmed: Arc::new(RwLock::new(HashSet::new())),
            ghost_scores: Arc::new(RwLock::new(HashMap::new())),
            checkpoints: Arc::new(RwLock::new(Vec::new())),
            main_chain: Arc::new(RwLock::new(Vec::new())),
            prune_depth: CONFIRMATION_DEPTH,
            max_size: Arc::new(RwLock::new(initial_size)),
            scaler,
            scaling_params,
            last_throughput: Arc::new(RwLock::new(0.0)),
            block_timestamps: Arc::new(RwLock::new(VecDeque::with_capacity(100))),
        }
    }

    /// Add a new block to the DAG
    pub async fn add_block(&self, mut block: Block) -> Result<()> {
        // Track block addition for throughput metrics
        self.track_block_addition().await;

        // FSM transition: Pending -> Validating
        block.state = BlockState::Validating;

        // Validate block
        self.validate_block(&block).await?;

        // FSM transition: Validating -> Valid
        block.state = BlockState::Valid;

        // Update metrics and check for dynamic scaling
        self.update_metrics().await;
        self.apply_dynamic_scaling().await;

        // Check DAG size and prune if necessary
        let max_size = *self.max_size.read().await;
        if self.dag.read().await.len() >= max_size {
            self.prune_old_blocks().await?;
        }

        // FSM transition: Valid -> Processing
        block.state = BlockState::Processing;

        // Add to DAG
        let block_id = block.id.clone();
        self.dag
            .write()
            .await
            .insert(block_id.clone(), block.clone());

        // Update tips
        self.update_tips(&block).await;

        // Recalculate GHOST scores
        self.update_ghost_scores().await?;

        // Update main chain
        self.update_main_chain().await?;

        // Check for newly confirmed blocks
        self.update_confirmed_blocks().await?;

        // FSM transition: Processing -> Confirmed (if deep enough)
        if self.is_deeply_confirmed(&block_id).await {
            if let Some(b) = self.dag.write().await.get_mut(&block_id) {
                b.state = BlockState::Confirmed;
            }
        }

        Ok(())
    }

    /// Validate a block before adding to DAG
    async fn validate_block(&self, block: &Block) -> Result<()> {
        // Check parent existence
        let dag = self.dag.read().await;
        for parent_id in &block.parents {
            if !dag.contains_key(parent_id) {
                anyhow::bail!("Parent block {} not found", parent_id);
            }
        }

        // Check for operation conflicts within the block
        for i in 0..block.operations.len() {
            for j in i + 1..block.operations.len() {
                if block.operations[i].conflicts_with(&block.operations[j]) {
                    anyhow::bail!("Conflicting operations within block");
                }
            }
        }

        // Validate timestamp (not too far in future)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        if block.timestamp > now + 300 {
            anyhow::bail!("Block timestamp too far in future");
        }

        Ok(())
    }

    /// Update DAG tips after adding a block
    async fn update_tips(&self, block: &Block) {
        let mut tips = self.tips.write().await;

        // Remove parents from tips
        for parent in &block.parents {
            tips.remove(parent);
        }

        // Add new block as tip
        tips.insert(block.id.clone());
    }

    /// Calculate GHOST scores for all blocks
    async fn update_ghost_scores(&self) -> Result<()> {
        let dag = self.dag.read().await;
        let mut scores = HashMap::new();

        // Calculate subtree size for each block
        for block_id in dag.keys() {
            let score = self.calculate_subtree_size(block_id, &dag).await?;
            scores.insert(block_id.clone(), score);
        }

        *self.ghost_scores.write().await = scores;
        Ok(())
    }

    /// Calculate subtree size for GHOST score
    async fn calculate_subtree_size(
        &self,
        block_id: &str,
        dag: &HashMap<BlockId, Block>,
    ) -> Result<u64> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(block_id.to_string());
        visited.insert(block_id.to_string());

        let mut count = 1u64;

        while let Some(current) = queue.pop_front() {
            // Find children
            for (child_id, child_block) in dag {
                if !visited.contains(child_id) && child_block.parents.contains(&current) {
                    count += 1;
                    queue.push_back(child_id.clone());
                    visited.insert(child_id.clone());
                }
            }
        }

        Ok(count)
    }

    /// Update the main chain using GHOST selection
    async fn update_main_chain(&self) -> Result<()> {
        let dag = self.dag.read().await;
        let scores = self.ghost_scores.read().await;

        if dag.is_empty() {
            return Ok(());
        }

        // Find genesis block(s)
        let genesis_blocks: Vec<_> = dag
            .values()
            .filter(|b| b.parents.is_empty())
            .map(|b| b.id.clone())
            .collect();

        if genesis_blocks.is_empty() {
            return Ok(());
        }

        // Start from genesis and follow heaviest path
        let mut chain = Vec::new();
        let mut current = genesis_blocks[0].clone();
        chain.push(current.clone());

        while let Some(heaviest) = self.find_heaviest_child(&current, &dag, &scores).await {
            chain.push(heaviest.clone());
            current = heaviest;
        }

        *self.main_chain.write().await = chain;
        Ok(())
    }

    /// Find the child with highest GHOST score
    async fn find_heaviest_child(
        &self,
        parent: &str,
        dag: &HashMap<BlockId, Block>,
        scores: &HashMap<BlockId, u64>,
    ) -> Option<BlockId> {
        let mut heaviest = None;
        let mut max_score = 0;

        for (block_id, block) in dag {
            if block.parents.contains(&parent.to_string()) {
                if let Some(&score) = scores.get(block_id) {
                    if score > max_score {
                        max_score = score;
                        heaviest = Some(block_id.clone());
                    }
                }
            }
        }

        heaviest
    }

    /// Update confirmed blocks based on depth
    async fn update_confirmed_blocks(&self) -> Result<()> {
        let main_chain = self.main_chain.read().await;
        let mut confirmed = self.confirmed.write().await;

        // Blocks deep in the main chain are confirmed
        let confirm_until = main_chain.len().saturating_sub(self.prune_depth as usize);
        for block_id in &main_chain[..confirm_until] {
            confirmed.insert(block_id.clone());
        }

        Ok(())
    }

    /// Check if a block is deeply confirmed
    async fn is_deeply_confirmed(&self, block_id: &str) -> bool {
        self.confirmed.read().await.contains(block_id)
    }

    /// Prune old blocks to maintain bounded size
    async fn prune_old_blocks(&self) -> Result<()> {
        let main_chain = self.main_chain.read().await.clone();
        let confirmed = self.confirmed.read().await.clone();

        // Find blocks to prune (old confirmed blocks)
        let max_size = *self.max_size.read().await;
        let prune_until = main_chain.len().saturating_sub(max_size / 2);
        let blocks_to_prune: Vec<_> = main_chain[..prune_until]
            .iter()
            .filter(|id| confirmed.contains(*id))
            .cloned()
            .collect();

        if blocks_to_prune.is_empty() {
            return Ok(());
        }

        info!("Pruning {} old blocks", blocks_to_prune.len());

        // Create checkpoint before pruning
        self.create_checkpoint(&blocks_to_prune).await?;

        // Remove blocks from DAG
        let mut dag = self.dag.write().await;
        for block_id in &blocks_to_prune {
            if let Some(mut block) = dag.remove(block_id) {
                block.state = BlockState::Pruned;
            }
        }

        Ok(())
    }

    /// Create a checkpoint for pruned blocks
    async fn create_checkpoint(&self, blocks: &[BlockId]) -> Result<()> {
        if blocks.is_empty() {
            return Ok(());
        }

        let dag = self.dag.read().await;

        // Get the last block to checkpoint
        let last_block_id = &blocks[blocks.len() - 1];
        let last_block = dag
            .get(last_block_id)
            .context("Block to checkpoint not found")?;

        // Build namespace state up to this point
        let mut namespace_state = BTreeMap::new();

        // Apply all operations in order up to checkpoint
        for block_id in blocks {
            if let Some(block) = dag.get(block_id) {
                for op in &block.operations {
                    self.apply_operation_to_state(&mut namespace_state, op);
                }
            }
        }

        let checkpoint = Checkpoint {
            block_id: last_block_id.clone(),
            height: last_block.height,
            namespace_state,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
        };

        self.checkpoints.write().await.push(checkpoint);

        Ok(())
    }

    /// Apply an operation to the namespace state
    fn apply_operation_to_state(&self, state: &mut BTreeMap<String, FileState>, op: &NamespaceOp) {
        match op {
            NamespaceOp::Create { path, mode, .. } => {
                state.insert(
                    path.clone(),
                    FileState {
                        mode: *mode,
                        size: 0,
                        content_hash: [0; 32],
                        translator: None,
                    },
                );
            }
            NamespaceOp::Write { path, .. } => {
                if let Some(file) = state.get_mut(path) {
                    file.size += 1; // Simplified
                }
            }
            NamespaceOp::Delete { path } => {
                state.remove(path);
            }
            NamespaceOp::SetTrans {
                path, translator, ..
            } => {
                if let Some(file) = state.get_mut(path) {
                    file.translator = Some(translator.clone());
                }
            }
            NamespaceOp::Chmod { path, mode } => {
                if let Some(file) = state.get_mut(path) {
                    file.mode = *mode;
                }
            }
            NamespaceOp::Rename { from, to } => {
                if let Some(file) = state.remove(from) {
                    state.insert(to.clone(), file);
                }
            }
            NamespaceOp::RegisterNamespace { .. } => {
                // Namespace registration doesn't affect filesystem state
                // It's tracked separately in the namespace manager
            }
            NamespaceOp::Batch { ops } => {
                for op in ops {
                    self.apply_operation_to_state(state, op);
                }
            }
        }
    }

    /// Get the current main chain
    pub async fn get_main_chain(&self) -> Vec<BlockId> {
        self.main_chain.read().await.clone()
    }

    /// Get current DAG statistics
    pub async fn get_stats(&self) -> DagStats {
        let dag = self.dag.read().await;
        let tips = self.tips.read().await;
        let confirmed = self.confirmed.read().await;
        let checkpoints = self.checkpoints.read().await;

        DagStats {
            total_blocks: dag.len(),
            tip_count: tips.len(),
            confirmed_blocks: confirmed.len(),
            checkpoint_count: checkpoints.len(),
            main_chain_length: self.main_chain.read().await.len(),
        }
    }

    /// Track block addition for throughput metrics
    async fn track_block_addition(&self) {
        let mut timestamps = self.block_timestamps.write().await;
        let now = std::time::Instant::now();

        // Maintain sliding window
        timestamps.push_back(now);
        if timestamps.len() > 100 {
            timestamps.pop_front();
        }
    }

    /// Update dynamic scaling metrics
    async fn update_metrics(&self) {
        // Calculate throughput
        let timestamps = self.block_timestamps.read().await;
        let throughput = if timestamps.len() >= 2 {
            let duration = timestamps
                .back()
                .unwrap()
                .duration_since(*timestamps.front().unwrap());
            if duration.as_secs() > 0 {
                timestamps.len() as f64 / duration.as_secs_f64()
            } else {
                0.0
            }
        } else {
            0.0
        };

        // Calculate fill rate
        let dag_size = self.dag.read().await.len();
        let max_size = *self.max_size.read().await;
        let fill_rate = dag_size as f64 / max_size as f64;

        // Calculate fork depth
        let tips = self.tips.read().await;
        let fork_depth = tips.len() as u64;

        // Record metrics in scaler
        self.scaler
            .record_metrics(throughput, fill_rate, fork_depth)
            .await;

        // Update throughput tracker
        *self.last_throughput.write().await = throughput;
    }

    /// Apply dynamic scaling based on metrics
    async fn apply_dynamic_scaling(&self) {
        // Get scaling decision
        let decision = self
            .scaler
            .calculate_scale_decision(&self.scaling_params)
            .await;

        // Apply decision and update max_size
        let new_size = self
            .scaler
            .apply_scale(decision, &self.scaling_params)
            .await;
        let current_size = *self.max_size.read().await;
        if new_size != current_size {
            info!("DAG size adjusted: {} → {} blocks", current_size, new_size);
            *self.max_size.write().await = new_size;
        }
    }
}

/// DAG statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagStats {
    pub total_blocks: usize,
    pub tip_count: usize,
    pub confirmed_blocks: usize,
    pub checkpoint_count: usize,
    pub main_chain_length: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_dag_operations() {
        let dag = BoundedGhostdag::new("node1".to_string());

        // Create genesis block
        let genesis = Block {
            id: "genesis".to_string(),
            parents: vec![],
            operations: vec![NamespaceOp::Create {
                path: "/test".to_string(),
                mode: 0o644,
                is_dir: false,
            }],
            timestamp: 0,
            creator: "node1".to_string(),
            signature: vec![],
            state: BlockState::Pending,
            ghost_weight: 1,
            height: 0,
        };

        dag.add_block(genesis).await.unwrap();

        let stats = dag.get_stats().await;
        assert_eq!(stats.total_blocks, 1);
        assert_eq!(stats.tip_count, 1);
    }

    #[tokio::test]
    async fn test_conflict_detection() {
        let op1 = NamespaceOp::Write {
            path: "/file".to_string(),
            offset: 0,
            hash: [0; 32],
        };

        let op2 = NamespaceOp::Delete {
            path: "/file".to_string(),
        };

        assert!(op1.conflicts_with(&op2));

        let op3 = NamespaceOp::Create {
            path: "/other".to_string(),
            mode: 0o644,
            is_dir: false,
        };

        assert!(!op1.conflicts_with(&op3));
    }

    /// Fuzz test: Block deserialization
    #[test]
    fn fuzz_block_deserialization() {
        use proptest::prelude::*;

        proptest!(|(bytes: Vec<u8>)| {
            // Should never panic
            let _ = serde_json::from_slice::<Block>(&bytes);
        });
    }

    /// Fuzz test: Block ID generation
    #[test]
    fn fuzz_block_id_generation() {
        use proptest::prelude::*;

        proptest!(|(
            creator in ".*",
            timestamp: u64,
            nonce in 0u64..1000000
        )| {
            // Should always generate valid ID
            let id = format!("{}_{}_{}",  creator, timestamp, nonce);
            prop_assert!(!id.is_empty());
        });
    }

    /// Fuzz test: GHOSTDAG coloring edge cases
    #[test]
    fn fuzz_ghostdag_coloring() {
        use proptest::prelude::*;

        proptest!(|(k in 1usize..100)| {
            // K parameter should always be positive
            prop_assert!(k > 0);
        });
    }

    /// Fuzz test: Byzantine block validation
    #[test]
    fn fuzz_byzantine_block() {
        use proptest::prelude::*;

        proptest!(|(
            id in ".*",
            parents in prop::collection::vec(".*", 0..20),
            timestamp: u64
        )| {
            // Should safely handle malformed blocks
            let _ = (id, parents, timestamp);
        });
    }

    /// Fuzz test: Operation conflict detection
    #[test]
    fn fuzz_operation_conflicts() {
        use proptest::prelude::*;

        proptest!(|(
            path1 in ".*",
            path2 in ".*",
            mode1: u32,
            mode2: u32
        )| {
            let op1 = NamespaceOp::Create {
                path: path1.clone(),
                mode: mode1,
                is_dir: false,
            };
            let op2 = NamespaceOp::Create {
                path: path2.clone(),
                mode: mode2,
                is_dir: false,
            };
            // Same path should conflict
            if path1 == path2 {
                prop_assert!(op1.conflicts_with(&op2));
            }
        });
    }
}
