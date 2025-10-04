//! Authentication and Authorization Module
//!
//! Provides secure user authentication for the 9P.e server with:
//! - Argon2id password hashing
//! - Session management
//! - Capability-based authorization
//! - User database persistence

mod clock;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::{Result, Context, bail};
use serde::{Deserialize, Serialize};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rand::rngs::OsRng;
use tracing::{info, warn};
use uuid::Uuid;
use chrono::{DateTime, Utc, Duration};

pub use clock::{Clock, RealClock, MockClock};

/// Authentication service for managing users and sessions
pub struct AuthService<C: Clock = RealClock> {
    /// User database
    users: Arc<RwLock<UserDatabase>>,

    /// Active sessions
    sessions: Arc<RwLock<HashMap<SessionToken, Session>>>,

    /// Configuration
    config: AuthConfig,

    /// Argon2 hasher
    hasher: Argon2<'static>,

    /// Clock for time operations
    clock: C,
}

/// Authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Database file path
    pub db_path: PathBuf,

    /// Session timeout in seconds
    pub session_timeout: u64,

    /// Maximum failed login attempts before lockout
    pub max_failed_attempts: u32,

    /// Lockout duration in seconds
    pub lockout_duration: u64,

    /// Enable anonymous access
    pub allow_anonymous: bool,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            db_path: PathBuf::from("/etc/9pe/users.db"),
            session_timeout: 3600,  // 1 hour
            max_failed_attempts: 5,
            lockout_duration: 300,   // 5 minutes
            allow_anonymous: false,
        }
    }
}

/// User database
#[derive(Debug, Clone, Serialize, Deserialize)]
struct UserDatabase {
    /// Map of username to user record
    users: HashMap<String, User>,

    /// Database version for migration support
    version: u32,
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
#[derive(Debug, Clone)]
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
}

/// Session token
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct SessionToken(String);

impl SessionToken {
    /// Generate a new random session token
    fn generate() -> Self {
        SessionToken(Uuid::new_v4().to_string())
    }

    /// Get the token string
    pub fn as_str(&self) -> &str {
        &self.0
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

        // Load or create user database
        let users = if config.db_path.exists() {
            Self::load_database(&config.db_path).await?
        } else {
            // Create new database with default admin user
            let mut db = UserDatabase {
                users: HashMap::new(),
                version: 1,
            };

            // Create default admin user
            let admin_password = Self::generate_password();
            info!("Creating default admin user with password: {}", admin_password);

            let salt = SaltString::generate(&mut OsRng);
            let password_hash = hasher
                .hash_password(admin_password.as_bytes(), &salt)
                .map_err(|e| anyhow::anyhow!("Failed to hash password: {}", e))?
                .to_string();

            db.users.insert("admin".to_string(), User {
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
            });

            // Save the database
            Self::save_database(&config.db_path, &db).await?;

            db
        };

        Ok(Self {
            users: Arc::new(RwLock::new(users)),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            config,
            hasher,
            clock,
        })
    }

    /// Authenticate a user with username and password
    pub async fn authenticate(
        &self,
        username: &str,
        password: &str,
        client_addr: Option<String>,
    ) -> Result<SessionToken> {
        let mut users = self.users.write().await;

        // Check if user exists
        let user = users.users.get_mut(username)
            .ok_or_else(|| anyhow::anyhow!("Invalid username or password"))?;

        // Check if account is locked
        if let Some(locked_until) = user.locked_until {
            if self.clock.now() < locked_until {
                bail!("Account is locked until {}", locked_until);
            } else {
                // Clear lockout
                user.locked_until = None;
                user.failed_attempts = 0;
            }
        }

        // Check if account is enabled
        if !user.enabled {
            bail!("Account is disabled");
        }

        // Verify password
        let parsed_hash = PasswordHash::new(&user.password_hash)
            .map_err(|e| anyhow::anyhow!("Invalid password hash: {}", e))?;

        if self.hasher.verify_password(password.as_bytes(), &parsed_hash).is_err() {
            // Increment failed attempts
            user.failed_attempts += 1;

            // Lock account if too many failed attempts
            if user.failed_attempts >= self.config.max_failed_attempts {
                user.locked_until = Some(
                    self.clock.now() + Duration::seconds(self.config.lockout_duration as i64)
                );
                warn!("Account {} locked due to too many failed attempts", username);
            }

            // Save database
            Self::save_database(&self.config.db_path, &users).await?;

            bail!("Invalid username or password");
        }

        // Reset failed attempts and update last login
        user.failed_attempts = 0;
        user.last_login = Some(self.clock.now());

        // Create session
        let token = SessionToken::generate();
        let session = Session {
            token: token.clone(),
            username: username.to_string(),
            uid: user.uid,
            gid: user.gid,
            capabilities: user.capabilities.clone(),
            created_at: self.clock.now(),
            expires_at: self.clock.now() + Duration::seconds(self.config.session_timeout as i64),
            client_addr,
        };

        // Store session
        let mut sessions = self.sessions.write().await;
        sessions.insert(token.clone(), session);

        // Save database
        Self::save_database(&self.config.db_path, &users).await?;

        info!("User {} authenticated successfully", username);

        Ok(token)
    }

