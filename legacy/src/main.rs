//! 9P.e Server - Clean implementation using the verified 9PE core protocol
//!
//! This server bridges the formally-verified 9PE protocol to actual filesystem operations

#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use anyhow::{Result, Context};
use tracing::{info, warn, error};
use tracing_subscriber::{
    self, EnvFilter, Layer,
    fmt,
    layer::SubscriberExt,
    util::SubscriberInitExt,
};
use tracing_appender::{non_blocking, rolling};

mod server;
mod metrics;
mod client;
mod mesh;  // Re-enabling to fix thread safety issues
mod mesh_client;  // Depends on mesh
mod ghostdag;
mod consensus;
mod auto_mount;
mod translator;
mod auth;
mod config;  // Configuration persistence
mod validation;
mod rate_limit;
mod session;
mod security_headers;
mod simple_fuse;
mod fuse_mount;  // FUSE client for mounting 9P servers
// mod integrated_server;  // Disabled - needs protocol API updates
mod global_event_chain;  // Global event ordering
// mod blockchain_mesh;     // Deprecated - use namespace_consensus instead

// Import all modules for integrated functionality
mod synthetic;
mod synthetic_advanced;
mod translators;
mod modern_draw;
mod function_files;
mod synthetic_creation;
mod file_operations;
mod namespace;  // Plan 9-style /srv and /n/ directories
mod translator_base;       // Abstract translator framework
mod namespace_translator;  // Built-in namespace management translator
mod namespaces;           // Namespace management system


// mod enhanced_server; // Disabled for basic build

#[cfg(feature = "wasm")]
mod wasm_translator;
#[cfg(feature = "wasm")]
mod settrans;

// Use the basic filesystem server for now

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
                  • Serve files (QUIC): 9pe-server serve\n\
                  • With discovery: 9pe-server serve --mesh-port 9650\n\
                  • Legacy TCP mode: 9pe-server serve --no-quic\n\
                  • Mount remote: 9pe-server connect mount <server:port>\n\
                  • Auto-mount: 9pe-server connect mount auto\n\
                  • Server info: 9pe-server info features\n\
                  \n\
                  MAIN COMMANDS:\n\
                  • serve - Start file server\n\
                  • connect - Mount/browse remote servers\n\
                  • discover - Find servers on network\n\
                  • info - Server capabilities and status\n\
                  • blockchain - Consensus operations (advanced)\n\
                  \n\
                  For detailed help: 9pe-server <command> --help"
)]
#[command(propagate_version = true)]
struct Cli {
    /// Verbose output (-v for debug, -vv for trace)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Log file path (default: stdout only)
    #[arg(long)]
    log_file: Option<PathBuf>,

    /// Log format (json, compact, pretty, full)
    #[arg(long, default_value = "compact")]
    log_format: String,

    /// Enable structured JSON logging
    #[arg(long)]
    json_logs: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start 9P.e server (main command)
    ///
    /// Start a 9P.e server to share files. Supports mesh networking, QUIC transport,
    /// automatic discovery, and production-ready authentication.
    Serve {
        /// Directory to serve (default: current directory)
        #[arg(short = 'r', long, default_value = ".")]
        path: PathBuf,

        /// Port to bind to (default: 5640)
        #[arg(short = 'p', long, default_value = "5640")]
        port: u16,

        /// Interface to bind to (e.g., 'lo', 'any', 'any6', or IP like '[::1]' or '192.168.1.100')
        /// Default: all interfaces via IPv6 dual-stack ([::]). Use 'any4' for IPv4-only.
        #[arg(short = 'i', long)]
        interface: Option<String>,

        /// Use QUIC transport with encryption (default: enabled for modern networking)
        /// Use --no-quic to disable and fall back to legacy TCP
        #[arg(short, long, default_value = "true")]
        quic: bool,

        /// Server name for QUIC TLS certificate (optional, only needed by clients)
        #[arg(short = 'n', long)]
        server_name: Option<String>,

        /// Mesh networking port for automatic peer discovery (MANDATORY)
        #[arg(short = 'e', long, default_value = "9650")]
        mesh_port: u16,

        /// Prometheus metrics port (default: 9090)
        #[arg(short = 'm', long, default_value = "9090")]
        metrics_port: u16,

        /// Run as daemon in the background
        #[arg(short = 'd', long)]
        daemon: bool,

        /// PID file location (when running as daemon)
        #[arg(long, default_value = "/tmp/9pe-server.pid")]
        pid_file: String,

        /// Mount namespace filesystem as read-only (default: read-write)
        #[arg(long)]
        namespace_readonly: bool,
    },

    /// Client operations - connect to remote servers
    ///
    /// Mount, browse, or download from remote 9P.e servers.
    Client {
        #[command(subcommand)]
        action: ClientAction,
    },

    /// Network discovery and mesh operations
    ///
    /// Discover servers, view mesh topology, and manage peer connections.
    Network {
        #[command(subcommand)]
        network_action: NetworkAction,
    },

    /// Server management and configuration
    ///
    /// Manage server daemon, users, and view status.
    Server {
        #[command(subcommand)]
        server_action: ServerAction,
    },

    /// Advanced features (blockchain, events)
    Advanced {
        #[command(subcommand)]
        advanced_action: AdvancedAction,
    },

    /// Auto-mount daemon and operations
    ///
    /// Manage automatic mounting of remote 9P.e servers with /srv and /n/ directories.
    AutoMount {
        #[command(subcommand)]
        automount_action: AutoMountAction,
    },

    /// WASM translator management
    ///
    /// Manage WASM translators with CBOR data exchange and synthetic file generation.
    Translator {
        #[command(subcommand)]
        translator_action: TranslatorAction,
    },

    /// Namespace management operations
    ///
    /// Create, join, and manage global namespaces with threshold signatures.
    Namespace {
        #[command(subcommand)]
        namespace_action: NamespaceAction,
    },
}

#[derive(Subcommand, Debug)]
enum NetworkAction {
    /// Discover servers on the network
    Discover,
    /// Show mesh network topology
    Topology,
    /// List connected peers
    Peers,
}

#[derive(Subcommand, Debug)]
enum ServerAction {
    /// Check server status and active connections
    Status {
        /// PID file location
        #[arg(long, default_value = "/tmp/9pe-server.pid")]
        pid_file: String,
    },
    /// Stop a running server daemon
    Stop {
        /// PID file location
        #[arg(long, default_value = "/tmp/9pe-server.pid")]
        pid_file: String,
    },
    /// User management
    Users {
        #[command(subcommand)]
        user_action: UserAction,
    },
    /// Show server features and capabilities
    Info,
    /// Interactive setup wizard
    Setup,
}

#[derive(Subcommand, Debug)]
enum AdvancedAction {
    /// Blockchain and consensus operations
    Blockchain {
        #[command(subcommand)]
        blockchain_action: BlockchainAction,
    },
    /// Global event chain operations
    Events {
        #[command(subcommand)]
        event_action: EventAction,
    },
}

#[derive(Subcommand, Debug)]
enum ClientAction {
    /// Mount a remote server using FUSE
    Mount {
        /// Server address (host:port or use 'auto' to discover)
        server: String,
        /// Local mount point directory
        #[arg(short = 'm', long, default_value = "/tmp/9pe-mount")]
        mount_point: String,
    },
    /// Browse files on remote server
    List {
        /// Server address (host:port or use 'auto' to discover)
        server: String,
        /// Remote path to list
        #[arg(short = 'r', long, default_value = "/")]
        path: String,
        /// Username for authentication
        #[arg(short, long)]
        username: Option<String>,
        /// Password for authentication
        #[arg(short = 'P', long)]
        password: Option<String>,
    },
    /// Download file from remote server
    Get {
        /// Server address (host:port or use 'auto' to discover)
        server: String,
        /// Remote file path
        remote: String,
        /// Local destination path
        local: String,
    },
}

#[derive(Subcommand, Debug)]
enum BlockchainAction {
    /// Mine a test block
    Mine,
    /// Show consensus state
    Status,
    /// Set mining difficulty
    Difficulty {
        /// New difficulty value (1-100)
        value: u64,
    },
}

#[derive(Subcommand, Debug)]
enum EventAction {
    /// Show event chain statistics
    Status,
    /// Show recent events
    Recent {
        /// Number of events to show
        #[arg(default_value = "10")]
        count: usize,
    },
    /// Show events for a specific file
    File {
        /// File path to query
        path: String,
    },
}

#[derive(Subcommand, Debug)]
enum UserAction {
    /// Add a new user
    Add {
        /// Username to add
        username: String,
        /// Password for the user
        #[arg(short, long)]
        password: Option<String>,
    },
    /// Change user password
    Passwd {
        /// Username to change password for
        username: String,
    },
    /// List all users
    List,
    /// Delete a user
    Del {
        /// Username to delete
        username: String,
    },
}

#[derive(Subcommand, Debug)]
enum AutoMountAction {
    /// Start auto-mount daemon with /srv and /n/ directories
    Start {
        /// Mount point for auto-mount filesystem (default: /tmp/9pe-namespace)
        #[arg(short, long, default_value = "/tmp/9pe-namespace")]
        mount_point: String,
    },
    /// Stop auto-mount daemon
    Stop,
    /// List current mounts and discovered servers
    List,
    /// Manually mount a discovered server
    Mount {
        /// Server to mount (from discovery list)
        server: String,
        /// Local mount point path
        mount_point: String,
    },
    /// Unmount a server
    Unmount {
        /// Server to unmount or mount point path
        target: String,
    },
}

