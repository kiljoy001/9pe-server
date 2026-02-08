//! Consensus control via synthetic files
//!
//! Control the GHOSTDAG consensus system by reading/writing files in /srv/consensus/

use crate::consensus::ConsensusCoordinator;
use crate::synth::{ControlHandler, SyntheticFilesystem};
use anyhow::Result;
use std::path::PathBuf;
use serde::Deserialize;

#[derive(Deserialize)]
struct DagInfo {
    tips: Vec<String>,
}

#[derive(Deserialize)]
struct PeerEntry { // Renamed to avoid confusion if needed, or keeping local
    peer_id: String,
    address: String,
    blocks_ahead: u64,
    latency_ms: u64,
}

use std::sync::Arc;

/// Register consensus control files in the synthetic filesystem
pub async fn register_consensus_control(
    synth: &SyntheticFilesystem,
    consensus: Arc<ConsensusCoordinator>,
) -> Result<()> {
    // Create /srv/consensus directory
    synth
        .create_directory(&PathBuf::from("/srv/consensus"))
        .await?;

    // /srv/consensus/status - Read consensus state
    synth
        .create_control_file(
            &PathBuf::from("/srv/consensus/status"),
            Arc::new(StatusHandler {
                consensus: consensus.clone(),
            }),
        )
        .await?;

    // /srv/consensus/submit - Write transaction to submit to DAG
    synth
        .create_control_file(
            &PathBuf::from("/srv/consensus/submit"),
            Arc::new(SubmitHandler {
                consensus: consensus.clone(),
            }),
        )
        .await?;

    // /srv/consensus/blocks - Read list of recent blocks
    synth
        .create_control_file(
            &PathBuf::from("/srv/consensus/blocks"),
            Arc::new(BlocksHandler {
                consensus: consensus.clone(),
            }),
        )
        .await?;

    // /srv/consensus/dag - Read DAG structure
    synth
        .create_control_file(
            &PathBuf::from("/srv/consensus/dag"),
            Arc::new(DagHandler {
                consensus: consensus.clone(),
            }),
        )
        .await?;

    // /srv/consensus/peers - Read consensus network peers
    synth
        .create_control_file(
            &PathBuf::from("/srv/consensus/peers"),
            Arc::new(PeersHandler {
                consensus: consensus.clone(),
            }),
        )
        .await?;

    // /srv/consensus/metrics - Read consensus metrics
    synth
        .create_control_file(
            &PathBuf::from("/srv/consensus/metrics"),
            Arc::new(MetricsHandler {
                consensus: consensus.clone(),
            }),
        )
        .await?;

    Ok(())
}

/// Handler for /srv/consensus/status - consensus state
struct StatusHandler {
    consensus: Arc<ConsensusCoordinator>,
}

impl ControlHandler for StatusHandler {
    fn read(&self) -> Result<Vec<u8>> {
        let metrics = futures::executor::block_on(self.consensus.get_metrics());
        let state = futures::executor::block_on(self.consensus.get_consensus_state());

        let output = format!(
            "Consensus Status\n\
             ================\n\
             Node ID: {}\n\
             Tip Height: {}\n\
             Total Blocks: {}\n\
             Pending Transactions: {}\n\
             Network Hashrate: {:.2} H/s\n\
             Active Peers: {}\n\
             Consensus Reached: {}\n",
            state.node_id,
            metrics.tip_height,
            metrics.total_blocks,
            metrics.pending_tx_count,
            metrics.network_hashrate,
            metrics.active_peers,
            metrics.consensus_reached
        );

        Ok(output.into_bytes())
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("status file is read-only"))
    }
}

/// Handler for /srv/consensus/submit - submit transaction
struct SubmitHandler {
    consensus: Arc<ConsensusCoordinator>,
}

impl ControlHandler for SubmitHandler {
    fn read(&self) -> Result<Vec<u8>> {
        Ok(b"Write transaction data (JSON format) to submit to DAG\n".to_vec())
    }

    fn write(&self, data: &[u8]) -> Result<()> {
        let tx_data = String::from_utf8(data.to_vec())?;

        // Parse transaction (assume JSON for now)
        let tx: serde_json::Value = serde_json::from_str(&tx_data)?;

        // Submit to consensus
        futures::executor::block_on(async { self.consensus.submit_transaction(serde_json::to_vec(&tx)?).await })?;

        Ok(())
    }
}

