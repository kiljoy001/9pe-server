//! Client command implementation

use clap::{Args, Subcommand};
use anyhow::{Result, Context};
use std::path::PathBuf;
use tracing::{info, warn};
use std::fs;

use crate::fuse_mount::{mount_9p_fuse, cleanup_broken_mounts};

/// Connect to a 9P.e server as a client
#[derive(Args, Debug)]
pub struct ClientCommand {
    #[command(subcommand)]
    pub action: ClientAction,
}

#[derive(Subcommand, Debug)]
pub enum ClientAction {
    /// Connect to a server
    Connect(ConnectArgs),
    /// Mount a remote filesystem
    Mount(MountArgs),
    /// Clean up broken FUSE mounts
    Cleanup,
}

#[derive(Args, Debug)]
pub struct ConnectArgs {
    /// Server address (can be hostname or IP)
    pub server: String,

    /// Server port
    #[arg(short, long, default_value = "5640")]
    pub port: u16,

    /// Server name for QUIC TLS verification
    #[arg(short = 'n', long)]
    pub server_name: Option<String>,

    /// Use legacy TCP instead of QUIC
    #[arg(long)]
    pub tcp: bool,
}

#[derive(Args, Debug)]
pub struct MountArgs {
    /// Server address (host:port or use 'auto' to discover)
    pub server: String,

    /// Local mount point directory
    #[arg(short, long)]
    pub mount_point: Option<PathBuf>,
}

impl ClientCommand {
    pub async fn execute(self) -> Result<()> {
        match self.action {
            ClientAction::Connect(args) => {
                info!("Connecting to {}:{}", args.server, args.port);
                // Client connection logic here
                Ok(())
            }
            ClientAction::Mount(args) => {
                Self::mount_server(args).await
            }
            ClientAction::Cleanup => {
                info!("🧹 Cleaning up broken FUSE mounts...");
                cleanup_broken_mounts().await?;
                info!("✅ Cleanup completed");
                Ok(())
            }
        }
    }

    async fn mount_server(args: MountArgs) -> Result<()> {
        // Clean up any broken mounts first
        if let Err(e) = cleanup_broken_mounts().await {
            warn!("Failed to cleanup broken mounts: {}", e);
        }

        // Parse server address
        let (host, port) = if args.server.contains(':') {
            let parts: Vec<&str> = args.server.split(':').collect();
            (parts[0].to_string(), parts[1].parse().unwrap_or(5640))
        } else {
            (args.server.clone(), 5640)
        };

        let server_addr = format!("{}:{}", host, port);
        let server_name = format!("{}_{}", host.replace('.', "_"), port);

        // Mount to user's home directory where they have control
        let mount_point = args.mount_point.unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join("9pe").join(&server_name)
        });

        info!("🗻 Mounting {}:{} at {:?}", host, port, mount_point);

        // Mount using FUSE
        mount_9p_fuse(server_addr, mount_point.clone()).await
            .with_context(|| format!("Failed to mount 9P server using FUSE"))?;

        info!("✅ Server mounted successfully");
        info!("📁 Access remote files at: {:?}", mount_point);
        info!("💡 Use 'fusermount -u {:?}' to unmount", mount_point);

        Ok(())
    }

    async fn ensure_plan9_namespace() -> Result<()> {
        // Try to create Plan 9 directories
        let srv_dir = PathBuf::from("/srv");
        let n_dir = PathBuf::from("/n");

        // Create /srv if it doesn't exist
        if !srv_dir.exists() {
            if let Err(e) = fs::create_dir_all(&srv_dir) {
                info!("⚠️  Cannot create /srv ({}), services will be limited", e);
            } else {
                info!("📁 Created /srv directory");
            }
        }

        // Create /n if it doesn't exist
        if !n_dir.exists() {
            if let Err(e) = fs::create_dir_all(&n_dir) {
                info!("⚠️  Cannot create /n ({}), will use fallback mount points", e);
            } else {
                info!("📁 Created /n directory");
            }
        }

        Ok(())
    }

    /// Check if we can create directories in the Plan 9 namespace
    async fn can_create_plan9_namespace(mount_path: &PathBuf) -> bool {
        match fs::create_dir_all(mount_path) {
            Ok(_) => {
                // Clean up the test directory
                let _ = fs::remove_dir(mount_path);
                true
            }
            Err(_) => false,
        }
    }
}