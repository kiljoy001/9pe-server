//! Tests for Connection State Machine with Timeouts (#1)
//! Application-level session management on top of QUIC transport

use std::time::{Duration, Instant};
use std::collections::HashMap;
use std::net::SocketAddr;

/// Connection states for 9P.e sessions
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    /// Initial handshake phase
    Handshake { started: Instant, timeout: Duration },
    /// Authentication in progress
    Authenticating { started: Instant, timeout: Duration },
    /// Fully authenticated and active
    Active { last_seen: Instant, idle_timeout: Duration },
    /// Gracefully shutting down
    Draining { started: Instant, drain_timeout: Duration },
    /// Connection terminated
    Terminated { reason: String },
}

/// State machine for managing 9P.e connection lifecycle
pub struct ConnectionStateMachine {
    sessions: HashMap<u32, ConnectionState>,
    next_session_id: u32,
    default_handshake_timeout: Duration,
    default_auth_timeout: Duration,
    default_idle_timeout: Duration,
    default_drain_timeout: Duration,
}

impl ConnectionStateMachine {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            next_session_id: 1,
            default_handshake_timeout: Duration::from_secs(30),
            default_auth_timeout: Duration::from_secs(60),
            default_idle_timeout: Duration::from_secs(600), // 10 minutes
            default_drain_timeout: Duration::from_secs(5),
        }
    }

    /// Start a new session in handshake state
    pub fn start_session(&mut self, _addr: SocketAddr) -> u32 {
        let session_id = self.next_session_id;
        self.next_session_id += 1;

        let state = ConnectionState::Handshake {
            started: Instant::now(),
            timeout: self.default_handshake_timeout,
        };

        self.sessions.insert(session_id, state);
        session_id
    }

    /// Advance session to authentication state
    pub fn begin_authentication(&mut self, session_id: u32) -> Result<(), String> {
        match self.sessions.get(&session_id) {
            Some(ConnectionState::Handshake { .. }) => {
                let new_state = ConnectionState::Authenticating {
                    started: Instant::now(),
                    timeout: self.default_auth_timeout,
                };
                self.sessions.insert(session_id, new_state);
                Ok(())
            }
            Some(state) => Err(format!("Invalid state transition from {:?}", state)),
            None => Err("Session not found".to_string()),
        }
    }

    /// Mark session as authenticated and active
    pub fn activate_session(&mut self, session_id: u32) -> Result<(), String> {
        match self.sessions.get(&session_id) {
            Some(ConnectionState::Authenticating { .. }) => {
                let new_state = ConnectionState::Active {
                    last_seen: Instant::now(),
                    idle_timeout: self.default_idle_timeout,
                };
                self.sessions.insert(session_id, new_state);
                Ok(())
            }
            Some(state) => Err(format!("Invalid state transition from {:?}", state)),
            None => Err("Session not found".to_string()),
        }
    }

    /// Update last activity for active session
    pub fn update_activity(&mut self, session_id: u32) -> Result<(), String> {
        match self.sessions.get_mut(&session_id) {
            Some(ConnectionState::Active { last_seen, .. }) => {
                *last_seen = Instant::now();
                Ok(())
            }
            Some(state) => Err(format!("Cannot update activity for state {:?}", state)),
            None => Err("Session not found".to_string()),
        }
    }

    /// Begin graceful shutdown
    pub fn begin_drain(&mut self, session_id: u32) -> Result<(), String> {
        match self.sessions.get(&session_id) {
            Some(ConnectionState::Active { .. }) => {
                let new_state = ConnectionState::Draining {
                    started: Instant::now(),
                    drain_timeout: self.default_drain_timeout,
                };
                self.sessions.insert(session_id, new_state);
                Ok(())
            }
            Some(state) => Err(format!("Invalid state transition from {:?}", state)),
            None => Err("Session not found".to_string()),
        }
    }

    /// Terminate session
    pub fn terminate_session(&mut self, session_id: u32, reason: String) {
        self.sessions.insert(session_id, ConnectionState::Terminated { reason });
    }

    /// Clean up expired sessions
    pub fn cleanup_expired(&mut self) -> Vec<(u32, String)> {
        let now = Instant::now();
        let mut expired = Vec::new();

        let expired_sessions: Vec<_> = self.sessions.iter()
            .filter_map(|(id, state)| {
                match state {
                    ConnectionState::Handshake { started, timeout } => {
                        if now.duration_since(*started) > *timeout {
                            Some((*id, "Handshake timeout".to_string()))
                        } else { None }
                    }
                    ConnectionState::Authenticating { started, timeout } => {
                        if now.duration_since(*started) > *timeout {
                            Some((*id, "Authentication timeout".to_string()))
                        } else { None }
                    }
                    ConnectionState::Active { last_seen, idle_timeout } => {
                        if now.duration_since(*last_seen) > *idle_timeout {
                            Some((*id, "Idle timeout".to_string()))
                        } else { None }
                    }
                    ConnectionState::Draining { started, drain_timeout } => {
                        if now.duration_since(*started) > *drain_timeout {
                            Some((*id, "Drain timeout".to_string()))
                        } else { None }
                    }
                    ConnectionState::Terminated { .. } => None,
                }
            })
            .collect();

        for (session_id, reason) in expired_sessions {
            self.terminate_session(session_id, reason.clone());
            expired.push((session_id, reason));
        }

        expired
    }

    /// Get current session state
    pub fn get_state(&self, session_id: u32) -> Option<&ConnectionState> {
        self.sessions.get(&session_id)
    }

    /// Get statistics
    pub fn get_stats(&self) -> ConnectionStats {
        let mut stats = ConnectionStats::default();

        for state in self.sessions.values() {
            match state {
                ConnectionState::Handshake { .. } => stats.handshaking += 1,
                ConnectionState::Authenticating { .. } => stats.authenticating += 1,
                ConnectionState::Active { .. } => stats.active += 1,
                ConnectionState::Draining { .. } => stats.draining += 1,
                ConnectionState::Terminated { .. } => stats.terminated += 1,
            }
        }

        stats.total = self.sessions.len();
        stats
    }
}

