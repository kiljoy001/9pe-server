//! Unified Blockchain Mesh Networking Layer
//!
//! Integrates GhostDAG consensus with libp2p mesh networking for distributed file systems
//! Provides blockchain-backed file operations with p2p synchronization

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use tokio::sync::{RwLock, mpsc, Mutex};
use anyhow::Result;
use tracing::{info, warn, error, debug};
use serde::{Serialize, Deserialize};

use crate::ghostdag::{GhostDAG, Block, BlockHash, FileOperation, hash_to_string};
use crate::mesh::{MeshMessage, MeshNetwork, DiscoveredPeer};
use crate::auth::{AuthService, SignedCapability, Permissions};

/// Blockchain-backed p2p network state
pub struct BlockchainMesh {
    /// The underlying mesh network
    mesh: Arc<Mutex<MeshNetwork>>,

    /// GhostDAG consensus layer
    ghostdag: Arc<GhostDAG>,

    /// Auth service for capability verification
    auth_service: Arc<AuthService>,

    /// Discovered peers with their blockchain state
    peer_states: Arc<RwLock<HashMap<String, PeerBlockchainState>>>,

    /// File version tracking (path -> block hash)
    file_versions: Arc<RwLock<HashMap<String, BlockHash>>>,

    /// Pending operations to be mined
    pending_operations: Arc<RwLock<Vec<FileOperation>>>,

    /// Mining task handle
    mining_task: Option<tokio::task::JoinHandle<()>>,

    /// Block propagation channel
    block_sender: mpsc::UnboundedSender<Block>,
    block_receiver: Option<mpsc::UnboundedReceiver<Block>>,
}

/// Blockchain state of a peer
#[derive(Debug, Clone)]
pub struct PeerBlockchainState {
    pub peer_id: String,
    pub chain_height: u64,
    pub tip_hashes: Vec<BlockHash>,
    pub capabilities: Vec<String>,
    pub last_sync: SystemTime,
    pub trust_score: u32,  // 0-100 based on behavior
}

/// File version record on blockchain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileVersion {
    pub path: String,
    pub hash: String,
    pub block_hash: BlockHash,
    pub timestamp: u64,
    pub author: String,
    pub permissions: u32,
}

/// Unified message for blockchain mesh
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BlockchainMeshMessage {
    /// New block announcement
    BlockAnnouncement {
        block: Block,
        blue_score: u64,
        sender_id: String,
    },

    /// Request blocks from peer
    BlockRequest {
        from_height: u64,
        to_height: u64,
        sender_id: String,
    },

    /// Response with requested blocks
    BlockResponse {
        blocks: Vec<Block>,
        sender_id: String,
    },

    /// File operation with signature
    SignedFileOperation {
        operation: FileOperation,
        capability: SignedCapability,
        sender_id: String,
    },

    /// Consensus vote for block
    ConsensusVote {
        block_hash: BlockHash,
        vote: bool,
        voter_id: String,
        signature: Vec<u8>,
    },

    /// State synchronization
    StateSync {
        height: u64,
        tips: Vec<BlockHash>,
        file_count: usize,
        sender_id: String,
    },
}

impl BlockchainMesh {
    /// Create a new blockchain mesh network
    pub async fn new(
        mesh_port: u16,
        ghostdag_k: usize,
        auth_service: Arc<AuthService>,
    ) -> Result<Self> {
        info!("🔗 Initializing Blockchain Mesh Network");

        // Create mesh network
        let mesh = MeshNetwork::new(mesh_port).await?;

        // Initialize GhostDAG
        let ghostdag = Arc::new(GhostDAG::new(ghostdag_k));

        // Create block propagation channel
        let (block_sender, block_receiver) = mpsc::unbounded_channel();

        let mut network = Self {
            mesh: Arc::new(Mutex::new(mesh)),
            ghostdag,
            auth_service,
            peer_states: Arc::new(RwLock::new(HashMap::new())),
            file_versions: Arc::new(RwLock::new(HashMap::new())),
            pending_operations: Arc::new(RwLock::new(Vec::new())),
            mining_task: None,
            block_sender,
            block_receiver: Some(block_receiver),
        };

        // Start background tasks
        network.start_background_tasks().await?;

        Ok(network)
    }

