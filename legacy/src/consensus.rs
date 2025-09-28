//! Consensus integration for 9P.e
//!
//! Bridges GhostDAG consensus with mesh networking and file operations

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::OnceCell;
use anyhow::Result;
use tracing::{info, debug};

use crate::ghostdag::{GhostDAG, Block, BlockHash, FileOperation, hash_to_string};
// use crate::mesh::{MeshMessage, get_mesh_network};  // Temporarily disabled

/// Global GhostDAG instance
static GHOSTDAG: OnceCell<Arc<GhostDAG>> = OnceCell::const_new();

/// Initialize the global GhostDAG instance
pub async fn init_ghostdag(k: usize) -> Arc<GhostDAG> {
    GHOSTDAG.get_or_init(|| async {
        info!("🔗 Initializing GhostDAG with k={}", k);
        Arc::new(GhostDAG::new(k))
    }).await.clone()
}

/// Get the global GhostDAG instance
pub async fn get_ghostdag() -> Option<Arc<GhostDAG>> {
    GHOSTDAG.get().map(|g| g.clone())
}

/// Mine a new block with file operations
pub async fn mine_block(operations: Vec<FileOperation>, miner_id: String) -> Result<Block> {
    let ghostdag = init_ghostdag(3).await;

    // Get current tips as parents
    let tips = ghostdag.tips.read().await;
    let parent_hashes = tips.clone();

    // Get current height
    let mut max_height = 0u64;
    if !parent_hashes.is_empty() {
        let dag = ghostdag.dag.read().await;
        for parent_hash in &parent_hashes {
            if let Some(parent) = dag.get(parent_hash) {
                max_height = max_height.max(parent.block.height);
            }
        }
    }

    // Get difficulty
    let difficulty = *ghostdag.difficulty.read().await;

    // Create new block
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_secs();

    let mut block = Block {
        hash: [0; 32],
        parent_hashes,
        timestamp,
        height: max_height + 1,
        operations,
        miner: miner_id,
        nonce: 0,
        difficulty,
    };

    // Mine (find nonce that meets difficulty)
    info!("⛏️ Mining block at height {} with difficulty {}...", block.height, difficulty);
    let start = SystemTime::now();

    while !block.meets_difficulty() {
        block.nonce += 1;
        block.hash = block.compute_hash();

        if block.nonce % 100000 == 0 {
            debug!("Mining... nonce: {}", block.nonce);
        }
    }

    let elapsed = SystemTime::now().duration_since(start)?;
    info!("✅ Block mined! Hash: {} (took {:?})", hash_to_string(&block.hash), elapsed);

    // Add to our DAG
    ghostdag.add_block(block.clone()).await?;

    // Broadcast to network (disabled until mesh is fixed)
    // if let Some(mesh) = get_mesh_network().await {
    //     let sender = mesh.message_sender();
    //     let message = MeshMessage::ConsensusMessage {
    //         node_id: block.miner.clone(),
    //         block_hash: hash_to_string(&block.hash),
    //         parent_hashes: block.parent_hashes
    //             .iter()
    //             .map(|h| hash_to_string(h))
    //             .collect(),
    //         blue_score: ghostdag.compute_blue_score(&block.hash).await as u64,
    //     };

    //     if let Err(e) = sender.send(message) {
    //         warn!("Failed to broadcast block: {}", e);
    //     }
    // }

    Ok(block)
}

/// Create block from mesh message
pub fn block_from_mesh_message(
    node_id: String,
    block_hash_str: String,
    parent_hash_strs: Vec<String>,
) -> Result<Block> {
    // Parse block hash
    let block_hash = hex::decode(&block_hash_str)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid block hash length"))?;

    // Parse parent hashes
    let mut parent_hashes = Vec::new();
    for parent_str in parent_hash_strs {
        let parent_hash: BlockHash = hex::decode(&parent_str)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid parent hash length"))?;
        parent_hashes.push(parent_hash);
    }

    // Create block (operations will be empty for received blocks)
    let block = Block {
        hash: block_hash,
        parent_hashes,
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs(),
        height: 0, // Will be computed when added
        operations: vec![], // Empty for now
        miner: node_id,
        nonce: 0,
        difficulty: 1,
    };

    Ok(block)
}

/// Process block from network
pub async fn process_network_block(
    node_id: String,
    block_hash: String,
    parent_hashes: Vec<String>,
) -> Result<()> {
    let ghostdag = init_ghostdag(3).await;

    let block = block_from_mesh_message(node_id, block_hash, parent_hashes)?;
    ghostdag.add_block(block).await?;

    Ok(())
}

/// Get consensus state summary
pub async fn get_consensus_state() -> Result<String> {
    if let Some(ghostdag) = get_ghostdag().await {
        let state = ghostdag.get_state_summary().await;

        let best_tip_str = if let Some(tip) = state.best_tip {
            hash_to_string(&tip)
        } else {
            "none".to_string()
        };

        Ok(format!(
            "🔗 GhostDAG Consensus State:\n\
             📊 Total blocks: {}\n\
             🔵 Blue blocks: {}\n\
             🔴 Red blocks: {}\n\
             📍 Tips: {}\n\
             🎯 Best tip: {}\n\
             ⛏️ Difficulty: {}\n\
             📏 K parameter: {}",
            state.total_blocks,
            state.blue_blocks,
            state.red_blocks,
            state.tips,
            best_tip_str,
            state.difficulty,
            state.k_parameter
        ))
    } else {
        Ok("❌ GhostDAG not initialized".to_string())
    }
}

/// Commands for consensus operations
pub mod commands {
    use super::*;

    /// Mine a test block
    pub async fn mine_test_block() -> Result<()> {
        let operations = vec![
            FileOperation::Create {
                path: "/test/block.txt".to_string(),
                content: b"Test block data".to_vec(),
            },
        ];

        let node_id = "test-miner".to_string();
        let block = mine_block(operations, node_id).await?;

        info!("🎉 Successfully mined block: {}", hash_to_string(&block.hash));
        info!("   Height: {}, Nonce: {}", block.height, block.nonce);

        Ok(())
    }

    /// Show consensus state
    pub async fn show_consensus_state() -> Result<()> {
        let state = get_consensus_state().await?;
        println!("{}", state);
        Ok(())
    }

    /// Adjust mining difficulty
    pub async fn set_difficulty(difficulty: u64) -> Result<()> {
        let ghostdag = init_ghostdag(3).await;
        *ghostdag.difficulty.write().await = difficulty;
        info!("⛏️ Mining difficulty set to {}", difficulty);
        Ok(())
    }
}