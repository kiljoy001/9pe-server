//! Consensus message handler for GHOSTDAG protocol operations
//!
//! Handles ConsensusPropose, ConsensusVote, and ConsensusCommit messages,
//! wiring them to the underlying GHOSTDAG consensus implementation.

use anyhow::{bail, Result};
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::consensus::{ConsensusCoordinator, ConsensusResult, GhostdagBlock, BlockHash};
use crate::protocol::NinePMessage;
use super::connection_state::ConnectionState;

/// Handler for consensus-related protocol messages
pub struct ConsensusHandler {
    /// The consensus coordinator managing the GHOSTDAG instance
    coordinator: Arc<ConsensusCoordinator>,

    /// Connection state for auth checks
    connection_state: ConnectionState,
}

impl ConsensusHandler {
    /// Create a new consensus handler
    pub fn new(
        coordinator: Arc<ConsensusCoordinator>,
        connection_state: ConnectionState,
    ) -> Self {
        Self {
            coordinator,
            connection_state,
        }
    }

    /// Check if the connection is authenticated for consensus operations
    async fn require_auth(&self) -> Result<()> {
        if !self.connection_state.is_authenticated().await {
            bail!("Authentication required for consensus operations");
        }
        Ok(())
    }

    /// Handle a ConsensusPropose message
    ///
    /// This is the first step in adding a block - a node proposes a new block
    /// with its hash and parent hashes. For full block submission, the node
    /// should use the synthetic file interface at /srv/consensus/propose
    pub async fn handle_propose(
        &self,
        block_hash: [u8; 32],
        parent_hashes: Vec<[u8; 32]>,
    ) -> Result<NinePMessage> {
        self.require_auth().await?;

        debug!(
            "Consensus propose: block={} parents={}",
            hex::encode(&block_hash),
            parent_hashes.len()
        );

        // Check if this is a query for whether we have the block
        let dag = self.coordinator.method.read().await;

        if dag.blocks.contains_key(&block_hash) {
            // Block already exists
            return Ok(NinePMessage::Error {
                ename: "Block already exists in DAG".to_string(),
                errno: 17, // EEXIST
            });
        }

        // Check if all parents exist
        let mut missing_parents = Vec::new();
        for parent in &parent_hashes {
            if !dag.blocks.contains_key(parent) && !parent.iter().all(|&b| b == 0) {
                missing_parents.push(*parent);
            }
        }
        drop(dag);

        if !missing_parents.is_empty() {
            // Return which parents are missing so the proposer can send them first
            let missing_hex: Vec<String> = missing_parents.iter()
                .map(|h| hex::encode(h))
                .collect();

            return Ok(NinePMessage::Error {
                ename: format!("Missing parent blocks: {}", missing_hex.join(", ")),
                errno: 2, // ENOENT
            });
        }

        // Proposal is valid - respond with acceptance
        // The actual block data should be submitted via /srv/consensus/submit
        info!(
            "Consensus proposal accepted for block {}",
            hex::encode(&block_hash)
        );

        Ok(NinePMessage::ConsensusVote {
            block_hash,
            vote: true, // We accept this proposal
        })
    }

    /// Handle a ConsensusVote message
    ///
    /// Records a vote for or against a block from a peer
    pub async fn handle_vote(
        &self,
        block_hash: [u8; 32],
        vote: bool,
    ) -> Result<NinePMessage> {
        self.require_auth().await?;

        debug!(
            "Consensus vote: block={} vote={}",
            hex::encode(&block_hash),
            vote
        );

        // Record the vote
        let mut dag = self.coordinator.method.write().await;

        match dag.vote_block(block_hash, vote) {
            Ok(ConsensusResult::VoteRecorded(hash, v)) => {
                info!(
                    "Vote recorded for block {}: {}",
                    hex::encode(&hash),
                    if v { "accept" } else { "reject" }
                );

                // Return acknowledgment
                Ok(NinePMessage::ConsensusVote {
                    block_hash: hash,
                    vote: v,
                })
            }
            Err(e) => {
                warn!("Vote failed for block {}: {:?}", hex::encode(&block_hash), e);
                Ok(NinePMessage::Error {
                    ename: format!("Vote failed: {:?}", e),
                    errno: 22, // EINVAL
                })
            }
            Ok(other) => {
                // Unexpected result type
                Ok(NinePMessage::Error {
                    ename: format!("Unexpected result: {:?}", other),
                    errno: 22,
                })
            }
        }
    }