    /// Start all background tasks
    async fn start_background_tasks(&mut self) -> Result<()> {
        // Start block propagation task
        self.start_block_propagation().await;

        // Start peer sync task
        self.start_peer_sync().await;

        // Start mining task
        self.start_mining().await;

        // Start consensus task
        self.start_consensus_protocol().await;

        Ok(())
    }

    /// Start block propagation to peers
    async fn start_block_propagation(&mut self) {
        let mesh = Arc::clone(&self.mesh);
        let ghostdag = Arc::clone(&self.ghostdag);
        let mut receiver = self.block_receiver.take().unwrap();

        tokio::spawn(async move {
            while let Some(block) = receiver.recv().await {
                info!("📡 Propagating block {} to mesh network", hash_to_string(&block.hash));

                // Calculate blue score
                let blue_score = ghostdag.compute_blue_score(&block.hash).await;

                // Create announcement message
                let message = MeshMessage::ConsensusMessage {
                    node_id: block.miner.clone(),
                    block_hash: hash_to_string(&block.hash),
                    parent_hashes: block.parent_hashes
                        .iter()
                        .map(|h| hash_to_string(h))
                        .collect(),
                    blue_score: blue_score as u64,
                };

                // Send to mesh network
                let mesh_guard = mesh.lock().await;
                if let Ok(sender) = mesh_guard.message_sender() {
                    if let Err(e) = sender.send(message) {
                        error!("Failed to propagate block: {}", e);
                    }
                }
            }
        });
    }

