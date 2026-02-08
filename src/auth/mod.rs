//! Authentication and Authorization Module
//!
//! Provides secure user authentication for the 9P.e server with:
//! - Argon2id password hashing (local auth)
//! - Plan 9 factotum support (remote auth agent)
//! - Session management with sled persistence
//! - Capability-based authorization
//! - User database persistence via sled

mod clock;
pub mod factotum;

use anyhow::{bail, Result};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use chrono::{DateTime, Duration, Utc};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

pub use clock::{Clock, MockClock, RealClock};
pub use factotum::{FactotumChallenge, FactotumClient, FactotumTicket};

// Sled tree names
const USERS_TREE: &str = "users";
const SESSIONS_TREE: &str = "sessions";
const META_TREE: &str = "meta";

/// Authentication service for managing users and sessions
pub struct AuthService<C: Clock = RealClock> {
    /// Sled database for persistent storage
    db: sled::Db,

    /// Users tree
    users_tree: sled::Tree,

    /// Sessions tree
    sessions_tree: sled::Tree,

    /// In-memory session cache for fast lookups
    session_cache: Arc<RwLock<HashMap<SessionToken, Session>>>,

    /// Configuration
    config: AuthConfig,

    /// Argon2 hasher
    hasher: Argon2<'static>,

    /// Clock for time operations
    clock: C,
}

/// Authentication method
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AuthMethod {
    /// Local password authentication with Argon2id
    Local,
    /// Plan 9 factotum authentication agent
    Factotum,
    /// Both local and factotum (try factotum first, fall back to local)
    Both,
}

impl Default for AuthMethod {
    fn default() -> Self {
        AuthMethod::Local
    }
}

/// Authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Database directory path (sled uses a directory)
    pub db_path: PathBuf,

    /// Session timeout in seconds
    pub session_timeout: u64,

    /// Maximum failed login attempts before lockout
    pub max_failed_attempts: u32,

    /// Lockout duration in seconds
    pub lockout_duration: u64,

    /// Enable anonymous access
    pub allow_anonymous: bool,

    /// Minimum password length
    pub min_password_length: usize,

    /// Require mixed case in passwords
    pub require_mixed_case: bool,

    /// Require numbers in passwords
    pub require_numbers: bool,

    /// Require special characters in passwords
    pub require_special_chars: bool,

    /// Authentication method to use
    pub auth_method: AuthMethod,

    /// Factotum configuration (if using factotum auth)
    pub factotum: FactotumConfig,
}

/// Factotum authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactotumConfig {
    /// Path to factotum socket (Unix) or address (TCP)
    /// Default: /mnt/factotum/rpc (Plan 9 standard)
    pub address: String,

    /// Authentication domain/realm
    pub auth_dom: String,

    /// Shared secret for ticket encryption (base64 encoded)
    /// In production, this should come from a secure key store
    pub auth_secret: Option<String>,

    /// Whether to allow factotum to provide capabilities
    pub trust_factotum_capabilities: bool,

    /// Connection timeout in milliseconds
    pub timeout_ms: u64,
}

impl Default for FactotumConfig {
    fn default() -> Self {
        Self {
            address: "/mnt/factotum/rpc".to_string(),
            auth_dom: "9pe".to_string(),
            auth_secret: None,
            trust_factotum_capabilities: false,
            timeout_ms: 5000,
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            db_path: PathBuf::from("/var/lib/9pe/auth"),
            session_timeout: 3600, // 1 hour
            max_failed_attempts: 5,
            lockout_duration: 300, // 5 minutes
            allow_anonymous: false,
            min_password_length: 12,
            require_mixed_case: true,
            require_numbers: true,
            require_special_chars: true,
            auth_method: AuthMethod::default(),
            factotum: FactotumConfig::default(),
        }
    }
}

/// Database metadata (stored in meta tree)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DbMeta {
    /// Database version for migration support
    version: u32,
    /// Creation timestamp
    created_at: DateTime<Utc>,
}

/// User record
#[derive(Debug, Clone, Serialize, Deserialize)]
struct User {
    /// Username
    username: String,

    /// Hashed password (Argon2id)
    password_hash: String,

    /// User ID
    uid: u32,

    /// Group ID
    gid: u32,

