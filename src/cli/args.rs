//! Global arguments shared across all commands

use clap::Args;
use anyhow::Result;
use tracing_subscriber::EnvFilter;

/// Global arguments available to all commands
#[derive(Args, Debug)]
pub struct GlobalArgs {
    /// Verbosity level (can be repeated)
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Quiet mode - suppress non-error output
    #[arg(short, long, conflicts_with = "verbose")]
    pub quiet: bool,

    /// Log format (json, pretty, compact)
    #[arg(long, default_value = "pretty")]
    pub log_format: LogFormat,

    /// Config file path
    #[arg(long, env = "NINEPEE_CONFIG")]
    pub config: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum LogFormat {
    Json,
    Pretty,
    Compact,
}

impl std::str::FromStr for LogFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "pretty" => Ok(Self::Pretty),
            "compact" => Ok(Self::Compact),
            _ => Err(format!("Unknown log format: {}", s)),
        }
    }
}

impl GlobalArgs {
    /// Initialize logging based on global arguments
    pub fn init_logging(&self) -> Result<()> {
        let level = match (self.quiet, self.verbose) {
            (true, _) => "error",
            (false, 0) => "info",
            (false, 1) => "debug",
            (false, _) => "trace",
        };

        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(level));

        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(filter);

        match self.log_format {
            LogFormat::Json => {
                subscriber.json().init();
            }
            LogFormat::Pretty => {
                subscriber.pretty().init();
            }
            LogFormat::Compact => {
                subscriber.compact().init();
            }
        }

        Ok(())
    }
}