    /// Synchronize with peers periodically
    async fn start_peer_sync(&self) {
        let peer_states = Arc::clone(&self.peer_states);
        let ghostdag = Arc::clone(&self.ghostdag);
        let mesh = Arc::clone(&self.mesh);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));

            loop {
                interval.tick().await;

                // Get current chain state
                let tips = ghostdag.tips.read().await;
                let height = ghostdag.get_max_height().await;

                // Check each peer's state
                let peers = peer_states.read().await;
                for (peer_id, state) in peers.iter() {
                    if state.chain_height > height {
                        info!("🔄 Peer {} has higher chain ({}), requesting blocks",
                              peer_id, state.chain_height);

                        // Request missing blocks
                        let request = BlockchainMeshMessage::BlockRequest {
                            from_height: height + 1,
                            to_height: state.chain_height,
                            sender_id: "self".to_string(),
                        };

                        // Send request (would need to serialize and send via mesh)
                        debug!("Requesting blocks from {} to {}", height + 1, state.chain_height);
                    }
                }
            }
        });
    }

    /// Start mining pending operations
    async fn start_mining(&self) {
        let pending = Arc::clone(&self.pending_operations);
        let ghostdag = Arc::clone(&self.ghostdag);
        let block_sender = self.block_sender.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(10));

            loop {
                interval.tick().await;

                // Check if we have pending operations
                let mut ops = pending.write().await;
                if ops.is_empty() {
                    continue;
                }

                // Take operations to mine
                let operations_to_mine: Vec<_> = ops.drain(..ops.len().min(100)).collect();
                drop(ops);

                info!("⛏️ Mining block with {} operations", operations_to_mine.len());

                // Mine the block
                match mine_block_internal(&ghostdag, operations_to_mine).await {
                    Ok(block) => {
                        info!("✅ Mined block at height {}: {}",
                              block.height, hash_to_string(&block.hash));

                        // Propagate to network
                        if let Err(e) = block_sender.send(block) {
                            error!("Failed to propagate mined block: {}", e);
                        }
                    }
                    Err(e) => {
                        error!("Mining failed: {}", e);
                    }
                }
            }
        });
    }

    /// Start consensus protocol handler
    async fn start_consensus_protocol(&self) {
        let ghostdag = Arc::clone(&self.ghostdag);
        let peer_states = Arc::clone(&self.peer_states);

        tokio::spawn(async move {
            info!("🗳️ Starting consensus protocol handler");

            // This would handle:
            // 1. Block validation from peers
            // 2. Blue/Red set computation
            // 3. Fork resolution
            // 4. Consensus voting

            let mut interval = tokio::time::interval(Duration::from_secs(5));

            loop {
                interval.tick().await;

                // Check for conflicting tips
                let tips = ghostdag.tips.read().await;
                if tips.len() > 1 {
                    debug!("Multiple tips detected, running consensus");

                    // Run GhostDAG consensus to select preferred chain
                    for tip in tips.iter() {
                        let blue_score = ghostdag.compute_blue_score(tip).await;
                        debug!("Tip {} has blue score {}", hash_to_string(tip), blue_score);
                    }
                }
            }
        });
    }

    /// Submit a file operation to the blockchain
    pub async fn submit_file_operation(
        &self,
        operation: FileOperation,
        capability: Option<SignedCapability>,
    ) -> Result<()> {
        // Verify capability if provided
        if let Some(cap) = &capability {
            self.auth_service.verify_capability(cap).await?;
        }

        // Add to pending operations
        self.pending_operations.write().await.push(operation.clone());

        info!("📝 Submitted file operation: {:?}", operation.op_type);

        Ok(())
    }

    /// Get the current version of a file from blockchain
    pub async fn get_file_version(&self, path: &str) -> Option<FileVersion> {
        let versions = self.file_versions.read().await;

        if let Some(&block_hash) = versions.get(path) {
            // Get block from DAG
            let dag = self.ghostdag.dag.read().await;
            if let Some(node) = dag.get(&block_hash) {
                // Find the file operation in block
                for op in &node.block.operations {
                    if op.path == path {
                        return Some(FileVersion {
                            path: path.to_string(),
                            hash: op.content_hash.clone(),
                            block_hash,
                            timestamp: node.block.timestamp,
                            author: node.block.miner.clone(),
                            permissions: op.permissions,
                        });
                    }
                }
            }
        }

        None
    }

    /// Handle incoming blockchain mesh messages
    pub async fn handle_message(&self, message: BlockchainMeshMessage) -> Result<()> {
        match message {
            BlockchainMeshMessage::BlockAnnouncement { block, blue_score, sender_id } => {
                info!("📦 Received block from {}: height={}, blue_score={}",
                      sender_id, block.height, blue_score);

                // Validate and add block
                if block.verify() {
                    self.ghostdag.add_block(block).await?;

                    // Update peer state
                    let mut states = self.peer_states.write().await;
                    if let Some(state) = states.get_mut(&sender_id) {
                        state.chain_height = state.chain_height.max(block.height);
                        state.last_sync = SystemTime::now();
                    }
                } else {
                    warn!("Invalid block from {}", sender_id);
                }
            }

            BlockchainMeshMessage::BlockRequest { from_height, to_height, sender_id } => {
                info!("📨 Block request from {} for heights {}-{}",
                      sender_id, from_height, to_height);

                // Gather requested blocks
                let blocks = self.ghostdag.get_blocks_range(from_height, to_height).await;

                // Send response
                let response = BlockchainMeshMessage::BlockResponse {
                    blocks,
                    sender_id: "self".to_string(),
                };

                // Would send via mesh network
                debug!("Sending {} blocks to {}", response.blocks.len(), sender_id);
            }

            BlockchainMeshMessage::SignedFileOperation { operation, capability, sender_id } => {
                info!("📄 File operation from {}: {:?}", sender_id, operation.op_type);

                // Verify capability
                if self.auth_service.verify_capability(&capability).await.is_ok() {
                    // Add to pending operations
                    self.pending_operations.write().await.push(operation);
                } else {
                    warn!("Invalid capability from {}", sender_id);
                }
            }

            _ => {
                debug!("Unhandled message type");
            }
        }

        Ok(())
    }

    /// Get blockchain statistics
    pub async fn get_stats(&self) -> BlockchainStats {
        let dag = self.ghostdag.dag.read().await;
        let tips = self.ghostdag.tips.read().await;
        let peers = self.peer_states.read().await;
        let versions = self.file_versions.read().await;
        let pending = self.pending_operations.read().await;

        BlockchainStats {
            total_blocks: dag.len(),
            chain_height: self.ghostdag.get_max_height().await,
            active_tips: tips.len(),
            connected_peers: peers.len(),
            tracked_files: versions.len(),
            pending_operations: pending.len(),
            consensus_k: self.ghostdag.k,
        }
    }
}

