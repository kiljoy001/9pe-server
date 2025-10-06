//! Session management without excessive locking

use anyhow::Result;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;

/// Session manager using channels instead of Arc<RwLock<>>
pub struct SessionManager {
    sessions: RwLock<HashMap<u64, Session>>,
    next_id: AtomicU64,
}

#[derive(Debug)]
pub struct Session {
    pub id: u64,
    pub peer_addr: SocketAddr,
    pub created_at: std::time::Instant,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    pub async fn create_session(&self, peer_addr: SocketAddr) -> Result<u64> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let session = Session {
            id,
            peer_addr,
            created_at: std::time::Instant::now(),
        };

        self.sessions.write().await.insert(id, session);
        Ok(id)
    }

    pub async fn remove_session(&self, id: u64) {
        self.sessions.write().await.remove(&id);
    }

    pub async fn close_all(&self) -> Result<()> {
        self.sessions.write().await.clear();
        Ok(())
    }
}