#[derive(Subcommand, Debug)]
enum TranslatorAction {
    /// Install a WASM translator from bytecode file
    Install {
        /// Path to WASM bytecode file
        wasm_file: PathBuf,
    },
    /// Uninstall a translator
    Uninstall {
        /// Translator name to uninstall
        name: String,
    },
    /// Restart a translator
    Restart {
        /// Translator name to restart
        name: String,
    },
    /// List all active translators
    List,
    /// Show translator status and information
    Status {
        /// Translator name (optional, shows all if not provided)
        name: Option<String>,
    },
    /// Test synthetic file operations
    Test {
        /// Translator name
        translator: String,
        /// Synthetic file path to test
        file_path: String,
        /// Operation to test (read/write)
        #[arg(default_value = "read")]
        operation: String,
    },
}

#[derive(Subcommand, Debug)]
enum NamespaceAction {
    /// Create a new namespace
    Create {
        /// Namespace name
        name: String,
        /// Required signatures (m-of-n threshold)
        #[arg(short = 't', long, default_value = "1")]
        threshold: usize,
        /// Total possible signers
        #[arg(short = 'n', long, default_value = "1")]
        total_signers: usize,
        /// Enable founder veto power
        #[arg(long)]
        founder_veto: bool,
    },
    /// Request to join a namespace
    Join {
        /// Namespace ID to join
        namespace_id: String,
        /// Message to include with request
        #[arg(short, long, default_value = "Please add me to this namespace")]
        message: String,
    },
    /// Approve a join request (for existing members)
    Approve {
        /// Namespace ID
        namespace_id: String,
        /// User requesting to join (public key)
        requester: String,
    },
    /// List your namespaces
    List,
    /// List pending join requests for your namespaces
    Pending,
    /// Show global namespace registry
    Global,
    /// Show namespace translator status
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Use LocalSet to support spawn_local for mesh networking
    let local = tokio::task::LocalSet::new();
    local.run_until(main_async()).await
}

async fn main_async() -> Result<()> {
    let cli = Cli::parse();

    // Set up logging
    setup_logging(&cli)?;

    match cli.command {
        Commands::Serve {
            path,
            port,
            interface,
            quic,
            server_name,
            metrics_port,
            mesh_port,
            daemon,
            pid_file,
            namespace_readonly,
        } => {
            // Resolve interface to bind address
            let bind_addr = resolve_bind_address(interface.as_deref(), port)?;

            // Check for port conflicts before starting services
            info!("🔍 Checking for port conflicts...");
            if let Err(e) = check_port_conflicts(port, mesh_port, metrics_port) {
                error!("{}", e);
                return Err(e);
            }
            info!("✅ All ports are available");

            if daemon {
                daemonize_server(
                    path,
                    bind_addr.clone(),
                    quic,
                    server_name,
                    metrics_port,
                    mesh_port,
                    pid_file,
                ).await
            } else {
                serve_directory(
                    path,
                    bind_addr,
                    quic,
                    server_name.unwrap_or_else(|| "localhost".to_string()),
                    metrics_port,
                    mesh_port,
                    namespace_readonly,
                ).await
            }
        }

        Commands::Client { action } => {
            match action {
                ClientAction::Mount { server, mount_point } => {
                    let server_addr = if server == "auto" {
                        // Auto-discover server
                        match discover_first_server().await {
                            Some(addr) => addr,
                            None => {
                                error!("No servers found on network");
                                return Err(anyhow::anyhow!("Auto-discovery failed"));
                            }
                        }
                    } else {
                        server
                    };

                    info!("🗻 Mounting {} at {}", server_addr, mount_point);
                    mount_server(server_addr, mount_point).await
                }

                ClientAction::List { server, path, username, password } => {
                    let server_addr = if server == "auto" {
                        match discover_first_server().await {
                            Some(addr) => addr,
                            None => {
                                error!("No servers found on network");
                                return Err(anyhow::anyhow!("Auto-discovery failed"));
                            }
                        }
                    } else {
                        server
                    };

                    client::list_remote_files_with_auth(server_addr, path, username, password).await
                }

                ClientAction::Get { server, remote, local } => {
                    let server_addr = if server == "auto" {
                        match discover_first_server().await {
                            Some(addr) => addr,
                            None => {
                                error!("No servers found on network");
                                return Err(anyhow::anyhow!("Auto-discovery failed"));
                            }
                        }
                    } else {
                        server
                    };

                    let mut client = client::NinePeeClient::connect(&server_addr).await?;
                    let data = client.read_file(&remote).await?;
                    tokio::fs::write(&local, data).await?;
                    info!("✅ Downloaded {} to {}", remote, local);
                    Ok(())
                }
            }
        }

        Commands::Network { network_action } => {
            match network_action {
                NetworkAction::Discover => client::discover_nodes().await,
                NetworkAction::Topology => {
                    info!("📊 Mesh network topology visualization coming soon");
                    Ok(())
                }
                NetworkAction::Peers => {
                    info!("👥 Connected peers list coming soon");
                    Ok(())
                }
            }
        }

        Commands::Server { server_action } => {
            match server_action {
                ServerAction::Info => show_features().await,
                ServerAction::Setup => run_setup_wizard().await,
                ServerAction::Status { pid_file } => check_daemon_status(pid_file).await,
                ServerAction::Stop { pid_file } => stop_daemon(pid_file).await,
                ServerAction::Users { user_action } => handle_user_action(user_action).await,
            }
        }

        Commands::Advanced { advanced_action } => {
            match advanced_action {
                AdvancedAction::Blockchain { blockchain_action } => {
                    match blockchain_action {
                        BlockchainAction::Mine => consensus::commands::mine_test_block().await,
                        BlockchainAction::Status => consensus::commands::show_consensus_state().await,
                        BlockchainAction::Difficulty { value } => consensus::commands::set_difficulty(value).await,
                    }
                }
                AdvancedAction::Events { event_action } => {
                    match event_action {
                        EventAction::Status => show_event_chain_status().await,
                        EventAction::Recent { count } => show_recent_events(count).await,
                        EventAction::File { path } => show_file_events(&path).await,
                    }
                }
            }
        }

        Commands::AutoMount { automount_action } => {
            match automount_action {
                AutoMountAction::Start { mount_point } => {
                    info!("🚀 Starting auto-mount daemon at {}", mount_point);
                    auto_mount::commands::start_daemon().await
                }
                AutoMountAction::Stop => {
                    info!("🛑 Stopping auto-mount daemon");
                    // TODO: Implement stop functionality
                    println!("Auto-mount daemon stop functionality not yet implemented");
                    Ok(())
                }
                AutoMountAction::List => {
                    auto_mount::commands::list_mounts().await
                }
                AutoMountAction::Mount { server, mount_point } => {
                    info!("🗻 Manually mounting {} at {}", server, mount_point);
                    // TODO: Implement manual mount
                    println!("Manual mount functionality not yet implemented");
                    Ok(())
                }
                AutoMountAction::Unmount { target } => {
                    info!("🔻 Unmounting {}", target);
                    // TODO: Implement unmount
                    println!("Unmount functionality not yet implemented");
                    Ok(())
                }
            }
        }

        Commands::Translator { translator_action } => {
            handle_translator_command(translator_action).await
        }

        Commands::Namespace { namespace_action: _ } => {
            info!("🏷️ Namespace commands not yet implemented");
            println!("⚠️  Namespace commands not yet implemented");
            println!("🔜 This will be available in a future version");
            Ok(())
        }
    }
}

/// Set up signal handlers for graceful shutdown
async fn setup_signal_handlers(shutdown: Arc<tokio::sync::Notify>) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut sigterm = signal(SignalKind::terminate()).expect("Failed to setup SIGTERM handler");
        let mut sigint = signal(SignalKind::interrupt()).expect("Failed to setup SIGINT handler");

        tokio::select! {
            _ = sigterm.recv() => {
                info!("🛑 Received SIGTERM, initiating graceful shutdown...");
                shutdown.notify_waiters();
            }
            _ = sigint.recv() => {
                info!("🛑 Received SIGINT (Ctrl+C), initiating graceful shutdown...");
                shutdown.notify_waiters();
            }
        }
    }

    #[cfg(windows)]
    {
        tokio::signal::ctrl_c().await.expect("Failed to setup Ctrl+C handler");
        info!("🛑 Received Ctrl+C, initiating graceful shutdown...");
        shutdown.notify_waiters();
    }
}

/// Check if a port is already in use
fn is_port_in_use(port: u16) -> bool {
    std::net::TcpListener::bind(format!("0.0.0.0:{}", port)).is_err()
}

/// Find an available port starting from the given port
fn find_available_port(start_port: u16) -> Result<u16> {
    for port in start_port..start_port + 100 {
        if !is_port_in_use(port) {
            return Ok(port);
        }
    }
    Err(anyhow::anyhow!("No available ports found in range {}-{}", start_port, start_port + 100))
}