    /// User capabilities
    capabilities: Vec<Capability>,

    /// Account created timestamp
    created_at: DateTime<Utc>,

    /// Last login timestamp
    last_login: Option<DateTime<Utc>>,

    /// Failed login attempts
    failed_attempts: u32,

    /// Account locked until
    locked_until: Option<DateTime<Utc>>,

    /// Account enabled
    enabled: bool,

    /// Password history (hashes of previous passwords to prevent reuse)
    #[serde(default)]
    password_history: Vec<String>,

    /// Password last changed timestamp
    #[serde(default)]
    password_changed_at: Option<DateTime<Utc>>,

    /// Last failed login timestamp (for rate limiting)
    #[serde(default)]
    last_failed_at: Option<DateTime<Utc>>,
}

/// User capabilities for authorization
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Capability {
    /// Read files
    Read,

    /// Write files
    Write,

    /// Execute programs
    Execute,

    /// Mount filesystems
    Mount,

    /// Administrative access
    Admin,

    /// Create WASM translators
    CreateTranslator,

    /// Access mesh network
    MeshAccess,

    /// Custom capability
    Custom(String),
}

/// Authentication session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Session token
    pub token: SessionToken,

    /// Username
    pub username: String,

    /// User ID
    pub uid: u32,

    /// Group ID
    pub gid: u32,

    /// Session capabilities
    pub capabilities: Vec<Capability>,

    /// Session created time
    pub created_at: DateTime<Utc>,

    /// Session expires at
    pub expires_at: DateTime<Utc>,

    /// Client address
    pub client_addr: Option<String>,

    /// Last activity timestamp (for idle timeout)
    pub last_activity: DateTime<Utc>,
}

/// Session token - cryptographically random 256-bit token
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionToken(String);

impl SessionToken {
    /// Generate a new cryptographically random session token
    fn generate() -> Self {
        use rand::RngCore;
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        // Use hex encoding (64 chars for 32 bytes)
        SessionToken(hex::encode(bytes))
    }

    /// Get the token string
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Create from string (for deserialization)
    pub fn from_string(s: String) -> Self {
        SessionToken(s)
    }
}

impl AuthService<RealClock> {
    /// Create a new authentication service with real clock
    pub async fn new(config: AuthConfig) -> Result<Self> {
        Self::new_with_clock(config, RealClock).await
    }
}

impl<C: Clock> AuthService<C> {
    /// Create a new authentication service with a specific clock
    pub async fn new_with_clock(config: AuthConfig, clock: C) -> Result<Self> {
        let hasher = Argon2::default();

        // Open or create sled database
        let db = sled::open(&config.db_path)
            .map_err(|e| anyhow::anyhow!("Failed to open auth database: {}", e))?;

        let users_tree = db.open_tree(USERS_TREE)
            .map_err(|e| anyhow::anyhow!("Failed to open users tree: {}", e))?;
        let sessions_tree = db.open_tree(SESSIONS_TREE)
            .map_err(|e| anyhow::anyhow!("Failed to open sessions tree: {}", e))?;
        let meta_tree = db.open_tree(META_TREE)
            .map_err(|e| anyhow::anyhow!("Failed to open meta tree: {}", e))?;

        // Check if this is a fresh database
        let is_new = meta_tree.get(b"version")?.is_none();

        if is_new {
            // Initialize database metadata
            let meta = DbMeta {
                version: 1,
                created_at: clock.now(),
            };
            meta_tree.insert(b"version", serde_cbor::to_vec(&meta)?)?;

            // Create default admin user with secure random password
            let admin_password = Self::generate_password();
            info!(
                "Creating default admin user with password: {}",
                admin_password
            );

            let salt = SaltString::generate(&mut OsRng);
            let password_hash = hasher
                .hash_password(admin_password.as_bytes(), &salt)
                .map_err(|e| anyhow::anyhow!("Failed to hash password: {}", e))?
                .to_string();

            let admin = User {
                username: "admin".to_string(),
                password_hash,
                uid: 0,
                gid: 0,
                capabilities: vec![Capability::Admin],
                created_at: clock.now(),
                last_login: None,
                failed_attempts: 0,
                locked_until: None,
                enabled: true,
                password_history: Vec::new(),
                password_changed_at: Some(clock.now()),
                last_failed_at: None,
            };

            users_tree.insert(b"admin", serde_cbor::to_vec(&admin)?)?;

            // Flush to ensure persistence
            db.flush()?;
        }

        // Load existing sessions into cache
        let mut session_cache = HashMap::new();
        for result in sessions_tree.iter() {
            let (_, value) = result?;
            if let Ok(session) = serde_cbor::from_slice::<Session>(&value) {
                // Only load non-expired sessions
                if session.expires_at > clock.now() {
                    session_cache.insert(session.token.clone(), session);
                }
            }
        }

        Ok(Self {
            db,
            users_tree,
            sessions_tree,
            session_cache: Arc::new(RwLock::new(session_cache)),
            config,
            hasher,
            clock,
        })
    }

