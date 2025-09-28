//! Simple Ordering - Consensus without unnecessary cryptography
//!
//! Just sequence numbers and timestamps. No blockchain bullshit.

use std::collections::{VecDeque, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use anyhow::Result;
use tracing::{info, debug};

const MAX_EVENTS: usize = 1000;
const FORGET_AFTER_SECS: u64 = 300; // 5 minutes

/// Dead simple event
#[derive(Debug, Clone)]
pub struct SimpleEvent {
    /// Monotonic sequence number
    pub seq: u64,

    /// When it happened (for ordering)
    pub timestamp: u64,

    /// What happened (just a type code)
    pub event_type: u16,

    /// Which node saw it
    pub node_id: String,
}

/// Simple ordering service - no crypto needed
pub struct SimpleOrdering {
    /// Next sequence number
    next_seq: Arc<RwLock<u64>>,

    /// Recent events (rolling buffer)
    events: Arc<RwLock<VecDeque<SimpleEvent>>>,

    /// Already seen (for dedup) - just sequence numbers
    seen: Arc<RwLock<HashSet<u64>>>,
}

impl SimpleOrdering {
    pub async fn new() -> Result<Self> {
        info!("📝 Simple Ordering Service (no crypto!)");

        Ok(Self {
            next_seq: Arc::new(RwLock::new(0)),
            events: Arc::new(RwLock::new(VecDeque::with_capacity(MAX_EVENTS))),
            seen: Arc::new(RwLock::new(HashSet::new())),
        })
    }

    /// Submit event - returns sequence number
    pub async fn submit(&self, event_type: u16) -> Result<u64> {
        let seq = {
            let mut s = self.next_seq.write().await;
            *s += 1;
            *s
        };

        let event = SimpleEvent {
            seq,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)?
                .as_secs(),
            event_type,
            node_id: whoami::username(), // Or could be node ID
        };

        // Add to buffer
        let mut events = self.events.write().await;
        events.push_back(event);

        // Keep buffer size limited
        while events.len() > MAX_EVENTS {
            events.pop_front();
        }

        // Mark as seen
        self.seen.write().await.insert(seq);

        debug!("Event {} submitted", seq);
        Ok(seq)
    }

    /// Handle event from network
    pub async fn handle_network_event(&self, event: SimpleEvent) -> Result<()> {
        // Already seen?
        if self.seen.read().await.contains(&event.seq) {
            return Ok(());
        }

        // Add in order
        let mut events = self.events.write().await;

        // Find position (by timestamp, then seq)
        let pos = events.iter().position(|e| {
            e.timestamp > event.timestamp ||
            (e.timestamp == event.timestamp && e.seq > event.seq)
        }).unwrap_or(events.len());

        events.insert(pos, event.clone());

        // Maintain size
        while events.len() > MAX_EVENTS {
            events.pop_front();
        }

        self.seen.write().await.insert(event.seq);

        Ok(())
    }

    /// Get current state for new nodes
    pub async fn get_snapshot(&self) -> Vec<SimpleEvent> {
        self.events.read().await.iter().cloned().collect()
    }

    /// Apply snapshot when joining
    pub async fn apply_snapshot(&self, snapshot: Vec<SimpleEvent>) -> Result<()> {
        let mut events = self.events.write().await;
        events.clear();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs();

        // Only keep recent events
        for event in snapshot {
            if now - event.timestamp < FORGET_AFTER_SECS {
                events.push_back(event.clone());
                self.seen.write().await.insert(event.seq);
            }
        }

        // Update sequence
        if let Some(last) = events.back() {
            *self.next_seq.write().await = last.seq;
        }

        Ok(())
    }

    /// Periodic cleanup
    pub async fn cleanup_old_events(&self) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs();

        let mut events = self.events.write().await;
        let before = events.len();

        // Remove old events
        events.retain(|e| now - e.timestamp < FORGET_AFTER_SECS);

        let removed = before - events.len();
        if removed > 0 {
            debug!("Cleaned up {} old events", removed);
        }

        Ok(())
    }
}

/// Event types for file operations (no crypto needed)
pub mod event_types {
    pub const FILE_WRITE: u16 = 1;
    pub const FILE_READ: u16 = 2;
    pub const FILE_DELETE: u16 = 3;
    pub const NODE_JOIN: u16 = 10;
    pub const NODE_LEAVE: u16 = 11;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_simple_ordering() {
        let ordering = SimpleOrdering::new().await.unwrap();

        // Submit some events
        let seq1 = ordering.submit(event_types::FILE_WRITE).await.unwrap();
        let seq2 = ordering.submit(event_types::FILE_READ).await.unwrap();

        assert_eq!(seq1, 1);
        assert_eq!(seq2, 2);

        // Check they're in order
        let snapshot = ordering.get_snapshot().await;
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].seq, 1);
        assert_eq!(snapshot[1].seq, 2);
    }
}