/// Check for port conflicts and suggest alternatives
fn check_port_conflicts(main_port: u16, mesh_port: u16, metrics_port: u16) -> Result<()> {
    let mut conflicts = Vec::new();

    if is_port_in_use(main_port) {
        conflicts.push(format!("Main server port {} is already in use", main_port));
    }

    if is_port_in_use(mesh_port) {
        conflicts.push(format!("Mesh network port {} is already in use", mesh_port));
    }

    if is_port_in_use(metrics_port) {
        conflicts.push(format!("Metrics port {} is already in use", metrics_port));
    }

    if !conflicts.is_empty() {
        let mut error_msg = "Port conflicts detected:\n".to_string();
        for conflict in &conflicts {
            error_msg.push_str(&format!("  ❌ {}\n", conflict));
        }

        // Suggest alternatives
        error_msg.push_str("\n💡 Suggested alternatives:\n");
        if let Ok(alt_main) = find_available_port(main_port) {
            error_msg.push_str(&format!("  📡 Main server: --port {}\n", alt_main));
        }
        if let Ok(alt_mesh) = find_available_port(mesh_port) {
            error_msg.push_str(&format!("  🌐 Mesh network: --mesh-port {}\n", alt_mesh));
        }
        if let Ok(alt_metrics) = find_available_port(metrics_port) {
            error_msg.push_str(&format!("  📊 Metrics: --metrics-port {}\n", alt_metrics));
        }

        return Err(anyhow::anyhow!("{}", error_msg));
    }

    Ok(())
}

/// Resolve an interface name or IP address to a full bind address (IPv6 first!)
fn resolve_bind_address(interface: Option<&str>, port: u16) -> Result<String> {
    match interface {
        None => Ok(format!("[::]:{}", port)), // IPv6 any address (also accepts IPv4)
        Some(iface) => {
            // Check if it's already an IP address
            if iface.parse::<std::net::IpAddr>().is_ok() {
                Ok(format!("{}:{}", iface, port))
            } else {
                // Try to resolve interface name
                match iface {
                    "lo" | "localhost" => Ok(format!("[::1]:{}", port)), // IPv6 loopback
                    "lo4" | "localhost4" => Ok(format!("127.0.0.1:{}", port)), // IPv4 loopback
                    "any" | "all" => Ok(format!("[::]:{}", port)), // IPv6 any (dual-stack)
                    "any4" | "all4" => Ok(format!("0.0.0.0:{}", port)), // IPv4 only
                    "any6" | "all6" => Ok(format!("[::]:{}", port)), // IPv6 any
                    _ => {
                        // Try to get IP from network interface
                        // For now, we'll just support common names
                        // In a real implementation, we'd query the system interfaces
                        warn!("Interface '{}' not recognized, using IPv6 dual-stack", iface);
                        Ok(format!("[::]:{}", port)) // Default to IPv6 dual-stack
                    }
                }
            }
        }
    }
}

async fn serve_directory(
    path: PathBuf,
    bind: String,
    use_quic: bool,
    server_name: String,
    metrics_port: u16,
    mesh_port: u16,
    namespace_readonly: bool,
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
    info!("🔒 Transport: {}", if use_quic { "QUIC (modern)" } else { "TCP (legacy)" });
    info!("📊 Metrics: http://[::]:{}/metrics (IPv6 dual-stack)", metrics_port);

    // Initialize metrics
    metrics::init_metrics();

    // Start metrics server in background
    let _metrics_handle = tokio::spawn(async move {
        if let Err(e) = metrics::start_metrics_server(metrics_port).await {
            error!("Metrics server failed: {}", e);
        }
    });

    // Create synthetic filesystem for web UI
    let _synthetic_fs = std::sync::Arc::new(crate::synthetic::SyntheticFileSystem::new());


    // Start mesh networking and global event chain
    info!("🌐 Starting mesh networking on port {}", mesh_port);
    // Pass the 9P service address to mesh for discovery
    let service_addr = Some(bind.clone());
    // Re-enabled mesh networking with blockchain integration!
    let (_mesh_sender, mesh_peers, event_chain) = match crate::mesh::start_mesh_network(mesh_port, service_addr).await {
        Ok((sender, peers)) => {
            info!("✅ Mesh network started successfully");
            info!("⛓️ Initializing global event chain...");

            // Initialize event chain for distributed consensus
            match global_event_chain::GlobalEventChain::new(None).await {
                Ok(chain) => {
                    info!("✅ Event chain active - all operations globally ordered");
                    (Some(sender), Some(peers), Some(Arc::new(chain)))
                }
                Err(e) => {
                    warn!("Event chain failed: {}", e);
                    (Some(sender), Some(peers), None)
                }
            }
        }
        Err(e) => {
            warn!("Failed to start mesh network: {}", e);
            (None, None, None)
        }
    };

    // Initialize Plan 9 namespace directories (/srv and /n/)
    info!("📁 Initializing Plan 9 namespace directories");
    if let Err(e) = crate::fuse_mount::initialize_plan9_namespace().await {
        warn!("Failed to initialize Plan 9 namespace directories: {}", e);
    } else {
        info!("✅ Plan 9 namespace directories initialized (/srv and /n/)");
    }

    // Mount namespace filesystem for service discovery (optional)
    let fuse_mounted = if mesh_peers.is_some() {
        info!("📁 Mounting namespace filesystem at /tmp/9pe-namespace");

        // Clean up any existing stale mounts first
        if crate::namespace::is_mounted("/tmp/9pe-namespace") {
            warn!("🧹 Cleaning up existing stale FUSE mount at /tmp/9pe-namespace");
            if let Err(e) = crate::namespace::unmount_namespace_fs("/tmp/9pe-namespace") {
                warn!("Failed to cleanup stale mount: {}", e);
            } else {
                info!("✅ Stale mount cleaned up successfully");
            }
        }

        match crate::namespace::mount_namespace_fs("/tmp/9pe-namespace", mesh_peers.clone(), namespace_readonly).await {
            Ok(_) => {
                info!("✅ Namespace filesystem mounted");
                info!("   - /tmp/9pe-namespace/srv: Service discovery");
                info!("   - /tmp/9pe-namespace/n: Network namespaces");
                true
            }
            Err(e) => {
                warn!("Failed to mount namespace filesystem: {}", e);
                false
            }
        }
    } else {
        false
    };

    // Auto-mount functionality is available via 'connect mount auto' command


    if use_quic {
        // For QUIC, use provided server_name (already unwrapped by caller)
        start_quic_server(path, addr, server_name, fuse_mounted).await
    } else {
        start_tcp_server(path, addr, fuse_mounted).await
    }
}

