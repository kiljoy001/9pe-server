//! Global Event Chain - Single Source of Truth for Distributed Events
//!
//! A simplified blockchain that maintains a single, globally-agreed sequence of events
//! Uses GhostDAG for consensus but focuses on event ordering, not state preservation

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{RwLock, mpsc};
use anyhow::Result;
use tracing::{info, warn, debug};
use serde::{Serialize, Deserialize};

use crate::ghostdag::{GhostDAG, Block, BlockHash, hash_to_string};
use crate::mesh::{MeshMessage, MeshNetwork};

/// Global event that needs ordering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GlobalEvent {
    /// File operation occurred
    FileEvent {
        node_id: String,
        path: String,
        operation: String,  // create, write, delete, read
        hash: String,       // Content hash for verification
        timestamp: u64,
    },

    /// Node joined/left network
    NodeEvent {
        node_id: String,
        action: String,     // join, leave, update
        address: String,
        timestamp: u64,
    },

    /// Permission change
    PermissionEvent {
        path: String,
        granted_to: String,
        permissions: u32,
        timestamp: u64,
    },

    /// Generic event for extensibility
    CustomEvent {
        event_type: String,
        data: Vec<u8>,
        timestamp: u64,
    },
}

/// Simplified event block - just events and ordering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventBlock {
    pub hash: BlockHash,
    pub parent_hash: BlockHash,  // Single parent for simple chain
    pub height: u64,
    pub events: Vec<GlobalEvent>,
    pub timestamp: u64,
    pub proposer: String,
    pub nonce: u64,
}

/// Global Event Chain - maintains single ordered history
pub struct GlobalEventChain {
    /// GhostDAG for consensus (but we only use the main chain)
    ghostdag: Arc<GhostDAG>,

    /// The single chain of events (height -> block)
    chain: Arc<RwLock<Vec<EventBlock>>>,

    /// Current chain tip
    tip: Arc<RwLock<BlockHash>>,

    /// Event buffer for batching
    event_buffer: Arc<RwLock<Vec<GlobalEvent>>>,

    /// Recent events cache (only last 1k events)
    recent_events: Arc<RwLock<Vec<GlobalEvent>>>,

    /// Maximum events to keep in memory
    max_events: usize,

    /// Mesh network for propagation
    mesh: Option<Arc<MeshNetwork>>,

    /// Event stream for applications
    event_sender: mpsc::UnboundedSender<GlobalEvent>,
    event_receiver: Option<mpsc::UnboundedReceiver<GlobalEvent>>,
}

impl EventBlock {
    /// Create genesis block
    pub fn genesis() -> Self {
        let mut block = Self {
            hash: [0; 32],
            parent_hash: [0; 32],
            height: 0,
            events: vec![],
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            proposer: "genesis".to_string(),
            nonce: 0,
        };
        block.hash = block.compute_hash();
        block
    }

    /// Compute block hash
    pub fn compute_hash(&self) -> BlockHash {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();

        hasher.update(&self.parent_hash);
        hasher.update(self.height.to_le_bytes());
        hasher.update(self.timestamp.to_le_bytes());
        hasher.update(self.proposer.as_bytes());
        hasher.update(self.nonce.to_le_bytes());

        // Hash events
        for event in &self.events {
            let event_bytes = bincode::serialize(event).unwrap_or_default();
            hasher.update(&event_bytes);
        }

        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        hash
    }

    /// Simple proof of work
    pub fn mine(&mut self, difficulty: u32) {
        while !self.meets_difficulty(difficulty) {
            self.nonce += 1;
            self.hash = self.compute_hash();
        }
    }

    /// Check if hash meets difficulty
    fn meets_difficulty(&self, difficulty: u32) -> bool {
        let zeros = difficulty / 8;
        let remainder = difficulty % 8;

        for i in 0..zeros as usize {
            if self.hash[i] != 0 {
                return false;
            }
        }

        if remainder > 0 && zeros < 32 {
            let mask = 0xFF >> remainder;
            if self.hash[zeros as usize] & !mask != 0 {
                return false;
            }
        }

        true
    }
}

impl GlobalEventChain {
    /// Create new event chain (keeps only last 1k events)
    pub async fn new(mesh: Option<Arc<MeshNetwork>>) -> Result<Self> {
        info!("🌍 Initializing Global Event Chain (1k event limit)");

        // Create simple GhostDAG with k=1 (single parent)
        let ghostdag = Arc::new(GhostDAG::new(1));

        // Create genesis block
        let genesis = EventBlock::genesis();
        let genesis_hash = genesis.hash;

        // Initialize chain with limited history
        let chain = Arc::new(RwLock::new(vec![genesis]));

        // Create event channel
        let (event_sender, event_receiver) = mpsc::unbounded_channel();

        let mut event_chain = Self {
            ghostdag,
            chain,
            tip: Arc::new(RwLock::new(genesis_hash)),
            event_buffer: Arc::new(RwLock::new(Vec::new())),
            recent_events: Arc::new(RwLock::new(Vec::new())),
            max_events: 1000,  // Only keep last 1k events
            mesh,
            event_sender,
            event_receiver: Some(event_receiver),
        };

        // Start event processing
        event_chain.start_event_processor().await;

        Ok(event_chain)
    }

