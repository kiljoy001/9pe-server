use anyhow::Result;
use ninepe_server::cli::Cli;

#[tokio::main]
async fn main() -> Result<()> {
    // Parse arguments and execute command
    let cli = Cli::parse_args();
    cli.execute().await
}