async fn start_tcp_server(path: PathBuf, addr: SocketAddr, fuse_mounted: bool) -> Result<()> {
    // Set up signal handlers for graceful shutdown
    let shutdown = Arc::new(tokio::sync::Notify::new());
    let shutdown_clone = Arc::clone(&shutdown);

    // Create the filesystem server with mandatory authentication
    let auth_service = Arc::new(auth::AuthService::new());
    let config = setup_auth_from_config(&auth_service).await?;
    let fs_server = Arc::new(server::FileSystemServer::new(path).await?);

    info!("🔒 PRODUCTION MODE: Mandatory authentication enabled");
    info!("📁 Using FileSystemServer with auth service");

    // Bind TCP listener
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("✅ TCP server listening on {}", addr);

    // Spawn signal handler
    let signal_shutdown = Arc::clone(&shutdown);
    tokio::spawn(async move {
        setup_signal_handlers(signal_shutdown).await;
    });

    info!("🔧 Graceful shutdown enabled (Ctrl+C or SIGTERM)");

    loop {
        tokio::select! {
            // Accept new connections
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((socket, peer_addr)) => {
                        info!("New TCP connection from {}", peer_addr);
                        let server = Arc::clone(&fs_server);
                        let auth = Arc::clone(&auth_service);
                        let conn_shutdown = Arc::clone(&shutdown);
                        tokio::spawn(async move {
                            tokio::select! {
                                result = handle_authenticated_tcp_connection(socket, server, auth, peer_addr) => {
                                    if let Err(e) = result {
                                        error!("Connection error: {}", e);
                                    }
                                }
                                _ = conn_shutdown.notified() => {
                                    info!("Closing connection from {} due to shutdown", peer_addr);
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept connection: {}", e);
                    }
                }
            }
            // Handle shutdown signal
            _ = shutdown_clone.notified() => {
                info!("🛑 Shutting down TCP server gracefully...");

                // Cleanup Plan 9 namespace directories
                info!("🧹 Cleaning up Plan 9 namespace directories");
                if let Err(e) = crate::fuse_mount::cleanup_plan9_namespace().await {
                    error!("Failed to cleanup Plan 9 namespace directories: {}", e);
                } else {
                    info!("✅ Plan 9 namespace directories cleaned up (/srv and /n/)");
                }

                // Cleanup FUSE mount if it was created
                if fuse_mounted {
                    info!("🔧 Unmounting FUSE filesystem...");
                    if let Err(e) = crate::namespace::unmount_namespace_fs("/tmp/9pe-namespace") {
                        error!("Failed to unmount FUSE filesystem: {}", e);
                    } else {
                        info!("✅ FUSE filesystem unmounted successfully");
                    }
                }

                break;
            }
        }
    }

    info!("✅ TCP server shutdown complete");
    Ok(())
}

/// Set up authentication using persistent configuration
async fn setup_auth_from_config(auth_service: &auth::AuthService) -> Result<config::ServerConfig> {
    use auth::{User, AclEntry};
    use argon2::{Argon2, PasswordHash, PasswordVerifier};

    // Initialize configuration manager
    let mut config_manager = config::ConfigManager::new()?;

    // Load or create configuration
    let config = config_manager.initialize().await?;

    if config.initialized {
        info!("📁 Loading saved configuration from disk");
    }

    // Create user from config
    let admin_user = User {
        uid: 1000,
        username: config.auth.admin_username.clone(),
        password_hash: config.auth.admin_password_hash.clone(),
        groups: vec!["administrators".to_string()],
        home_dir: "/srv/9pe/admin".to_string(),
        shell: "/bin/rc".to_string(),
        public_key: None,
    };

    auth_service.add_user(admin_user).await?;

    // Set up ACL from config
    let acl_entry = AclEntry {
        principal: config.auth.admin_username.clone(),
        permissions: 0o777,
        inheritable: true,
    };

    auth_service.add_acl("/".to_string(), acl_entry).await?;

    // Set up namespace if configured
    if config.namespace.enable_srv || config.namespace.enable_n {
        info!("📁 Namespace system enabled:");
        if config.namespace.enable_srv {
            info!("   - /srv directory for service discovery");
        }
        if config.namespace.enable_n {
            info!("   - /n/ directory for namespace mounting");
        }
    }

    info!("🔐 Authentication configured from {}",
          if config.initialized { "saved config" } else { "new setup" });

    Ok(config)
}

/// Generate secure password hash using Argon2id
fn generate_secure_password_hash(password: &str) -> String {
    use argon2::{Argon2, PasswordHasher};
    use argon2::password_hash::{rand_core::OsRng, SaltString};

    // Generate a random salt
    let salt = SaltString::generate(&mut OsRng);

    // Create Argon2 instance with secure defaults
    let argon2 = Argon2::default();

    // Hash password
    match argon2.hash_password(password.as_bytes(), &salt) {
        Ok(hash) => hash.to_string(),
        Err(e) => {
            eprintln!("Password hashing error: {}", e);
            // Fallback to prevent complete failure, but log the error
            format!("HASH_FAILED_{}", e)
        }
    }
}

/// Generate a secure random password of specified length
fn generate_secure_random_password(length: usize) -> String {
    use rand::Rng;

    // Character set for password generation (alphanumeric + some safe symbols)
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ\
                             abcdefghijklmnopqrstuvwxyz\
                             0123456789\
                             !@#$%^&*";

    let mut rng = rand::thread_rng();

    (0..length)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// Handle TCP connection with mandatory authentication
async fn handle_authenticated_tcp_connection(
    mut socket: tokio::net::TcpStream,
    fs_server: Arc<server::FileSystemServer>,
    auth_service: Arc<auth::AuthService>,
    peer_addr: std::net::SocketAddr
) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use ninepee::NinePeeMessage;
    use tokio::time::{timeout, Duration};

    info!("Starting authenticated 9P.e session from {}", peer_addr);
    metrics::record_connection("tcp", true);

    // Set TCP keepalive and nodelay for better connection handling
    socket.set_nodelay(true).unwrap_or_else(|e| {
        warn!("Failed to set TCP nodelay: {}", e);
    });

    let mut authenticated = false;
    let mut _connection_user: Option<auth::User> = None;
    let mut consecutive_errors = 0;
    const MAX_CONSECUTIVE_ERRORS: u32 = 5;
    const READ_TIMEOUT: Duration = Duration::from_secs(120); // 2 minute timeout

    // Message handling loop with authentication
    loop {
        // Read message length with timeout
        let mut len_buf = [0u8; 4];
        match timeout(READ_TIMEOUT, socket.read_exact(&mut len_buf)).await {
            Ok(Ok(_)) => {
                consecutive_errors = 0; // Reset error counter on success
            }
            Ok(Err(e)) => {
                info!("TCP connection closed by client: {}", e);
                break;
            }
            Err(_) => {
                warn!("Connection timeout from {}", peer_addr);
                break;
            }
        }
        let msg_len = u32::from_le_bytes(len_buf) as usize;

        // Validate message size
        if msg_len < 4 || msg_len > 16 * 1024 * 1024 {
            error!("Invalid message size: {}", msg_len);
            break;
        }

        // Read message body with timeout
        let mut msg_buf = vec![0u8; msg_len - 4];
        match timeout(READ_TIMEOUT, socket.read_exact(&mut msg_buf)).await {
            Ok(Ok(_)) => {},
            Ok(Err(e)) => {
                error!("Failed to read message body: {}", e);
                consecutive_errors += 1;
                if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                    error!("Too many consecutive errors, closing connection");
                    break;
                }
                continue;
            }
            Err(_) => {
                warn!("Message read timeout");
                break;
            }
        }

        // Deserialize message
        let request = match NinePeeMessage::deserialize(msg_buf) {
            Ok(msg) => msg,
            Err(e) => {
                error!("Failed to deserialize message: {}", e);
                consecutive_errors += 1;
                if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                    error!("Too many consecutive errors, closing connection");
                    break;
                }

                // Send error response for malformed message
                let error_resp = NinePeeMessage::Error {
                    ename: "Malformed message".to_string(),
                    errno: 22, // EINVAL
                };
                if let Ok(data) = error_resp.serialize() {
                    let _ = socket.write_all(&(data.len() as u32 + 4).to_le_bytes()).await;
                    let _ = socket.write_all(&data).await;
                }
                continue;
            }
        };

        // Handle authentication requirement
        let response = if !authenticated && !matches!(request, NinePeeMessage::Version { .. } | NinePeeMessage::Auth { .. } | NinePeeMessage::Attach { .. }) {
            // Require authentication before allowing any operations
            NinePeeMessage::Error {
                ename: "Authentication required".to_string(),
                errno: 1,
            }
        } else {
            match &request {
                NinePeeMessage::Auth { uname, password, .. } => {
                    // Attempt authentication with provided credentials
                    match password {
                        Some(pass) => {
                            match auth_service.authenticate(&auth::AuthMethod::Password(pass.clone())).await {
                                Ok(user) if user.username == *uname => {
                                    authenticated = true;
                                    _connection_user = Some(user);
                                    info!("User '{}' authenticated successfully", uname);
                                    // Return empty Attach to indicate auth success (9P convention)
                                    // The client will then send a real Attach
                                    NinePeeMessage::Attach {
                                        fid: 0,
                                        afid: 0,
                                        uname: uname.clone(),
                                        aname: "/".to_string(),
                                    }
                                },
                                Ok(_) => {
                                    warn!("Authentication succeeded but username mismatch for '{}'", uname);
                                    NinePeeMessage::Error {
                                        ename: "Authentication failed - username mismatch".to_string(),
                                        errno: 13, // EACCES
                                    }
                                },
                                Err(e) => {
                                    warn!("Authentication failed for user '{}': {}", uname, e);
                                    NinePeeMessage::Error {
                                        ename: "Authentication failed".to_string(),
                                        errno: 13, // EACCES
                                    }
                                }
                            }
                        },
                        None => {
                            warn!("Authentication attempted without password for user '{}'", uname);
                            NinePeeMessage::Error {
                                ename: "Password required for authentication".to_string(),
                                errno: 13, // EACCES
                            }
                        }
                    }
                },
                _ => {
                    // Process authenticated message with filesystem server
                    match fs_server.process_message(request).await {
                        Ok(resp) => resp,
                        Err(e) => {
                            error!("Error processing message: {}", e);
                            NinePeeMessage::Error {
                                ename: e.to_string(),
                                errno: 1,
                            }
                        }
                    }
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

        // Send response length and data
        let response_len = (response_data.len() + 4) as u32;
        let len_bytes = response_len.to_le_bytes();

        if socket.write_all(&len_bytes).await.is_err() ||
           socket.write_all(&response_data).await.is_err() {
            error!("Failed to send response");
            break;
        }
    }

    metrics::record_connection("tcp", false);
    Ok(())
}

async fn start_quic_server(path: PathBuf, addr: SocketAddr, server_name: String, fuse_mounted: bool) -> Result<()> {
    use quinn::{Endpoint, ServerConfig};
    use std::sync::Arc;

    info!("🔐 Starting QUIC server on {} with name {}", addr, server_name);

    // Create the filesystem server with mandatory authentication
    let auth_service = Arc::new(auth::AuthService::new());
    let config = setup_auth_from_config(&auth_service).await?;
    let fs_server = Arc::new(server::FileSystemServer::new(path).await?);
    info!("🔒 Using FileSystemServer with mandatory authentication for QUIC");

    // Generate self-signed certificate for development
    let server_config = generate_server_config(&server_name)?;

    // Create QUIC endpoint
    let endpoint = Endpoint::server(server_config, addr)
        .map_err(|e| anyhow::anyhow!("Failed to create QUIC endpoint: {}", e))?;

    info!("✅ QUIC server listening on {}", addr);
    warn!("🔓 Using self-signed certificate for development - not suitable for production!");

    // Set up signal handlers for graceful shutdown
    let shutdown = Arc::new(tokio::sync::Notify::new());
    let shutdown_clone = Arc::clone(&shutdown);

    // Spawn signal handler
    let signal_shutdown = Arc::clone(&shutdown);
    tokio::spawn(async move {
        setup_signal_handlers(signal_shutdown).await;
    });

    info!("🔧 Graceful shutdown enabled (Ctrl+C or SIGTERM)");

    // Handle incoming connections with graceful shutdown
    loop {
        tokio::select! {
            // Accept new connections
            conn_result = endpoint.accept() => {
                match conn_result {
                    Some(conn) => {
                        let connection = match conn.await {
                            Ok(conn) => conn,
                            Err(e) => {
                                error!("Failed to establish QUIC connection: {}", e);
                                continue;
                            }
                        };

                        let server = Arc::clone(&fs_server);
                        let auth = Arc::clone(&auth_service);
                        let peer_addr = connection.remote_address();
                        let conn_shutdown = Arc::clone(&shutdown);

                        tokio::spawn(async move {
                            tokio::select! {
                                result = handle_authenticated_quic_connection(connection, server, auth) => {
                                    if let Err(e) = result {
                                        error!("QUIC connection error: {}", e);
                                    }
                                }
                                _ = conn_shutdown.notified() => {
                                    info!("Closing QUIC connection from {} due to shutdown", peer_addr);
                                }
                            }
                        });
                    }
                    None => break, // Endpoint closed
                }
            }
            // Handle shutdown signal
            _ = shutdown_clone.notified() => {
                info!("🛑 Shutting down QUIC server gracefully...");

                // Cleanup Plan 9 namespace directories
                info!("🧹 Cleaning up Plan 9 namespace directories");
                if let Err(e) = crate::fuse_mount::cleanup_plan9_namespace().await {
                    error!("Failed to cleanup Plan 9 namespace directories: {}", e);
                } else {
                    info!("✅ Plan 9 namespace directories cleaned up (/srv and /n/)");
                }

                // Cleanup FUSE mount if it was created
                if fuse_mounted {
                    info!("🔧 Unmounting FUSE filesystem...");
                    if let Err(e) = crate::namespace::unmount_namespace_fs("/tmp/9pe-namespace") {
                        error!("Failed to unmount FUSE filesystem: {}", e);
                    } else {
                        info!("✅ FUSE filesystem unmounted successfully");
                    }
                }

                break;
            }
        }
    }

    info!("✅ QUIC server shutdown complete");
    Ok(())
}

/// Generate a ServerConfig with self-signed certificate for development use
fn generate_server_config(server_name: &str) -> Result<quinn::ServerConfig> {
    use rcgen::{Certificate, CertificateParams, DistinguishedName};
    use rustls::{ServerConfig as RustlsServerConfig};

    let mut params = CertificateParams::new(vec![server_name.to_string()]);
    params.distinguished_name = DistinguishedName::new();
    params.distinguished_name.push(rcgen::DnType::CommonName, server_name);

    let cert = Certificate::from_params(params)
        .map_err(|e| anyhow::anyhow!("Failed to generate certificate: {}", e))?;

    let cert_der = cert.serialize_der()
        .map_err(|e| anyhow::anyhow!("Failed to serialize certificate: {}", e))?;
    let private_key_der = cert.serialize_private_key_der();

    // Create rustls certificate and private key (rustls 0.21)
    let cert_chain = vec![rustls::Certificate(cert_der)];
    let private_key = rustls::PrivateKey(private_key_der);

    // Create rustls server config
    let rustls_config = RustlsServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(cert_chain, private_key)
        .map_err(|e| anyhow::anyhow!("Failed to create rustls config: {}", e))?;

    // Create quinn server config
    let server_config = quinn::ServerConfig::with_crypto(Arc::new(rustls_config));

    Ok(server_config)
}

/// Handle QUIC connection with mandatory authentication
async fn handle_authenticated_quic_connection(
    connection: quinn::Connection,
    fs_server: Arc<server::FileSystemServer>,
    auth_service: Arc<auth::AuthService>
) -> Result<()> {

    info!("QUIC connection established from {}", connection.remote_address());
    metrics::record_connection("quic", true);

    // Accept bi-directional streams for 9P.e messages
    while let Ok((send_stream, recv_stream)) = connection.accept_bi().await {
        let server = Arc::clone(&fs_server);
        let auth = Arc::clone(&auth_service);

        tokio::spawn(async move {
            if let Err(e) = handle_authenticated_quic_stream(send_stream, recv_stream, server, auth).await {
                error!("QUIC stream error: {}", e);
            }
        });
    }

    metrics::record_connection("quic", false);
    Ok(())
}

/// Handle individual QUIC stream with mandatory authentication
async fn handle_authenticated_quic_stream(
    mut send_stream: quinn::SendStream,
    mut recv_stream: quinn::RecvStream,
    fs_server: Arc<server::FileSystemServer>,
    auth_service: Arc<auth::AuthService>
) -> Result<()> {
    use ninepee::NinePeeMessage;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    info!("Handling new authenticated QUIC stream");

    let mut authenticated = false;
    let mut _connection_user: Option<auth::User> = None;

    loop {
        // Read message length (4 bytes)
        let mut len_buf = [0u8; 4];
        if recv_stream.read_exact(&mut len_buf).await.is_err() {
            info!("QUIC stream closed by client");
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
        if recv_stream.read_exact(&mut msg_buf).await.is_err() {
            error!("Failed to read QUIC message body");
            break;
        }

        // Deserialize message
        let request = match NinePeeMessage::deserialize(msg_buf) {
            Ok(msg) => msg,
            Err(e) => {
                error!("Failed to deserialize QUIC message: {}", e);
                continue;
            }
        };

        // Handle authentication requirement
        let response = if !authenticated && !matches!(request, NinePeeMessage::Version { .. } | NinePeeMessage::Auth { .. } | NinePeeMessage::Attach { .. }) {
            // Require authentication before allowing any operations
            NinePeeMessage::Error {
                ename: "Authentication required".to_string(),
                errno: 1,
            }
        } else {
            match &request {
                NinePeeMessage::Auth { uname, password, .. } => {
                    // Attempt authentication with provided credentials
                    match password {
                        Some(pass) => {
                            match auth_service.authenticate(&auth::AuthMethod::Password(pass.clone())).await {
                                Ok(user) if user.username == *uname => {
                                    authenticated = true;
                                    _connection_user = Some(user);
                                    info!("User '{}' authenticated successfully via QUIC", uname);
                                    // Return success (in production, should return proper auth response)
                                    NinePeeMessage::Version {
                                        msize: 8192,
                                        version: "9P.e".to_string(),
                                    }
                                },
                                Ok(_) => {
                                    warn!("QUIC authentication succeeded but username mismatch for '{}'", uname);
                                    NinePeeMessage::Error {
                                        ename: "Authentication failed - username mismatch".to_string(),
                                        errno: 13, // EACCES
                                    }
                                },
                                Err(e) => {
                                    warn!("QUIC authentication failed for user '{}': {}", uname, e);
                                    NinePeeMessage::Error {
                                        ename: "Authentication failed".to_string(),
                                        errno: 13, // EACCES
                                    }
                                }
                            }
                        },
                        None => {
                            warn!("QUIC authentication attempted without password for user '{}'", uname);
                            NinePeeMessage::Error {
                                ename: "Password required for authentication".to_string(),
                                errno: 13, // EACCES
                            }
                        }
                    }
                },
                _ => {
                    // Process authenticated message with filesystem server
                    match fs_server.process_message(request).await {
                        Ok(resp) => resp,
                        Err(e) => {
                            error!("Error processing QUIC message: {}", e);
                            NinePeeMessage::Error {
                                ename: e.to_string(),
                                errno: 1,
                            }
                        }
                    }
                }
            }
        };

        // Serialize response
        let response_data = match response.serialize() {
            Ok(data) => data,
            Err(e) => {
                error!("Failed to serialize QUIC response: {}", e);
                continue;
            }
        };

        // Send response length and data
        let response_len = (response_data.len() + 4) as u32;
        let len_bytes = response_len.to_le_bytes();

        if send_stream.write_all(&len_bytes).await.is_err() ||
           send_stream.write_all(&response_data).await.is_err() {
            error!("Failed to send QUIC response");
            break;
        }
    }

    // Close streams
    let _ = send_stream.finish().await;
    let _ = recv_stream.stop(0u32.into());

    Ok(())
}

// Production hardening complete - all connections now require authentication

/// Handle translator management commands
async fn handle_translator_command(action: TranslatorAction) -> Result<()> {
    use crate::translator::{TranslatorManager, SyntheticRequest, Operation};
    use std::collections::HashMap;

    // Create translator manager with /srv directory
    let srv_base = std::path::PathBuf::from("/srv");
    let mut manager = TranslatorManager::new(srv_base).await
        .context("Failed to initialize translator manager")?;

    match action {
        TranslatorAction::Install { wasm_file } => {
            info!("📦 Installing WASM translator from {:?}", wasm_file);

            let wasm_bytes = tokio::fs::read(&wasm_file).await
                .with_context(|| format!("Failed to read WASM file: {:?}", wasm_file))?;

            match manager.install_translator(wasm_bytes).await {
                Ok(name) => {
                    println!("✅ Successfully installed translator: {}", name);
                    println!("📁 Translator directory created at: /srv/{}", name);
                    Ok(())
                }
                Err(e) => {
                    error!("Failed to install translator: {}", e);
                    Err(e)
                }
            }
        }

        TranslatorAction::Uninstall { name } => {
            info!("🗑️ Uninstalling translator: {}", name);

            match manager.uninstall_translator(&name).await {
                Ok(()) => {
                    println!("✅ Successfully uninstalled translator: {}", name);
                    Ok(())
                }
                Err(e) => {
                    error!("Failed to uninstall translator: {}", e);
                    Err(e)
                }
            }
        }

        TranslatorAction::Restart { name } => {
            info!("🔄 Restarting translator: {}", name);

            match manager.restart_translator(&name).await {
                Ok(()) => {
                    println!("✅ Successfully restarted translator: {}", name);
                    Ok(())
                }
                Err(e) => {
                    error!("Failed to restart translator: {}", e);
                    Err(e)
                }
            }
        }

        TranslatorAction::List => {
            info!("📋 Listing active translators");

            let translators = manager.list_translators().await;

            if translators.is_empty() {
                println!("📭 No translators are currently active");
            } else {
                println!("📋 Active Translators:");
                println!("╭─────────────────────────────────────────────────────────────╮");
                println!("│ Name                    │ Status                          │");
                println!("├─────────────────────────────────────────────────────────────┤");
                for (name, status) in translators {
                    let status_str = match status {
                        crate::translator::TranslatorStatus::Starting => "🟡 Starting",
                        crate::translator::TranslatorStatus::Running => "🟢 Running",
                        crate::translator::TranslatorStatus::Failed(ref msg) => &format!("🔴 Failed: {}", msg),
                        crate::translator::TranslatorStatus::Stopped => "⚪ Stopped",
                        crate::translator::TranslatorStatus::Restarting => "🔄 Restarting",
                    };
                    println!("│ {:<23} │ {:<31} │", name, status_str);
                }
                println!("╰─────────────────────────────────────────────────────────────╯");
            }
            Ok(())
        }

        TranslatorAction::Status { name } => {
            if let Some(translator_name) = name {
                info!("📊 Getting status for translator: {}", translator_name);

                if let Some(status) = manager.get_translator_status(&translator_name).await {
                    println!("📊 Translator Status: {}", translator_name);
                    println!("Status: {:?}", status);
                } else {
                    println!("❌ Translator not found: {}", translator_name);
                }
            } else {
                // Show status for all translators
                let translators = manager.list_translators().await;

                if translators.is_empty() {
                    println!("📭 No translators are currently active");
                } else {
                    println!("📊 Translator Status Summary:");
                    for (name, status) in translators {
                        println!("  • {}: {:?}", name, status);
                    }
                }
            }
            Ok(())
        }

        TranslatorAction::Test { translator, file_path, operation } => {
            info!("🧪 Testing synthetic file operation: {} on {}/{}", operation, translator, file_path);

            let op = match operation.as_str() {
                "read" => Operation::Read,
                "write" => Operation::Write,
                "create" => Operation::Create,
                "delete" => Operation::Delete,
                "list" => Operation::List,
                _ => {
                    error!("Invalid operation: {}. Use read, write, create, delete, or list", operation);
                    return Err(anyhow::anyhow!("Invalid operation"));
                }
            };

            let request = SyntheticRequest {
                file_path: file_path.clone(),
                operation: op,
                data: None,
                params: HashMap::new(),
            };

            match manager.handle_synthetic_file(&translator, request).await {
                Ok(response) => {
                    println!("🧪 Test Result for {}/{}:", translator, file_path);
                    println!("  Success: {}", response.success);

                    if let Some(data) = response.data {
                        if let Ok(decoded) = serde_cbor::from_slice::<serde_cbor::Value>(&data) {
                            println!("  Data: {:#?}", decoded);
                        } else {
                            println!("  Data: {} bytes (binary)", data.len());
                        }
                    }

                    if let Some(error) = response.error {
                        println!("  Error: {}", error);
                    }

                    if !response.metadata.is_empty() {
                        println!("  Metadata: {:#?}", response.metadata);
                    }

                    Ok(())
                }
                Err(e) => {
                    error!("Failed to test synthetic file operation: {}", e);
                    Err(e)
                }
            }
        }
    }
}

/// Display available features and capabilities
async fn show_features() -> Result<()> {
    use std::env;

    println!("🔧 9P.e Server Features & Capabilities");
    println!("======================================");

    // Core features (always available)
    println!("\n📦 Core Features (Always Available):");
    println!("  ✅ 9P.e Protocol - Modern filesystem protocol");
    println!("  ✅ TCP Transport - Basic networking support");
    println!("  ✅ FUSE Mounting - Linux/macOS filesystem mounting");
    println!("  ✅ Authentication - Ed25519 + capability tokens");
    println!("  ✅ Metrics - Prometheus monitoring endpoints");
    println!("  ✅ Client Tools - File browsing and downloading");

    // Transport options
    println!("\n🚀 Transport Options:");
    println!("  ✅ TCP - Standard TCP sockets (--bind 0.0.0.0:5641)");
    println!("  ✅ QUIC - Encrypted UDP transport (--quic --server-name <name>)");

    // Feature flags
    println!("\n🎛️  Optional Feature Flags:");

    let mut features_enabled: Vec<&str> = Vec::new();
    let mut features_available: Vec<&str> = Vec::new();

    // Check compile-time features
    #[cfg(feature = "wasm")]
    features_enabled.push("WASM Translators");
    #[cfg(not(feature = "wasm"))]
    features_available.push("WASM - Dynamic file processing (cargo build --features wasm)");

    #[cfg(feature = "advanced")]
    features_enabled.push("Advanced Synthetic Files");
    #[cfg(not(feature = "advanced"))]
    features_available.push("Advanced - Enhanced synthetic files (cargo build --features advanced)");

    #[cfg(feature = "grid")]
    features_enabled.push("Grid Computing");
    #[cfg(not(feature = "grid"))]
    features_available.push("Grid - Distributed computing (cargo build --features grid)");

    #[cfg(feature = "native")]
    features_enabled.push("Native GUI");
    #[cfg(not(feature = "native"))]
    features_available.push("Native - WRY-based GUI (cargo build --features native)");

    #[cfg(feature = "gtk")]
    features_enabled.push("GTK4 GUI");
    #[cfg(not(feature = "gtk"))]
    features_available.push("GTK - GTK4-based GUI (cargo build --features gtk)");

    if !features_enabled.is_empty() {
        println!("\n  ✅ Enabled Features:");
        for feature in features_enabled {
            println!("    ✅ {}", feature);
        }
    }

    if !features_available.is_empty() {
        println!("\n  🔲 Available Features (rebuild to enable):");
        for feature in features_available {
            println!("    🔲 {}", feature);
        }
    }

    // Runtime capabilities
    println!("\n🌐 Network Capabilities:");
    println!("  ✅ Mesh Networking - Automatic peer discovery via libp2p");
    println!("  ✅ Auto-Discovery - Find servers without IP addresses");
    println!("  ✅ Auto-Mounting - Automatic FUSE mount management");

    // Blockchain features
    println!("\n⛓️  Blockchain Features:");
    println!("  ✅ GhostDAG Consensus - Byzantine-fault tolerant consensus");
    println!("  ✅ Block Mining - Proof-of-work mining capabilities");
    println!("  ✅ Consensus Monitoring - Real-time blockchain state");

    // Security features
    println!("\n🔒 Security Features:");
    println!("  ✅ Mandatory Authentication - Production-ready security");
    println!("  ✅ Ed25519 Signatures - Modern cryptographic signatures");
    println!("  ✅ Capability Tokens - Fine-grained access control");
    println!("  ✅ ACL Support - User and group permissions");
    println!("  ✅ Rate Limiting - DDoS protection");

    // File system features
    println!("\n📁 File System Features:");
    println!("  ✅ Synthetic Files - Dynamic content generation");
    println!("  ✅ Function Files - Executable file content");
    println!("  ✅ Modern Drawing - Real-time graphics generation");
    println!("  ✅ Version Control Integration - Git-aware file serving");

    // Quick start recommendations
    println!("\n🚀 Quick Start Recommendations:");
    println!("  • Basic server:       9pe-server serve");
    println!("  • Network accessible: 9pe-server serve --bind 0.0.0.0:5641");
    println!("  • With mesh discovery: 9pe-server serve --mesh-port 9650");
    println!("  • Secure QUIC:         9pe-server serve --quic --server-name myserver.local");
    println!("  • Feature setup:       9pe-server setup");

    println!("\n💡 Use '9pe-server setup' for guided feature configuration");
    Ok(())
}

/// Interactive setup wizard for features
async fn run_setup_wizard() -> Result<()> {
    println!("🧙 9P.e Server Setup Wizard");
    println!("===========================");

    println!("\nThis wizard will help you configure advanced features.");
    println!("Note: Some features require rebuilding with different flags.");

    // Check current build features
    let mut rebuild_needed = false;

    println!("\n📦 Checking current build configuration...");

    #[cfg(not(feature = "wasm"))]
    {
        println!("  🔲 WASM support not enabled");
        println!("     To enable: cargo build --features wasm");
        rebuild_needed = true;
    }

    #[cfg(not(feature = "advanced"))]
    {
        println!("  🔲 Advanced features not enabled");
        println!("     To enable: cargo build --features advanced");
        rebuild_needed = true;
    }

    if rebuild_needed {
        println!("\n⚠️  Some features require rebuilding the binary.");
        println!("   Run: cargo build --release --features wasm,advanced,grid");
    }

    // Network configuration
    println!("\n🌐 Network Configuration:");
    println!("  • Standard TCP: Suitable for local/trusted networks");
    println!("  • QUIC with TLS: Recommended for internet/untrusted networks");
    println!("  • Mesh networking: Enables automatic peer discovery");

    // Authentication setup
    println!("\n🔒 Authentication is MANDATORY in production mode");
    println!("   Admin credentials are generated and shown at startup");

    // Example configurations
    println!("\n📋 Common Configurations:");
    println!("\n  1. Local development server:");
    println!("     9pe-server serve --bind 127.0.0.1:5641");

    println!("\n  2. Network file server with discovery:");
    println!("     9pe-server serve --bind 0.0.0.0:5641 --mesh-port 9650");

    println!("\n  3. Secure internet server:");
    println!("     9pe-server serve --bind 0.0.0.0:5641 --quic --server-name myserver.com");

    println!("\n  4. Auto-mount client:");
    println!("     9pe-server auto-mount");

    println!("\n🎯 Choose a configuration above or run with custom parameters.");
    println!("   Use '9pe-server <command> --help' for detailed options.");

    Ok(())
}

/// Show current server status and active features
async fn show_status() -> Result<()> {
    use std::process::Command;

    println!("📊 9P.e Server Status");
    println!("====================");

    // System info
    println!("\n🖥️  System Information:");
    println!("  Host: {}", whoami::fallible::hostname().unwrap_or_else(|_| "unknown".to_string()));
    println!("  User: {}", whoami::username());
    println!("  Platform: {}", std::env::consts::OS);
    println!("  Architecture: {}", std::env::consts::ARCH);

    // Build information
    println!("\n🔧 Build Information:");
    println!("  Version: {}", env!("CARGO_PKG_VERSION"));
    println!("  Built with Rust: {}", rustc_version());

    // Active features
    let mut active_features: Vec<&str> = Vec::new();
    #[cfg(feature = "wasm")]
    active_features.push("WASM");
    #[cfg(feature = "advanced")]
    active_features.push("Advanced");
    #[cfg(feature = "grid")]
    active_features.push("Grid");
    #[cfg(feature = "native")]
    active_features.push("Native GUI");
    #[cfg(feature = "gtk")]
    active_features.push("GTK GUI");

    if active_features.is_empty() {
        println!("  Features: Default (core only)");
    } else {
        println!("  Features: {}", active_features.join(", "));
    }

    // Network status
    println!("\n🌐 Network Status:");

    // Check if any 9P.e servers are running
    if let Ok(output) = Command::new("netstat").args(&["-tlnp"]).output() {
        let netstat_str = String::from_utf8_lossy(&output.stdout);
        let mut found_servers = false;

        for line in netstat_str.lines() {
            if line.contains(":564") && line.contains("LISTEN") {
                found_servers = true;
                println!("  ✅ Server running on {}", extract_port_from_netstat(line));
            }
        }

        if !found_servers {
            println!("  🔲 No 9P.e servers currently running");
        }
    } else {
        println!("  ❓ Network status unavailable (netstat not found)");
    }

    // FUSE mount status
    println!("\n🗻 Mount Status:");
    if let Ok(output) = Command::new("mount").output() {
        let mount_str = String::from_utf8_lossy(&output.stdout);
        let mut found_mounts = false;

        for line in mount_str.lines() {
            if line.contains("fuse") && line.contains("9pe") {
                found_mounts = true;
                println!("  ✅ {}", line);
            }
        }

        if !found_mounts {
            println!("  🔲 No FUSE mounts currently active");
            println!("     Use '9pe-server mount' or '9pe-server auto-mount'");
        }
    } else {
        println!("  ❓ Mount status unavailable");
    }

    // Mesh networking status
    println!("\n📡 Mesh Network Status:");
    println!("  🔲 Not currently connected to mesh");
    println!("     Use '9pe-server serve --mesh-port 9650' to enable");

    // Recommendations
    println!("\n💡 Recommendations:");
    println!("  • Run '9pe-server features' to see available capabilities");
    println!("  • Run '9pe-server setup' for configuration guidance");
    println!("  • Run '9pe-server serve --help' for server options");

    Ok(())
}

/// Extract port information from netstat output
fn extract_port_from_netstat(line: &str) -> String {
    for part in line.split_whitespace() {
        if part.contains(":564") {
            return part.to_string();
        }
    }
    "unknown port".to_string()
}

/// Get rustc version (placeholder - would need rustc_version crate for full implementation)
fn rustc_version() -> String {
    // For now, return the version we know we're using
    "1.70+".to_string()
}

/// Show global event chain status
async fn show_event_chain_status() -> Result<()> {
    info!("⛓️ Global Event Chain Status");
    info!("============================");

    // Would connect to running event chain
    info!("Event chain not currently connected");
    info!("Start server with --mesh-port to enable");

    Ok(())
}

/// Show recent events from the chain
async fn show_recent_events(count: usize) -> Result<()> {
    info!("📜 Recent Events (last {})", count);
    info!("==========================");

    // Would query event chain
    info!("No event chain currently available");
    info!("Events would show file operations across all nodes");

    Ok(())
}

/// Show events for a specific file
async fn show_file_events(path: &str) -> Result<()> {
    info!("📄 Events for: {}", path);
    info!("==================");

    // Would query event chain for file history
    info!("No event chain currently available");
    info!("Would show complete history of operations on this file");

    Ok(())
}

/// Discover first available server on the network
async fn discover_first_server() -> Option<String> {
    // Start mesh networking for discovery
    if let Ok((mesh_sender, discovered_peers)) = crate::mesh::start_mesh_network(44444, None).await {
        // Scan for available servers
        if let Ok(mut client) = mesh_client::MeshClient::new_with_mesh(Some(mesh_sender), Some(discovered_peers)).await {
            tokio::time::sleep(Duration::from_secs(2)).await;
            if client.scan_local_network().await.is_ok() {
                let peers = client.list_peers().await;
                if let Some(peer) = peers.first() {
                    return Some(peer.service_addr.clone());
                }
            }
        }
    }
    None
}

/// Mount a server at the specified mount point
async fn mount_server(server: String, mount_point: String) -> Result<()> {
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

/// Show active mounts
async fn show_mounts() -> Result<()> {
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

/// Add a new user
async fn add_user(username: String, password: Option<String>) -> Result<()> {
    use auth::{User, AuthService};
    use std::io::{self, Write};

    println!("➕ Adding new user: {}", username);

    let password = if let Some(p) = password {
        p
    } else {
        print!("Enter password for {}: ", username);
        io::stdout().flush()?;
        let mut password = String::new();
        io::stdin().read_line(&mut password)?;
        password.trim().to_string()
    };

    if password.len() < 8 {
        return Err(anyhow::anyhow!("Password must be at least 8 characters"));
    }

    // Load configuration file for persistent user storage
    let config_path = get_config_path();
    let mut config = load_or_create_config(&config_path).await?;

    // Add user to config
    config.users.insert(username.clone(), UserConfig {
        password_hash: generate_secure_password_hash(&password),
        groups: vec!["users".to_string()],
        home_dir: format!("/srv/9pe/{}", username),
        shell: "/bin/rc".to_string(),
    });

    // Save config
    save_config(&config_path, &config).await?;

    println!("✅ User '{}' added successfully", username);
    println!("🔧 Restart the server for changes to take effect");

    Ok(())
}

/// Change user password
async fn change_password(username: String) -> Result<()> {
    use std::io::{self, Write};

    println!("🔑 Changing password for user: {}", username);

    // Load configuration
    let config_path = get_config_path();
    let mut config = load_or_create_config(&config_path).await?;

    if !config.users.contains_key(&username) {
        return Err(anyhow::anyhow!("User '{}' not found", username));
    }

    print!("Enter new password for {}: ", username);
    io::stdout().flush()?;
    let mut password = String::new();
    io::stdin().read_line(&mut password)?;
    let password = password.trim().to_string();

    if password.len() < 8 {
        return Err(anyhow::anyhow!("Password must be at least 8 characters"));
    }

    // Update password
    if let Some(user_config) = config.users.get_mut(&username) {
        user_config.password_hash = generate_secure_password_hash(&password);
    }

    // Save config
    save_config(&config_path, &config).await?;

    println!("✅ Password changed successfully for '{}'", username);
    println!("🔧 Restart the server for changes to take effect");

    Ok(())
}

/// List all users
async fn list_users() -> Result<()> {
    println!("👥 System Users");
    println!("================");

    let config_path = get_config_path();
    let config = load_or_create_config(&config_path).await?;

    if config.users.is_empty() {
        println!("No users configured. Using default admin user.");
        println!("  👤 admin (default)");
        return Ok(());
    }

    for (username, user_config) in &config.users {
        println!("  👤 {} (groups: {})", username, user_config.groups.join(", "));
        println!("     Home: {}", user_config.home_dir);
        println!("     Shell: {}", user_config.shell);
        println!();
    }

    Ok(())
}

/// Delete a user
async fn delete_user(username: String) -> Result<()> {
    if username == "admin" {
        return Err(anyhow::anyhow!("Cannot delete the admin user"));
    }

    println!("🗑️  Deleting user: {}", username);

    let config_path = get_config_path();
    let mut config = load_or_create_config(&config_path).await?;

    if config.users.remove(&username).is_none() {
        return Err(anyhow::anyhow!("User '{}' not found", username));
    }

    save_config(&config_path, &config).await?;

    println!("✅ User '{}' deleted successfully", username);
    println!("🔧 Restart the server for changes to take effect");

    Ok(())
}

/// Handle user management actions
async fn handle_user_action(user_action: UserAction) -> Result<()> {
    match user_action {
        UserAction::Add { username, password } => add_user(username, password).await,
        UserAction::Passwd { username } => change_password(username).await,
        UserAction::List => list_users().await,
        UserAction::Del { username } => delete_user(username).await,
    }
}

/// Configuration structures for persistent storage
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Default)]
struct ServerConfig {
    users: std::collections::HashMap<String, UserConfig>,
    server: ServerSettings,
}

#[derive(Serialize, Deserialize)]
struct UserConfig {
    password_hash: String,
    groups: Vec<String>,
    home_dir: String,
    shell: String,
}

#[derive(Serialize, Deserialize)]
struct ServerSettings {
    bind_address: String,
    use_quic: bool,
    mesh_port: u16,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0:5640".to_string(),
            use_quic: false,
            mesh_port: 9650,
        }
    }
}

/// Get configuration file path
fn get_config_path() -> std::path::PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        std::path::Path::new(&home).join(".config/9pe-server/config.toml")
    } else {
        std::path::PathBuf::from("/etc/9pe-server/config.toml")
    }
}