/// Blockchain network statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainStats {
    pub total_blocks: usize,
    pub chain_height: u64,
    pub active_tips: usize,
    pub connected_peers: usize,
    pub tracked_files: usize,
    pub pending_operations: usize,
    pub consensus_k: usize,
}

/// Internal mining function
async fn mine_block_internal(
    ghostdag: &Arc<GhostDAG>,
    operations: Vec<FileOperation>,
) -> Result<Block> {
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
        miner: whoami::username(),
        nonce: 0,
        difficulty,
    };

    // Mine (find nonce that meets difficulty)
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

    // Add to DAG
    ghostdag.add_block(block.clone()).await?;

    Ok(block)
}

/// Integration point for mesh network events
pub async fn handle_mesh_event(
    blockchain: &BlockchainMesh,
    message: MeshMessage,
) -> Result<()> {
    match message {
        MeshMessage::ConsensusMessage { node_id, block_hash, parent_hashes, blue_score } => {
            // Convert to blockchain message
            info!("🔗 Received consensus message from {}", node_id);

            // Update peer state
            let mut states = blockchain.peer_states.write().await;
            states.entry(node_id.clone()).or_insert(PeerBlockchainState {
                peer_id: node_id.clone(),
                chain_height: 0,
                tip_hashes: vec![],
                capabilities: vec![],
                last_sync: SystemTime::now(),
                trust_score: 50,
            });
        }

        MeshMessage::FileSystemEvent { node_id, path, operation, timestamp } => {
            // Convert to file operation
            let file_op = match operation.as_str() {
                "create" => crate::ghostdag::FileOperation::Create {
                    path: path.clone(),
                    content: vec![], // Empty content for events
                },
                "delete" => crate::ghostdag::FileOperation::Delete {
                    path: path.clone(),
                },
                "modify" | "write" => crate::ghostdag::FileOperation::Modify {
                    path: path.clone(),
                    content: vec![], // Empty content for events
                },
                _ => crate::ghostdag::FileOperation::Create {
                    path: path.clone(),
                    content: vec![],
                },
            };

            // Submit to blockchain
            blockchain.submit_file_operation(file_op, None).await?;
        }

        _ => {
            debug!("Other mesh message: {:?}", message);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_blockchain_mesh_creation() {
        let auth = Arc::new(AuthService::new());
        let mesh = BlockchainMesh::new(9650, 3, auth).await;
        assert!(mesh.is_ok());
    }

    #[tokio::test]
    async fn test_file_operation_submission() {
        let auth = Arc::new(AuthService::new());
        let mesh = BlockchainMesh::new(9651, 3, auth).await.unwrap();

        let op = FileOperation {
            op_type: crate::ghostdag::OperationType::Create,
            path: "/test/file.txt".to_string(),
            content_hash: "hash123".to_string(),
            permissions: 0o644,
            owner: "test".to_string(),
            timestamp: 0,
        };

        let result = mesh.submit_file_operation(op, None).await;
        assert!(result.is_ok());
    }
}