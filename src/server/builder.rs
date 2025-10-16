//! Server builder with dependency injection

use anyhow::Result;
use std::path::PathBuf;

use super::{Server, ServerConfig};
use crate::network::NetworkConfig;
use crate::transport::TransportType;

/// Builder for creating a Server with dependency injection
pub struct ServerBuilder {
    network_config: Option<NetworkConfig>,
    transport: Option<TransportType>,
    root_directory: Option<PathBuf>,
    max_message_size: u32,
    worker_threads: Option<usize>,
    mesh_enabled: bool,
    mesh_port: u16,
    metrics_enabled: bool,
    metrics_port: u16,
    translator_directory: Option<PathBuf>,
    settrans_directory: Option<PathBuf>,
    auto_mount_enabled: bool,
    config: Option<crate::config::Config>,
}

impl Default for ServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerBuilder {
    pub fn new() -> Self {
        Self {
            network_config: None,
            transport: None,
            root_directory: None,
            max_message_size: 8 * 1024 * 1024, // 8MB default
            worker_threads: None,
            mesh_enabled: true,
            mesh_port: 9650,
            metrics_enabled: true,
            metrics_port: 9090,
            translator_directory: None,
            settrans_directory: None,
            auto_mount_enabled: true, // Auto-mount enabled by default
            config: None,
        }
    }

    pub fn with_config(mut self, config: crate::config::Config) -> Self {
        self.config = Some(config);
        self
    }

    pub fn network_config(mut self, config: NetworkConfig) -> Self {
        self.network_config = Some(config);
        self
    }

    pub fn transport(mut self, transport: TransportType) -> Self {
        self.transport = Some(transport);
        self
    }

    pub fn root_directory(mut self, path: PathBuf) -> Self {
        self.root_directory = Some(path);
        self
    }

    pub fn max_message_size(mut self, size: u32) -> Self {
        self.max_message_size = size;
        self
    }

    pub fn worker_threads(mut self, threads: Option<usize>) -> Self {
        self.worker_threads = threads;
        self
    }

    pub fn mesh_enabled(mut self, enabled: bool) -> Self {
        self.mesh_enabled = enabled;
        self
    }

    pub fn mesh_port(mut self, port: u16) -> Self {
        self.mesh_port = port;
        self
    }

    pub fn metrics_enabled(mut self, enabled: bool) -> Self {
        self.metrics_enabled = enabled;
        self
    }

    pub fn metrics_port(mut self, port: u16) -> Self {
        self.metrics_port = port;
        self
    }

    pub fn translator_directory(mut self, path: PathBuf) -> Self {
        self.translator_directory = Some(path);
        self
    }

    pub fn settrans_directory(mut self, path: PathBuf) -> Self {
        self.settrans_directory = Some(path);
        self
    }

    pub fn auto_mount_enabled(mut self, enabled: bool) -> Self {
        self.auto_mount_enabled = enabled;
        self
    }

    pub async fn build(self) -> Result<Server> {
        // Use user home directory for all 9pe paths
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let ninep_home = PathBuf::from(&home).join(".9pe");

        // Extract consensus config and node_id from file config if present
        let (consensus_config, node_id) = if let Some(ref file_config) = self.config {
            let consensus = if file_config.consensus.enabled {
                Some(file_config.consensus.clone())
            } else {
                None
            };
            (consensus, file_config.server.node_id.clone())
        } else {
            (None, format!("node-{}", uuid::Uuid::new_v4()))
        };

        let config = ServerConfig {
            network: self.network_config.unwrap_or_default(),
            transport: self.transport.unwrap_or_default(),
            root_directory: self.root_directory.unwrap_or_else(|| PathBuf::from(".")),
            max_message_size: self.max_message_size,
            worker_threads: self.worker_threads,
            mesh_enabled: self.mesh_enabled,
            mesh_port: self.mesh_port,
            metrics_enabled: self.metrics_enabled,
            metrics_port: self.metrics_port,
            translator_directory: self
                .translator_directory
                .unwrap_or_else(|| ninep_home.join("translators")),
            settrans_directory: self
                .settrans_directory
                .unwrap_or_else(|| ninep_home.join("settrans")),
            auto_mount_enabled: self.auto_mount_enabled,
            consensus_config,
            node_id,
        };

        Server::new(config).await
    }
}