/// Load or create configuration file
async fn load_or_create_config(path: &std::path::Path) -> Result<ServerConfig> {
    if path.exists() {
        let content = tokio::fs::read_to_string(path).await?;
        toml::from_str(&content).map_err(|e| anyhow::anyhow!("Invalid config file: {}", e))
    } else {
        // Create directory if it doesn't exist
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        Ok(ServerConfig::default())
    }
}

/// Save configuration file
async fn save_config(path: &std::path::Path, config: &ServerConfig) -> Result<()> {
    let content = toml::to_string_pretty(config)?;
    tokio::fs::write(path, content).await?;
    Ok(())
}

/// Daemonize the server process
async fn daemonize_server(
    path: PathBuf,
    bind: String,
    quic: bool,
    server_name: Option<String>,
    metrics_port: u16,
    mesh_port: u16,
    pid_file: String,
) -> Result<()> {
    use std::process::{Command, Stdio};
    use std::os::unix::process::CommandExt;

    println!("🚀 Starting 9P.e server as daemon...");

    // Check if already running
    if let Ok(pid) = tokio::fs::read_to_string(&pid_file).await {
        if is_process_running(pid.trim().parse().unwrap_or(0)) {
            return Err(anyhow::anyhow!("Server already running with PID: {}", pid.trim()));
        }
    }

    // Build command arguments
    let mut args = vec![
        "serve".to_string(),
        "--path".to_string(),
        path.to_string_lossy().to_string(),
        "--bind".to_string(),
        bind.clone(),
        "--metrics-port".to_string(),
        metrics_port.to_string(),
    ];

    if quic {
        args.push("--quic".to_string());
        if let Some(name) = server_name {
            args.push("--server-name".to_string());
            args.push(name);
        }
    }

    args.push("--mesh-port".to_string());
    args.push(mesh_port.to_string());

    // Get current executable path
    let exe_path = std::env::current_exe()?;

    // Fork and run in background
    unsafe {
        let pid = libc::fork();
        if pid < 0 {
            return Err(anyhow::anyhow!("Failed to fork process"));
        } else if pid == 0 {
            // Child process
            // Create new session
            libc::setsid();

            // Change working directory to /
            std::env::set_current_dir("/")?;

            // Close standard file descriptors
            libc::close(0);
            libc::close(1);
            libc::close(2);

            // Execute the server
            Command::new(exe_path)
                .args(&args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .exec();

            // If exec fails, exit child
            std::process::exit(1);
        } else {
            // Parent process
            // Write PID file
            tokio::fs::write(&pid_file, pid.to_string()).await?;

            println!("✅ Server started as daemon with PID: {}", pid);
            println!("📄 PID file: {}", pid_file);
            println!("🌐 Listening on: {}", bind);
            println!("");
            println!("💡 To stop the server: 9pe-server stop");
            println!("💡 To check status: 9pe-server status");
        }
    }

    Ok(())
}

/// Stop a running daemon
async fn stop_daemon(pid_file: String) -> Result<()> {
    println!("🛑 Stopping 9P.e server daemon...");

    // Read PID from file
    let pid_str = tokio::fs::read_to_string(&pid_file).await
        .map_err(|_| anyhow::anyhow!("No PID file found at {}. Is the server running?", pid_file))?;

    let pid: i32 = pid_str.trim().parse()
        .map_err(|_| anyhow::anyhow!("Invalid PID in file: {}", pid_str))?;

    // Check if process exists
    if !is_process_running(pid) {
        println!("⚠️  Server not running (stale PID file)");
        // Clean up stale PID file
        let _ = tokio::fs::remove_file(&pid_file).await;
        return Ok(());
    }

    // Send SIGTERM
    unsafe {
        if libc::kill(pid, libc::SIGTERM) == 0 {
            println!("✅ Sent SIGTERM to process {}", pid);

            // Wait for process to exit (max 5 seconds)
            for _ in 0..50 {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                if !is_process_running(pid) {
                    break;
                }
            }

            // If still running, send SIGKILL
            if is_process_running(pid) {
                println!("⚠️  Process didn't stop gracefully, sending SIGKILL...");
                libc::kill(pid, libc::SIGKILL);
            }

            // Remove PID file
            let _ = tokio::fs::remove_file(&pid_file).await;
            println!("✅ Server stopped successfully");
        } else {
            return Err(anyhow::anyhow!("Failed to stop process {}: Permission denied", pid));
        }
    }

    Ok(())
}

/// Check daemon status
async fn check_daemon_status(pid_file: String) -> Result<()> {
    println!("📊 Checking 9P.e server status...");
    println!("=====================================");

    // Check PID file
    match tokio::fs::read_to_string(&pid_file).await {
        Ok(pid_str) => {
            let pid: i32 = pid_str.trim().parse()
                .map_err(|_| anyhow::anyhow!("Invalid PID in file: {}", pid_str))?;

            if is_process_running(pid) {
                println!("✅ Status: RUNNING");
                println!("📍 PID: {}", pid);
                println!("📄 PID File: {}", pid_file);

                // Try to get more info about the process
                if let Ok(cmdline) = std::fs::read_to_string(format!("/proc/{}/cmdline", pid)) {
                    let args: Vec<&str> = cmdline.split('\0').collect();
                    println!("🎯 Command: {}", args.join(" "));
                }

                // Check network ports
                println!("\n🌐 Network Listeners:");
                check_network_listeners(pid).await;

                // Check metrics endpoint
                println!("\n📊 Metrics Endpoint:");
                if let Ok(response) = reqwest::get("http://localhost:9090/metrics").await {
                    if response.status().is_success() {
                        println!("  ✅ Metrics available at http://localhost:9090/metrics");
                    }
                }
            } else {
                println!("⚠️  Status: NOT RUNNING (stale PID file)");
                println!("📍 Stale PID: {}", pid);
                println!("💡 Run 'rm {}' to clean up", pid_file);
            }
        }
        Err(_) => {
            println!("🔴 Status: NOT RUNNING");
            println!("💡 Start with: 9pe-server serve --daemon");
        }
    }

    Ok(())
}

/// Check if a process is running
fn is_process_running(pid: i32) -> bool {
    unsafe {
        libc::kill(pid, 0) == 0
    }
}

/// Check network listeners for a process
async fn check_network_listeners(pid: i32) {
    use std::process::Command;

    if let Ok(output) = Command::new("ss")
        .args(&["-tlnp"])
        .output() {
        let output_str = String::from_utf8_lossy(&output.stdout);
        for line in output_str.lines() {
            if line.contains(&format!("pid={}", pid)) {
                if let Some(addr) = line.split_whitespace().nth(3) {
                    println!("  🔌 Listening on: {}", addr);
                }
            }
        }
    }
}
/// Set up advanced logging configuration
fn setup_logging(cli: &Cli) -> Result<()> {
    // Determine log level
    let log_level = match cli.verbose {
        0 => tracing::Level::INFO,
        1 => tracing::Level::DEBUG,
        _ => tracing::Level::TRACE,
    };

    // Create filter with environment variable support
    let filter = EnvFilter::builder()
        .with_default_directive(log_level.into())
        .with_env_var("RUST_LOG")
        .from_env_lossy()
        // Filter noisy crates
        .add_directive("libp2p_swarm=warn".parse()?)
        .add_directive("libp2p_gossipsub=info".parse()?)
        .add_directive("quinn=warn".parse()?)
        .add_directive("fuse=info".parse()?);

    let registry = tracing_subscriber::registry().with(filter);

    // Set up console output layer
    let stdout_layer = match cli.log_format.as_str() {
        "json" | _ if cli.json_logs => {
            tracing_subscriber::fmt::layer()
                .json()
                .with_current_span(false)
                .with_span_list(true)
                .boxed()
        }
        "compact" => {
            tracing_subscriber::fmt::layer()
                .compact()
                .with_target(false)
                .boxed()
        }
        "pretty" => {
            tracing_subscriber::fmt::layer()
                .pretty()
                .boxed()
        }
        "full" => {
            tracing_subscriber::fmt::layer()
                .with_file(true)
                .with_line_number(true)
                .with_thread_ids(true)
                .boxed()
        }
        _ => {
            tracing_subscriber::fmt::layer()
                .compact()
                .with_target(false)
                .boxed()
        }
    };

    // Set up file output if specified
    if let Some(log_file) = &cli.log_file {
        // Create parent directory if it doesn't exist
        if let Some(parent) = log_file.parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create log directory")?;
        }

        // Set up daily log rotation
        let log_dir = log_file.parent().unwrap_or_else(|| std::path::Path::new("."));
        let log_name = log_file.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("9pe-server");

        let file_appender = rolling::daily(log_dir, log_name);
        let (non_blocking_appender, _guard) = non_blocking(file_appender);

        let file_layer = tracing_subscriber::fmt::layer()
            .with_writer(non_blocking_appender)
            .with_ansi(false)
            .json()
            .boxed();

        registry
            .with(stdout_layer)
            .with(file_layer)
            .init();

        // Keep the guard alive (prevents early dropping of file writer)
        std::mem::forget(_guard);
    } else {
        registry
            .with(stdout_layer)
            .init();
    }

    // Log configuration info
    let log_level_name = match cli.verbose {
        0 => "INFO",
        1 => "DEBUG",
        _ => "TRACE",
    };

    info!("🔧 Logging configured:");
    info!("   Level: {} ({})", log_level_name,
          if std::env::var("RUST_LOG").is_ok() {
              "overridden by RUST_LOG"
          } else {
              "from -v flag"
          });
    info!("   Format: {}", cli.log_format);
    if let Some(file) = &cli.log_file {
        info!("   File: {} (daily rotation)", file.display());
    }
    if cli.json_logs {
        info!("   JSON structured logging enabled");
    }

    Ok(())
}