    /// Submit an event to the global chain
    pub async fn submit_event(&self, event: GlobalEvent) -> Result<()> {
        // Add to buffer
        self.event_buffer.write().await.push(event.clone());

        // Send to stream
        let _ = self.event_sender.send(event);

        // If buffer is large enough, create block
        if self.event_buffer.read().await.len() >= 10 {
            self.create_block().await?;
        }

        Ok(())
    }

    /// Create a new block from buffered events
    async fn create_block(&self) -> Result<()> {
        let mut buffer = self.event_buffer.write().await;
        if buffer.is_empty() {
            return Ok(());
        }

        // Take all events
        let events: Vec<_> = buffer.drain(..).collect();
        drop(buffer);

        // Get current tip
        let parent_hash = *self.tip.read().await;
        let chain = self.chain.read().await;
        let parent_height = chain.len() as u64 - 1;

        // Create new block
        let mut block = EventBlock {
            hash: [0; 32],
            parent_hash,
            height: parent_height + 1,
            events,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)?
                .as_secs(),
            proposer: whoami::username(),
            nonce: 0,
        };

        // Mine with low difficulty (just for ordering)
        block.mine(8); // Very easy difficulty

        info!("⛓️ Created block {} at height {} with {} events",
              hash_to_string(&block.hash), block.height, block.events.len());

        // Add to chain
        self.add_block(block.clone()).await?;

        // Propagate to network
        self.propagate_block(block).await?;

        Ok(())
    }

    /// Add block to chain (after validation) - maintains 1k event limit
    async fn add_block(&self, block: EventBlock) -> Result<()> {
        // Validate parent
        let tip = self.tip.read().await;
        if block.parent_hash != *tip {
            warn!("Block has wrong parent, rejecting");
            return Ok(()); // Not an error, just ignore
        }

        // Add to chain
        let mut chain = self.chain.write().await;
        chain.push(block.clone());

        // Prune old blocks to keep memory bounded
        // Keep roughly last 100 blocks (assuming ~10 events per block = 1k events)
        if chain.len() > 100 {
            let remove_count = chain.len() - 100;
            chain.drain(0..remove_count);
            debug!("Pruned {} old blocks", remove_count);
        }

        // Update tip
        drop(tip);
        *self.tip.write().await = block.hash;

        info!("✅ Added block {} at height {} ({} events)",
              hash_to_string(&block.hash)[0..8].to_string(),
              block.height,
              block.events.len());

        // Update recent events cache (maintain 1k limit)
        let mut recent = self.recent_events.write().await;
        for event in block.events.clone() {
            recent.push(event.clone());
            let _ = self.event_sender.send(event);
        }

        // Prune if over 1k events
        if recent.len() > self.max_events {
            let remove_count = recent.len() - self.max_events;
            recent.drain(0..remove_count);
            debug!("Pruned {} old events from cache", remove_count);
        }

        Ok(())
    }

    /// Propagate block to mesh network
    async fn propagate_block(&self, block: EventBlock) -> Result<()> {
        if let Some(mesh) = &self.mesh {
            let message = MeshMessage::ConsensusMessage {
                node_id: block.proposer.clone(),
                block_hash: hash_to_string(&block.hash),
                parent_hashes: vec![hash_to_string(&block.parent_hash)],
                blue_score: block.height,
            };

            // Send to mesh (would need proper integration)
            debug!("Propagating block {} to mesh", hash_to_string(&block.hash));
        }

        Ok(())
    }

    /// Start background event processor
    async fn start_event_processor(&mut self) {
        let buffer = Arc::clone(&self.event_buffer);
        let chain = Arc::clone(&self.chain);
        let tip = Arc::clone(&self.tip);
        let recent_events = Arc::clone(&self.recent_events);
        let event_sender = self.event_sender.clone();
        let max_events = self.max_events;

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));

            loop {
                interval.tick().await;

                // Create block if we have events
                if !buffer.read().await.is_empty() {
                    // Create block manually without the full chain object
                    let mut event_buffer = buffer.write().await;
                    if !event_buffer.is_empty() {
                        let events: Vec<_> = event_buffer.drain(..).collect();
                        drop(event_buffer);

                        // Get current tip
                        let parent_hash = *tip.read().await;
                        let chain_guard = chain.read().await;
                        let parent_height = chain_guard.len() as u64 - 1;
                        drop(chain_guard);

                        // Create new block
                        let mut block = EventBlock {
                            hash: [0; 32],
                            parent_hash,
                            height: parent_height + 1,
                            events: events.clone(),
                            timestamp: SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap()
                                .as_secs(),
                            proposer: whoami::username(),
                            nonce: 0,
                        };

                        // Mine with low difficulty
                        block.mine(8);

                        info!("⛓️ Created block {} at height {} with {} events",
                              hash_to_string(&block.hash), block.height, block.events.len());

                        // Add to chain
                        let mut chain_guard = chain.write().await;
                        chain_guard.push(block.clone());

                        // Prune old blocks
                        if chain_guard.len() > 100 {
                            let remove_count = chain_guard.len() - 100;
                            chain_guard.drain(0..remove_count);
                            debug!("Pruned {} old blocks", remove_count);
                        }
                        drop(chain_guard);

                        // Update tip
                        *tip.write().await = block.hash;

                        // Update recent events cache
                        let mut recent = recent_events.write().await;
                        for event in block.events.clone() {
                            recent.push(event.clone());
                            let _ = event_sender.send(event);
                        }

                        // Prune if over max events
                        if recent.len() > max_events {
                            let remove_count = recent.len() - max_events;
                            recent.drain(0..remove_count);
                        }
                    }
                }
            }
        });
    }

    /// Get event stream receiver
    pub fn take_event_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<GlobalEvent>> {
        self.event_receiver.take()
    }

    /// Get the current chain height
    pub async fn get_height(&self) -> u64 {
        self.chain.read().await.len() as u64 - 1
    }

    /// Get recent events (up to last 1k)
    pub async fn get_recent_events(&self, count: usize) -> Vec<GlobalEvent> {
        let recent = self.recent_events.read().await;
        let total = recent.len();

        if count >= total {
            recent.clone()
        } else {
            recent[total - count..].to_vec()
        }
    }

    /// Get events for a specific path (from recent cache only)
    pub async fn get_path_events(&self, path: &str) -> Vec<GlobalEvent> {
        let recent = self.recent_events.read().await;
        let mut events = Vec::new();

        for event in recent.iter() {
            if let GlobalEvent::FileEvent { path: event_path, .. } = event {
                if event_path == path {
                    events.push(event.clone());
                }
            }
        }

        events
    }

    /// Get available event range (since we prune old events)
    pub async fn get_event_window(&self) -> (usize, usize) {
        let recent = self.recent_events.read().await;
        (recent.len().saturating_sub(self.max_events), recent.len())
    }

    /// Handle incoming block from network
    pub async fn handle_network_block(&self, block_data: Vec<u8>) -> Result<()> {
        // Deserialize block
        let block: EventBlock = bincode::deserialize(&block_data)?;

        info!("📦 Received network block at height {}", block.height);

        // Validate and add
        self.add_block(block).await?;

        Ok(())
    }

}

