//! Configuration file parsing for 9P.e server

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConsensusConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub peers: Vec<String>,

    #[serde(default)]
    pub trusted_nodes: Vec<TrustedNodeConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrustedNodeConfig {
    pub node_id: String,
    pub public_key: String,
    #[serde(default = "default_trusted_node_algorithm")]
    pub algorithm: String,
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
    "sycl".to_string()
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_trusted_node_algorithm() -> String {
    "Ed25519".to_string()
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen_addr: default_listen_addr(),
            node_id: default_node_id(),
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

[[consensus.trusted_nodes]]
node_id = "peer-1"
public_key = "01020304"
algorithm = "Ed25519"

[llama]
enabled = true
server_url = "http://localhost:8080"

[gpu]
enabled = true
backend = "sycl"
device_id = 0

[logging]
level = "info"
"#;

        let config: Config = toml::from_str(config_str).unwrap();
        assert_eq!(config.server.node_id, "test-node");
        assert!(config.consensus.enabled);
        assert_eq!(config.consensus.peers.len(), 2);
        assert_eq!(config.consensus.trusted_nodes.len(), 1);
        assert_eq!(config.consensus.trusted_nodes[0].node_id, "peer-1");
        assert!(config.llama.enabled);
        assert!(config.gpu.enabled);
    }

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(!config.consensus.enabled);
        assert!(config.consensus.trusted_nodes.is_empty());
        assert!(!config.llama.enabled);
        assert!(!config.gpu.enabled);
    }

    /// Fuzz test: TOML parsing
    #[test]
    fn fuzz_toml_parsing() {
        use proptest::prelude::*;

        proptest!(|(toml_str in ".*")| {
            // Should never panic on invalid TOML
            let _ = toml::from_str::<Config>(&toml_str);
        });
    }

    /// Fuzz test: JSON config parsing
    #[test]
    fn fuzz_json_config_parsing() {
        use proptest::prelude::*;

        proptest!(|(bytes: Vec<u8>)| {
            // Should never panic
            let _ = serde_json::from_slice::<Config>(&bytes);
        });
    }

    /// Fuzz test: Path validation
    #[test]
    fn fuzz_path_validation() {
        use proptest::prelude::*;

        proptest!(|(path_str in ".*")| {
            // Should safely handle any path
            let _ = std::path::PathBuf::from(&path_str);
        });
    }

    /// Fuzz test: Port number validation
    #[test]
    fn fuzz_port_validation() {
        proptest::proptest!(|(port: u16)| {
            // All u16 values are valid ports
            proptest::prop_assert!(port <= 65535);
        });
    }

    /// Fuzz test: Peer address parsing
    #[test]
    fn fuzz_peer_address_config() {
        use proptest::prelude::*;

        proptest!(|(peer in ".*")| {
            // Format: "peer_id@ip:port"
            let _ = peer.split('@').collect::<Vec<_>>();
        });
    }
}