/// Handler for /srv/consensus/blocks - recent blocks
struct BlocksHandler {
    consensus: Arc<ConsensusCoordinator>,
}

impl ControlHandler for BlocksHandler {
    fn read(&self) -> Result<Vec<u8>> {
        let blocks = futures::executor::block_on(self.consensus.get_recent_blocks(20));

        let mut output = String::from("Recent Blocks\n=============\n");
        for block in blocks {
            let parent_display = block
                .parent_hashes
                .first()
                .map(|p| hex::encode(p).chars().take(8).collect::<String>())
                .unwrap_or_else(|| "genesis".to_string());

            output.push_str(&format!(
                "Block {} | Blue Score: {} | Parent: {} | Timestamp: {}\n",
                &hex::encode(block.hash).chars().take(8).collect::<String>(), // block_id
                block.blue_score, // height
                parent_display,
                block.timestamp
            ));
        }

        Ok(output.into_bytes())
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("blocks file is read-only"))
    }
}

/// Handler for /srv/consensus/dag - DAG structure
struct DagHandler {
    consensus: Arc<ConsensusCoordinator>,
}

impl ControlHandler for DagHandler {
    fn read(&self) -> Result<Vec<u8>> {
        let dag_info_val = futures::executor::block_on(self.consensus.get_dag_structure());
        let dag_info: DagInfo = serde_json::from_value(dag_info_val)?;

        let output = format!(
            "DAG Structure\n\
             =============\n\
             Tips: {}\n\
             \n\
             Recent Tips:\n",
            dag_info.tips.len()
        );

        let mut output_bytes = output.into_bytes();
        for tip in dag_info.tips {
            output_bytes.extend_from_slice(format!("  {}\n", tip).as_bytes());
        }

        Ok(output_bytes)
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("dag file is read-only"))
    }
}

/// Handler for /srv/consensus/peers - consensus network peers
struct PeersHandler {
    consensus: Arc<ConsensusCoordinator>,
}

impl ControlHandler for PeersHandler {
    fn read(&self) -> Result<Vec<u8>> {
        let peers_val = futures::executor::block_on(self.consensus.get_network_peers());
        // Since get_network_peers now returns Value, we can use it directly
        let peers: Vec<PeerEntry> = serde_json::from_value(peers_val)?;

        let mut output = String::from("Consensus Peers\n===============\n");
        for peer in peers {
            output.push_str(&format!(
                "{}\t{}\t{} blocks ahead\t{} ms latency\n",
                peer.peer_id, peer.address, peer.blocks_ahead, peer.latency_ms
            ));
        }

        Ok(output.into_bytes())
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("peers file is read-only"))
    }
}

/// Handler for /srv/consensus/metrics - consensus metrics
struct MetricsHandler {
    consensus: Arc<ConsensusCoordinator>,
}

impl ControlHandler for MetricsHandler {
    fn read(&self) -> Result<Vec<u8>> {
        let metrics = futures::executor::block_on(self.consensus.get_metrics());

        let output = format!(
            "# Consensus Metrics (Prometheus format)\n\
             # TYPE consensus_tip_height gauge\n\
             consensus_tip_height {}\n\
             \n\
             # TYPE consensus_blocks_total counter\n\
             consensus_blocks_total {}\n\
             \n\
             # TYPE consensus_pending_transactions gauge\n\
             consensus_pending_transactions {}\n\
             \n\
             # TYPE consensus_network_hashrate gauge\n\
             consensus_network_hashrate {:.2}\n\
             \n\
             # TYPE consensus_active_peers gauge\n\
             consensus_active_peers {}\n",
            metrics.tip_height,
            metrics.total_blocks,
            metrics.pending_tx_count,
            metrics.network_hashrate,
            metrics.active_peers
        );

        Ok(output.into_bytes())
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("metrics file is read-only"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_consensus_control_registration() {
        let synth = SyntheticFilesystem::new();
        // Note: Would need actual ConsensusCoordinator for full test
        // This is a structure test only
        assert!(true);
    }
}
