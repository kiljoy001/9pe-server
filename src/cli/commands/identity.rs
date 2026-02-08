//! Identity management commands
//!
//! Commands for managing client cryptographic identities used for 9P.e authentication.

use anyhow::Result;
use clap::{Args, Subcommand};
use std::path::PathBuf;

use crate::client::ClientIdentity;

/// Identity management commands
#[derive(Args, Debug)]
pub struct IdentityCommand {
    #[command(subcommand)]
    pub action: IdentityAction,
}

#[derive(Subcommand, Debug)]
pub enum IdentityAction {
    /// Generate a new identity
    Generate {
        /// Output file path (default: ~/.9pe/identity.json)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Force overwrite if file exists
        #[arg(short, long)]
        force: bool,
    },

    /// Show identity information
    Show {
        /// Identity file path (default: ~/.9pe/identity.json)
        #[arg(short, long)]
        path: Option<PathBuf>,
    },

    /// Export public key for sharing
    Export {
        /// Identity file path (default: ~/.9pe/identity.json)
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Output format (text, json)
        #[arg(short, long, default_value = "text")]
        format: String,
    },
}

impl IdentityCommand {
    pub async fn execute(self) -> Result<()> {
        match self.action {
            IdentityAction::Generate { output, force } => {
                let path = output.unwrap_or_else(|| {
                    ClientIdentity::default_path().unwrap_or_else(|_| PathBuf::from("identity.json"))
                });

                if path.exists() && !force {
                    println!("Identity file already exists at {:?}", path);
                    println!("Use --force to overwrite, or --output to specify a different path.");
                    return Ok(());
                }

                println!("Generating new identity...");
                let identity = ClientIdentity::generate()?;
                identity.save(&path)?;

                println!();
                println!("Identity generated successfully!");
                println!();
                println!("  Node ID:    {}", identity.node_id);
                println!("  Public Key: {}", hex::encode(&identity.ed25519_public));
                println!("  Saved to:   {:?}", path);
                println!();
                println!("Your identity file contains your private key.");
                println!("Keep it secure and do not share it.");
            }

            IdentityAction::Show { path } => {
                let path = path.unwrap_or_else(|| {
                    ClientIdentity::default_path().unwrap_or_else(|_| PathBuf::from("identity.json"))
                });

                if !path.exists() {
                    println!("No identity found at {:?}", path);
                    println!("Run '9pe identity generate' to create one.");
                    return Ok(());
                }

                let identity = ClientIdentity::load(&path)?;

                println!("9P.e Client Identity");
                println!("====================");
                println!();
                println!("Node ID:     {}", identity.node_id);
                println!("Public Key:  {}", hex::encode(&identity.ed25519_public));
                println!("Path:        {:?}", path);
                println!();
                println!("Permissions:");
                println!("  Submit Jobs:       {}", identity.permissions.can_submit_jobs);
                println!("  Monitor Resources: {}", identity.permissions.can_monitor_resources);
                println!("  View Logs:         {}", identity.permissions.can_view_logs);
                println!("  Max Concurrent:    {}", identity.permissions.max_concurrent_jobs);
            }

            IdentityAction::Export { path, format } => {
                let path = path.unwrap_or_else(|| {
                    ClientIdentity::default_path().unwrap_or_else(|_| PathBuf::from("identity.json"))
                });

                if !path.exists() {
                    println!("No identity found at {:?}", path);
                    println!("Run '9pe identity generate' to create one.");
                    return Ok(());
                }

                let identity = ClientIdentity::load(&path)?;

                match format.as_str() {
                    "json" => {
                        let export = serde_json::json!({
                            "node_id": identity.node_id,
                            "ed25519_public": hex::encode(&identity.ed25519_public),
                        });
                        println!("{}", serde_json::to_string_pretty(&export)?);
                    }
                    _ => {
                        println!("{}", identity.node_id);
                    }
                }
            }
        }

        Ok(())
    }
}
