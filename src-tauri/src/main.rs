// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use tokio::sync::RwLock;

// Import the dashboard backend from main crate
use plan9e_server::tauri_dashboard::*;

#[tokio::main]
async fn main() {
    let state = DashboardState {
        server: Arc::new(RwLock::new(None)),
        config: Arc::new(RwLock::new(ServerConfig {
            protocol: "tcp".to_string(),
            port: 5640,
            root_path: "/tmp".to_string(),
            max_msg_size: 65536,
            auth_enabled: false,
        })),
        metrics_history: Arc::new(RwLock::new(Vec::new())),
    };

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            get_metrics,
            get_server_config,
            start_server,
            stop_server,
            list_files,
            get_file_content
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}