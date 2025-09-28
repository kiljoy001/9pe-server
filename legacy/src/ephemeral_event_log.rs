//! Ephemeral Event Log - Gossip-based event ordering without persistent blockchain
//!
//! Events are gossiped through the network with a rolling window.
//! New nodes joining get the current epoch via gossip catchup.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use tokio::sync::RwLock;
use anyhow::Result;
use tracing::{info, debug, warn};
use serde::{Serialize, Deserialize};
use sha2::{Sha256, Digest};

use crate::mesh::{MeshMessage, MeshNetwork};

/// Maximum events to keep in memory (last 1k)
const MAX_EVENTS: usize = 1000;

/// Epoch duration (events older than this are forgotten)
const EPOCH_DURATION_SECS: u64 = 3600; // 1 hour epochs

/// Event with ordering information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderedEvent {
    pub event: GlobalEvent,
    pub sequence: u64,      // Global sequence number
    pub timestamp: u64,      // Unix timestamp
    pub node_id: String,     // Node that created event
    pub hash: [u8; 32],      // Event hash for integrity
}

/// Global event types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GlobalEvent {
    FileOperation {
        path: String,
        operation: String,
        hash: String,
    },
    NodeJoined {
        node_id: String,
        address: String,
    },
    NodeLeft {
        node_id: String,
    },
    PermissionChange {
        path: String,
        permissions: u32,
    },
}

/// Current epoch snapshot for new joiners
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpochSnapshot {
    pub epoch_number: u64,
    pub start_time: u64,
    pub events: Vec<OrderedEvent>,
    pub node_states: Vec<(String, String)>, // (node_id, address)
}

/// Ephemeral event log - no persistent storage
pub struct EphemeralEventLog {
    /// Rolling window of recent events (max 1k)
    events: Arc<RwLock<VecDeque<OrderedEvent>>>,

    /// Current epoch number
    epoch: Arc<RwLock<u64>>,

    /// Last epoch start time
    epoch_start: Arc<RwLock<u64>>,

    /// Global sequence counter
    sequence: Arc<RwLock<u64>>,

    /// Known nodes in current epoch
    active_nodes: Arc<RwLock<Vec<(String, String)>>>,

    /// Mesh network for gossip
    mesh: Option<Arc<MeshNetwork>>,

    /// Our node ID
    node_id: String,
}

impl OrderedEvent {
    /// Create a new ordered event
    pub fn new(event: GlobalEvent, sequence: u64, node_id: String) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut evt = Self {
            event,
            sequence,
            timestamp,
            node_id,
            hash: [0; 32],
        };

        evt.hash = evt.compute_hash();
        evt
    }

    /// Compute event hash
    fn compute_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.sequence.to_le_bytes());
        hasher.update(self.timestamp.to_le_bytes());
        hasher.update(self.node_id.as_bytes());

        // Hash event data
        let event_bytes = bincode::serialize(&self.event).unwrap_or_default();
        hasher.update(&event_bytes);

        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        hash
    }

    /// Verify event integrity
    pub fn verify(&self) -> bool {
        self.hash == self.compute_hash()
    }
}

impl EphemeralEventLog {
    /// Create new ephemeral event log
    pub async fn new(node_id: String, mesh: Option<Arc<MeshNetwork>>) -> Result<Self> {
        info!("📝 Initializing Ephemeral Event Log (no persistent state)");

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs();

        let log = Self {
            events: Arc::new(RwLock::new(VecDeque::with_capacity(MAX_EVENTS))),
            epoch: Arc::new(RwLock::new(1)),
            epoch_start: Arc::new(RwLock::new(now)),
            sequence: Arc::new(RwLock::new(0)),
            active_nodes: Arc::new(RwLock::new(Vec::new())),
            mesh,
            node_id,
        };

        // Start epoch rotation
        log.start_epoch_rotation().await;

        // Request current epoch from network
        log.request_epoch_sync().await?;

        Ok(log)
    }

    /// Submit an event to the log
    pub async fn submit_event(&self, event: GlobalEvent) -> Result<()> {
        // Get next sequence number
        let sequence = {
            let mut seq = self.sequence.write().await;
            *seq += 1;
            *seq
        };

        // Create ordered event
        let ordered = OrderedEvent::new(event, sequence, self.node_id.clone());

        // Add to local log
        self.add_event(ordered.clone()).await?;

        // Gossip to network
        self.gossip_event(ordered).await?;

        Ok(())
    }

    /// Add event to local log (maintains 1k limit)
    async fn add_event(&self, event: OrderedEvent) -> Result<()> {
        let mut events = self.events.write().await;

        // Add event
        events.push_back(event.clone());

        // Maintain limit
        while events.len() > MAX_EVENTS {
            events.pop_front();
        }

        debug!("Event {} added (cache size: {})", event.sequence, events.len());

        Ok(())
    }

    /// Gossip event to network
    async fn gossip_event(&self, event: OrderedEvent) -> Result<()> {
        if let Some(_mesh) = &self.mesh {
            let _message = MeshMessage::FileSystemEvent {
                node_id: event.node_id.clone(),
                path: match &event.event {
                    GlobalEvent::FileOperation { path, .. } => path.clone(),
                    _ => String::new(),
                },
                operation: format!("{:?}", event.event),
                timestamp: event.timestamp,
            };

            // Would send via mesh
            debug!("Gossiping event {} to network", event.sequence);
        }

        Ok(())
    }

