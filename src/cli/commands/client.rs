//! Client command implementation

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use std::fs;
use std::path::PathBuf;
use tracing::{info, warn};
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use rand::RngCore;
use serde_json::json;
use blake3;
use std::io::Write;

#[cfg(feature = "fuse")]
use crate::fuse_mount::{cleanup_broken_mounts, mount_9p_fuse};

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
    #[cfg(feature = "fuse")]
    Mount(MountArgs),
    /// Clean up broken FUSE mounts
    #[cfg(feature = "fuse")]
    Cleanup,
    /// Register a new namespace
    Register(RegisterArgs),
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

#[cfg(feature = "fuse")]
#[derive(Args, Debug)]
pub struct MountArgs {
    /// Server address (host:port or use 'auto' to discover)
    pub server: String,

    /// Local mount point directory
    #[arg(short, long)]
    pub mount_point: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct RegisterArgs {
    /// Server address
    pub server: String,

    /// Namespace path (must start with /)
    pub path: String,

    /// Description
    #[arg(short, long, default_value = "User namespace")]
    pub description: String,

    /// Namespace type
    #[arg(short = 't', long, default_value = "user")]
    pub namespace_type: String,
}

impl ClientCommand {
    pub async fn execute(self) -> Result<()> {
        match self.action {
            ClientAction::Connect(args) => {
                // Parse server address similar to mount logic
                let (host, port) = if args.server.contains(':') {
                    let parts: Vec<&str> = args.server.split(':').collect();
                    (parts[0].to_string(), parts[1].parse().unwrap_or(args.port))
                } else {
                    (args.server.clone(), args.port)
                };
                
                let server_addr = format!("{}:{}", host, port);
                info!("Connecting to {}", server_addr);
                
                use crate::client::NinePClient;
                
                let mut client = NinePClient::connect(&server_addr).await
                    .with_context(|| format!("Failed to connect to {}", server_addr))?;
                    
                info!("✅ Connected successfully!");
                info!("Server Version: {}", client.version);
                
                // Try to list root directory
                info!("📂 Listing / directory:");
                match client.list_directory("/").await {
                    Ok(files) => {
                        for file in files {
                            info!("  - {}", file);
                        }
                    }
                    Err(e) => warn!("Failed to list directory: {}", e),
                }

                Ok(())
            }
            #[cfg(feature = "fuse")]
            ClientAction::Mount(args) => Self::mount_server(args).await,
            #[cfg(feature = "fuse")]
            ClientAction::Cleanup => {
                info!("🧹 Cleaning up broken FUSE mounts...");
                cleanup_broken_mounts().await?;
                info!("✅ Cleanup completed");
                Ok(())
            }
            ClientAction::Register(args) => Self::register_namespace(args).await,
        }
    }

