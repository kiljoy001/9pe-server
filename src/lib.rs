//! 9P.e Server - Minimal Working Version
//!
//! This version includes only the components that compile without additional work

pub mod server;
pub mod metrics;
pub mod web_ui;

// Components that compile with minor fixes
pub mod synthetic;

// Re-export necessary types
pub use anyhow::{Result, Error};
pub use server::FileSystemServer;

// Feature-gated modules that need more work
#[cfg(feature = "advanced")]
pub mod auth;

#[cfg(feature = "advanced")]
pub mod synthetic_advanced;

#[cfg(feature = "advanced")]
pub mod translators;

#[cfg(feature = "advanced")]
pub mod translator_composition;

#[cfg(feature = "advanced")]
pub mod translator_synthetic;

#[cfg(feature = "advanced")]
pub mod namespaces;

#[cfg(feature = "advanced")]
pub mod integrated_server;

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