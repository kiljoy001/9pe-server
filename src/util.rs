// Utility functions for 9P.e server

use std::path::PathBuf;
use tracing::info;

/// Detect if running as root/system or user-level
pub fn is_system_mode() -> bool {
    // Check effective UID - 0 = root
    unsafe { libc::geteuid() == 0 }
}

/// Get appropriate base directory for server data based on privilege level
pub fn get_base_directory() -> PathBuf {
    if is_system_mode() {
        // System mode: use root directories
        PathBuf::from("/")
    } else {
        // User mode: use home directory
        dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
    }
}

/// Get srv directory path based on privilege level
pub fn get_srv_directory() -> PathBuf {
    let base = get_base_directory();
    if is_system_mode() {
        base.join("srv")
    } else {
        base.join("srv")  // ~/srv
    }
}

/// Get /n namespace directory path based on privilege level
pub fn get_n_directory() -> PathBuf {
    let base = get_base_directory();
    if is_system_mode() {
        base.join("n")
    } else {
        base.join("n")  // ~/n
    }
}

/// Get settrans directory path based on privilege level
pub fn get_settrans_directory() -> PathBuf {
    get_srv_directory().join("settrans")
}

/// Log the current execution mode
pub fn log_execution_mode() {
    if is_system_mode() {
        info!("Running in SYSTEM mode (as root) - using /srv and /n");
    } else {
        info!("Running in USER mode - using ~/srv and ~/n");
    }
}
