//! Secure Session Management with Token Rotation
//!
//! Provides secure session handling with:
//! - Cryptographically secure token generation
//! - Automatic token rotation
//! - Session expiration and cleanup
//! - CSRF token validation
//! - Session fingerprinting

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use anyhow::{Result, bail};
use tracing::{warn, info, debug};
use sha2::{Sha256, Digest};
use rand::{thread_rng, Rng};

/// Session configuration
#[derive(Clone)]
pub struct SessionConfig {
    /// Session timeout in seconds
    pub session_timeout_secs: u64,
    /// Token rotation interval in seconds
    pub token_rotation_secs: u64,
    /// Maximum sessions per user
    pub max_sessions_per_user: usize,
    /// Enable session fingerprinting
    pub enable_fingerprinting: bool,
    /// CSRF token validity in seconds
    pub csrf_token_validity_secs: u64,
    /// Session cleanup interval in seconds
    pub cleanup_interval_secs: u64,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            session_timeout_secs: 3600,        // 1 hour
            token_rotation_secs: 900,          // 15 minutes
            max_sessions_per_user: 5,
            enable_fingerprinting: true,
            csrf_token_validity_secs: 3600,    // 1 hour
            cleanup_interval_secs: 300,        // 5 minutes
        }
    }
}

/// Session data
#[derive(Clone, Debug)]
pub struct Session {
    /// Unique session ID
    pub session_id: String,
    /// Current session token
    pub token: String,
    /// User ID associated with session
    pub user_id: String,
    /// Client IP address
    pub ip_address: IpAddr,
    /// Session fingerprint
    pub fingerprint: String,
    /// Creation timestamp
    pub created_at: u64,
    /// Last activity timestamp
    pub last_activity: u64,
    /// Token rotation timestamp
    pub token_rotated_at: u64,
    /// CSRF token
    pub csrf_token: String,
    /// Session metadata
    pub metadata: HashMap<String, String>,
    /// Whether session is valid
    pub is_valid: bool,
}

impl Session {
    fn new(user_id: String, ip_address: IpAddr, fingerprint: String) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();

        Self {
            session_id: generate_secure_token(32),
            token: generate_secure_token(64),
            csrf_token: generate_secure_token(32),
            user_id,
            ip_address,
            fingerprint,
            created_at: now,
            last_activity: now,
            token_rotated_at: now,
            metadata: HashMap::new(),
            is_valid: true,
        }
    }

    fn should_rotate_token(&self, config: &SessionConfig) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();

        now - self.token_rotated_at > config.token_rotation_secs
    }

    fn rotate_token(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();

        self.token = generate_secure_token(64);
        self.token_rotated_at = now;
        debug!("Rotated token for session {}", self.session_id);
    }

    fn is_expired(&self, config: &SessionConfig) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();

        now - self.last_activity > config.session_timeout_secs
    }

    fn update_activity(&mut self) {
        self.last_activity = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();
    }
}

/// Generate a cryptographically secure token
fn generate_secure_token(length: usize) -> String {
    let mut rng = thread_rng();
    let token: String = (0..length)
        .map(|_| {
            let idx = rng.gen_range(0..62);
            match idx {
                0..=25 => (b'a' + (idx as u8)) as char,
                26..=51 => (b'A' + ((idx - 26) as u8)) as char,
                52..=61 => (b'0' + ((idx - 52) as u8)) as char,
                _ => unreachable!(),
            }
        })
        .collect();
    token
}

/// Generate session fingerprint from request data
pub fn generate_fingerprint(
    user_agent: &str,
    accept_language: &str,
    accept_encoding: &str,
    ip: IpAddr,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(user_agent.as_bytes());
    hasher.update(accept_language.as_bytes());
    hasher.update(accept_encoding.as_bytes());
    hasher.update(ip.to_string().as_bytes());

    let result = hasher.finalize();
    hex::encode(result)
}

/// Secure session manager
pub struct SessionManager {
    config: SessionConfig,
    sessions: Arc<RwLock<HashMap<String, Session>>>,
    user_sessions: Arc<RwLock<HashMap<String, Vec<String>>>>,
    csrf_tokens: Arc<RwLock<HashMap<String, (String, u64)>>>, // token -> (session_id, expiry)
}

