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
        base.join("srv") // ~/srv
    }
}

/// Get /n namespace directory path based on privilege level
pub fn get_n_directory() -> PathBuf {
    let base = get_base_directory();
    if is_system_mode() {
        base.join("n")
    } else {
        base.join("n") // ~/n
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_system_mode_deterministic() {
        // is_system_mode should return consistent result
        let first = is_system_mode();
        let second = is_system_mode();
        assert_eq!(first, second, "is_system_mode should be deterministic");
    }

    #[test]
    fn test_base_directory_exists() {
        let base = get_base_directory();
        // Base directory should be a valid path
        assert!(
            !base.as_os_str().is_empty(),
            "Base directory should not be empty"
        );

        // Should be either root or have a home component
        let base_str = base.to_string_lossy();
        assert!(
            base_str == "/" || base_str.contains("home") || base_str == ".",
            "Base directory should be root, home, or current dir"
        );
    }

    #[test]
    fn test_srv_directory_structure() {
        let srv = get_srv_directory();
        // Should end with "srv"
        assert_eq!(
            srv.file_name().unwrap(),
            "srv",
            "srv directory should be named 'srv'"
        );
    }

    #[test]
    fn test_n_directory_structure() {
        let n_dir = get_n_directory();
        // Should end with "n"
        assert_eq!(
            n_dir.file_name().unwrap(),
            "n",
            "n directory should be named 'n'"
        );
    }

    #[test]
    fn test_settrans_directory_structure() {
        let settrans = get_settrans_directory();
        // Should end with "settrans"
        assert_eq!(
            settrans.file_name().unwrap(),
            "settrans",
            "settrans directory should be named 'settrans'"
        );

        // Parent should be srv directory
        let parent = settrans.parent().unwrap();
        assert_eq!(
            parent.file_name().unwrap(),
            "srv",
            "settrans parent should be srv"
        );
    }

    #[test]
    fn test_directory_consistency() {
        // All directory functions should use same base
        let base = get_base_directory();
        let srv = get_srv_directory();
        let n_dir = get_n_directory();

        assert!(srv.starts_with(&base), "srv should be under base directory");
        assert!(n_dir.starts_with(&base), "n should be under base directory");
    }

    #[test]
    fn test_log_execution_mode_no_panic() {
        // Should not panic when called
        log_execution_mode();
    }

    /// Fuzz test: Path operations should handle edge cases
    #[test]
    fn fuzz_path_operations() {
        use proptest::prelude::*;

        proptest!(|(component in "[a-zA-Z0-9_-]{1,20}")| {
            // Should safely handle arbitrary path components
            let base = get_base_directory();
            let _ = base.join(&component);
        });
    }
}