    /// Request epoch sync when joining network
    async fn request_epoch_sync(&self) -> Result<()> {
        info!("🔄 Requesting current epoch from network");

        // In a real implementation, this would:
        // 1. Broadcast epoch sync request via gossip
        // 2. Receive current epoch from peers
        // 3. Validate and apply epoch snapshot

        Ok(())
    }

    /// Handle epoch sync request from new joiner
    pub async fn handle_sync_request(&self, requester: String) -> Result<EpochSnapshot> {
        let events = self.events.read().await;
        let epoch = *self.epoch.read().await;
        let epoch_start = *self.epoch_start.read().await;
        let nodes = self.active_nodes.read().await;

        let snapshot = EpochSnapshot {
            epoch_number: epoch,
            start_time: epoch_start,
            events: events.iter().cloned().collect(),
            node_states: nodes.clone(),
        };

        info!("📤 Sending epoch {} snapshot to {}", epoch, requester);

        Ok(snapshot)
    }

    /// Apply epoch snapshot when joining
    pub async fn apply_epoch_snapshot(&self, snapshot: EpochSnapshot) -> Result<()> {
        info!("📥 Applying epoch {} snapshot ({} events)",
              snapshot.epoch_number, snapshot.events.len());

        // Replace our state with snapshot
        *self.epoch.write().await = snapshot.epoch_number;
        *self.epoch_start.write().await = snapshot.start_time;

        // Load events (respect 1k limit)
        let mut events = self.events.write().await;
        events.clear();
        for event in snapshot.events.into_iter().take(MAX_EVENTS) {
            if event.verify() {
                events.push_back(event);
            } else {
                warn!("Invalid event in snapshot, skipping");
            }
        }

        // Update sequence to continue from snapshot
        if let Some(last) = events.back() {
            *self.sequence.write().await = last.sequence;
        }

        // Update known nodes
        *self.active_nodes.write().await = snapshot.node_states;

        info!("✅ Epoch snapshot applied successfully");

        Ok(())
    }

    /// Start epoch rotation task
    async fn start_epoch_rotation(&self) {
        let events = Arc::clone(&self.events);
        let epoch = Arc::clone(&self.epoch);
        let epoch_start = Arc::clone(&self.epoch_start);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));

            loop {
                interval.tick().await;

                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();

                let start = *epoch_start.read().await;

                // Check if epoch should rotate
                if now - start > EPOCH_DURATION_SECS {
                    let mut ep = epoch.write().await;
                    *ep += 1;

                    let mut es = epoch_start.write().await;
                    *es = now;

                    // Clear old events from previous epoch
                    let mut evts = events.write().await;
                    evts.clear();

                    info!("🔄 Epoch rotated to {}", *ep);
                }
            }
        });
    }

    /// Get recent events
    pub async fn get_recent_events(&self, count: usize) -> Vec<OrderedEvent> {
        let events = self.events.read().await;
        events.iter()
            .rev()
            .take(count)
            .cloned()
            .collect()
    }

    /// Get current stats
    pub async fn get_stats(&self) -> EphemeralStats {
        let events = self.events.read().await;
        let epoch = *self.epoch.read().await;
        let nodes = self.active_nodes.read().await;

        EphemeralStats {
            current_epoch: epoch,
            cached_events: events.len(),
            active_nodes: nodes.len(),
            max_events: MAX_EVENTS,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EphemeralStats {
    pub current_epoch: u64,
    pub cached_events: usize,
    pub active_nodes: usize,
    pub max_events: usize,
}

/// Handle incoming gossip message
pub async fn handle_gossip_message(
    log: &EphemeralEventLog,
    event: OrderedEvent,
) -> Result<()> {
    // Verify event
    if !event.verify() {
        warn!("Invalid gossip event received");
        return Ok(());
    }

    // Check if we already have it (deduplication)
    let events = log.events.read().await;
    for existing in events.iter() {
        if existing.hash == event.hash {
            debug!("Duplicate event {}, ignoring", event.sequence);
            return Ok(());
        }
    }
    drop(events);

    // Add to our log
    log.add_event(event).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ephemeral_log() {
        let log = EphemeralEventLog::new("test".to_string(), None).await.unwrap();

        let event = GlobalEvent::FileOperation {
            path: "/test.txt".to_string(),
            operation: "create".to_string(),
            hash: "abc123".to_string(),
        };

        log.submit_event(event).await.unwrap();

        let recent = log.get_recent_events(10).await;
        assert_eq!(recent.len(), 1);
    }

    #[tokio::test]
    async fn test_event_limit() {
        let log = EphemeralEventLog::new("test".to_string(), None).await.unwrap();

        // Add more than MAX_EVENTS
        for i in 0..1100 {
            let event = GlobalEvent::FileOperation {
                path: format!("/file{}.txt", i),
                operation: "create".to_string(),
                hash: format!("hash{}", i),
            };
            log.submit_event(event).await.unwrap();
        }

        // Should only keep last 1000
        let stats = log.get_stats().await;
        assert_eq!(stats.cached_events, 1000);
    }
}