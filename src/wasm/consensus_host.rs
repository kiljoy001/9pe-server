//! Consensus host functions for WASM transformers
//!
//! This module provides read-only access to GHOSTDAG consensus state
//! for WASM transformers, allowing them to query network state and
//! work distribution information without being able to influence consensus.

use anyhow::Result;
use std::sync::{Arc, Mutex};
use wasmtime::{Caller, Linker};
use tracing::{debug, error, info};
use once_cell::sync::Lazy;

use crate::consensus::ConsensusState;

/// Placeholder network statistics structure
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct NetworkStats {
    pub node_count: u32,
    pub connected_peers: u32,
    pub consensus_score: f64,
}

impl NetworkStats {
    fn from_consensus_state(_consensus: &ConsensusState) -> Self {
        // Placeholder implementation
        Self {
            node_count: 1,
            connected_peers: 0,
            consensus_score: 1.0,
        }
    }
}

/// Global consensus state shared across WASM instances
static CONSENSUS_STATE: Lazy<Arc<Mutex<Option<ConsensusState>>>> = Lazy::new(|| {
    Arc::new(Mutex::new(None))
});

/// Add consensus host functions to the WASM linker
pub fn add_consensus_functions<T>(linker: &mut Linker<T>) -> Result<()>
where
    T: 'static,
{
    // Consensus state queries
    linker.func_wrap("consensus", "get_dag_height", consensus_get_dag_height)?;
    linker.func_wrap("consensus", "get_confirmed_block_count", consensus_get_confirmed_block_count)?;
    linker.func_wrap("consensus", "get_pending_work_count", consensus_get_pending_work_count)?;
    linker.func_wrap("consensus", "get_confidence_score", consensus_get_confidence_score)?;

    // Block queries
    linker.func_wrap("consensus", "is_block_confirmed", consensus_is_block_confirmed)?;
    linker.func_wrap("consensus", "get_block_ghost_score", consensus_get_block_ghost_score)?;
    linker.func_wrap("consensus", "get_main_chain_tip", consensus_get_main_chain_tip)?;

    // Network state
    linker.func_wrap("consensus", "get_active_nodes", consensus_get_active_nodes)?;
    linker.func_wrap("consensus", "get_network_stats", consensus_get_network_stats)?;

    // Work distribution queries
    linker.func_wrap("consensus", "query_work_capacity", consensus_query_work_capacity)?;
    linker.func_wrap("consensus", "estimate_work_completion", consensus_estimate_work_completion)?;

    Ok(())
}

/// Update the global consensus state (called by the main server)
pub fn update_consensus_state(state: ConsensusState) -> Result<()> {
    match CONSENSUS_STATE.lock() {
        Ok(mut global_state) => {
            *global_state = Some(state);
            debug!("Updated consensus state for WASM transformers");
            Ok(())
        }
        Err(e) => {
            error!("Failed to update consensus state: {}", e);
            anyhow::bail!("Failed to update consensus state: {}", e)
        }
    }
}

// Host function implementations

/// Get the current DAG height
fn consensus_get_dag_height<T>(_caller: Caller<'_, T>) -> i64 {
    match CONSENSUS_STATE.lock() {
        Ok(state) => {
            if let Some(ref consensus) = *state {
                consensus.dag_height as i64
            } else {
                -1 // No consensus state available
            }
        }
        Err(e) => {
            error!("Failed to lock consensus state: {}", e);
            -1
        }
    }
}

/// Get the number of confirmed blocks
fn consensus_get_confirmed_block_count<T>(_caller: Caller<'_, T>) -> i64 {
    match CONSENSUS_STATE.lock() {
        Ok(state) => {
            if let Some(ref consensus) = *state {
                consensus.confirmed_blocks.len() as i64
            } else {
                -1
            }
        }
        Err(e) => {
            error!("Failed to lock consensus state: {}", e);
            -1
        }
    }
}

