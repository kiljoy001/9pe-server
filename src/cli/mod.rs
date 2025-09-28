//! CLI module - Command-line interface with clean separation of concerns

use clap::{Parser, Subcommand};
use anyhow::Result;

pub mod commands;
pub mod args;

pub use commands::{ServeCommand, ClientCommand, AutoMountCommand};
pub use args::GlobalArgs;

/// 9P.e Server - Everything is a file, and every file is a function
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    /// Global arguments
    #[command(flatten)]
    pub global: GlobalArgs,

    /// Subcommand to execute
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start the 9P.e server (default)
    Serve(ServeCommand),

    /// Connect to a 9P.e server as a client
    Client(ClientCommand),

    /// Auto-mount management
    #[command(name = "auto-mount")]
    AutoMount(AutoMountCommand),

    /// Show version information
    Version,
}

impl Cli {
    /// Parse command-line arguments
    pub fn parse_args() -> Self {
        Self::parse()
    }

    /// Execute the parsed command
    pub async fn execute(self) -> Result<()> {
        // Initialize logging based on global args
        self.global.init_logging()?;

        // Execute the appropriate command
        match self.command {
            Command::Serve(cmd) => cmd.execute().await,
            Command::Client(cmd) => cmd.execute().await,
            Command::AutoMount(cmd) => cmd.execute().await,
            Command::Version => {
                println!("9P.e Server v{}", env!("CARGO_PKG_VERSION"));
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing() {
        let cli = Cli::parse_from(&["test", "serve"]);
        assert!(matches!(cli.command, Command::Serve(_)));
    }

    #[test]
    fn test_version_command() {
        let cli = Cli::parse_from(&["test", "version"]);
        assert!(matches!(cli.command, Command::Version));
    }
}