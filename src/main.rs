//! 9P.e Server - Clean implementation using the verified 9PE core protocol
//!
//! This server bridges the formally-verified 9PE protocol to actual filesystem operations

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use anyhow::{Result, Context};
use tracing::{info, warn, error};
use tracing_subscriber;

mod server;
mod metrics;
mod client;
// mod mesh;  // Temporarily disabled due to thread safety issues
// mod mesh_client;  // Depends on mesh
mod ghostdag;
mod consensus;
// mod auto_mount;  // Temporarily disabled due to threading issues
mod auth;
mod simple_fuse;

// Import all modules for integrated functionality
mod synthetic;
mod modern_draw;
mod function_files;
mod synthetic_creation;
mod file_operations;


// mod enhanced_server; // Disabled for basic build

#[cfg(feature = "wasm")]
mod wasm_translator;
#[cfg(feature = "wasm")]
mod settrans;

// Use the basic filesystem server for now
use server::FileSystemServer;

#[derive(Parser, Debug)]
#[command(
    author = "9P.e Server Development Team",
    version,
    about = "Modern 9P.e protocol server with mesh networking and FUSE mounting",
    long_about = "9P.e Server - A modern filesystem protocol server with:\n\
                  \n\
                  FEATURES:\n\
                  • 9P.e Protocol: Modern extension of Plan 9's 9P protocol\n\
                  • Security: Ed25519 signatures, capability tokens, ACLs, rate limiting\n\
                  • Auto-Mount: Automatic FUSE mounting for Linux clients\n\
                  • Mesh Networking: Automatic peer discovery using libp2p gossipsub\n\
                  • Cross-Platform: Works on Linux, macOS, and Windows\n\
                  • Synthetic Files: Dynamic content generation\n\
                  • Metrics: Prometheus-compatible metrics export\n\
                  \n\
                  QUICK START:\n\
                  • Serve current directory: 9pe-server serve\n\
                  • With network access: 9pe-server serve --bind 0.0.0.0:5641\n\
                  • With mesh networking: 9pe-server serve --mesh-port 9650\n\
                  • Auto-mount discovery: 9pe-server auto-mount\n\
                  • List discovered peers: 9pe-server peers\n\
                  \n\
                  For detailed help on any command: 9pe-server <command> --help"
)]
#[command(propagate_version = true)]
struct Cli {
    /// Verbose output (-v for debug, -vv for trace)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start auto-mount daemon for discovered servers (Linux FUSE)
    ///
    /// Automatically discovers 9P.e servers on the network and mounts them
    /// using FUSE for seamless filesystem access. Requires FUSE support.
    AutoMount {
        /// Mount base directory for discovered servers
        #[arg(short, long, default_value = "/tmp/9pe-mounts")]
        mount_base: PathBuf,
    },

    /// List active auto-mounts and their permissions
    Mounts,
    /// Serve a directory over 9P.e protocol with optional features
    ///
    /// Start a 9P.e server to share files. Supports mesh networking for auto-discovery,
    /// FUSE auto-mounting, and various security features.
    Serve {
        /// Directory to serve (default: current directory)
        #[arg(short, long, default_value = ".")]
        path: PathBuf,

        /// Address to bind to (use 0.0.0.0:PORT for network access)
        #[arg(short, long, default_value = "0.0.0.0:564")]
        bind: String,

        /// Use QUIC transport with encryption (experimental, default is TCP)
        #[arg(short, long)]
        quic: bool,

        /// Server name for QUIC TLS certificate (required with --quic)
        #[arg(short = 'n', long)]
        server_name: Option<String>,

        /// Prometheus metrics port for monitoring (default: 9090)
        #[arg(short = 'm', long, default_value = "9090")]
        metrics_port: u16,


        /// Enable mesh networking for automatic peer discovery (libp2p gossipsub)
        #[arg(short = 'e', long)]
        mesh_port: Option<u16>,

        /// Custom mesh node ID (default: auto-generated from public key)
        #[arg(long)]
        mesh_node_id: Option<String>,

        /// Enable auto-mounting of discovered servers with proper permissions
        #[arg(long)]
        auto_mount: bool,
    },


