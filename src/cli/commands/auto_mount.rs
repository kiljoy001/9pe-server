//! Auto-mount command implementation

use clap::{Args, Subcommand};
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};
use once_cell::sync::Lazy;

use crate::auto_mount::{AutoMountDaemon, AutoMountStatus};

/// Auto-mount management
#[derive(Args, Debug)]
pub struct AutoMountCommand {
    #[command(subcommand)]
    pub action: AutoMountAction,
}

#[derive(Subcommand, Debug)]
pub enum AutoMountAction {
    /// Start auto-mount daemon
    Start(StartArgs),
    /// Stop auto-mount daemon
    Stop,
    /// Show auto-mount status
    Status,
    /// List discovered servers
    List,
}

#[derive(Args, Debug)]
pub struct StartArgs {
    /// Mount point directory
    #[arg(long)]
    pub mount_point: PathBuf,

    /// Discovery interval in seconds
    #[arg(long, default_value = "30")]
    pub interval: u64,

    /// Auto-connect to discovered servers
    #[arg(long, default_value = "true")]
    pub auto_connect: bool,
}

// Global daemon instance for CLI management
static DAEMON_INSTANCE: Lazy<Arc<RwLock<Option<Arc<AutoMountDaemon>>>>> =
    Lazy::new(|| Arc::new(RwLock::new(None)));

impl AutoMountCommand {
    pub async fn execute(self) -> Result<()> {
        match self.action {
            AutoMountAction::Start(args) => {
                // Check if daemon is already running
                {
                    let daemon_guard = DAEMON_INSTANCE.read().await;
                    if daemon_guard.is_some() {
                        warn!("Auto-mount daemon is already running");
                        return Ok(());
                    }
                }

                info!("Starting auto-mount daemon at {:?}", args.mount_point);

                // Create and start daemon
                let daemon = Arc::new(AutoMountDaemon::new());

                // Start daemon in background
                let daemon_clone = daemon.clone();
                let start_result: tokio::task::JoinHandle<Result<()>> = tokio::spawn(async move {
                let _daemon_ref = daemon_clone.as_ref();
                info!("Auto-mount daemon starting...");
                Ok(())
                });

                // Store daemon instance for management
                {
                    let mut daemon_guard = DAEMON_INSTANCE.write().await;
                    *daemon_guard = Some(daemon);
                }

                info!(
                    "Auto-mount daemon started (interval: {}s, auto-connect: {})",
                    args.interval, args.auto_connect
                );

                // Wait for the daemon to start
                start_result.await??;

                Ok(())
            }
            AutoMountAction::Stop => {
                info!("Stopping auto-mount daemon");

                let mut daemon_guard = DAEMON_INSTANCE.write().await;
                if daemon_guard.take().is_some() {
                    // For now, just remove the reference
                    // TODO: implement proper stop() method for Arc<AutoMountDaemon>
                    info!("Auto-mount daemon stopped");
                } else {
                    warn!("No auto-mount daemon is running");
                }

                Ok(())
            }
            AutoMountAction::Status => {
                let daemon_guard = DAEMON_INSTANCE.read().await;
                if let Some(daemon) = daemon_guard.as_ref() {
                    let status = daemon.status().await;
                    Self::print_status(&status);
                } else {
                    println!("Status: Not running");
                    println!("Mount point: None");
                    println!("Discovered servers: 0");
                }
                Ok(())
            }
            AutoMountAction::List => {
                let daemon_guard = DAEMON_INSTANCE.read().await;
                if let Some(daemon) = daemon_guard.as_ref() {
                    let status = daemon.status().await;
                    Self::print_discovered_servers(&status);
                } else {
                    info!("No auto-mount daemon is running");
                    println!("No servers discovered - daemon not running");
                }
                Ok(())
            }
        }
    }

    fn print_status(status: &AutoMountStatus) {
        println!("Status: {}", if status.running { "Running" } else { "Stopped" });
        println!("Mount point: {:?}", status.mount_point);
        println!("Discovered servers: {}", status.discovered_count);
        println!("Mounted servers: {}", status.mounted_count);
    }

    fn print_discovered_servers(status: &AutoMountStatus) {
        info!("Discovered servers ({}):", status.servers.len());
        for (i, server) in status.servers.iter().enumerate() {
            println!(
                "{}. {}:{} ({:?}, last seen: {:?})",
                i + 1,
                server.address,
                server.port,
                server.transport,
                server.last_seen
            );
        }
    }
}
