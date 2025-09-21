//! Tauri Application for 9P.e Server
//!
//! Provides a native desktop application with web-based UI

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};
use tauri::{Manager, State};
use tokio::net::TcpListener;
use tracing::{info, error};

use crate::server::FileSystemServer;
use crate::metrics;

/// Application state shared across Tauri
pub struct AppState {
    pub server: Arc<RwLock<Option<Arc<FileSystemServer>>>>,
    pub current_path: Arc<RwLock<PathBuf>>,
    pub server_handle: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
    pub server_running: Arc<RwLock<bool>>,
    pub server_addr: Arc<RwLock<Option<String>>>,
}

/// File information for the UI
#[derive(Serialize, Deserialize)]
pub struct FileEntry {
    name: String,
    path: String,
    is_dir: bool,
    size: u64,
    modified: u64,
}

/// Directory listing response
#[derive(Serialize, Deserialize)]
pub struct DirListing {
    path: String,
    entries: Vec<FileEntry>,
}

/// Server status
#[derive(Serialize, Deserialize)]
pub struct ServerStatus {
    running: bool,
    protocol: String,
    address: String,
    connections: u32,
}

/// Initialize the Tauri application
pub fn init_app() -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            list_directory,
            read_file,
            write_file,
            create_directory,
            delete_item,
            get_server_status,
            start_server,
            stop_server,
        ])
        .setup(|app| {
            // Initialize with default path
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());

            app.manage(AppState {
                server: Arc::new(RwLock::new(None)),
                current_path: Arc::new(RwLock::new(PathBuf::from(home))),
                server_handle: Arc::new(RwLock::new(None)),
                server_running: Arc::new(RwLock::new(false)),
                server_addr: Arc::new(RwLock::new(None)),
            });

            // Initialize metrics
            metrics::init_metrics();

            Ok(())
        })
}

// Tauri Commands

#[tauri::command]
async fn list_directory(
    path: String,
    state: State<'_, AppState>,
) -> Result<DirListing, String> {
    let full_path = PathBuf::from(&path);

    let mut entries = Vec::new();
    let mut dir = tokio::fs::read_dir(&full_path).await
        .map_err(|e| e.to_string())?;

    while let Some(entry) = dir.next_entry().await.map_err(|e| e.to_string())? {
        let metadata = entry.metadata().await.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().to_string();

        entries.push(FileEntry {
            name: name.clone(),
            path: full_path.join(&name).to_string_lossy().to_string(),
            is_dir: metadata.is_dir(),
            size: metadata.len(),
            modified: metadata.modified()
                .map(|t| t.duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs())
                .unwrap_or(0),
        });
    }

    // Sort directories first, then by name
    entries.sort_by(|a, b| {
        match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        }
    });

    Ok(DirListing {
        path,
        entries,
    })
}

#[tauri::command]
async fn read_file(path: String) -> Result<Vec<u8>, String> {
    tokio::fs::read(&path).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn write_file(path: String, contents: Vec<u8>) -> Result<(), String> {
    tokio::fs::write(&path, contents).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_directory(path: String) -> Result<(), String> {
    tokio::fs::create_dir(&path).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_item(path: String) -> Result<(), String> {
    let metadata = tokio::fs::metadata(&path).await
        .map_err(|e| e.to_string())?;

    if metadata.is_dir() {
        tokio::fs::remove_dir_all(&path).await
    } else {
        tokio::fs::remove_file(&path).await
    }
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_server_status(state: State<'_, AppState>) -> Result<ServerStatus, String> {
    let running = *state.server_running.read().await;
    let address = state.server_addr.read().await.clone().unwrap_or_else(|| "Not running".to_string());

    Ok(ServerStatus {
        running,
        protocol: "9P.e".to_string(),
        address,
        connections: metrics::get_active_connections() as u32,
    })
}

#[tauri::command]
async fn start_server(
    address: String,
    use_quic: bool,
    state: State<'_, AppState>,
) -> Result<String, String> {
    // Check if already running
    if *state.server_running.read().await {
        return Err("Server already running".to_string());
    }

    // Get the current path
    let path = state.current_path.read().await.clone();

    // Create filesystem server
    let fs_server = Arc::new(FileSystemServer::new(path.clone())
        .map_err(|e| format!("Failed to create server: {}", e))?);

    // Parse address
    let addr = address.parse::<std::net::SocketAddr>()
        .map_err(|e| format!("Invalid address: {}", e))?;

    if use_quic {
        // TODO: Implement QUIC server
        return Err("QUIC not yet implemented in Tauri mode".to_string());
    }

    // Start TCP server
    let server_clone = Arc::clone(&fs_server);
    let handle = tokio::spawn(async move {
        let listener = match TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                error!("Failed to bind: {}", e);
                return;
            }
        };

        info!("TCP server listening on {}", addr);

        loop {
            match listener.accept().await {
                Ok((socket, peer_addr)) => {
                    info!("New connection from {}", peer_addr);
                    metrics::record_connection("tcp", true);

                    let server = Arc::clone(&server_clone);
                    tokio::spawn(async move {
                        if let Err(e) = handle_tcp_connection(socket, server).await {
                            error!("Connection error: {}", e);
                        }
                        metrics::record_connection("tcp", false);
                    });
                }
                Err(e) => {
                    error!("Accept error: {}", e);
                    break;
                }
            }
        }
    });

    // Update state
    *state.server.write().await = Some(fs_server);
    *state.server_handle.write().await = Some(handle);
    *state.server_running.write().await = true;
    *state.server_addr.write().await = Some(address.clone());

    Ok(format!("Server started on {} with TCP", address))
}

async fn handle_tcp_connection(
    mut socket: tokio::net::TcpStream,
    fs_server: Arc<FileSystemServer>,
) -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use plan9e::protocol::NinePeeMessage;

    loop {
        // Read message length (4 bytes)
        let mut len_buf = [0u8; 4];
        if socket.read_exact(&mut len_buf).await.is_err() {
            break;
        }

        let msg_len = u32::from_le_bytes(len_buf) as usize;
        if msg_len < 4 || msg_len > 16 * 1024 * 1024 {
            break;
        }

        // Read message body
        let mut msg_buf = vec![0u8; msg_len - 4];
        if socket.read_exact(&mut msg_buf).await.is_err() {
            break;
        }

        // Process message
        let request = NinePeeMessage::deserialize(msg_buf)?;
        metrics::record_message("received", &format!("{:?}", request));

        let response = fs_server.process_message(request).await?;
        metrics::record_message("sent", &format!("{:?}", response));

        let response_data = response.serialize()?;

        // Send response
        let response_len = (response_data.len() + 4) as u32;
        socket.write_all(&response_len.to_le_bytes()).await?;
        socket.write_all(&response_data).await?;
    }

    Ok(())
}

#[tauri::command]
async fn stop_server(state: State<'_, AppState>) -> Result<(), String> {
    if !*state.server_running.read().await {
        return Err("Server not running".to_string());
    }

    // Cancel the server task
    if let Some(handle) = state.server_handle.write().await.take() {
        handle.abort();
    }

    // Clear state
    *state.server.write().await = None;
    *state.server_running.write().await = false;
    *state.server_addr.write().await = None;

    Ok(())
}