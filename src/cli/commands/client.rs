//! Client command implementation

use clap::{Args, Subcommand};
use anyhow::{Result, Context};
use std::path::PathBuf;
use tracing::info;

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
    /// Server address
    pub server: String,

    /// Local mount point
    pub mount_point: PathBuf,

    /// Server port
    #[arg(short, long, default_value = "5640")]
    pub port: u16,
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
                info!("Mounting {} at {:?}", args.server, args.mount_point);
                // Mount logic here
                Ok(())
            }
        }
    }
}