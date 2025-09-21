//! 9P.e Server - Clean implementation using the verified 9PE core protocol
//!
//! This server bridges the formally-verified 9PE protocol to actual filesystem operations

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::net::SocketAddr;
use std::sync::Arc;
use anyhow::{Result, Context};
use tracing::{info, warn, error};
use tracing_subscriber;

mod server;
mod metrics;
mod web_ui;
// mod tauri_app;  // Commented out until system deps available
use server::FileSystemServer;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    /// Verbose output
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Serve a directory over 9P.e protocol
    Serve {
        /// Directory to serve
        #[arg(short, long, default_value = ".")]
        path: PathBuf,

        /// Address to bind to
        #[arg(short, long, default_value = "0.0.0.0:564")]
        bind: String,

        /// Use QUIC transport (default is TCP for compatibility)
        #[arg(short, long)]
        quic: bool,

        /// Server name for QUIC TLS (required with --quic)
        #[arg(short = 'n', long)]
        server_name: Option<String>,

        /// Enable Grafana metrics on port
        #[arg(short = 'm', long, default_value = "9090")]
        metrics_port: u16,

        /// Enable Web UI on port
        #[arg(short = 'w', long)]
        web_ui_port: Option<u16>,
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
        Commands::Serve { path, bind, quic, server_name, metrics_port, web_ui_port } => {
            serve_directory(path, bind, quic, server_name, metrics_port, web_ui_port).await
        }
    }
}

async fn serve_directory(
    path: PathBuf,
    bind: String,
    use_quic: bool,
    server_name: Option<String>,
    metrics_port: u16,
    web_ui_port: Option<u16>,
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

    // Start web UI if requested
    if let Some(port) = web_ui_port {
        let web_path = path.clone();
        let web_config = web_ui::WebConfig {
            root_path: web_path,
            bind_addr: format!("0.0.0.0:{}", port).parse()?,
        };
        tokio::spawn(async move {
            if let Err(e) = web_ui::start_web_ui(web_config).await {
                error!("Web UI failed: {}", e);
            }
        });
        info!("🖥️  Web UI: http://0.0.0.0:{}", port);
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