impl SessionManager {
    pub fn new(config: SessionConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            user_sessions: Arc::new(RwLock::new(HashMap::new())),
            csrf_tokens: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new session
    pub async fn create_session(
        &self,
        user_id: String,
        ip_address: IpAddr,
        fingerprint: String,
    ) -> Result<Session> {
        // Check if user has too many sessions
        {
            let user_sessions = self.user_sessions.read().await;
            if let Some(sessions) = user_sessions.get(&user_id) {
                if sessions.len() >= self.config.max_sessions_per_user {
                    warn!("User {} exceeded maximum sessions", user_id);
                    bail!("Maximum sessions exceeded");
                }
            }
        }

        let session = Session::new(user_id.clone(), ip_address, fingerprint);
        let session_id = session.session_id.clone();
        let csrf_token = session.csrf_token.clone();

        // Store session
        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(session.token.clone(), session.clone());
        }

        // Track user sessions
        {
            let mut user_sessions = self.user_sessions.write().await;
            user_sessions
                .entry(user_id.clone())
                .or_insert_with(Vec::new)
                .push(session_id.clone());
        }

        // Store CSRF token
        {
            let mut csrf_tokens = self.csrf_tokens.write().await;
            let expiry = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_secs() + self.config.csrf_token_validity_secs;
            csrf_tokens.insert(csrf_token, (session_id.clone(), expiry));
        }

        info!("Created session {} for user {}", session_id, user_id);
        Ok(session)
    }

    /// Validate and retrieve a session
    pub async fn validate_session(
        &self,
        token: &str,
        ip_address: IpAddr,
        fingerprint: &str,
    ) -> Result<Session> {
        let mut sessions = self.sessions.write().await;

        let session = sessions.get_mut(token)
            .ok_or_else(|| anyhow::anyhow!("Invalid session token"))?;

        // Check if session is expired
        if session.is_expired(&self.config) {
            session.is_valid = false;
            warn!("Session {} has expired", session.session_id);
            bail!("Session expired");
        }

        // Verify fingerprint if enabled
        if self.config.enable_fingerprinting && session.fingerprint != fingerprint {
            warn!("Session {} fingerprint mismatch", session.session_id);
            session.is_valid = false;
            bail!("Session fingerprint mismatch");
        }

        // Verify IP address hasn't changed drastically
        if session.ip_address != ip_address {
            debug!("Session {} IP changed from {} to {}",
                   session.session_id, session.ip_address, ip_address);
            // Could implement more sophisticated IP validation here
        }

        // Update activity
        session.update_activity();

        // Check if token should be rotated
        if session.should_rotate_token(&self.config) {
            let old_token = session.token.clone();
            let new_token = generate_secure_token(64);
            session.token = new_token.clone();
            session.token_rotated_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_secs();
            debug!("Rotated token for session {}", session.session_id);

            // Clone session before manipulating sessions map
            let session_clone = session.clone();

            // Now we can safely remove and reinsert
            sessions.remove(&old_token);
            sessions.insert(new_token, session_clone.clone());

            return Ok(session_clone);
        }

        Ok(session.clone())
    }

    /// Validate CSRF token
    pub async fn validate_csrf_token(&self, csrf_token: &str, session_id: &str) -> Result<bool> {
        let csrf_tokens = self.csrf_tokens.read().await;

        if let Some((stored_session_id, expiry)) = csrf_tokens.get(csrf_token) {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_secs();

            if now > *expiry {
                warn!("CSRF token expired for session {}", session_id);
                return Ok(false);
            }

            if stored_session_id != session_id {
                warn!("CSRF token session mismatch: expected {}, got {}",
                      stored_session_id, session_id);
                return Ok(false);
            }

            Ok(true)
        } else {
            warn!("Invalid CSRF token for session {}", session_id);
            Ok(false)
        }
    }

    /// Invalidate a session
    pub async fn invalidate_session(&self, token: &str) -> Result<()> {
        let mut sessions = self.sessions.write().await;

        if let Some(mut session) = sessions.remove(token) {
            session.is_valid = false;

            // Remove from user sessions
            let mut user_sessions = self.user_sessions.write().await;
            if let Some(sessions) = user_sessions.get_mut(&session.user_id) {
                sessions.retain(|id| id != &session.session_id);
            }

            // Remove CSRF token
            let mut csrf_tokens = self.csrf_tokens.write().await;
            csrf_tokens.remove(&session.csrf_token);

            info!("Invalidated session {}", session.session_id);
        }

        Ok(())
    }

    /// Invalidate all sessions for a user
    pub async fn invalidate_user_sessions(&self, user_id: &str) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        let mut user_sessions = self.user_sessions.write().await;
        let mut csrf_tokens = self.csrf_tokens.write().await;

        if let Some(session_ids) = user_sessions.remove(user_id) {
            for session_id in session_ids {
                // Find and remove sessions
                sessions.retain(|_, session| {
                    if session.session_id == session_id {
                        csrf_tokens.remove(&session.csrf_token);
                        false
                    } else {
                        true
                    }
                });
            }

            info!("Invalidated all sessions for user {}", user_id);
        }

