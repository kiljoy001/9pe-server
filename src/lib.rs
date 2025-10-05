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

pub mod auth;
pub mod auto_mount;
pub mod config;
pub mod cli;
pub mod consensus;
pub mod error;
pub mod fuse_mount;
pub mod mesh;
pub mod network;
pub mod protocol;
pub mod server;
pub mod settrans;
pub mod synth;
pub mod transport;
pub mod wasm;

// Re-export commonly used types for convenience
pub use server::{Server, ServerConfig};
pub use network::{NetworkConfig, BindAddress};
pub use transport::{TransportType, Transport, Connection, ConnectionListener};
pub use error::{ServerError, Result};
pub use fuse_mount::{mount_9p_fuse, unmount_fuse, cleanup_broken_mounts};

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