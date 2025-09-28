//! 9P.e Server - Clean Architecture Implementation
//!
//! This is the refactored version with proper separation of concerns,
//! dependency injection, and modern Rust patterns.

use anyhow::Result;
use clap::Parser;

use ninep_server::cli::Cli;

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments and execute
    let cli = Cli::parse();

    // Execute the CLI command with integrated error handling and logging
    cli.execute().await?;

    Ok(())
}