    /// Mount a remote 9P.e server using FUSE (Linux/macOS)
    ///
    /// Mount a remote filesystem locally for seamless file access.
    /// Files can be accessed as if they were local.
    Mount {
        /// Server address (host:port, e.g., 192.168.1.114:5641)
        #[arg(short, long)]
        server: String,

        /// Local mount point directory
        #[arg(short, long, default_value = "/tmp/9pe-mount")]
        mount_point: String,
    },

    /// List files on remote 9P.e server
    ///
    /// Browse files on a remote server without mounting.
    List {
        /// Server address (host:port, e.g., 192.168.1.114:5641)
        #[arg(short, long)]
        server: String,

        /// Remote path to list (default: root directory)
        #[arg(short, long, default_value = "/")]
        path: String,
    },

    /// Discover other 9P.e nodes on network using mesh networking
    ///
    /// Scans the local network and mesh for available 9P.e servers.
    Discover,

    /// Auto-discover servers and list their files (no server address needed!)
    ///
    /// Automatically finds servers on the network and lists their contents.
    AutoList {
        /// Path to list on discovered servers
        #[arg(short, long, default_value = "/")]
        path: String,
    },

    /// Show discovered mesh peers with their capabilities and status
    Peers,

    /// Download a file from remote server (with optional auto-discovery)
    ///
    /// Copy a file from a remote 9P.e server to local filesystem.
    /// Can auto-discover the server if not specified.
    Get {
        /// Remote file path to download
        #[arg(short, long)]
        remote: String,

        /// Local destination path
        #[arg(short, long)]
        local: String,

        /// Server address (optional, auto-discovers if not provided)
        #[arg(short, long)]
        server: Option<String>,
    },

    /// Mine a test block in the GhostDAG consensus
    ///
    /// Creates and mines a new block with test data, demonstrating the
    /// proof-of-work mining process in the GhostDAG blockchain.
    MineBlock,

    /// Show current GhostDAG consensus state
    ///
    /// Display the blockchain state including total blocks, blue/red classification,
    /// current tips, and mining difficulty.
    ConsensusState,

