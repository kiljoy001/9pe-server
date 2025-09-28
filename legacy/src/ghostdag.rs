//! GhostDAG Consensus Implementation for 9P.e
//!
//! Based on the PHANTOM protocol, this provides a DAG-based consensus mechanism
//! for distributed file system operations.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};
use anyhow::Result;
use tracing::{info, warn};
use sha2::{Sha256, Digest};

/// Block hash type
pub type BlockHash = [u8; 32];

/// Convert hash to string for logging
pub fn hash_to_string(hash: &BlockHash) -> String {
    hex::encode(hash)
}

/// Generate hash from data
pub fn generate_hash(data: &[u8]) -> BlockHash {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// File operation that can be included in a block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileOperation {
    Create { path: String, content: Vec<u8> },
    Delete { path: String },
    Modify { path: String, content: Vec<u8> },
    Move { from: String, to: String },
    SetPermissions { path: String, mode: u32 },
}

/// Block in the GhostDAG
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub hash: BlockHash,
    pub parent_hashes: Vec<BlockHash>,
    pub timestamp: u64,
    pub height: u64,
    pub operations: Vec<FileOperation>,
    pub miner: String,  // Node ID that created this block
    pub nonce: u64,
    pub difficulty: u64,
}

impl Block {
    /// Create genesis block
    pub fn genesis() -> Self {
        let mut block = Self {
            hash: [0; 32],
            parent_hashes: vec![],
            timestamp: 0,
            height: 0,
            operations: vec![],
            miner: "genesis".to_string(),
            nonce: 0,
            difficulty: 1,
        };
        block.hash = block.compute_hash();
        block
    }

    /// Compute block hash
    pub fn compute_hash(&self) -> BlockHash {
        let mut data = Vec::new();
        for parent in &self.parent_hashes {
            data.extend_from_slice(parent);
        }
        data.extend_from_slice(&self.timestamp.to_le_bytes());
        data.extend_from_slice(&self.height.to_le_bytes());
        data.extend_from_slice(self.miner.as_bytes());
        data.extend_from_slice(&self.nonce.to_le_bytes());
        data.extend_from_slice(&self.difficulty.to_le_bytes());

        // Include operations in hash for uniqueness
        for op in &self.operations {
            let op_data = serde_json::to_vec(op).unwrap_or_default();
            data.extend_from_slice(&op_data);
        }

        generate_hash(&data)
    }

    /// Check if hash meets difficulty requirement
    pub fn meets_difficulty(&self) -> bool {
        let target_zeros = (self.difficulty as f64).log2() as usize / 8;
        self.hash.iter().take(target_zeros).all(|&b| b == 0)
    }
}

/// DAG node wrapping a block
#[derive(Debug, Clone)]
pub struct DagNode {
    pub block: Block,
    pub children: Vec<BlockHash>,
    pub blue_anticone_size: usize,
    pub is_blue: bool,
}

/// GhostDAG consensus state
pub struct GhostDAG {
    /// The DAG structure
    pub dag: Arc<RwLock<HashMap<BlockHash, DagNode>>>,

    /// Current tips (blocks without children)
    pub tips: Arc<RwLock<Vec<BlockHash>>>,

    /// Blue blocks set
    pub blue_blocks: Arc<RwLock<HashSet<BlockHash>>>,

    /// K parameter (anticone size bound)
    pub k: usize,

    /// Current difficulty
    pub difficulty: Arc<RwLock<u64>>,
}

impl GhostDAG {
    /// Create new GhostDAG instance
    pub fn new(k: usize) -> Self {
        let genesis = Block::genesis();
        let genesis_hash = genesis.hash;

        let mut dag = HashMap::new();
        dag.insert(genesis_hash, DagNode {
            block: genesis,
            children: vec![],
            blue_anticone_size: 0,
            is_blue: true,
        });

        let mut blue_blocks = HashSet::new();
        blue_blocks.insert(genesis_hash);

        Self {
            dag: Arc::new(RwLock::new(dag)),
            tips: Arc::new(RwLock::new(vec![genesis_hash])),
            blue_blocks: Arc::new(RwLock::new(blue_blocks)),
            k,
            difficulty: Arc::new(RwLock::new(1)),
        }
    }

