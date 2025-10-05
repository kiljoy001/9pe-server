//! Configuration file parsing for 9P.e server

use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,

    #[serde(default)]
    pub consensus: ConsensusConfig,

    #[serde(default)]
    pub llama: LlamaConfig,

    #[serde(default)]
    pub gpu: GpuConfig,

    #[serde(default)]
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_listen_addr")]
    pub listen_addr: String,

    #[serde(default = "default_node_id")]
    pub node_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub peers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlamaConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_llama_url")]
    pub server_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_gpu_backend")]
    pub backend: String,

    #[serde(default)]
    pub device_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
}

// Defaults
fn default_listen_addr() -> String {
    "0.0.0.0:5640".to_string()
}

fn default_node_id() -> String {
    format!("node-{}", uuid::Uuid::new_v4())
}

fn default_llama_url() -> String {
    "http://localhost:8080".to_string()
}

fn default_gpu_backend() -> String {
    "opencl".to_string()
}

fn default_log_level() -> String {
    "info".to_string()
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen_addr: default_listen_addr(),
            node_id: default_node_id(),
        }
    }
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            peers: Vec::new(),
        }
    }
}

impl Default for LlamaConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            server_url: default_llama_url(),
        }
    }
}

impl Default for GpuConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            backend: default_gpu_backend(),
            device_id: 0,
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            consensus: ConsensusConfig::default(),
            llama: LlamaConfig::default(),
            gpu: GpuConfig::default(),
            logging: LoggingConfig::default(),
        }
    }
}

impl Config {
    /// Load configuration from TOML file
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

        Ok(config)
    }

    /// Create default config
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sample_config() {
        let config_str = r#"
[server]
listen_addr = "0.0.0.0:9009"
node_id = "test-node"

[consensus]
enabled = true
peers = ["192.168.1.2:9009", "[::1]:9009"]

[llama]
enabled = true
server_url = "http://localhost:8080"

[gpu]
enabled = true
backend = "opencl"
device_id = 0

[logging]
level = "info"
"#;

        let config: Config = toml::from_str(config_str).unwrap();
        assert_eq!(config.server.node_id, "test-node");
        assert!(config.consensus.enabled);
        assert_eq!(config.consensus.peers.len(), 2);
        assert!(config.llama.enabled);
        assert!(config.gpu.enabled);
    }

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(!config.consensus.enabled);
        assert!(!config.llama.enabled);
        assert!(!config.gpu.enabled);
    }
}
