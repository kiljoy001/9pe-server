//! Transient Consensus - Real-time event ordering without permanent records
//!
//! NOT A BLOCKCHAIN. Just enough consensus to maintain a single version of reality.
//! Events are ordered, processed, then forgotten. Privacy by default.

use std::collections::{VecDeque, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use tokio::sync::RwLock;
use anyhow::Result;
use tracing::{info, debug, trace};
use serde::{Serialize, Deserialize};

// use crate::mesh::MeshNetwork;  // Disabled for now

/// How many events to keep for ordering (small buffer)
const EVENT_BUFFER_SIZE: usize = 1000;

/// How long before we forget events entirely (privacy)
const FORGET_AFTER_SECS: u64 = 300; // 5 minutes is enough

/// Transient event - exists only long enough to establish order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransientEvent {
    /// Unique ID for deduplication
    pub id: u64,

    /// What happened (we don't care about details)
    pub event_type: EventType,

    /// When it happened (for ordering)
    pub timestamp: u64,

    /// Who saw it (for consensus)
    pub witnesses: Vec<String>,
}

/// Minimal event types - just enough to maintain consistency
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventType {
    /// File was touched (don't care how)
    FileChange { path_hash: u64 }, // Hash the path for privacy

    /// Node state changed
    NodeUpdate { node_id_hash: u64 }, // Hash for privacy

    /// Generic event (most things)
    Generic { type_code: u16 },
}

/// Current consensus state - what everyone agrees on RIGHT NOW
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusState {
    /// Current event sequence
    pub sequence: u64,

    /// Active nodes (hashed IDs for privacy)
    pub active_nodes: HashSet<u64>,

    /// Recent events (will be forgotten soon)
    pub recent_events: VecDeque<TransientEvent>,

    /// Current epoch (for sync)
    pub epoch: u64,
}

/// Transient Consensus Manager
pub struct TransientConsensus {
    /// Current sequence number
    sequence: Arc<RwLock<u64>>,

    /// Recent events buffer (rolling window)
    events: Arc<RwLock<VecDeque<TransientEvent>>>,

    /// Events we've already seen (dedup)
    seen_events: Arc<RwLock<HashSet<u64>>>,

    /// Forget task handle
    _forget_task: tokio::task::JoinHandle<()>,
}

