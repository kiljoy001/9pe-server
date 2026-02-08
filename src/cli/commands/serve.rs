//! Serve command implementation with minimalist design
//! Configuration is now handled primarily via toml file (default or --config)

use anyhow::{Context, Result};
use clap::Args;
use std::path::{Path, PathBuf};
use tracing::info;

use crate::server::ServerBuilder;

/// Start the 9P.e server
#[derive(Args, Debug)]
pub struct ServeCommand {
    /// Config file path
    #[arg(long, short)]
    pub config: Option<String>,
}

impl ServeCommand {
    /// Execute the serve command
    pub async fn execute(self, config_path: Option<String>) -> Result<()> {
        info!("Starting 9P.e server...");

        // Prefer command-line config path over global arg if both present
        // Use `self.config` if present, else `config_path` (from global args).
        let path_to_use = self.config.or(config_path);

        // Load config file if provided
        let file_config = if let Some(path) = path_to_use.as_ref() {
            info!("Loading configuration from: {}", path);
            Some(crate::config::Config::from_file(Path::new(path))?)
        } else {
            None
        };

        // Build the server - rely on builder defaults (synthetic) and config file (optional root)
        let mut builder = ServerBuilder::new();

        // If config file was loaded, pass it to the builder
        if let Some(config) = file_config {
            builder = builder.with_config(config);
        }

        let server = builder.build().await.context("Failed to build server")?;

        // Start the server
        // Detailed logging is handled inside server.run()
        
        server.run().await.context("Server error")?;

        Ok(())
    }
}