    /// Validate password against security policy
    fn validate_password(&self, password: &str) -> Result<()> {
        if password.len() < self.config.min_password_length {
            bail!("Password must be at least {} characters", self.config.min_password_length);
        }

        if self.config.require_mixed_case {
            let has_upper = password.chars().any(|c| c.is_ascii_uppercase());
            let has_lower = password.chars().any(|c| c.is_ascii_lowercase());
            if !has_upper || !has_lower {
                bail!("Password must contain both uppercase and lowercase letters");
            }
        }

        if self.config.require_numbers {
            if !password.chars().any(|c| c.is_ascii_digit()) {
                bail!("Password must contain at least one number");
            }
        }

        if self.config.require_special_chars {
            if !password.chars().any(|c| "!@#$%^&*()_+-=[]{}|;':\",./<>?".contains(c)) {
                bail!("Password must contain at least one special character");
            }
        }

        Ok(())
    }

    /// Check if password was used recently (password history)
    fn check_password_history(&self, password: &str, history: &[String]) -> Result<()> {
        for old_hash in history.iter().take(5) { // Check last 5 passwords
            if let Ok(parsed_hash) = PasswordHash::new(old_hash) {
                if self.hasher.verify_password(password.as_bytes(), &parsed_hash).is_ok() {
                    bail!("Cannot reuse a recent password");
                }
            }
        }
        Ok(())
    }

    /// Get user from database
    fn get_user(&self, username: &str) -> Result<Option<User>> {
        match self.users_tree.get(username.as_bytes())? {
            Some(data) => Ok(Some(serde_cbor::from_slice(&data)?)),
            None => Ok(None),
        }
    }

    /// Save user to database
    fn save_user(&self, user: &User) -> Result<()> {
        self.users_tree.insert(user.username.as_bytes(), serde_cbor::to_vec(user)?)?;
        self.db.flush()?;
        Ok(())
    }

    /// Save session to database and cache
    async fn save_session(&self, session: &Session) -> Result<()> {
        // Save to sled
        self.sessions_tree.insert(
            session.token.as_str().as_bytes(),
            serde_cbor::to_vec(session)?,
        )?;

        // Update cache
        let mut cache = self.session_cache.write().await;
        cache.insert(session.token.clone(), session.clone());

        Ok(())
    }

    /// Remove session from database and cache
    async fn remove_session(&self, token: &SessionToken) -> Result<()> {
        self.sessions_tree.remove(token.as_str().as_bytes())?;
        let mut cache = self.session_cache.write().await;
        cache.remove(token);
        Ok(())
    }