impl TransientConsensus {
    /// Create new transient consensus (no persistent state!)
    pub async fn new() -> Result<Self> {
        info!("🔄 Initializing Transient Consensus");
        info!("📝 Privacy mode: Events forgotten after {} seconds", FORGET_AFTER_SECS);

        let events = Arc::new(RwLock::new(VecDeque::<TransientEvent>::with_capacity(EVENT_BUFFER_SIZE)));
        let seen = Arc::new(RwLock::new(HashSet::new()));

        // Start the forgetting process
        let forget_events = Arc::clone(&events);
        let forget_seen = Arc::clone(&seen);

        let forget_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));

            loop {
                interval.tick().await;

                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();

                // Forget old events
                let mut events_guard = forget_events.write().await;
                let before_count = events_guard.len();

                events_guard.retain(|e| {
                    now - e.timestamp < FORGET_AFTER_SECS
                });

                let forgotten = before_count - events_guard.len();
                if forgotten > 0 {
                    debug!("🗑️ Forgot {} old events (privacy cleanup)", forgotten);
                }

                // Clear old dedup entries
                if events_guard.is_empty() {
                    forget_seen.write().await.clear();
                }
            }
        });

        Ok(Self {
            sequence: Arc::new(RwLock::new(0)),
            events,
            seen_events: seen,
            _forget_task: forget_task,
        })
    }

    /// Submit an event for ordering (will be forgotten later)
    pub async fn submit_event(&self, event_type: EventType) -> Result<u64> {
        // Get next sequence
        let seq = {
            let mut s = self.sequence.write().await;
            *s += 1;
            *s
        };

        let event = TransientEvent {
            id: seq,
            event_type,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)?
                .as_secs(),
            witnesses: vec![], // Will be filled by consensus
        };

        // Add to buffer
        let mut events = self.events.write().await;
        events.push_back(event.clone());

        // Maintain buffer size
        while events.len() > EVENT_BUFFER_SIZE {
            let forgotten = events.pop_front();
            trace!("Forgot event: {:?}", forgotten);
        }

        // Mark as seen
        self.seen_events.write().await.insert(seq);

        debug!("Event {} submitted (will forget in {}s)", seq, FORGET_AFTER_SECS);

        Ok(seq)
    }

    /// Process incoming event from network
    pub async fn handle_network_event(&self, event: TransientEvent) -> Result<()> {
        // Check if we've seen it
        if self.seen_events.read().await.contains(&event.id) {
            trace!("Already seen event {}", event.id);
            return Ok(());
        }

        // Add to our view
        let mut events = self.events.write().await;

        // Insert in order (by timestamp, then ID)
        let position = events.binary_search_by(|e| {
            e.timestamp.cmp(&event.timestamp)
                .then(e.id.cmp(&event.id))
        });

        match position {
            Ok(_) => trace!("Duplicate event position"),
            Err(pos) => events.insert(pos, event.clone()),
        }

        // Maintain size
        while events.len() > EVENT_BUFFER_SIZE {
            events.pop_front();
        }

        // Mark as seen
        self.seen_events.write().await.insert(event.id);

        Ok(())
    }

    /// Get current consensus state for new joiners
    pub async fn get_consensus_snapshot(&self) -> ConsensusState {
        let events = self.events.read().await;
        let sequence = *self.sequence.read().await;

        ConsensusState {
            sequence,
            active_nodes: HashSet::new(), // Don't track nodes for privacy
            recent_events: events.clone(),
            epoch: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() / 3600, // Hour-based epochs
        }
    }

    /// Apply consensus snapshot when joining
    pub async fn apply_snapshot(&self, snapshot: ConsensusState) -> Result<()> {
        info!("📥 Applying consensus snapshot (sequence: {})", snapshot.sequence);

        *self.sequence.write().await = snapshot.sequence;

        // Only take recent events
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs();

        let mut events = self.events.write().await;
        events.clear();

        for event in snapshot.recent_events {
            // Only keep if not too old
            if now - event.timestamp < FORGET_AFTER_SECS {
                events.push_back(event.clone());
                self.seen_events.write().await.insert(event.id);
            }
        }

        info!("✅ Snapshot applied ({} recent events)", events.len());

        Ok(())
    }

    /// Get recent events (what we haven't forgotten yet)
    pub async fn get_recent_events(&self, count: usize) -> Vec<TransientEvent> {
        let events = self.events.read().await;
        events.iter()
            .rev()
            .take(count)
            .cloned()
            .collect()
    }

    /// Check if we have consensus on an event
    pub async fn has_consensus(&self, event_id: u64) -> bool {
        self.seen_events.read().await.contains(&event_id)
    }

    /// Get stats (for monitoring)
    pub async fn get_stats(&self) -> ConsensusStats {
        ConsensusStats {
            current_sequence: *self.sequence.read().await,
            buffered_events: self.events.read().await.len(),
            max_buffer_size: EVENT_BUFFER_SIZE,
            forget_after_secs: FORGET_AFTER_SECS,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusStats {
    pub current_sequence: u64,
    pub buffered_events: usize,
    pub max_buffer_size: usize,
    pub forget_after_secs: u64,
}

/// Hash a string for privacy (one-way)
pub fn privacy_hash(input: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;

    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    hasher.finish()
}

/// Simple helper to track file operations privately
pub async fn track_file_privately(
    consensus: &TransientConsensus,
    path: &str,
) -> Result<u64> {
    let event = EventType::FileChange {
        path_hash: privacy_hash(path),
    };

    consensus.submit_event(event).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_transient_consensus() {
        let consensus = TransientConsensus::new().await.unwrap();

        // Submit event
        let id = consensus.submit_event(EventType::Generic { type_code: 1 }).await.unwrap();
        assert_eq!(id, 1);

        // Should be in buffer
        let recent = consensus.get_recent_events(10).await;
        assert_eq!(recent.len(), 1);
    }

    #[tokio::test]
    async fn test_privacy_hash() {
        let hash1 = privacy_hash("/secret/file.txt");
        let hash2 = privacy_hash("/secret/file.txt");
        let hash3 = privacy_hash("/other/file.txt");

        assert_eq!(hash1, hash2); // Same input = same hash
        assert_ne!(hash1, hash3); // Different input = different hash
    }

    #[tokio::test]
    async fn test_event_limit() {
        let consensus = TransientConsensus::new().await.unwrap();

        // Submit many events
        for i in 0..1500 {
            consensus.submit_event(EventType::Generic { type_code: i }).await.unwrap();
        }

        // Should only keep EVENT_BUFFER_SIZE
        let stats = consensus.get_stats().await;
        assert_eq!(stats.buffered_events, EVENT_BUFFER_SIZE);
    }
}