#[derive(Debug, Default)]
pub struct ConnectionStats {
    pub total: usize,
    pub handshaking: usize,
    pub authenticating: usize,
    pub active: usize,
    pub draining: usize,
    pub terminated: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use std::thread;

    #[test]
    fn test_connection_state_transitions() {
        let mut fsm = ConnectionStateMachine::new();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);

        // Start new session
        let session_id = fsm.start_session(addr);
        assert!(matches!(
            fsm.get_state(session_id).unwrap(),
            ConnectionState::Handshake { .. }
        ));

        // Begin authentication
        assert!(fsm.begin_authentication(session_id).is_ok());
        assert!(matches!(
            fsm.get_state(session_id).unwrap(),
            ConnectionState::Authenticating { .. }
        ));

        // Activate session
        assert!(fsm.activate_session(session_id).is_ok());
        assert!(matches!(
            fsm.get_state(session_id).unwrap(),
            ConnectionState::Active { .. }
        ));

        // Update activity
        assert!(fsm.update_activity(session_id).is_ok());

        // Begin drain
        assert!(fsm.begin_drain(session_id).is_ok());
        assert!(matches!(
            fsm.get_state(session_id).unwrap(),
            ConnectionState::Draining { .. }
        ));

        // Terminate
        fsm.terminate_session(session_id, "Test termination".to_string());
        assert!(matches!(
            fsm.get_state(session_id).unwrap(),
            ConnectionState::Terminated { .. }
        ));
    }

    #[test]
    fn test_invalid_state_transitions() {
        let mut fsm = ConnectionStateMachine::new();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);

        let session_id = fsm.start_session(addr);

        // Can't activate from handshake directly
        assert!(fsm.activate_session(session_id).is_err());

        // Can't update activity before active
        assert!(fsm.update_activity(session_id).is_err());

        // Valid transition to auth
        assert!(fsm.begin_authentication(session_id).is_ok());

        // Can't go back to handshake
        assert!(fsm.begin_authentication(session_id).is_err());
    }

    #[test]
    fn test_timeout_cleanup() {
        let mut fsm = ConnectionStateMachine {
            sessions: HashMap::new(),
            next_session_id: 1,
            default_handshake_timeout: Duration::from_millis(100), // Very short for testing
            default_auth_timeout: Duration::from_millis(100),
            default_idle_timeout: Duration::from_millis(100),
            default_drain_timeout: Duration::from_millis(100),
        };

        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);

        // Start session and let it timeout
        let session_id = fsm.start_session(addr);

        // Wait for timeout
        thread::sleep(Duration::from_millis(150));

        // Cleanup should find expired session
        let expired = fsm.cleanup_expired();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].0, session_id);
        assert_eq!(expired[0].1, "Handshake timeout");

        // Session should now be terminated
        assert!(matches!(
            fsm.get_state(session_id).unwrap(),
            ConnectionState::Terminated { .. }
        ));
    }

    #[test]
    fn test_connection_stats() {
        let mut fsm = ConnectionStateMachine::new();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);

        // Create sessions in different states
        let s1 = fsm.start_session(addr); // Handshaking
        let s2 = fsm.start_session(addr); // Will be Authenticating
        let s3 = fsm.start_session(addr); // Will be Active

        fsm.begin_authentication(s2).unwrap();
        fsm.begin_authentication(s3).unwrap();
        fsm.activate_session(s3).unwrap();

        let stats = fsm.get_stats();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.handshaking, 1);
        assert_eq!(stats.authenticating, 1);
        assert_eq!(stats.active, 1);
        assert_eq!(stats.draining, 0);
        assert_eq!(stats.terminated, 0);
    }

    #[test]
    fn test_concurrent_session_management() {
        use std::sync::{Arc, Mutex};

        let fsm = Arc::new(Mutex::new(ConnectionStateMachine::new()));
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);

        let mut handles = vec![];

        // Multiple threads creating and managing sessions
        for i in 0..10 {
            let fsm_clone = Arc::clone(&fsm);
            let handle = thread::spawn(move || {
                let session_id = {
                    let mut fsm = fsm_clone.lock().unwrap();
                    fsm.start_session(addr)
                };

                // Simulate session progression
                thread::sleep(Duration::from_millis(i * 10));

                {
                    let mut fsm = fsm_clone.lock().unwrap();
                    fsm.begin_authentication(session_id).unwrap();
                    fsm.activate_session(session_id).unwrap();
                }

                session_id
            });
            handles.push(handle);
        }

        let session_ids: Vec<_> = handles.into_iter()
            .map(|h| h.join().unwrap())
            .collect();

        // All sessions should be active
        let fsm = fsm.lock().unwrap();
        let stats = fsm.get_stats();
        assert_eq!(stats.active, 10);

        // All session IDs should be unique
        let mut sorted_ids = session_ids.clone();
        sorted_ids.sort();
        sorted_ids.dedup();
        assert_eq!(sorted_ids.len(), session_ids.len());
    }
}