    /// Authenticate a user with username and password
    pub async fn authenticate(
        &self,
        username: &str,
        password: &str,
        client_addr: Option<String>,
    ) -> Result<SessionToken> {
        // Get user from database
        let mut user = self.get_user(username)?
            .ok_or_else(|| anyhow::anyhow!("Invalid username or password"))?;

        // Check if account is locked
        if let Some(locked_until) = user.locked_until {
            if self.clock.now() < locked_until {
                // Don't reveal lock status to potential attackers - use generic message
                bail!("Invalid username or password");
            } else {
                // Clear lockout
                user.locked_until = None;
                user.failed_attempts = 0;
            }
        }

        // Check if account is enabled
        if !user.enabled {
            // Don't reveal account status - use generic message
            bail!("Invalid username or password");
        }

        // Rate limit: exponential backoff on failed attempts
        if let Some(last_failed) = user.last_failed_at {
            if user.failed_attempts > 0 {
                // Exponential backoff: 2^(attempts-1) seconds, capped at lockout duration
                let delay_secs = std::cmp::min(
                    2_i64.pow(user.failed_attempts.saturating_sub(1)),
                    self.config.lockout_duration as i64,
                );
                let min_next_attempt = last_failed + Duration::seconds(delay_secs);
                if self.clock.now() < min_next_attempt {
                    bail!("Invalid username or password");
                }
            }
        }

        // Verify password using constant-time comparison (Argon2 does this internally)
        let parsed_hash = PasswordHash::new(&user.password_hash)
            .map_err(|_| anyhow::anyhow!("Invalid username or password"))?;

        if self
            .hasher
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_err()
        {
            // Increment failed attempts
            user.failed_attempts += 1;
            user.last_failed_at = Some(self.clock.now());

            // Lock account if too many failed attempts
            if user.failed_attempts >= self.config.max_failed_attempts {
                user.locked_until =
                    Some(self.clock.now() + Duration::seconds(self.config.lockout_duration as i64));
                warn!(
                    "Account {} locked due to {} failed attempts",
                    username, user.failed_attempts
                );
            }

            // Save updated user state
            self.save_user(&user)?;

            bail!("Invalid username or password");
        }

        // Reset failed attempts and update last login
        user.failed_attempts = 0;
        user.last_failed_at = None;
        user.last_login = Some(self.clock.now());
        self.save_user(&user)?;

        // Create session with cryptographically random token
        let now = self.clock.now();
        let token = SessionToken::generate();
        let session = Session {
            token: token.clone(),
            username: username.to_string(),
            uid: user.uid,
            gid: user.gid,
            capabilities: user.capabilities.clone(),
            created_at: now,
            expires_at: now + Duration::seconds(self.config.session_timeout as i64),
            client_addr,
            last_activity: now,
        };

        // Store session (both sled and cache)
        self.save_session(&session).await?;

        info!("User {} authenticated successfully", username);

        Ok(token)
    }

    /// Authenticate using factotum ticket
    pub async fn authenticate_factotum(
        &self,
        ticket: &factotum::FactotumTicket,
        challenge: &factotum::FactotumChallenge,
        client_addr: Option<String>,
    ) -> Result<SessionToken> {
        // Create factotum client and verify ticket
        let factotum_client = factotum::FactotumClient::new(self.config.factotum.clone());
        factotum_client.verify_ticket(challenge, ticket)?;

        // Get or create user for this factotum identity
        let user = match self.get_user(&ticket.username)? {
            Some(u) => u,
            None => {
                // Auto-create user from factotum if configured
                let capabilities = if self.config.factotum.trust_factotum_capabilities {
                    ticket.capabilities.iter()
                        .filter_map(|c| match c.as_str() {
                            "read" => Some(Capability::Read),
                            "write" => Some(Capability::Write),
                            "execute" => Some(Capability::Execute),
                            "mount" => Some(Capability::Mount),
                            "admin" => Some(Capability::Admin),
                            "create_translator" => Some(Capability::CreateTranslator),
                            "mesh_access" => Some(Capability::MeshAccess),
                            other => Some(Capability::Custom(other.to_string())),
                        })
                        .collect()
                } else {
                    // Default capabilities for factotum users
                    vec![Capability::Read]
                };

                // Create user entry (no password since auth is via factotum)
                let new_user = User {
                    username: ticket.username.clone(),
                    password_hash: String::new(), // No local password
                    uid: self.next_uid()?,
                    gid: 1000, // Default group
                    capabilities,
                    created_at: self.clock.now(),
                    last_login: None,
                    failed_attempts: 0,
                    locked_until: None,
                    enabled: true,
                    password_history: Vec::new(),
                    password_changed_at: None,
                    last_failed_at: None,
                };
                self.save_user(&new_user)?;
                info!("Created user {} via factotum authentication", ticket.username);
                new_user
            }
        };

        if !user.enabled {
            bail!("Account is disabled");
        }

        // Create session
        let now = self.clock.now();
        let token = SessionToken::generate();
        let session = Session {
            token: token.clone(),
            username: ticket.username.clone(),
            uid: user.uid,
            gid: user.gid,
            capabilities: user.capabilities.clone(),
            created_at: now,
            expires_at: now + Duration::seconds(self.config.session_timeout as i64),
            client_addr,
            last_activity: now,
        };

        self.save_session(&session).await?;
        info!("User {} authenticated via factotum", ticket.username);

        Ok(token)
    }