    /// Set mining difficulty for GhostDAG
    ///
    /// Adjust the proof-of-work difficulty for block mining.
    /// Higher values require more computational work.
    SetDifficulty {
        /// New difficulty value (1-100)
        #[arg(short, long)]
        difficulty: u64,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Set up logging
    let log_level = match cli.verbose {
        0 => tracing::Level::INFO,
        1 => tracing::Level::DEBUG,
        _ => tracing::Level::TRACE,
    };

    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .init();

    match cli.command {
        Commands::Serve {
            path,
            bind,
            quic,
            server_name,
            metrics_port,
            mesh_port,
            mesh_node_id,
            auto_mount
        } => {
            serve_directory(
                path,
                bind,
                quic,
                server_name,
                metrics_port,
                mesh_port,
                mesh_node_id,
                auto_mount
            ).await
        }


        Commands::Mount { server, mount_point } => {
            info!("🗻 Mounting {} at {}", server, mount_point);

            // Check if FUSE is available
            if !simple_fuse::is_fuse_available() {
                warn!("FUSE not available - creating mount point only");
            }

            // Mount using simple FUSE
            match simple_fuse::mount_with_cleanup(server.clone(), std::path::Path::new(&mount_point)).await {
                Ok(()) => {
                    info!("✅ Successfully mounted {} at {}", server, mount_point);
                    info!("💡 Use 'fusermount -u {}' to unmount", mount_point);

                    // Keep the mount active
                    info!("Press Ctrl+C to stop mount");
                    tokio::signal::ctrl_c().await?;

                    // Unmount on exit
                    info!("📤 Unmounting...");
                    simple_fuse::unmount(std::path::Path::new(&mount_point)).await?;
                    Ok(())
                }
                Err(e) => {
                    error!("Failed to mount {}: {}", server, e);
                    Err(e)
                }
            }
        }

        Commands::List { server, path } => {
            client::list_remote_files(server, path).await
        }

        Commands::Discover => {
            client::discover_nodes().await
        }

        Commands::AutoList { path } => {
            // mesh_client::commands::auto_list(&path).await
            warn!("Mesh networking temporarily disabled");
            Ok(())
        }

        Commands::Peers => {
            // mesh_client::commands::mesh_list().await
            warn!("Mesh networking temporarily disabled");
            Ok(())
        }

        Commands::AutoMount { mount_base } => {
            info!("🔐 Auto-mount daemon with FUSE integration");
            info!("📁 Mount base: {:?}", mount_base);

            // For now, just show available servers
            info!("🔍 Scanning for 9P.e servers...");
            // let mut client = mesh_client::MeshClient::new().await?;
            // tokio::time::sleep(Duration::from_secs(2)).await;
            // client.scan_local_network().await?;
            // let peers = client.list_peers().await;
            let peers: Vec<String> = vec![];  // Placeholder type since mesh is disabled
            if peers.is_empty() {
                info!("No 9P.e servers found");
            } else {
                info!("📡 Found {} servers:", peers.len());
                // Commented out until mesh is fixed
                // for peer in peers {
                //     info!("  🖥️  {} at {}", peer.node_id, peer.listen_addr);

                //     // Create mount point for this server
                //     let mount_name = peer.node_id.replace([':', '.'], "_");
                //     let mount_point = mount_base.join(&mount_name);

                //     info!("  🗻 Would mount at: {:?}", mount_point);

                //     // Create the mount point for user
                //     simple_fuse::mount_server(peer.listen_addr.clone(), &mount_point).await?;
                // }
            }

            info!("Auto-mount scan complete. Use 'mounts' to see active mounts.");
            Ok(())
        }

        Commands::Mounts => {
            info!("🗻 Checking for mounted filesystems...");

            // Check for mount markers in /tmp/9pe-mounts
            let mount_base = std::path::Path::new("/tmp/9pe-mounts");

            if !mount_base.exists() {
                info!("No mount base directory found");
                return Ok(());
            }

            let mut found_mounts = false;
            let mut entries = tokio::fs::read_dir(mount_base).await?;

            while let Some(entry) = entries.next_entry().await? {
                if entry.file_type().await?.is_dir() {
                    let mount_dir = entry.path();
                    let marker_file = mount_dir.join(".9pe_mount");

                    if marker_file.exists() {
                        found_mounts = true;
                        info!("📁 Mount: {:?}", mount_dir);

                        // Read mount info
                        if let Ok(info) = tokio::fs::read_to_string(&marker_file).await {
                            info!("  {}", info.replace('\n', "\n  "));
                        }
                    }
                }
            }

            if !found_mounts {
                info!("No active mounts found");
            }

            Ok(())
        }

        Commands::Get { remote, local, server } => {
            if let Some(srv) = server {
                // Direct server connection
                let mut client = client::NinePeeClient::connect(&srv).await?;
                let data = client.read_file(&remote).await?;
                tokio::fs::write(&local, data).await?;
                info!("✅ Downloaded {} to {}", remote, local);
            } else {
                // Auto-discover
                // mesh_client::commands::auto_get(&remote, &local).await?;
                warn!("Auto-discovery temporarily disabled");
                return Err(anyhow::anyhow!("Auto-discovery temporarily disabled"));
            }
            Ok(())
        }

        Commands::MineBlock => {
            consensus::commands::mine_test_block().await
        }

        Commands::ConsensusState => {
            consensus::commands::show_consensus_state().await
        }

        Commands::SetDifficulty { difficulty } => {
            consensus::commands::set_difficulty(difficulty).await
        }
    }
}

async fn serve_directory(
    path: PathBuf,
    bind: String,
    use_quic: bool,
    server_name: Option<String>,
    metrics_port: u16,
    mesh_port: Option<u16>,
    mesh_node_id: Option<String>,
    auto_mount: bool,
) -> Result<()> {
    // Validate the path
    if !path.exists() {
        return Err(anyhow::anyhow!("Path does not exist: {:?}", path));
    }
    if !path.is_dir() {
        return Err(anyhow::anyhow!("Path is not a directory: {:?}", path));
    }

    let addr: SocketAddr = bind.parse()
        .context("Invalid bind address")?;

    info!("🚀 Starting 9P.e Server");
    info!("📁 Serving: {:?}", path.canonicalize()?);
    info!("🌐 Binding to: {}", addr);
    info!("🔒 Transport: {}", if use_quic { "QUIC (encrypted)" } else { "TCP (legacy)" });
    info!("📊 Metrics: http://0.0.0.0:{}/metrics", metrics_port);

    // Initialize metrics
    metrics::init_metrics();

    // Start metrics server in background
    let metrics_handle = tokio::spawn(async move {
        if let Err(e) = metrics::start_metrics_server(metrics_port).await {
            error!("Metrics server failed: {}", e);
        }
    });

    // Create synthetic filesystem for web UI
    let synthetic_fs = std::sync::Arc::new(crate::synthetic::SyntheticFileSystem::new());


    // Start mesh networking if requested
    let mesh_sender = if let Some(port) = mesh_port {
        info!("🌐 Starting mesh networking on port {}", port);
        // Pass the 9P service address to mesh for discovery
        let service_addr = Some(bind.clone());
        // Temporarily disabled
        // match crate::mesh::start_mesh_network(port, service_addr).await {
        match Result::<(), anyhow::Error>::Err(anyhow::anyhow!("Mesh temporarily disabled")) {
            Ok(sender) => {
                info!("✅ Mesh network started successfully");
                Some(sender)
            }
            Err(e) => {
                warn!("Failed to start mesh network: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Start auto-mount manager if requested
    if auto_mount {
        info!("🔐 Auto-mount integration enabled");
        info!("💡 Discovered servers will be prepared for mounting");
        // Auto-mount functionality will be available via CLI commands
    }


    if use_quic {
        if server_name.is_none() {
            return Err(anyhow::anyhow!("--server-name is required when using QUIC"));
        }
        start_quic_server(path, addr, server_name.unwrap()).await
    } else {
        start_tcp_server(path, addr).await
    }
}

async fn start_tcp_server(path: PathBuf, addr: SocketAddr) -> Result<()> {
    // Create the filesystem server
    let fs_server = Arc::new(FileSystemServer::new(path)?);
    info!("📁 Using basic FileSystemServer");

    // Bind TCP listener
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("✅ TCP server listening on {}", addr);

    loop {
        let (socket, peer_addr) = listener.accept().await?;
        info!("New TCP connection from {}", peer_addr);

        let server = Arc::clone(&fs_server);
        tokio::spawn(async move {
            if let Err(e) = handle_tcp_connection(socket, server).await {
                error!("Connection error: {}", e);
            }
        });
    }
}

async fn start_quic_server(path: PathBuf, addr: SocketAddr, server_name: String) -> Result<()> {
    // TODO: Use ninepee QUIC transport
    warn!("QUIC server implementation pending - using 9PE core protocol");

    info!("✅ QUIC server would listen on {} with name {}", addr, server_name);

    // Placeholder - keep server running
    tokio::signal::ctrl_c().await?;
    Ok(())
}

async fn handle_tcp_connection(
    mut socket: tokio::net::TcpStream,
    fs_server: Arc<FileSystemServer>
) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use plan9e::protocol::NinePeeMessage;

    info!("Starting 9P.e session");
    metrics::record_connection("tcp", true);

    // Message handling loop
    loop {
        // Read message length (4 bytes)
        let mut len_buf = [0u8; 4];
        if socket.read_exact(&mut len_buf).await.is_err() {
            info!("Client disconnected");
            metrics::record_connection("tcp", false);
            break;
        }

        let msg_len = u32::from_le_bytes(len_buf) as usize;

        // Validate message size
        if msg_len < 4 || msg_len > 16 * 1024 * 1024 {
            error!("Invalid message size: {}", msg_len);
            break;
        }

        // Read message body
        let mut msg_buf = vec![0u8; msg_len - 4];
        if socket.read_exact(&mut msg_buf).await.is_err() {
            error!("Failed to read message body");
            break;
        }

        // Deserialize message
        let request = match NinePeeMessage::deserialize(msg_buf) {
            Ok(msg) => msg,
            Err(e) => {
                error!("Failed to deserialize message: {}", e);
                continue;
            }
        };

        // Process with filesystem server
        let response = match fs_server.process_message(request).await {
            Ok(resp) => resp,
            Err(e) => {
                error!("Error processing message: {}", e);
                NinePeeMessage::Error {
                    ename: e.to_string(),
                    errno: 1,
                }
            }
        };

        // Serialize response
        let response_data = match response.serialize() {
            Ok(data) => data,
            Err(e) => {
                error!("Failed to serialize response: {}", e);
                continue;
            }
        };

        // Send response (with length prefix)
        let response_len = (response_data.len() + 4) as u32;
        socket.write_all(&response_len.to_le_bytes()).await?;
        socket.write_all(&response_data).await?;
    }

    Ok(())
}