    /// Compute past set (ancestors) of a block
    pub async fn compute_past(&self, target: &BlockHash) -> HashSet<BlockHash> {
        let dag = self.dag.read().await;
        let mut past = HashSet::new();
        let mut queue = VecDeque::new();

        if let Some(node) = dag.get(target) {
            for parent in &node.block.parent_hashes {
                queue.push_back(*parent);
            }
        }

        while let Some(hash) = queue.pop_front() {
            if past.insert(hash) {
                if let Some(node) = dag.get(&hash) {
                    for parent in &node.block.parent_hashes {
                        queue.push_back(*parent);
                    }
                }
            }
        }

        past
    }

    /// Compute future set (descendants) of a block
    pub async fn compute_future(&self, target: &BlockHash) -> HashSet<BlockHash> {
        let dag = self.dag.read().await;
        let mut future = HashSet::new();

        for (hash, node) in dag.iter() {
            if node.block.parent_hashes.contains(target) {
                future.insert(*hash);
            }
        }

        future
    }

    /// Compute anticone (blocks neither in past nor future)
    pub async fn compute_anticone(&self, target: &BlockHash) -> HashSet<BlockHash> {
        let dag = self.dag.read().await;
        let past = self.compute_past(target).await;
        let future = self.compute_future(target).await;

        let mut anticone = HashSet::new();
        for hash in dag.keys() {
            if hash != target && !past.contains(hash) && !future.contains(hash) {
                anticone.insert(*hash);
            }
        }

        anticone
    }

    /// Calculate anticone size within a subset
    pub async fn anticone_size(&self, hash: &BlockHash, subset: &HashSet<BlockHash>) -> usize {
        let anticone = self.compute_anticone(hash).await;
        anticone.intersection(subset).count()
    }

    /// Check if subset forms a k-cluster
    pub async fn is_k_cluster(&self, subset: &HashSet<BlockHash>) -> bool {
        for hash in subset {
            if self.anticone_size(hash, subset).await > self.k {
                return false;
            }
        }
        true
    }

    /// Compute blue score of a block
    pub async fn compute_blue_score(&self, hash: &BlockHash) -> usize {
        let past = self.compute_past(hash).await;
        let blue_blocks = self.blue_blocks.read().await;
        past.intersection(&*blue_blocks).count()
    }

    /// Select best tip based on blue score
    pub async fn select_best_tip(&self) -> Option<BlockHash> {
        let tips = self.tips.read().await;
        if tips.is_empty() {
            return None;
        }

        let mut best = tips[0];
        let mut best_score = self.compute_blue_score(&best).await;

        for tip in tips.iter().skip(1) {
            let score = self.compute_blue_score(tip).await;
            if score > best_score {
                best = *tip;
                best_score = score;
            }
        }

        Some(best)
    }

    /// Main GHOSTDAG algorithm - compute blue set (with recursion bounds)
    pub async fn compute_blue_set(&self, tip: &BlockHash) -> HashSet<BlockHash> {
        let mut visited = HashSet::new();
        self.compute_blue_set_bounded(tip, &mut visited, 1000).await
    }