    /// Handle a ConsensusCommit message
    ///
    /// Finalizes a block as committed to the consensus
    pub async fn handle_commit(
        &self,
        block_hash: [u8; 32],
        blue_score: u64,
    ) -> Result<NinePMessage> {
        self.require_auth().await?;

        debug!(
            "Consensus commit: block={} blue_score={}",
            hex::encode(&block_hash),
            blue_score
        );

        let mut dag = self.coordinator.method.write().await;

        // Verify the block exists and has the expected blue score
        if let Some(block) = dag.blocks.get(&block_hash) {
            if block.blue_score != blue_score {
                warn!(
                    "Blue score mismatch for block {}: expected {}, got {}",
                    hex::encode(&block_hash),
                    blue_score,
                    block.blue_score
                );
            }
        }

        match dag.commit_block(block_hash) {
            Ok(ConsensusResult::BlockCommitted(hash, score)) => {
                info!(
                    "Block {} committed with blue score {}",
                    hex::encode(&hash),
                    score
                );

                Ok(NinePMessage::ConsensusCommit {
                    block_hash: hash,
                    blue_score: score,
                })
            }
            Err(e) => {
                warn!("Commit failed for block {}: {:?}", hex::encode(&block_hash), e);
                Ok(NinePMessage::Error {
                    ename: format!("Commit failed: {:?}", e),
                    errno: 22,
                })
            }
            Ok(other) => {
                Ok(NinePMessage::Error {
                    ename: format!("Unexpected result: {:?}", other),
                    errno: 22,
                })
            }
        }
    }

    /// Submit a full block to the consensus
    ///
    /// This is called when a complete block (with data, signature, PoW) is received,
    /// typically via the synthetic file system at /srv/consensus/submit
    pub async fn submit_block(&self, block: GhostdagBlock) -> Result<NinePMessage> {
        self.require_auth().await?;

        debug!(
            "Submitting block to consensus: hash={} parents={}",
            hex::encode(&block.hash),
            block.parent_hashes.len()
        );

        match self.coordinator.add_block(block.clone()).await {
            Ok(()) => {
                info!(
                    "Block {} added to consensus DAG",
                    hex::encode(&block.hash)
                );

                // Return the block hash and blue score
                Ok(NinePMessage::ConsensusCommit {
                    block_hash: block.hash,
                    blue_score: block.blue_score,
                })
            }
            Err(e) => {
                warn!("Block submission failed: {}", e);
                Ok(NinePMessage::Error {
                    ename: format!("Block rejected: {}", e),
                    errno: 22,
                })
            }
        }
    }

    /// Get the current consensus state
    pub async fn get_state(&self) -> Result<crate::consensus::ConsensusState> {
        Ok(self.coordinator.get_consensus_state().await)
    }

    /// Get consensus metrics
    pub async fn get_metrics(&self) -> crate::consensus::ConsensusMetrics {
        self.coordinator.get_metrics().await
    }

    /// Get recent blocks from the DAG
    pub async fn get_recent_blocks(&self, count: usize) -> Vec<GhostdagBlock> {
        self.coordinator.get_recent_blocks(count).await
    }

    /// Get the current required PoW difficulty
    pub async fn get_difficulty(&self) -> u32 {
        self.coordinator.calculate_difficulty().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_consensus_handler_creation() {
        let coordinator = Arc::new(ConsensusCoordinator::new("test-node".to_string()));
        let connection_state = ConnectionState::new();

        let handler = ConsensusHandler::new(coordinator, connection_state);

        // Should be able to get metrics
        let metrics = handler.get_metrics().await;
        assert_eq!(metrics.total_blocks, 0);
    }

    #[tokio::test]
    async fn test_propose_requires_auth() {
        let coordinator = Arc::new(ConsensusCoordinator::new("test-node".to_string()));
        let connection_state = ConnectionState::new();

        let handler = ConsensusHandler::new(coordinator, connection_state);

        // Should fail without auth
        let result = handler.handle_propose([0u8; 32], vec![]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_vote_requires_auth() {
        let coordinator = Arc::new(ConsensusCoordinator::new("test-node".to_string()));
        let connection_state = ConnectionState::new();

        let handler = ConsensusHandler::new(coordinator, connection_state);

        // Should fail without auth
        let result = handler.handle_vote([0u8; 32], true).await;
        assert!(result.is_err());
    }
}