/// Get the number of pending work items
fn consensus_get_pending_work_count<T>(_caller: Caller<'_, T>) -> i64 {
    match CONSENSUS_STATE.lock() {
        Ok(state) => {
            if let Some(ref consensus) = *state {
                consensus.pending_work.len() as i64
            } else {
                -1
            }
        }
        Err(e) => {
            error!("Failed to lock consensus state: {}", e);
            -1
        }
    }
}

/// Get consensus confidence score (0-100)
fn consensus_get_confidence_score<T>(_caller: Caller<'_, T>) -> i32 {
    match CONSENSUS_STATE.lock() {
        Ok(state) => {
            if let Some(ref consensus) = *state {
                (consensus.confidence_score() * 100.0) as i32
            } else {
                -1
            }
        }
        Err(e) => {
            error!("Failed to lock consensus state: {}", e);
            -1
        }
    }
}

/// Check if a block is confirmed
fn consensus_is_block_confirmed<T>(_caller: Caller<'_, T>, block_id_ptr: i32, block_id_len: i32) -> i32 {
    // In a real implementation, we'd read the block ID from WASM memory
    // For now, return mock result
    debug!("Check block confirmation for block (ptr: {}, len: {})", block_id_ptr, block_id_len);

    match CONSENSUS_STATE.lock() {
        Ok(state) => {
            if let Some(ref _consensus) = *state {
                // Mock: return 1 for confirmed, 0 for not confirmed
                1
            } else {
                -1
            }
        }
        Err(e) => {
            error!("Failed to lock consensus state: {}", e);
            -1
        }
    }
}

/// Get GHOST score for a block
fn consensus_get_block_ghost_score<T>(_caller: Caller<'_, T>, block_id_ptr: i32, block_id_len: i32) -> i64 {
    debug!("Get GHOST score for block (ptr: {}, len: {})", block_id_ptr, block_id_len);

    match CONSENSUS_STATE.lock() {
        Ok(state) => {
            if let Some(ref _consensus) = *state {
                // Mock: return a GHOST score
                100
            } else {
                -1
            }
        }
        Err(e) => {
            error!("Failed to lock consensus state: {}", e);
            -1
        }
    }
}

/// Get the current main chain tip block ID
fn consensus_get_main_chain_tip<T>(_caller: Caller<'_, T>) -> i32 {
    match CONSENSUS_STATE.lock() {
        Ok(state) => {
            if let Some(ref consensus) = *state {
                if let Some(_tip) = consensus.main_chain_tip() {
                    // In real implementation, write tip ID to WASM memory
                    // Return length of tip ID
                    32 // Mock length
                } else {
                    0 // No tip available
                }
            } else {
                -1
            }
        }
        Err(e) => {
            error!("Failed to lock consensus state: {}", e);
            -1
        }
    }
}

/// Get number of active nodes in the network
fn consensus_get_active_nodes<T>(_caller: Caller<'_, T>) -> i32 {
    match CONSENSUS_STATE.lock() {
        Ok(state) => {
            if let Some(ref _consensus) = *state {
                // Mock: return number of active nodes
                5
            } else {
                -1
            }
        }
        Err(e) => {
            error!("Failed to lock consensus state: {}", e);
            -1
        }
    }
}

/// Get network statistics (serialized to WASM memory)
fn consensus_get_network_stats<T>(_caller: Caller<'_, T>) -> i32 {
    match CONSENSUS_STATE.lock() {
        Ok(state) => {
            if let Some(ref consensus) = *state {
                let stats = NetworkStats::from_consensus_state(consensus);

                // In real implementation, serialize stats to WASM memory
                // For now, return success
                debug!("Network stats: {:?}", stats);
                0
            } else {
                -1
            }
        }
        Err(e) => {
            error!("Failed to lock consensus state: {}", e);
            -1
        }
    }
}