    /// Validate a session token
    pub async fn validate_session(&self, token: &SessionToken) -> Result<Session> {
        let sessions = self.sessions.read().await;

        let session = sessions.get(token)
            .ok_or_else(|| anyhow::anyhow!("Invalid session"))?;

        // Check if session expired
        if self.clock.now() > session.expires_at {
            bail!("Session expired");
        }

        Ok(session.clone())
    }

    /// Invalidate a session
    pub async fn logout(&self, token: &SessionToken) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(token);
        Ok(())
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
        let mut users = self.users.write().await;

        // Check if user already exists
        if users.users.contains_key(username) {
            bail!("User {} already exists", username);
        }

        // Hash password
        let salt = SaltString::generate(&mut OsRng);
        let password_hash = self.hasher
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| anyhow::anyhow!("Failed to hash password: {}", e))?
            .to_string();

        // Create user
        users.users.insert(username.to_string(), User {
            username: username.to_string(),
            password_hash,
            uid,
            gid,
            capabilities,
            created_at: Utc::now(),
            last_login: None,
            failed_attempts: 0,
            locked_until: None,
            enabled: true,
        });

        // Save database
        Self::save_database(&self.config.db_path, &users).await?;

        info!("User {} created successfully", username);

        Ok(())
    }

    /// Delete a user
    pub async fn delete_user(&self, username: &str) -> Result<()> {
        let mut users = self.users.write().await;

        if username == "admin" {
            bail!("Cannot delete admin user");
        }

        users.users.remove(username)
            .ok_or_else(|| anyhow::anyhow!("User {} not found", username))?;

        // Save database
        Self::save_database(&self.config.db_path, &users).await?;

        // Remove any active sessions for this user
        let mut sessions = self.sessions.write().await;
        sessions.retain(|_, session| session.username != username);

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
        let mut users = self.users.write().await;

        let user = users.users.get_mut(username)
            .ok_or_else(|| anyhow::anyhow!("User not found"))?;

        // Verify old password
        let parsed_hash = PasswordHash::new(&user.password_hash)
            .map_err(|e| anyhow::anyhow!("Invalid password hash: {}", e))?;

        if self.hasher.verify_password(old_password.as_bytes(), &parsed_hash).is_err() {
            bail!("Invalid old password");
        }

        // Hash new password
        let salt = SaltString::generate(&mut OsRng);
        let password_hash = self.hasher
            .hash_password(new_password.as_bytes(), &salt)
            .map_err(|e| anyhow::anyhow!("Failed to hash password: {}", e))?
            .to_string();

        user.password_hash = password_hash;

        // Save database
        Self::save_database(&self.config.db_path, &users).await?;

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

    /// Clean up expired sessions
    pub async fn cleanup_sessions(&self) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        let now = self.clock.now();

        sessions.retain(|_, session| session.expires_at > now);

        Ok(())
    }

    /// Load user database from file
    async fn load_database(path: &Path) -> Result<UserDatabase> {
        let data = tokio::fs::read(path).await
            .context("Failed to read user database")?;

        let db: UserDatabase = bincode::deserialize(&data)
            .context("Failed to deserialize user database")?;

        Ok(db)
    }

    /// Save user database to file
    async fn save_database(path: &Path, db: &UserDatabase) -> Result<()> {
        // Create directory if it doesn't exist
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await
                .context("Failed to create database directory")?;
        }

        let data = bincode::serialize(db)
            .context("Failed to serialize user database")?;

        tokio::fs::write(path, data).await
            .context("Failed to write user database")?;

        Ok(())
    }

    /// Generate a random password
    fn generate_password() -> String {
        use rand::Rng;
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ\
                                abcdefghijklmnopqrstuvwxyz\
                                0123456789!@#$%^&*";
        let mut rng = rand::thread_rng();

        (0..16)
            .map(|_| {
                let idx = rng.gen_range(0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_auth_service() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("users.db");

        let config = AuthConfig {
            db_path,
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
        ).await.unwrap();

        // Authenticate
        let token = auth.authenticate("testuser", "testpass123", None).await.unwrap();

        // Validate session
        let session = auth.validate_session(&token).await.unwrap();
        assert_eq!(session.username, "testuser");

        // Check capabilities
        assert!(auth.has_capability(&token, &Capability::Read).await.unwrap());
        assert!(auth.has_capability(&token, &Capability::Write).await.unwrap());
        assert!(!auth.has_capability(&token, &Capability::Admin).await.unwrap());

        // Logout
        auth.logout(&token).await.unwrap();

        // Session should be invalid now
        assert!(auth.validate_session(&token).await.is_err());
    }
}