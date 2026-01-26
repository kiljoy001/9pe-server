//! 9P.e Server - Clean Architecture Implementation
//!
//! This library provides a modular, well-architected 9P.e server implementation
//! with proper separation of concerns, dependency injection, and modern defaults.
//!
//! # Architecture
//!
//! The server is organized into several independent modules:
//!
//! - **Network**: IPv6-first networking with dual-stack support
//! - **Transport**: QUIC-first transport layer with TCP fallback
//! - **CLI**: Command-line interface using the command pattern
//! - **Server**: Core server implementation with dependency injection
//! - **Error**: Centralized error handling
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use ninep_server::{Server, NetworkConfig, TransportType};
//! use std::path::PathBuf;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let server = Server::builder()
//!         .network_config(NetworkConfig::default()) // IPv6 dual-stack
//!         .transport(TransportType::default())       // QUIC with encryption
//!         .root_directory(PathBuf::from("."))
//!         .build()
//!         .await?;
//!
//!     server.run().await?;
//!     Ok(())
//! }
//! ```
//!
//! # Features
//!
//! - **Modern Defaults**: IPv6 dual-stack, QUIC transport, encryption by default
//! - **Clean Architecture**: Proper separation of concerns, no God Objects
//! - **Dependency Injection**: Builder pattern for configuration
//! - **Type Safety**: Comprehensive error types, async traits
//! - **Testing**: Full test coverage with property-based testing

// Core modules (always enabled)
pub mod auth;
pub mod auto_mount;
pub mod cli;
pub mod config;
pub mod error;
pub mod fuse_mount;
pub mod network;
pub mod protocol;
pub mod server;
pub mod stats;
pub mod transport;
pub mod util;

// Consensus feature
#[cfg(feature = "consensus")]
pub mod consensus;
#[cfg(feature = "consensus")]
pub mod consensus_control;

// Mesh networking feature
#[cfg(feature = "mesh")]
pub mod mesh;
#[cfg(feature = "mesh")]
pub mod mesh_control;
#[cfg(feature = "mesh")]
pub mod namespace_manager;

// Translator/WASM feature
#[cfg(feature = "translators")]
pub mod settrans;
#[cfg(feature = "translators")]
pub mod wasm;

// Synthetic files feature
#[cfg(feature = "synthetic")]
pub mod synth;

// GPU feature
#[cfg(feature = "gpu")]
pub mod gpu;
#[cfg(feature = "gpu")]
pub mod sycl;
#[cfg(feature = "gpu")]
pub mod compute_control;

// Re-export commonly used types for convenience
pub use error::{Result, ServerError};
pub use fuse_mount::{cleanup_broken_mounts, mount_9p_fuse, unmount_fuse};
pub use network::{BindAddress, NetworkConfig};
pub use server::{Server, ServerConfig};
pub use transport::{Connection, ConnectionListener, Transport, TransportType};

// Re-export CLI for binary usage
pub use cli::Cli;

/// Server version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default 9P.e port (5640)
pub const DEFAULT_PORT: u16 = 5640;

/// Default mesh networking port (9650)
pub const DEFAULT_MESH_PORT: u16 = 9650;

/// Default metrics port (9090 - Prometheus standard)
pub const DEFAULT_METRICS_PORT: u16 = 9090;
