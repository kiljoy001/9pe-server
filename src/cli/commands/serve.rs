//! Serve command implementation with QUIC as default

use clap::Args;
use anyhow::{Result, Context};
use std::path::PathBuf;
use tracing::info;

use crate::network::NetworkConfig;
use crate::server::ServerBuilder;
use crate::transport::TransportType;

/// Start the 9P.e server
#[derive(Args, Debug)]
pub struct ServeCommand {
    /// Port to listen on
    #[arg(short, long, default_value = "5640")]
    pub port: u16,

    /// Interface to bind to (e.g., localhost, any, ::1, 0.0.0.0)
    /// Defaults to IPv6 dual-stack (::) which accepts both IPv6 and IPv4
    #[arg(short, long)]
    pub bind: Option<String>,

    /// Root directory to serve
    #[arg(short, long, default_value = ".")]
    pub root: PathBuf,

    /// Use QUIC transport with encryption (default: enabled for modern networking)
    /// Use --no-quic to disable and fall back to legacy TCP
    #[arg(long, default_value = "true")]
    pub quic: bool,

    /// Disable QUIC and use legacy TCP
    #[arg(long = "no-quic", conflicts_with = "quic")]
    pub no_quic: bool,

    /// Server name for QUIC TLS certificate (optional, only needed by clients)
    #[arg(short = 'n', long)]
    pub server_name: Option<String>,

    /// Mesh networking port
    #[arg(long, default_value = "9650")]
    pub mesh_port: u16,

    /// Metrics server port (Prometheus/Grafana)
    #[arg(long, default_value = "9090")]
    pub metrics_port: u16,

    /// Enable mesh networking
    #[arg(long, default_value = "true")]
    pub mesh: bool,

    /// Enable metrics server
    #[arg(long, default_value = "true")]
    pub metrics: bool,

    /// Maximum message size in bytes
    #[arg(long, default_value = "8388608")] // 8MB
    pub max_message_size: u32,

    /// Number of worker threads
    #[arg(long)]
    pub workers: Option<usize>,
}

impl ServeCommand {
    /// Execute the serve command
    pub async fn execute(self, config_path: Option<String>) -> Result<()> {
        info!("Starting 9P.e server...");

        // Load config file if provided
        let file_config = if let Some(path) = config_path.as_ref() {
            info!("Loading configuration from: {}", path);
            Some(crate::config::Config::from_file(std::path::Path::new(path))?)
        } else {
            None
        };

        // Configure network with IPv6 preference
        let network_config = NetworkConfig::new(self.port)
            .with_interface(self.bind.as_deref())?;

        // Determine transport type (QUIC by default unless --no-quic)
        let transport = if self.no_quic {
            info!("Using legacy TCP transport");
            TransportType::Tcp
        } else {
            info!("Using modern QUIC transport (default)");
            TransportType::Quic {
                server_name: self.server_name,
            }
        };

        // Build the server with dependency injection
        let mut builder = ServerBuilder::new()
            .network_config(network_config)
            .transport(transport)
            .root_directory(self.root)
            .max_message_size(self.max_message_size)
            .worker_threads(self.workers)
            .mesh_enabled(self.mesh)
            .mesh_port(self.mesh_port)
            .metrics_enabled(self.metrics)
            .metrics_port(self.metrics_port);

        // If config file was loaded, pass it to the builder
        if let Some(config) = file_config {
            builder = builder.with_config(config);
        }

        let server = builder.build()
            .await
            .context("Failed to build server")?;

        // Start the server
        info!(
            "Server listening on {} with {}",
            server.address(),
            if self.no_quic { "TCP" } else { "QUIC" }
        );

        server.run().await.context("Server error")?;

        Ok(())
    }
}