//! Configuration management for 9P.e server
//!
//! Handles first-run setup, configuration persistence, and settings management

use std::path::{Path, PathBuf};
use std::fs;
use anyhow::{Result, Context, bail};
use serde::{Serialize, Deserialize};
use tracing::{info, warn, debug};
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use dialoguer::{Input, Password, Confirm, Select};
use directories::ProjectDirs;

/// Main server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Server instance ID
    pub server_id: String,

    /// Server name (human-readable)
    pub server_name: String,

    /// Network binding configuration
    pub network: NetworkConfig,

    /// Authentication configuration
    pub auth: AuthConfig,

    /// Namespace configuration
    pub namespace: NamespaceConfig,

    /// Storage paths
    pub storage: StorageConfig,

    /// First run completed flag
    pub initialized: bool,

    /// Server version that created this config
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub bind_address: String,
    pub port: u16,
    pub mesh_port: Option<u16>,
    pub metrics_port: Option<u16>,
    pub quic_enabled: bool,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Path to user database
    pub user_db_path: PathBuf,

    /// Admin username
    pub admin_username: String,

    /// Admin password hash (Argon2id)
    pub admin_password_hash: String,

    /// Require authentication for all connections
    pub mandatory_auth: bool,

    /// Allow anonymous read-only access
    pub allow_anonymous: bool,

    /// Session timeout in seconds
    pub session_timeout: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceConfig {
    /// Default namespace name
    pub default_namespace: String,

    /// Namespace root path
    pub namespace_root: PathBuf,

    /// Enable /srv directory for service discovery
    pub enable_srv: bool,

    /// Enable /n/ directory for namespace mounting
    pub enable_n: bool,

    /// M-of-N threshold for namespace operations
    pub threshold: Option<ThresholdConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdConfig {
    pub m: u32,  // Required signatures
    pub n: u32,  // Total members
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Root directory for serving files
    pub root_dir: PathBuf,

    /// Data directory for server state
    pub data_dir: PathBuf,

    /// Log directory
    pub log_dir: PathBuf,

    /// Enable content caching
    pub enable_cache: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            server_id: uuid::Uuid::new_v4().to_string(),
            server_name: gethostname::gethostname().to_string_lossy().to_string(),
            network: NetworkConfig {
                bind_address: "0.0.0.0".to_string(),
                port: 5640,
                mesh_port: Some(9650),
                metrics_port: Some(9090),
                quic_enabled: false,
                tls_cert: None,
                tls_key: None,
            },
            auth: AuthConfig {
                user_db_path: PathBuf::from("~/.9pe/users.db"),
                admin_username: "admin".to_string(),
                admin_password_hash: String::new(), // Will be set during setup
                mandatory_auth: true,
                allow_anonymous: false,
                session_timeout: 3600,
            },
            namespace: NamespaceConfig {
                default_namespace: "local".to_string(),
                namespace_root: PathBuf::from("~/.9pe/namespaces"),
                enable_srv: true,
                enable_n: true,
                threshold: None,
            },
            storage: StorageConfig {
                root_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                data_dir: PathBuf::from("~/.9pe/data"),
                log_dir: PathBuf::from("~/.9pe/logs"),
                enable_cache: true,
            },
            initialized: false,
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// Configuration manager
pub struct ConfigManager {
    config_path: PathBuf,
    config: Option<ServerConfig>,
}

impl ConfigManager {
    /// Create a new configuration manager
    pub fn new() -> Result<Self> {
        let config_dir = Self::get_config_dir()?;
        let config_path = config_dir.join("config.toml");

        Ok(Self {
            config_path,
            config: None,
        })
    }

    /// Get the configuration directory, creating it if necessary
    fn get_config_dir() -> Result<PathBuf> {
        if let Some(proj_dirs) = ProjectDirs::from("com", "9pe", "server") {
            let config_dir = proj_dirs.config_dir();
            fs::create_dir_all(config_dir)?;
            Ok(config_dir.to_path_buf())
        } else {
            // Fallback to ~/.9pe
            let home = std::env::var("HOME").context("HOME not set")?;
            let config_dir = PathBuf::from(home).join(".9pe");
            fs::create_dir_all(&config_dir)?;
            Ok(config_dir)
        }
    }

    /// Check if this is the first run
    pub fn is_first_run(&self) -> bool {
        !self.config_path.exists()
    }

    /// Load existing configuration
    pub async fn load_config(&mut self) -> Result<ServerConfig> {
        if !self.config_path.exists() {
            bail!("Configuration file not found. Run setup first.");
        }

        let contents = tokio::fs::read_to_string(&self.config_path).await?;
        let config: ServerConfig = toml::from_str(&contents)?;

        // Validate config version compatibility
        if config.version != env!("CARGO_PKG_VERSION") {
            warn!("Config version mismatch. Config: {}, Server: {}",
                  config.version, env!("CARGO_PKG_VERSION"));
        }

        self.config = Some(config.clone());
        Ok(config)
    }

    /// Save configuration to disk
    pub async fn save_config(&mut self, config: &ServerConfig) -> Result<()> {
        let config_str = toml::to_string_pretty(config)?;

        // Ensure directory exists
        if let Some(parent) = self.config_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Write atomically (write to temp file then rename)
        let temp_path = self.config_path.with_extension("tmp");
        tokio::fs::write(&temp_path, config_str).await?;
        tokio::fs::rename(temp_path, &self.config_path).await?;

        // Set restrictive permissions (owner read/write only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = tokio::fs::metadata(&self.config_path).await?.permissions();
            perms.set_mode(0o600);
            tokio::fs::set_permissions(&self.config_path, perms).await?;
        }

        self.config = Some(config.clone());
        info!("Configuration saved to {:?}", self.config_path);
        Ok(())
    }

    /// Run interactive setup wizard
    pub async fn run_setup_wizard(&mut self) -> Result<ServerConfig> {
        println!("\n🚀 Welcome to 9P.e Server Setup Wizard\n");
        println!("This wizard will help you configure your server for first-time use.\n");

        let mut config = ServerConfig::default();

        // Server name
        config.server_name = Input::new()
            .with_prompt("Server name")
            .default(config.server_name)
            .interact_text()?;

        // Network configuration
        println!("\n📡 Network Configuration");
        config.network.bind_address = Input::new()
            .with_prompt("Bind address")
            .default(config.network.bind_address)
            .interact_text()?;

        config.network.port = Input::new()
            .with_prompt("Server port")
            .default(config.network.port)
            .interact_text()?;

        config.network.quic_enabled = Confirm::new()
            .with_prompt("Enable QUIC transport?")
            .default(false)
            .interact()?;

        // Authentication setup
        println!("\n🔐 Authentication Setup");
        config.auth.admin_username = Input::new()
            .with_prompt("Admin username")
            .default("admin".to_string())
            .interact_text()?;

        let password = Password::new()
            .with_prompt("Admin password")
            .with_confirmation("Confirm password", "Passwords do not match")
            .interact()?;

        // Hash the password with Argon2id
        use argon2::{
            password_hash::{PasswordHasher, SaltString},
            Argon2,
        };
        let salt = SaltString::generate(&mut rand::thread_rng());
        let argon2 = Argon2::default();
        config.auth.admin_password_hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| anyhow::anyhow!("Failed to hash password: {}", e))?
            .to_string();

        config.auth.mandatory_auth = Confirm::new()
            .with_prompt("Require authentication for all connections?")
            .default(true)
            .interact()?;

        // Namespace configuration
        println!("\n📁 Namespace Configuration");
        config.namespace.default_namespace = Input::new()
            .with_prompt("Default namespace name")
            .default("local".to_string())
            .interact_text()?;

        let enable_threshold = Confirm::new()
            .with_prompt("Enable M-of-N threshold signatures for namespace operations?")
            .default(false)
            .interact()?;

        if enable_threshold {
            let m: u32 = Input::new()
                .with_prompt("Required signatures (M)")
                .default(2)
                .interact_text()?;

            let n: u32 = Input::new()
                .with_prompt("Total members (N)")
                .default(3)
                .validate_with(|input: &u32| -> Result<(), &str> {
                    if *input >= m {
                        Ok(())
                    } else {
                        Err("N must be >= M")
                    }
                })
                .interact_text()?;

            config.namespace.threshold = Some(ThresholdConfig { m, n });
        }

        // Storage configuration
        println!("\n💾 Storage Configuration");
        let root_dir = Input::new()
            .with_prompt("Root directory to serve")
            .default(config.storage.root_dir.to_string_lossy().to_string())
            .interact_text()?;
        config.storage.root_dir = PathBuf::from(root_dir);

        // Expand paths
        config = self.expand_paths(config)?;

        // Mark as initialized
        config.initialized = true;

        // Save configuration
        self.save_config(&config).await?;

        println!("\n✅ Setup complete! Configuration saved to {:?}", self.config_path);
        println!("\n🔐 Admin credentials:");
        println!("   Username: {}", config.auth.admin_username);
        println!("   Password: [the password you entered]");
        println!("\n⚠️  Please save these credentials securely!");

        Ok(config)
    }

    /// Expand ~ in paths
    fn expand_paths(&self, mut config: ServerConfig) -> Result<ServerConfig> {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());

        let expand = |path: &PathBuf| -> PathBuf {
            if path.starts_with("~") {
                PathBuf::from(&home).join(path.strip_prefix("~").unwrap())
            } else {
                path.clone()
            }
        };

        config.auth.user_db_path = expand(&config.auth.user_db_path);
        config.namespace.namespace_root = expand(&config.namespace.namespace_root);
        config.storage.data_dir = expand(&config.storage.data_dir);
        config.storage.log_dir = expand(&config.storage.log_dir);

        Ok(config)
    }

    /// Initialize or load configuration
    pub async fn initialize(&mut self) -> Result<ServerConfig> {
        if self.is_first_run() {
            info!("First run detected. Starting setup wizard...");
            self.run_setup_wizard().await
        } else {
            info!("Loading existing configuration from {:?}", self.config_path);
            self.load_config().await
        }
    }

    /// Get current configuration
    pub fn get_config(&self) -> Option<&ServerConfig> {
        self.config.as_ref()
    }

    /// Update configuration
    pub async fn update_config<F>(&mut self, updater: F) -> Result<()>
    where
        F: FnOnce(&mut ServerConfig),
    {
        if let Some(ref mut config) = self.config {
            updater(config);
            let config_clone = config.clone();
            self.save_config(&config_clone).await?;
        } else {
            bail!("No configuration loaded");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_config_persistence() {
        let temp = tempdir().unwrap();
        std::env::set_var("HOME", temp.path());

        let mut manager = ConfigManager::new().unwrap();
        assert!(manager.is_first_run());

        let config = ServerConfig::default();
        manager.save_config(&config).await.unwrap();

        assert!(!manager.is_first_run());

        let loaded = manager.load_config().await.unwrap();
        assert_eq!(loaded.server_id, config.server_id);
    }
}