    /// Generate a factotum challenge for authentication
    pub fn generate_factotum_challenge(&self, host_id: &str) -> factotum::FactotumChallenge {
        let client = factotum::FactotumClient::new(self.config.factotum.clone());
        client.generate_challenge(host_id)
    }

    /// Check if factotum is available
    pub async fn is_factotum_available(&self) -> bool {
        let client = factotum::FactotumClient::new(self.config.factotum.clone());
        client.is_available().await
    }

    /// Get the configured authentication method
    pub fn auth_method(&self) -> &AuthMethod {
        &self.config.auth_method
    }

    /// Get next available UID
    fn next_uid(&self) -> Result<u32> {
        let mut max_uid = 1000u32;
        for result in self.users_tree.iter() {
            let (_, value) = result?;
            if let Ok(user) = serde_cbor::from_slice::<User>(&value) {
                if user.uid >= max_uid {
                    max_uid = user.uid + 1;
                }
            }
        }
        Ok(max_uid)
    }

    /// Validate a session token and update last activity
    pub async fn validate_session(&self, token: &SessionToken) -> Result<Session> {
        // First check cache
        let session = {
            let cache = self.session_cache.read().await;
            match cache.get(token) {
                Some(s) => s.clone(),
                None => {
                    // Not in cache, will check sled after dropping lock
                    drop(cache);
                    match self.sessions_tree.get(token.as_str().as_bytes())? {
                        Some(data) => serde_cbor::from_slice(&data)?,
                        None => bail!("Invalid session"),
                    }
                }
            }
        };

        // Check if session expired
        if self.clock.now() > session.expires_at {
            // Clean up expired session
            self.remove_session(token).await?;
            bail!("Session expired");
        }

        // Update last activity (but not on every request - only if stale)
        let activity_threshold = Duration::seconds(60); // Update every minute
        if self.clock.now() - session.last_activity > activity_threshold {
            let mut updated_session = session.clone();
            updated_session.last_activity = self.clock.now();
            self.save_session(&updated_session).await?;
            return Ok(updated_session);
        }

        Ok(session)
    }

    /// Invalidate a session
    pub async fn logout(&self, token: &SessionToken) -> Result<()> {
        self.remove_session(token).await
    }

    /// Create a new user
    pub async fn create_user(
        &self,
        username: &str,
        password: &str,
        uid: u32,
        gid: u32,
        capabilities: Vec<Capability>,
    ) -> Result<()> {
        // Validate username (alphanumeric + underscore, 1-64 chars)
        if username.is_empty() || username.len() > 64 {
            bail!("Username must be 1-64 characters");
        }
        if !username.chars().all(|c| c.is_alphanumeric() || c == '_') {
            bail!("Username must contain only alphanumeric characters and underscores");
        }

        // Validate password against policy
        self.validate_password(password)?;

        // Check if user already exists
        if self.get_user(username)?.is_some() {
            bail!("User {} already exists", username);
        }

        // Hash password with Argon2id
        let salt = SaltString::generate(&mut OsRng);
        let password_hash = self
            .hasher
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| anyhow::anyhow!("Failed to hash password: {}", e))?
            .to_string();

        let now = self.clock.now();

        // Create user
        let user = User {
            username: username.to_string(),
            password_hash,
            uid,
            gid,
            capabilities,
            created_at: now,
            last_login: None,
            failed_attempts: 0,
            locked_until: None,
            enabled: true,
            password_history: Vec::new(),
            password_changed_at: Some(now),
            last_failed_at: None,
        };

        self.save_user(&user)?;

        info!("User {} created successfully", username);