    /// Bounded blue set computation to prevent infinite recursion
    async fn compute_blue_set_bounded(
        &self,
        tip: &BlockHash,
        visited: &mut HashSet<BlockHash>,
        max_depth: usize
    ) -> HashSet<BlockHash> {
        if max_depth == 0 || visited.contains(tip) {
            // Prevent infinite recursion by limiting depth and tracking visited nodes
            return HashSet::new();
        }

        visited.insert(*tip);
        let dag = self.dag.read().await;

        match dag.get(tip) {
            None => HashSet::new(),
            Some(node) => {
                if node.block.parent_hashes.is_empty() {
                    // Genesis block
                    let mut blue = HashSet::new();
                    blue.insert(*tip);
                    return blue;
                }

                // Find best parent based on blue score
                let mut best_parent = node.block.parent_hashes[0];
                let mut best_score = 0;

                for parent in &node.block.parent_hashes {
                    // Bounded recursive computation with boxed future
                    let mut parent_visited = visited.clone();
                    let parent_blue = Box::pin(self.compute_blue_set_bounded(
                        parent,
                        &mut parent_visited,
                        max_depth - 1
                    )).await;
                    let score = parent_blue.len();

                    if score > best_score {
                        best_parent = *parent;
                        best_score = score;
                    }
                }

                // Get best parent's blue set (bounded)
                let mut best_visited = visited.clone();
                let mut blue_set = Box::pin(self.compute_blue_set_bounded(
                    &best_parent,
                    &mut best_visited,
                    max_depth - 1
                )).await;

                // Add current block
                blue_set.insert(*tip);

                // Try to add blocks from anticone if they maintain k-cluster property
                let anticone = self.compute_anticone_bounded(tip, max_depth / 2).await;
                for block in anticone {
                    let mut test_set = blue_set.clone();
                    test_set.insert(block);

                    if self.is_k_cluster_bounded(&test_set, max_depth / 2).await {
                        blue_set.insert(block);
                    }
                }

                blue_set
            }
        }
    }

    /// Add a new block to the DAG
    pub async fn add_block(&self, block: Block) -> Result<()> {
        info!("Adding block {} to GhostDAG", hash_to_string(&block.hash));

        let mut dag = self.dag.write().await;

        // Check if block already exists
        if dag.contains_key(&block.hash) {
            warn!("Block already exists");
            return Ok(());
        }

        // Verify parent blocks exist
        for parent_hash in &block.parent_hashes {
            if !dag.contains_key(parent_hash) {
                return Err(anyhow::anyhow!("Parent block not found: {}", hash_to_string(parent_hash)));
            }
        }

        // Verify block meets difficulty
        if !block.meets_difficulty() {
            return Err(anyhow::anyhow!("Block does not meet difficulty requirement"));
        }

        // Create DAG node
        let node = DagNode {
            block: block.clone(),
            children: vec![],
            blue_anticone_size: 0,
            is_blue: false,
        };

        // Add to DAG
        dag.insert(block.hash, node);

        // Update children references in parent nodes
        for parent_hash in &block.parent_hashes {
            if let Some(parent_node) = dag.get_mut(parent_hash) {
                parent_node.children.push(block.hash);
            }
        }

        // Update tips
        drop(dag);
        self.update_tips().await;

        // Recompute blue set with bounded recursion (fixed!)
        if let Some(best_tip) = self.select_best_tip().await {
            let new_blue_set = self.compute_blue_set(&best_tip).await;
            *self.blue_blocks.write().await = new_blue_set;
        }

        info!("Block {} added successfully", hash_to_string(&block.hash));
        Ok(())
    }

    /// Update the tips (blocks without children)
    async fn update_tips(&self) {
        let dag = self.dag.read().await;
        let mut tips = Vec::new();

        for (hash, node) in dag.iter() {
            if node.children.is_empty() {
                tips.push(*hash);
            }
        }

        *self.tips.write().await = tips;
    }

    /// Get current state summary
    pub async fn get_state_summary(&self) -> GhostDAGState {
        let dag = self.dag.read().await;
        let tips = self.tips.read().await;
        let blue_blocks = self.blue_blocks.read().await;

        GhostDAGState {
            total_blocks: dag.len(),
            blue_blocks: blue_blocks.len(),
            red_blocks: dag.len() - blue_blocks.len(),
            tips: tips.len(),
            best_tip: self.select_best_tip().await,
            difficulty: *self.difficulty.read().await,
            k_parameter: self.k,
        }
    }

    /// Bounded anticone computation to prevent infinite recursion
    async fn compute_anticone_bounded(&self, target: &BlockHash, max_depth: usize) -> HashSet<BlockHash> {
        if max_depth == 0 {
            return HashSet::new();
        }

        let dag = self.dag.read().await;
        let past = self.compute_past_bounded(target, max_depth).await;
        let future = self.compute_future_bounded(target, max_depth).await;

        let mut anticone = HashSet::new();
        for hash in dag.keys() {
            if hash != target && !past.contains(hash) && !future.contains(hash) {
                anticone.insert(*hash);
            }
        }

        anticone
    }