        Ok(())
    }

    /// Clean up expired sessions
    pub async fn cleanup_expired_sessions(&self) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        let mut user_sessions = self.user_sessions.write().await;
        let mut csrf_tokens = self.csrf_tokens.write().await;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();

        let mut expired_sessions = Vec::new();

        // Find expired sessions
        for (token, session) in sessions.iter() {
            if session.is_expired(&self.config) || !session.is_valid {
                expired_sessions.push((token.clone(), session.session_id.clone(),
                                      session.user_id.clone(), session.csrf_token.clone()));
            }
        }

        // Remove expired sessions
        for (token, session_id, user_id, csrf_token) in expired_sessions {
            sessions.remove(&token);

            // Remove from user sessions
            if let Some(sessions) = user_sessions.get_mut(&user_id) {
                sessions.retain(|id| id != &session_id);
            }

            // Remove CSRF token
            csrf_tokens.remove(&csrf_token);

            debug!("Cleaned up expired session {}", session_id);
        }

        // Clean up expired CSRF tokens
        csrf_tokens.retain(|_, (_, expiry)| now <= *expiry);

        info!("Session cleanup completed");
        Ok(())
    }

    /// Get session statistics
    pub async fn get_stats(&self) -> SessionStats {
        let sessions = self.sessions.read().await;
        let user_sessions = self.user_sessions.read().await;
        let csrf_tokens = self.csrf_tokens.read().await;

        let total_sessions = sessions.len();
        let active_users = user_sessions.len();
        let active_csrf_tokens = csrf_tokens.len();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();

        let expired_sessions = sessions.values()
            .filter(|s| s.is_expired(&self.config))
            .count();

        SessionStats {
            total_sessions,
            active_users,
            active_csrf_tokens,
            expired_sessions,
        }
    }
}

/// Session statistics
#[derive(Debug, Clone)]
pub struct SessionStats {
    pub total_sessions: usize,
    pub active_users: usize,
    pub active_csrf_tokens: usize,
    pub expired_sessions: usize,
}

/// Middleware helper for session validation
pub async fn require_valid_session(
    manager: &SessionManager,
    token: &str,
    ip: IpAddr,
    fingerprint: &str,
) -> Result<Session> {
    manager.validate_session(token, ip, fingerprint).await
}

/// Middleware helper for CSRF validation
pub async fn require_csrf_token(
    manager: &SessionManager,
    csrf_token: &str,
    session_id: &str,
) -> Result<()> {
    if !manager.validate_csrf_token(csrf_token, session_id).await? {
        bail!("Invalid CSRF token");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[tokio::test]
    async fn test_session_creation() {
        let config = SessionConfig::default();
        let manager = SessionManager::new(config);

        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let fingerprint = generate_fingerprint("Mozilla/5.0", "en-US", "gzip", ip);

        let session = manager.create_session(
            "user1".to_string(),
            ip,
            fingerprint
        ).await.unwrap();

        assert_eq!(session.user_id, "user1");
        assert!(session.is_valid);
        assert!(!session.token.is_empty());
        assert!(!session.csrf_token.is_empty());
    }

    #[tokio::test]
    async fn test_session_validation() {
        let config = SessionConfig::default();
        let manager = SessionManager::new(config);

        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let fingerprint = generate_fingerprint("Mozilla/5.0", "en-US", "gzip", ip);

        let session = manager.create_session(
            "user1".to_string(),
            ip,
            fingerprint.clone()
        ).await.unwrap();

        // Validate with correct token
        let validated = manager.validate_session(
            &session.token,
            ip,
            &fingerprint
        ).await.unwrap();

        assert_eq!(validated.session_id, session.session_id);

        // Validate with wrong token
        assert!(manager.validate_session(
            "wrong_token",
            ip,
            &fingerprint
        ).await.is_err());
    }

    #[tokio::test]
    async fn test_csrf_validation() {
        let config = SessionConfig::default();
        let manager = SessionManager::new(config);

        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let fingerprint = generate_fingerprint("Mozilla/5.0", "en-US", "gzip", ip);

        let session = manager.create_session(
            "user1".to_string(),
            ip,
            fingerprint
        ).await.unwrap();

        // Validate correct CSRF token
        assert!(manager.validate_csrf_token(
            &session.csrf_token,
            &session.session_id
        ).await.unwrap());

        // Validate wrong CSRF token
        assert!(!manager.validate_csrf_token(
            "wrong_csrf",
            &session.session_id
        ).await.unwrap());
    }

    #[tokio::test]
    async fn test_session_expiration() {
        let config = SessionConfig {
            session_timeout_secs: 1, // 1 second timeout for testing
            ..Default::default()
        };
        let manager = SessionManager::new(config);

        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let fingerprint = generate_fingerprint("Mozilla/5.0", "en-US", "gzip", ip);

        let session = manager.create_session(
            "user1".to_string(),
            ip,
            fingerprint.clone()
        ).await.unwrap();

        // Wait for expiration
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Should fail validation
        assert!(manager.validate_session(
            &session.token,
            ip,
            &fingerprint
        ).await.is_err());
    }
}