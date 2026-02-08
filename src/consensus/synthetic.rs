//! Synthetic filesystem interface for consensus operations
//!
//! Exposes GHOSTDAG consensus through the Plan 9 file interface:
//!
//! /srv/consensus/
//! ├── ctl           # Control file: write commands, read status
//! ├── status        # Read-only: current consensus state (JSON)
//! ├── blocks/       # Directory of recent blocks
//! │   ├── tip       # Hash of current tip block
//! │   ├── count     # Total block count
//! │   └── recent    # Recent block hashes (one per line)
//! ├── submit        # Write a block (CBOR-encoded GhostdagBlock)
//! ├── propose       # Write block_hash:parent1,parent2 to propose
//! ├── vote          # Write block_hash:1 or block_hash:0 to vote
//! ├── difficulty    # Read current PoW difficulty
//! └── metrics       # Read-only: performance metrics (JSON)

use crate::consensus::{ConsensusCoordinator, GhostdagBlock};
use crate::synth::{ControlHandler, SyntheticFilesystem};
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

/// Register consensus control files under /srv/consensus
pub async fn register_consensus_controls(
    synth: &Arc<SyntheticFilesystem>,
    coordinator: Arc<ConsensusCoordinator>,
) -> Result<()> {
    let base = PathBuf::from("/srv/consensus");
    synth.create_directory(&base).await?;

    // /srv/consensus/ctl - main control file
    synth
        .create_control_file(
            &base.join("ctl"),
            Arc::new(ConsensusCtlHandler {
                coordinator: coordinator.clone(),
            }),
        )
        .await?;

    // /srv/consensus/status - read current state
    synth
        .create_control_file(
            &base.join("status"),
            Arc::new(ConsensusStatusHandler {
                coordinator: coordinator.clone(),
            }),
        )
        .await?;

    // /srv/consensus/blocks/ directory
    let blocks_dir = base.join("blocks");
    synth.create_directory(&blocks_dir).await?;

    // /srv/consensus/blocks/tip - current tip block hash
    synth
        .create_control_file(
            &blocks_dir.join("tip"),
            Arc::new(ConsensusTipHandler {
                coordinator: coordinator.clone(),
            }),
        )
        .await?;

    // /srv/consensus/blocks/count - total block count
    synth
        .create_control_file(
            &blocks_dir.join("count"),
            Arc::new(ConsensusCountHandler {
                coordinator: coordinator.clone(),
            }),
        )
        .await?;

    // /srv/consensus/blocks/recent - recent block hashes
    synth
        .create_control_file(
            &blocks_dir.join("recent"),
            Arc::new(ConsensusRecentHandler {
                coordinator: coordinator.clone(),
            }),
        )
        .await?;

    // /srv/consensus/submit - submit a full block
    synth
        .create_control_file(
            &base.join("submit"),
            Arc::new(ConsensusSubmitHandler {
                coordinator: coordinator.clone(),
            }),
        )
        .await?;

    // /srv/consensus/propose - propose a block (hash:parents)
    synth
        .create_control_file(
            &base.join("propose"),
            Arc::new(ConsensusProposeHandler {
                coordinator: coordinator.clone(),
            }),
        )
        .await?;

    // /srv/consensus/vote - vote on a block (hash:0 or hash:1)
    synth
        .create_control_file(
            &base.join("vote"),
            Arc::new(ConsensusVoteHandler {
                coordinator: coordinator.clone(),
            }),
        )
        .await?;

    // /srv/consensus/difficulty - current PoW difficulty
    synth
        .create_control_file(
            &base.join("difficulty"),
            Arc::new(ConsensusDifficultyHandler {
                coordinator: coordinator.clone(),
            }),
        )
        .await?;

    // /srv/consensus/metrics - performance metrics
    synth
        .create_control_file(
            &base.join("metrics"),
            Arc::new(ConsensusMetricsHandler {
                coordinator: coordinator.clone(),
            }),
        )
        .await?;

    Ok(())
}

/// Control file handler - accepts commands, returns status
struct ConsensusCtlHandler {
    coordinator: Arc<ConsensusCoordinator>,
}

impl ControlHandler for ConsensusCtlHandler {
    fn read(&self) -> Result<Vec<u8>> {
        // Return available commands
        Ok(b"Commands:\n\
             gc        - garbage collect optimization caches\n\
             prune N   - prune blocks older than N blue score window\n\
             status    - show brief status\n".to_vec())
    }

