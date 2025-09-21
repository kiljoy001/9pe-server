//! 9P.e Server Implementation
//!
//! A revolutionary distributed OS that runs in userland, making computation and data
//! unified as files, with WASM-based programmability and distributed grid computing

pub mod server;
pub mod metrics;
pub mod web_ui;
pub mod auth;
pub mod synthetic;
pub mod synthetic_advanced;
pub mod translators;
pub mod translator_composition;
pub mod translator_synthetic;
pub mod namespaces;
pub mod integrated_server;

// Advanced features (requires additional deps)
#[cfg(feature = "wasm")]
pub mod wasm_composition;
#[cfg(feature = "wasm")]
pub mod wasm_api;
#[cfg(feature = "wasm")]
pub mod wasm_fs;
#[cfg(feature = "wasm")]
pub mod wasm_synthetic;

#[cfg(feature = "grid")]
pub mod grid_computing;

#[cfg(feature = "tauri")]
pub mod tauri_dashboard;

// Re-export necessary types
pub use anyhow::{Result, Error};
pub use server::FileSystemServer;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic() {
        // Basic test to ensure library compiles
        assert_eq!(2 + 2, 4);
    }
}