/// Query available work capacity in the network
fn consensus_query_work_capacity<T>(_caller: Caller<'_, T>, work_type: i32) -> i64 {
    debug!("Query work capacity for type: {}", work_type);

    match CONSENSUS_STATE.lock() {
        Ok(state) => {
            if let Some(ref _consensus) = *state {
                // Mock: return available capacity based on work type
                match work_type {
                    0 => 1000, // Compute work
                    1 => 500,  // Storage work
                    2 => 200,  // Network work
                    _ => 100,  // Custom work
                }
            } else {
                -1
            }
        }
        Err(e) => {
            error!("Failed to lock consensus state: {}", e);
            -1
        }
    }
}

/// Estimate work completion time in milliseconds
fn consensus_estimate_work_completion<T>(
    _caller: Caller<'_, T>,
    work_type: i32,
    work_size: i64,
    priority: i32
) -> i64 {
    debug!("Estimate completion for work type: {}, size: {}, priority: {}",
           work_type, work_size, priority);

    match CONSENSUS_STATE.lock() {
        Ok(state) => {
            if let Some(ref consensus) = *state {
                // Mock estimation based on network state
                let base_time = match work_type {
                    0 => work_size * 10,  // Compute: 10ms per unit
                    1 => work_size * 5,   // Storage: 5ms per unit
                    2 => work_size * 20,  // Network: 20ms per unit
                    _ => work_size * 15,  // Custom: 15ms per unit
                };

                // Adjust for priority (higher priority = faster)
                let priority_multiplier = match priority {
                    3 => 0.5, // Critical
                    2 => 0.7, // High
                    1 => 1.0, // Normal
                    _ => 1.5, // Low
                };

                // Adjust for network congestion
                let congestion_factor = if consensus.pending_work.len() > 10 {
                    1.5
                } else {
                    1.0
                };

                (base_time as f64 * priority_multiplier * congestion_factor) as i64
            } else {
                -1
            }
        }
        Err(e) => {
            error!("Failed to lock consensus state: {}", e);
            -1
        }
    }
}

/// Initialize consensus host functions
pub fn initialize_consensus_host() -> Result<()> {
    info!("Consensus host functions initialized");
    Ok(())
}

/// Get consensus diagnostics for debugging
pub fn get_consensus_diagnostics() -> Result<String> {
    match CONSENSUS_STATE.lock() {
        Ok(state) => {
            if let Some(ref consensus) = *state {
                Ok(format!(
                    "Consensus Diagnostics:\n\
                     - DAG Height: {}\n\
                     - Confirmed Blocks: {}\n\
                     - Pending Work: {}\n\
                     - Tips: {}\n\
                     - Main Chain Length: {}\n\
                     - Confidence Score: {:.2}%\n",
                    consensus.dag_height,
                    consensus.confirmed_blocks.len(),
                    consensus.pending_work.len(),
                    consensus.tips.len(),
                    consensus.main_chain.len(),
                    consensus.confidence_score() * 100.0
                ))
            } else {
                Ok("No consensus state available".to_string())
            }
        }
        Err(e) => {
            anyhow::bail!("Failed to get consensus diagnostics: {}", e)
        }
    }
}

/// Consensus event types for WASM transformer notifications
#[derive(Debug, Clone)]
pub enum ConsensusEvent {
    BlockConfirmed(String),
    WorkCompleted(String),
    NetworkPartition,
    NetworkHealed,
    HighCongestion,
    LowCongestion,
}

/// Consensus event handler for WASM transformers
pub struct ConsensusEventHandler {
    event_queue: Arc<Mutex<Vec<ConsensusEvent>>>,
}

impl Default for ConsensusEventHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsensusEventHandler {
    pub fn new() -> Self {
        Self {
            event_queue: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn emit_event(&self, event: ConsensusEvent) {
        if let Ok(mut queue) = self.event_queue.lock() {
            queue.push(event);
            // Keep only last 100 events
            if queue.len() > 100 {
                let queue_len = queue.len();
                queue.drain(0..queue_len - 100);
            }
        }
    }

    pub fn get_pending_events(&self) -> Vec<ConsensusEvent> {
        if let Ok(mut queue) = self.event_queue.lock() {
            let events = queue.clone();
            queue.clear();
            events
        } else {
            Vec::new()
        }
    }
}