    fn write(&self, data: &[u8]) -> Result<()> {
        let cmd = std::str::from_utf8(data)?.trim();
        let parts: Vec<&str> = cmd.split_whitespace().collect();

        match parts.get(0).map(|s| *s) {
            Some("gc") => {
                // Run garbage collection synchronously
                let rt = tokio::runtime::Handle::current();
                rt.block_on(async {
                    let mut dag = self.coordinator.method.write().await;
                    dag.garbage_collect();
                });
                Ok(())
            }
            Some("prune") => {
                let window: u64 = parts
                    .get(1)
                    .ok_or_else(|| anyhow::anyhow!("prune requires window size"))?
                    .parse()?;
                let rt = tokio::runtime::Handle::current();
                rt.block_on(async {
                    let mut dag = self.coordinator.method.write().await;
                    dag.prune_old_blocks(window)?;
                    Ok::<_, anyhow::Error>(())
                })?;
                Ok(())
            }
            Some("status") => {
                // Just acknowledge - actual status via /status file
                Ok(())
            }
            Some(other) => Err(anyhow::anyhow!("Unknown command: {}", other)),
            None => Err(anyhow::anyhow!("Empty command")),
        }
    }
}

/// Status handler - returns consensus state as JSON
struct ConsensusStatusHandler {
    coordinator: Arc<ConsensusCoordinator>,
}

impl ControlHandler for ConsensusStatusHandler {
    fn read(&self) -> Result<Vec<u8>> {
        let rt = tokio::runtime::Handle::current();
        let state = rt.block_on(self.coordinator.get_consensus_state());

        let json = serde_json::json!({
            "node_id": state.node_id,
            "block_count": state.block_count,
            "dag_height": state.dag_height,
            "tips": state.tips,
            "confidence": state.confidence_score(),
        });

        Ok(serde_json::to_vec_pretty(&json)?)
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("status is read-only"))
    }
}

/// Tip handler - returns current tip block hash
struct ConsensusTipHandler {
    coordinator: Arc<ConsensusCoordinator>,
}

impl ControlHandler for ConsensusTipHandler {
    fn read(&self) -> Result<Vec<u8>> {
        let rt = tokio::runtime::Handle::current();
        let blocks = rt.block_on(self.coordinator.get_recent_blocks(1));

        if let Some(block) = blocks.first() {
            Ok(format!("{}\n", hex::encode(&block.hash)).into_bytes())
        } else {
            Ok(b"none\n".to_vec())
        }
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("tip is read-only"))
    }
}

/// Count handler - returns total block count
struct ConsensusCountHandler {
    coordinator: Arc<ConsensusCoordinator>,
}

impl ControlHandler for ConsensusCountHandler {
    fn read(&self) -> Result<Vec<u8>> {
        let rt = tokio::runtime::Handle::current();
        let metrics = rt.block_on(self.coordinator.get_metrics());
        Ok(format!("{}\n", metrics.total_blocks).into_bytes())
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("count is read-only"))
    }
}

/// Recent blocks handler - returns recent block hashes
struct ConsensusRecentHandler {
    coordinator: Arc<ConsensusCoordinator>,
}

impl ControlHandler for ConsensusRecentHandler {
    fn read(&self) -> Result<Vec<u8>> {
        let rt = tokio::runtime::Handle::current();
        let blocks = rt.block_on(self.coordinator.get_recent_blocks(20));

        let mut output = String::new();
        for block in blocks {
            output.push_str(&hex::encode(&block.hash));
            output.push('\n');
        }

        Ok(output.into_bytes())
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("recent is read-only"))
    }
}

/// Submit handler - accepts CBOR-encoded blocks
struct ConsensusSubmitHandler {
    coordinator: Arc<ConsensusCoordinator>,
}

impl ControlHandler for ConsensusSubmitHandler {
    fn read(&self) -> Result<Vec<u8>> {
        Ok(b"Write CBOR-encoded GhostdagBlock to submit\n".to_vec())
    }

    fn write(&self, data: &[u8]) -> Result<()> {
        // Decode block from CBOR
        let block: GhostdagBlock = serde_cbor::from_slice(data)
            .map_err(|e| anyhow::anyhow!("Invalid block CBOR: {}", e))?;

        let rt = tokio::runtime::Handle::current();
        rt.block_on(self.coordinator.add_block(block))?;

        Ok(())
    }
}

/// Propose handler - write "hash:parent1,parent2" to propose
struct ConsensusProposeHandler {
    coordinator: Arc<ConsensusCoordinator>,
}

impl ControlHandler for ConsensusProposeHandler {
    fn read(&self) -> Result<Vec<u8>> {
        Ok(b"Write block_hash:parent1,parent2,... to propose\n\
             Example: abc123:def456,789012\n".to_vec())
    }

    fn write(&self, data: &[u8]) -> Result<()> {
        let input = std::str::from_utf8(data)?.trim();

        let parts: Vec<&str> = input.split(':').collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!(
                "Format: block_hash:parent1,parent2,..."
            ));
        }

        let block_hash = parse_hash(parts[0])?;
        let parent_hashes: Result<Vec<[u8; 32]>> = parts[1]
            .split(',')
            .filter(|s| !s.is_empty())
            .map(parse_hash)
            .collect();
        let parent_hashes = parent_hashes?;

        // Check if parents exist
        let rt = tokio::runtime::Handle::current();
        let dag = rt.block_on(async { self.coordinator.method.read().await });

        for parent in &parent_hashes {
            if !dag.blocks.contains_key(parent) && !parent.iter().all(|&b| b == 0) {
                return Err(anyhow::anyhow!(
                    "Missing parent: {}",
                    hex::encode(parent)
                ));
            }
        }

        // Proposal accepted (block data should follow via /submit)
        Ok(())
    }
}