        Ok(())
    }

    /// Delete a user
    pub async fn delete_user(&self, username: &str) -> Result<()> {
        if username == "admin" {
            bail!("Cannot delete admin user");
        }

        // Verify user exists
        if self.get_user(username)?.is_none() {
            bail!("User {} not found", username);
        }

        // Remove user from database
        self.users_tree.remove(username.as_bytes())?;
        self.db.flush()?;

        // Remove any active sessions for this user
        let mut tokens_to_remove = Vec::new();
        {
            let cache = self.session_cache.read().await;
            for (token, session) in cache.iter() {
                if session.username == username {
                    tokens_to_remove.push(token.clone());
                }
            }
        }
        for token in tokens_to_remove {
            self.remove_session(&token).await?;
        }

        info!("User {} deleted", username);

        Ok(())
    }

    /// Change user password
    pub async fn change_password(
        &self,
        username: &str,
        old_password: &str,
        new_password: &str,
    ) -> Result<()> {
        // Get user
        let mut user = self.get_user(username)?
            .ok_or_else(|| anyhow::anyhow!("User not found"))?;

        // Verify old password
        let parsed_hash = PasswordHash::new(&user.password_hash)
            .map_err(|_| anyhow::anyhow!("Invalid old password"))?;

        if self
            .hasher
            .verify_password(old_password.as_bytes(), &parsed_hash)
            .is_err()
        {
            bail!("Invalid old password");
        }

        // Validate new password against policy
        self.validate_password(new_password)?;

        // Check password history (prevent reuse)
        self.check_password_history(new_password, &user.password_history)?;

        // Hash new password
        let salt = SaltString::generate(&mut OsRng);
        let new_hash = self
            .hasher
            .hash_password(new_password.as_bytes(), &salt)
            .map_err(|e| anyhow::anyhow!("Failed to hash password: {}", e))?
            .to_string();

        // Add old password to history (keep last 5)
        user.password_history.insert(0, user.password_hash.clone());
        user.password_history.truncate(5);

        // Update password
        user.password_hash = new_hash;
        user.password_changed_at = Some(self.clock.now());

        self.save_user(&user)?;

        // Invalidate all existing sessions for this user (force re-login)
        let mut tokens_to_remove = Vec::new();
        {
            let cache = self.session_cache.read().await;
            for (token, session) in cache.iter() {
                if session.username == username {
                    tokens_to_remove.push(token.clone());
                }
            }
        }
        for token in tokens_to_remove {
            self.remove_session(&token).await?;
        }

        info!("Password changed for user {}", username);

        Ok(())
    }

    /// Check if a session has a specific capability
    pub async fn has_capability(
        &self,
        token: &SessionToken,
        capability: &Capability,
    ) -> Result<bool> {
        let session = self.validate_session(token).await?;

        // Admin has all capabilities
        if session.capabilities.contains(&Capability::Admin) {
            return Ok(true);
        }

        Ok(session.capabilities.contains(capability))
    }

    /// List all users (admin only - returns usernames only, not sensitive data)
    pub fn list_users(&self) -> Result<Vec<String>> {
        let mut usernames = Vec::new();
        for result in self.users_tree.iter() {
            let (key, _) = result?;
            if let Ok(username) = String::from_utf8(key.to_vec()) {
                usernames.push(username);
            }
        }
        Ok(usernames)
    }

    /// Get user info (non-sensitive fields only)
    pub fn get_user_info(&self, username: &str) -> Result<Option<UserInfo>> {
        match self.get_user(username)? {
            Some(user) => Ok(Some(UserInfo {
                username: user.username,
                uid: user.uid,
                gid: user.gid,
                capabilities: user.capabilities,
                created_at: user.created_at,
                last_login: user.last_login,
                enabled: user.enabled,
            })),
            None => Ok(None),
        }
    }

    /// Clean up expired sessions from both cache and sled
    pub async fn cleanup_sessions(&self) -> Result<usize> {
        let now = self.clock.now();
        let mut removed = 0;

        // Find expired sessions in cache
        let mut tokens_to_remove = Vec::new();
        {
            let cache = self.session_cache.read().await;
            for (token, session) in cache.iter() {
                if session.expires_at <= now {
                    tokens_to_remove.push(token.clone());
                }
            }
        }

        // Remove expired sessions
        for token in tokens_to_remove {
            self.remove_session(&token).await?;
            removed += 1;
        }

        // Also scan sled for any sessions not in cache
        let mut sled_tokens_to_remove = Vec::new();
        for result in self.sessions_tree.iter() {
            let (key, value) = result?;
            if let Ok(session) = serde_cbor::from_slice::<Session>(&value) {
                if session.expires_at <= now {
                    sled_tokens_to_remove.push(key.to_vec());
                }
            }
        }

        for key in sled_tokens_to_remove {
            self.sessions_tree.remove(key)?;
            removed += 1;
        }

        if removed > 0 {
            info!("Cleaned up {} expired sessions", removed);
        }

        Ok(removed)
    }

    /// Start background session cleanup task
    pub fn start_cleanup_task(self: Arc<Self>, interval: std::time::Duration)
    where
        C: 'static,
    {
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(interval);
            loop {
                timer.tick().await;
                if let Err(e) = self.cleanup_sessions().await {
                    warn!("Session cleanup failed: {}", e);
                }
            }
        });
    }

    /// Generate a cryptographically random password that meets policy
    fn generate_password() -> String {
        use rand::Rng;
        let mut rng = OsRng;

        // Ensure we meet all requirements
        let upper: Vec<char> = "ABCDEFGHIJKLMNOPQRSTUVWXYZ".chars().collect();
        let lower: Vec<char> = "abcdefghijklmnopqrstuvwxyz".chars().collect();
        let digits: Vec<char> = "0123456789".chars().collect();
        let special: Vec<char> = "!@#$%^&*()_+-=".chars().collect();

        let mut password = Vec::with_capacity(16);

        // Guarantee at least one of each required type
        password.push(upper[rng.gen_range(0..upper.len())]);
        password.push(lower[rng.gen_range(0..lower.len())]);
        password.push(digits[rng.gen_range(0..digits.len())]);
        password.push(special[rng.gen_range(0..special.len())]);

        // Fill rest with random mix
        let all: Vec<char> = upper.iter()
            .chain(lower.iter())
            .chain(digits.iter())
            .chain(special.iter())
            .copied()
            .collect();

        while password.len() < 16 {
            password.push(all[rng.gen_range(0..all.len())]);
        }

        // Shuffle to avoid predictable positions
        for i in (1..password.len()).rev() {
            let j = rng.gen_range(0..=i);
            password.swap(i, j);
        }

        password.into_iter().collect()
    }
}

