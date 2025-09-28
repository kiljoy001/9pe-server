//! 9P.e Server - Everything is a file, and every file is a function
//!
//! Core philosophy: Files are lazily-evaluated functions that transform input to output

pub mod server;
pub mod metrics;
// pub mod web_ui;  // Removed GUI bloat

// Core abstractions
pub mod synthetic;
pub mod function_files;
pub mod synthetic_creation;
pub mod file_operations;
pub mod modern_draw;

// Native window support - removed
// #[cfg(feature = "native")]
// pub mod native_window;

// #[cfg(feature = "gtk")]
// pub mod gtk_window;

// Pure CSS UI modules - removed
// pub mod pure_css_ui;
// pub mod css_ui_generator;

// Consensus and mesh networking
pub mod mesh;
pub mod mesh_client;
pub mod ghostdag;
pub mod consensus;
pub mod client;
pub mod transient_consensus;  // Privacy-first consensus, NOT a blockchain
pub mod ephemeral_event_log;   // Gossip-based ordering, no persistence
pub mod simple_ordering;       // Simplified consensus without crypto
pub mod namespace_consensus;   // Public network namespace consensus
pub mod namespace_transformers; // /n/ and /srv virtual directories
pub mod global_event_chain;    // Global event ordering with GhostDAG

// Abstract translator framework
pub mod translator_base;       // Factorized translator base for all types
pub mod namespace_translator;  // Built-in namespace management translator
pub mod translator;            // WASM and native translator management

// Advanced features (feature-gated)
#[cfg(feature = "wasm")]
pub mod wasm_translator;

#[cfg(feature = "wasm")]
pub mod settrans;

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