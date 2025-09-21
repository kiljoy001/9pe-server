//! Tauri Dashboard Backend
//!
//! Provides the Rust backend for the Grafana-style dashboard in Tauri

use std::sync::Arc;
use std::path::PathBuf;
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};
use tauri::State;

use crate::server::FileSystemServer;
use crate::metrics;

/// Dashboard metrics response
#[derive(Serialize, Deserialize)]
pub struct MetricsResponse {
    pub connections: u64,
    pub messages_per_sec: f64,
    pub throughput: f64,
    pub open_fids: u64,
    pub error_rate: f64,
    pub cpu_usage: f64,
    pub memory_mb: u64,
}

/// File information for browser
#[derive(Serialize, Deserialize)]
pub struct FileInfo {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: u64,
}

/// Server configuration
#[derive(Serialize, Deserialize)]
pub struct ServerConfig {
    pub protocol: String,
    pub port: u16,
    pub root_path: String,
    pub max_msg_size: u32,
    pub auth_enabled: bool,
}

/// Application state
pub struct DashboardState {
    pub server: Arc<RwLock<Option<Arc<FileSystemServer>>>>,
    pub config: Arc<RwLock<ServerConfig>>,
    pub metrics_history: Arc<RwLock<Vec<MetricsResponse>>>,
}

// Tauri commands that can be invoked from the frontend

#[tauri::command]
pub async fn get_metrics(state: State<'_, DashboardState>) -> Result<MetricsResponse, String> {
    // Fetch real metrics from Prometheus endpoint
    let response = reqwest::get("http://localhost:9090/metrics")
        .await
        .map_err(|e| e.to_string())?;

    let text = response.text().await.map_err(|e| e.to_string())?;

    // Parse Prometheus metrics
    let mut metrics = MetricsResponse {
        connections: 0,
        messages_per_sec: 0.0,
        throughput: 0.0,
        open_fids: 0,
        error_rate: 0.0,
        cpu_usage: 0.0,
        memory_mb: 0,
    };

    for line in text.lines() {
        if line.starts_with("ninepee_connections_active") {
            if let Some(value) = parse_metric_value(line) {
                metrics.connections = value as u64;
            }
        } else if line.starts_with("ninepee_messages_total") {
            // Calculate rate from counter
            if let Some(value) = parse_metric_value(line) {
                metrics.messages_per_sec = value / 60.0; // Simple approximation
            }
        } else if line.starts_with("ninepee_throughput_mbps") {
            if let Some(value) = parse_metric_value(line) {
                metrics.throughput = value;
            }
        } else if line.starts_with("ninepee_fids_open") {
            if let Some(value) = parse_metric_value(line) {
                metrics.open_fids = value as u64;
            }
        }
    }

    // Store in history
    let mut history = state.metrics_history.write().await;
    history.push(metrics.clone());
    if history.len() > 1440 { // Keep 24 hours at 1 minute intervals
        history.remove(0);
    }

    Ok(metrics)
}

#[tauri::command]
pub async fn list_directory(path: String) -> Result<Vec<FileInfo>, String> {
    let mut entries = Vec::new();
    let mut dir = tokio::fs::read_dir(&path).await
        .map_err(|e| e.to_string())?;

    while let Some(entry) = dir.next_entry().await.map_err(|e| e.to_string())? {
        let metadata = entry.metadata().await.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().to_string();

        entries.push(FileInfo {
            name: name.clone(),
            path: PathBuf::from(&path).join(&name).to_string_lossy().to_string(),
            is_dir: metadata.is_dir(),
            size: metadata.len(),
            modified: metadata.modified()
                .map(|t| t.duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs())
                .unwrap_or(0),
        });
    }

    // Sort directories first
    entries.sort_by(|a, b| {
        match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        }
    });

    Ok(entries)
}

#[tauri::command]
pub async fn update_config(
    config: ServerConfig,
    state: State<'_, DashboardState>
) -> Result<(), String> {
    let mut current = state.config.write().await;
    *current = config;
    Ok(())
}

#[tauri::command]
pub async fn restart_server(state: State<'_, DashboardState>) -> Result<(), String> {
    // Stop current server
    *state.server.write().await = None;

    // Start new server with updated config
    let config = state.config.read().await;
    let path = PathBuf::from(&config.root_path);

    match FileSystemServer::new(path) {
        Ok(server) => {
            *state.server.write().await = Some(Arc::new(server));

            // Start server on configured port
            let addr = format!("0.0.0.0:{}", config.port);

            if config.protocol == "quic" {
                // TODO: Start QUIC server
                return Err("QUIC not yet implemented".to_string());
            } else {
                // Start TCP server in background
                tokio::spawn(async move {
                    // Server startup logic here
                    tracing::info!("Server restarted on {}", addr);
                });
            }

            Ok(())
        }
        Err(e) => Err(format!("Failed to create server: {}", e))
    }
}

#[tauri::command]
pub async fn get_server_logs(
    level: Option<String>,
    count: Option<usize>
) -> Result<Vec<String>, String> {
    // In production, this would fetch from actual log storage
    // For now, return sample logs
    Ok(vec![
        format!("[INFO] Server started on TCP port 5641"),
        format!("[INFO] Metrics endpoint available at :9090"),
        format!("[INFO] Accepted connection from 127.0.0.1"),
    ])
}

// Helper function to parse Prometheus metric values
fn parse_metric_value(line: &str) -> Option<f64> {
    line.split_whitespace()
        .last()
        .and_then(|v| v.parse().ok())
}

/// Initialize Tauri application with dashboard
pub fn init_dashboard() -> tauri::Builder<tauri::Wry> {
    let state = DashboardState {
        server: Arc::new(RwLock::new(None)),
        config: Arc::new(RwLock::new(ServerConfig {
            protocol: "tcp".to_string(),
            port: 5641,
            root_path: "/tmp".to_string(),
            max_msg_size: 8192,
            auth_enabled: false,
        })),
        metrics_history: Arc::new(RwLock::new(Vec::new())),
    };

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            get_metrics,
            list_directory,
            update_config,
            restart_server,
            get_server_logs,
        ])
}