/// Simple integration with file operations
pub async fn track_file_operation(
    chain: &GlobalEventChain,
    path: String,
    operation: String,
    hash: String,
) -> Result<()> {
    let event = GlobalEvent::FileEvent {
        node_id: whoami::username(),
        path,
        operation,
        hash,
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs(),
    };

    chain.submit_event(event).await?;
    Ok(())
}

/// Get chain statistics (reflects only cached events)
pub async fn get_chain_stats(chain: &GlobalEventChain) -> ChainStats {
    let height = chain.get_height().await;
    let chain_data = chain.chain.read().await;
    let recent = chain.recent_events.read().await;

    let mut file_events = 0;
    let mut node_events = 0;

    // Count only recent events (last 1k)
    for event in recent.iter() {
        match event {
            GlobalEvent::FileEvent { .. } => file_events += 1,
            GlobalEvent::NodeEvent { .. } => node_events += 1,
            _ => {}
        }
    }

    ChainStats {
        height,
        total_blocks: chain_data.len(),
        cached_events: recent.len(),  // Changed from total_events
        file_events,
        node_events,
        avg_block_size: if chain_data.len() > 1 {
            recent.len() / chain_data.len().min(100) // Average over cached blocks
        } else {
            0
        },
        max_events: chain.max_events,  // Add max limit to stats
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainStats {
    pub height: u64,
    pub total_blocks: usize,
    pub cached_events: usize,  // Only last 1k events
    pub file_events: usize,
    pub node_events: usize,
    pub avg_block_size: usize,
    pub max_events: usize,     // Always 1000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_event_chain_creation() {
        let chain = GlobalEventChain::new(None).await;
        assert!(chain.is_ok());
    }

    #[tokio::test]
    async fn test_event_submission() {
        let chain = GlobalEventChain::new(None).await.unwrap();

        let event = GlobalEvent::FileEvent {
            node_id: "test".to_string(),
            path: "/test.txt".to_string(),
            operation: "create".to_string(),
            hash: "abc123".to_string(),
            timestamp: 0,
        };

        let result = chain.submit_event(event).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_block_mining() {
        let mut block = EventBlock::genesis();
        block.mine(8); // Easy difficulty
        assert!(block.meets_difficulty(8));
    }
}