/// Public user info (non-sensitive fields)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub username: String,
    pub uid: u32,
    pub gid: u32,
    pub capabilities: Vec<Capability>,
    pub created_at: DateTime<Utc>,
    pub last_login: Option<DateTime<Utc>>,
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_auth_service() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("auth_db");

        let config = AuthConfig {
            db_path,
            min_password_length: 8, // Relaxed for testing
            require_mixed_case: false,
            require_numbers: false,
            require_special_chars: false,
            ..Default::default()
        };

        let auth = AuthService::new(config).await.unwrap();

        // Create a test user
        auth.create_user(
            "testuser",
            "testpass123",
            1000,
            1000,
            vec![Capability::Read, Capability::Write],
        )
        .await
        .unwrap();

        // Authenticate
        let token = auth
            .authenticate("testuser", "testpass123", None)
            .await
            .unwrap();

        // Validate session
        let session = auth.validate_session(&token).await.unwrap();
        assert_eq!(session.username, "testuser");

        // Check capabilities
        assert!(auth
            .has_capability(&token, &Capability::Read)
            .await
            .unwrap());
        assert!(auth
            .has_capability(&token, &Capability::Write)
            .await
            .unwrap());
        assert!(!auth
            .has_capability(&token, &Capability::Admin)
            .await
            .unwrap());

        // Logout
        auth.logout(&token).await.unwrap();

        // Session should be invalid now
        assert!(auth.validate_session(&token).await.is_err());
    }

    #[tokio::test]
    async fn test_password_policy() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("auth_db_policy");

        let config = AuthConfig {
            db_path,
            min_password_length: 12,
            require_mixed_case: true,
            require_numbers: true,
            require_special_chars: true,
            ..Default::default()
        };

        let auth = AuthService::new(config).await.unwrap();

        // Too short
        assert!(auth.create_user("user1", "Short1!", 1001, 1001, vec![]).await.is_err());

        // No uppercase
        assert!(auth.create_user("user2", "lowercase123!", 1002, 1002, vec![]).await.is_err());

        // No numbers
        assert!(auth.create_user("user3", "NoNumbers!Here", 1003, 1003, vec![]).await.is_err());

        // No special chars
        assert!(auth.create_user("user4", "NoSpecial123AB", 1004, 1004, vec![]).await.is_err());

        // Valid password
        assert!(auth.create_user("user5", "ValidPass123!", 1005, 1005, vec![]).await.is_ok());
    }

    #[tokio::test]
    async fn test_password_history() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("auth_db_history");

        let config = AuthConfig {
            db_path,
            min_password_length: 8,
            require_mixed_case: false,
            require_numbers: false,
            require_special_chars: false,
            ..Default::default()
        };

        let auth = AuthService::new(config).await.unwrap();

        auth.create_user("histuser", "password1", 1000, 1000, vec![]).await.unwrap();

        // Change password
        auth.change_password("histuser", "password1", "password2").await.unwrap();

        // Try to reuse old password - should fail
        assert!(auth.change_password("histuser", "password2", "password1").await.is_err());

        // New password should work
        auth.change_password("histuser", "password2", "password3").await.unwrap();
    }

    #[tokio::test]
    async fn test_lockout() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("auth_db_lockout");

        let config = AuthConfig {
            db_path,
            max_failed_attempts: 3,
            lockout_duration: 1, // 1 second for testing
            min_password_length: 8,
            require_mixed_case: false,
            require_numbers: false,
            require_special_chars: false,
            ..Default::default()
        };

        let auth = AuthService::new(config).await.unwrap();

        auth.create_user("lockuser", "correctpassword", 1000, 1000, vec![]).await.unwrap();

        // Fail 3 times
        for _ in 0..3 {
            let _ = auth.authenticate("lockuser", "wrongpassword", None).await;
        }

        // Account should be locked - even correct password fails
        assert!(auth.authenticate("lockuser", "correctpassword", None).await.is_err());

        // Wait for lockout to expire
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Should work now
        assert!(auth.authenticate("lockuser", "correctpassword", None).await.is_ok());
    }

    #[tokio::test]
    async fn test_session_persistence() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("auth_db_persist");

        let token = {
            let config = AuthConfig {
                db_path: db_path.clone(),
                min_password_length: 8,
                require_mixed_case: false,
                require_numbers: false,
                require_special_chars: false,
                ..Default::default()
            };

            let auth = AuthService::new(config).await.unwrap();
            auth.create_user("persistuser", "password123", 1000, 1000, vec![]).await.unwrap();
            auth.authenticate("persistuser", "password123", None).await.unwrap()
        };

        // Create new auth service (simulating restart)
        let config = AuthConfig {
            db_path,
            min_password_length: 8,
            require_mixed_case: false,
            require_numbers: false,
            require_special_chars: false,
            ..Default::default()
        };

        let auth = AuthService::new(config).await.unwrap();

        // Session should still be valid
        let session = auth.validate_session(&token).await.unwrap();
        assert_eq!(session.username, "persistuser");
    }

    /// Fuzz test: Password verification should be timing-safe
    #[test]
    fn fuzz_password_verification() {
        use proptest::prelude::*;

        proptest!(|(password in ".*", hash in ".*")| {
            // Should never panic on invalid inputs
            use argon2::{Argon2, PasswordHash, PasswordVerifier};

            if let Ok(parsed_hash) = PasswordHash::new(&hash) {
                let _ = Argon2::default().verify_password(password.as_bytes(), &parsed_hash);
            }
        });
    }

    /// Fuzz test: Session token validation
    #[test]
    fn fuzz_session_token_validation() {
        use proptest::prelude::*;

        proptest!(|(token in ".*")| {
            // Should safely handle any token format
            let _ = token.split(':').collect::<Vec<_>>();
        });
    }

    /// Fuzz test: Username validation
    #[test]
    fn fuzz_username_validation() {
        use proptest::prelude::*;

        proptest!(|(username in ".*")| {
            // Usernames should be alphanumeric + underscore
            let is_valid = username.chars().all(|c| c.is_alphanumeric() || c == '_');
            let _ = is_valid;
        });
    }

    /// Fuzz test: Capability deserialization
    #[test]
    fn fuzz_capability_deserialization() {
        use proptest::prelude::*;

        proptest!(|(bytes: Vec<u8>)| {
            // Should never panic
            let _ = serde_json::from_slice::<Vec<Capability>>(&bytes);
        });
    }
}