/// Vote handler - write "hash:1" to accept or "hash:0" to reject
struct ConsensusVoteHandler {
    coordinator: Arc<ConsensusCoordinator>,
}

impl ControlHandler for ConsensusVoteHandler {
    fn read(&self) -> Result<Vec<u8>> {
        Ok(b"Write block_hash:1 (accept) or block_hash:0 (reject)\n".to_vec())
    }

    fn write(&self, data: &[u8]) -> Result<()> {
        let input = std::str::from_utf8(data)?.trim();

        let parts: Vec<&str> = input.split(':').collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!("Format: block_hash:0 or block_hash:1"));
        }

        let block_hash = parse_hash(parts[0])?;
        let vote = match parts[1] {
            "1" | "yes" | "accept" => true,
            "0" | "no" | "reject" => false,
            other => return Err(anyhow::anyhow!("Invalid vote: {}", other)),
        };

        let rt = tokio::runtime::Handle::current();
        rt.block_on(async {
            let mut dag = self.coordinator.method.write().await;
            dag.vote_block(block_hash, vote)
                .map_err(|e| anyhow::anyhow!("Vote failed: {:?}", e))?;
            Ok::<_, anyhow::Error>(())
        })?;

        Ok(())
    }
}

/// Difficulty handler - returns current PoW difficulty
struct ConsensusDifficultyHandler {
    coordinator: Arc<ConsensusCoordinator>,
}

impl ControlHandler for ConsensusDifficultyHandler {
    fn read(&self) -> Result<Vec<u8>> {
        let rt = tokio::runtime::Handle::current();
        let difficulty = rt.block_on(self.coordinator.calculate_difficulty());
        Ok(format!("{}\n", difficulty).into_bytes())
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("difficulty is read-only"))
    }
}

/// Metrics handler - returns performance metrics as JSON
struct ConsensusMetricsHandler {
    coordinator: Arc<ConsensusCoordinator>,
}

impl ControlHandler for ConsensusMetricsHandler {
    fn read(&self) -> Result<Vec<u8>> {
        let rt = tokio::runtime::Handle::current();
        let metrics = rt.block_on(self.coordinator.get_metrics());
        let dag = rt.block_on(async { self.coordinator.method.read().await });
        let mem = dag.get_memory_usage();
        let stats = dag.get_consensus_stats();

        let json = serde_json::json!({
            "tip_height": metrics.tip_height,
            "total_blocks": metrics.total_blocks,
            "blue_blocks": stats.blue_blocks,
            "red_blocks": stats.red_blocks,
            "current_depth": stats.current_depth,
            "memory_optimization_ratio": stats.memory_optimization_ratio,
            "memory": {
                "tree_eval_cache": mem.tree_eval_cache_size,
                "consensus_buffer": mem.consensus_buffer_size,
                "catalytic_cache": mem.catalytic_cache_size,
                "streaming_window": mem.streaming_window_size,
            },
            "consensus_reached": metrics.consensus_reached,
        });

        Ok(serde_json::to_vec_pretty(&json)?)
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("metrics is read-only"))
    }
}

/// Parse a hex-encoded 32-byte hash
fn parse_hash(s: &str) -> Result<[u8; 32]> {
    let bytes = hex::decode(s.trim())
        .map_err(|e| anyhow::anyhow!("Invalid hex: {}", e))?;

    if bytes.len() != 32 {
        return Err(anyhow::anyhow!(
            "Hash must be 32 bytes, got {}",
            bytes.len()
        ));
    }

    let mut hash = [0u8; 32];
    hash.copy_from_slice(&bytes);
    Ok(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hash() {
        let valid = "0000000000000000000000000000000000000000000000000000000000000000";
        assert!(parse_hash(valid).is_ok());

        let too_short = "00000000";
        assert!(parse_hash(too_short).is_err());

        let invalid_hex = "xyz";
        assert!(parse_hash(invalid_hex).is_err());
    }

    #[tokio::test]
    async fn test_register_controls() {
        let synth = Arc::new(SyntheticFilesystem::new());
        let coordinator = Arc::new(ConsensusCoordinator::new("test".to_string()));

        let result = register_consensus_controls(&synth, coordinator).await;
        assert!(result.is_ok());

        // Check files were created
        assert!(synth.get_node(&PathBuf::from("/srv/consensus/ctl")).await.is_some());
        assert!(synth.get_node(&PathBuf::from("/srv/consensus/status")).await.is_some());
        assert!(synth.get_node(&PathBuf::from("/srv/consensus/submit")).await.is_some());
    }
}