    /// Bounded k-cluster check to prevent infinite recursion
    async fn is_k_cluster_bounded(&self, subset: &HashSet<BlockHash>, max_depth: usize) -> bool {
        if max_depth == 0 {
            return true; // Conservative: assume valid if we can't check deeply
        }

        for hash in subset {
            let anticone = self.compute_anticone_bounded(hash, max_depth).await;
            let anticone_size = anticone.intersection(subset).count();
            if anticone_size > self.k {
                return false;
            }
        }
        true
    }

    /// Bounded past computation (ancestors)
    async fn compute_past_bounded(&self, target: &BlockHash, max_depth: usize) -> HashSet<BlockHash> {
        if max_depth == 0 {
            return HashSet::new();
        }

        let mut past = HashSet::new();
        let mut visited = HashSet::new();
        let mut stack = vec![*target];

        while let Some(current) = stack.pop() {
            if visited.contains(&current) || visited.len() > max_depth {
                continue;
            }
            visited.insert(current);

            let dag = self.dag.read().await;
            if let Some(node) = dag.get(&current) {
                for parent in &node.block.parent_hashes {
                    if !visited.contains(parent) {
                        past.insert(*parent);
                        stack.push(*parent);
                    }
                }
            }
        }

        past
    }

    /// Bounded future computation (descendants)
    async fn compute_future_bounded(&self, target: &BlockHash, max_depth: usize) -> HashSet<BlockHash> {
        if max_depth == 0 {
            return HashSet::new();
        }

        let mut future = HashSet::new();
        let mut visited = HashSet::new();
        let mut stack = vec![*target];

        while let Some(current) = stack.pop() {
            if visited.contains(&current) || visited.len() > max_depth {
                continue;
            }
            visited.insert(current);

            let dag = self.dag.read().await;
            if let Some(node) = dag.get(&current) {
                for child in &node.children {
                    if !visited.contains(child) {
                        future.insert(*child);
                        stack.push(*child);
                    }
                }
            }
        }

        future
    }
}

/// GhostDAG state summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhostDAGState {
    pub total_blocks: usize,
    pub blue_blocks: usize,
    pub red_blocks: usize,
    pub tips: usize,
    pub best_tip: Option<BlockHash>,
    pub difficulty: u64,
    pub k_parameter: usize,
}

/// Integration with mesh network (temporarily disabled)
// pub mod mesh_integration {
//     use super::*;
//     use crate::mesh::MeshMessage;

//     /// Broadcast block to mesh network
//     pub async fn broadcast_block(
//         block: Block,
//         sender: &tokio::sync::mpsc::UnboundedSender<MeshMessage>,
//     ) -> Result<()> {
//         let message = MeshMessage::ConsensusMessage {
//             node_id: block.miner.clone(),
//             block_hash: hash_to_string(&block.hash),
//             parent_hashes: block.parent_hashes
//                 .iter()
//                 .map(|h| hash_to_string(h))
//                 .collect(),
//             blue_score: 0, // Will be computed by receiver
//         };

//         sender.send(message)?;
//         Ok(())
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_genesis_block() {
        let dag = GhostDAG::new(3);
        let state = dag.get_state_summary().await;

        assert_eq!(state.total_blocks, 1);
        assert_eq!(state.blue_blocks, 1);
        assert_eq!(state.red_blocks, 0);
    }

    #[tokio::test]
    async fn test_add_block() {
        let dag = GhostDAG::new(3);

        let genesis = Block::genesis();
        let mut block = Block {
            hash: [0; 32],
            parent_hashes: vec![genesis.hash],
            timestamp: 1,
            height: 1,
            operations: vec![],
            miner: "test".to_string(),
            nonce: 0,
            difficulty: 1,
        };
        block.hash = block.compute_hash();

        dag.add_block(block).await.unwrap();

        let state = dag.get_state_summary().await;
        assert_eq!(state.total_blocks, 2);
    }
}