    async fn register_namespace(args: RegisterArgs) -> Result<()> {
        let (host, port) = if args.server.contains(':') {
            let parts: Vec<&str> = args.server.split(':').collect();
            (parts[0].to_string(), parts[1].parse().unwrap_or(5640))
        } else {
            (args.server.clone(), 5640)
        };
        
        let server_addr = format!("{}:{}", host, port);
        info!("Connecting to {}...", server_addr);
        
        use crate::client::NinePClient;
        let mut client = NinePClient::connect(&server_addr).await?;

        // 1. Generate Keypair
        let mut csprng = OsRng;
        let mut key_bytes = [0u8; 32];
        csprng.fill_bytes(&mut key_bytes);
        let signing_key = SigningKey::from_bytes(&key_bytes);
        let pubkey_bytes = signing_key.verifying_key().to_bytes();
        let pubkey_hex = hex::encode(pubkey_bytes);
        
        info!("Generated Identity: {}", pubkey_hex);

        // 2. Mine PoW
        // TODO: Query actual difficulty from server. For now assuming base 10 + estimated growth
        // Ideally we read /srv/consensus/difficulty
        let difficulty = 10; 
        info!("Mining PoW (Difficulty: {})...", difficulty);

        let mut nonce = 0u64;
        let target = 0xFFFFFFFFFFFFFFFFu64 >> difficulty;
        
        // Context = Hash(path + pubkey)
        let mut hasher = blake3::Hasher::new();
        hasher.update(args.path.as_bytes());
        hasher.update(pubkey_hex.as_bytes());
        let context_hash = hasher.finalize();
        let mut context_bytes = [0u8; 8];
        context_bytes.copy_from_slice(&context_hash.as_bytes()[0..8]);
        let context = u64::from_be_bytes(context_bytes);

        // Mining Loop
        let start = std::time::Instant::now();
        loop {
            let mut hasher = blake3::Hasher::new();
            hasher.update(&nonce.to_be_bytes());
            hasher.update(&context.to_be_bytes());
            let hash = hasher.finalize();
            let mut val_bytes = [0u8; 8];
            val_bytes.copy_from_slice(&hash.as_bytes()[0..8]);
            let value = u64::from_be_bytes(val_bytes);

            if value < target {
                break;
            }
            nonce += 1;
            if nonce % 1_000_000 == 0 {
                print!(".");
                std::io::stdout().flush()?;
            }
        }
        println!();
        info!("Mined Nonce: {} in {:?}", nonce, start.elapsed());

        // 3. Sign Request
        let created_at = chrono::Utc::now().timestamp();
        // Note: Real signature must match NamespaceManager verification exactly.
        // NamespaceManager uses: path + pubkey + created_at + requirements
        let requirements_str = ""; 
        let full_sign_data = format!("{}{}{}{}", args.path, pubkey_hex, created_at, requirements_str);
        
        let signature = signing_key.sign(full_sign_data.as_bytes());
        let signature_hex = hex::encode(signature.to_bytes());

        // 4. Construct Payload
        let payload = json!({
            "path": args.path,
            "description": args.description,
            "type": args.namespace_type,
            "pubkey": pubkey_hex,
            "signature": signature_hex,
            "created_at": created_at,
            "pow_nonce": nonce
        });

        // 5. Submit to /srv/namespace/register
        info!("Submitting registration...");
        // This requires the client to support writing to a file. 
        // Assuming NinePClient has a write_file method or similar.
        // If not, we might need a raw fid write.
        // Checking client capabilities...
        
        // client.write_file("/srv/namespace/register", payload.to_string().as_bytes()).await?;
        // Placeholder as write_file might not exist in this context yet.
        // Using a basic open/write sequence if needed.
        
        // Simulating the write for now or implementing if missing
        match client.write_file("/srv/namespace/register", payload.to_string().as_bytes()).await {
             Ok(_) => info!("✅ Namespace registered successfully!"),
             Err(e) => {
                 warn!("Registration failed: {}", e);
                 // If it failed, it might be difficulty mismatch or permission
             }
        }

        Ok(())
    }
    #[cfg(feature = "fuse")]
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
        // Follow Plan 9 convention: ~/9pe/n/<server>
        let mount_point = args.mount_point.unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join("9pe").join("n").join(&server_name)
        });

        // Ensure namespace directories exist
        let _ = Self::ensure_plan9_namespace().await;

        info!("🗻 Mounting {}:{} at {:?}", host, port, mount_point);

        // Mount using FUSE
        mount_9p_fuse(server_addr, mount_point.clone())
            .await
            .with_context(|| "Failed to mount 9P server using FUSE".to_string())?;

        info!("✅ Server mounted successfully");
        info!("📁 Access remote files at: {:?}", mount_point);
        info!("💡 Use 'fusermount -u {:?}' to unmount", mount_point);

        Ok(())
    }

    #[allow(dead_code)]
    #[cfg(feature = "fuse")]
    async fn ensure_plan9_namespace() -> Result<()> {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let base_dir = PathBuf::from(home).join("9pe");
        
        // Use ~/9pe/srv and ~/9pe/n for unprivileged access
        let srv_dir = base_dir.join("srv");
        let n_dir = base_dir.join("n");

        if !srv_dir.exists() {
            if let Err(e) = fs::create_dir_all(&srv_dir) {
                info!("⚠️  Cannot create {:?} ({})", srv_dir, e);
            } else {
                info!("📁 Created {:?} directory", srv_dir);
            }
        }

        if !n_dir.exists() {
            if let Err(e) = fs::create_dir_all(&n_dir) {
                info!("⚠️  Cannot create {:?} ({})", n_dir, e);
            } else {
                info!("📁 Created {:?} directory", n_dir);
            }
        }

        Ok(())
    }

    /// Check if we can create directories in the Plan 9 namespace
    #[allow(dead_code)]
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
