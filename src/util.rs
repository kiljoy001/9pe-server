use std::path::PathBuf;
use tracing::info;

pub fn log_execution_mode() {
    info!("Execution mode: Standard Server");
}

pub fn get_settrans_directory() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".9pe/settrans")
}

pub fn get_n_directory() -> PathBuf {
    // Allow override via environment variable
    if let Ok(path) = std::env::var("NINEP_NAMESPACE_ROOT") {
        return PathBuf::from(path);
    }

    // Default to user-local directory to avoid root requirement
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".9pe/n")
}
