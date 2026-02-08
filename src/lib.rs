//! 9P.e Protocol Implementation
//!
//! A revolutionary extension of the Plan 9 filesystem protocol with:
//! - Async streaming and multiplexing
//! - ChaCha20-Poly1305 + Ed25519 encryption
//! - Hurd-style translator system
//! - Synthetic files with live content generation
//! - GHOSTDAG consensus with 464x space optimization
//! - Full backward compatibility with 9P2000
//! - Sovereign peer-to-peer identity system
//!
//! All components are formally verified with property-based testing.

#![warn(missing_docs)]
#![warn(clippy::all)]

/// Core protocol message types and serialization

/// Server version
pub const VERSION: &str = "1.0.0";

pub mod protocol;

/// GHOSTDAG consensus implementation with pebbling optimizations
pub mod consensus;

/// Hurd-style translator system with sandboxing
pub mod translators;


/// Cryptographic authentication and encryption
pub mod crypto;

/// Authentication service with Argon2id + sled persistence
pub mod auth;

/// Backward compatibility with 9P2000
pub mod compatibility;

/// Memory management and resource bounds
pub mod memory;

/// Concurrent operations and thread safety
pub mod concurrency;

/// QUIC transport layer replacing TCP + streaming
pub mod transport;

/// Rate limiting and connection management (simplified for QUIC)
pub mod rate_limiter;

/// Server utility functions
pub mod util;

/// Sovereign identity system for peer-to-peer nodes
pub mod identity;

/// UUIDv8-based extended file identifiers
pub mod fid;

/// DHT integration for peer discovery using sovereign identities
pub mod dht;

// Re-export main types for convenience
pub use protocol::*;
pub use consensus::*;
pub use translators::*;
pub use crypto::*;
pub use compatibility::*;
pub use memory::*;
pub use concurrency::*;
pub use transport::*;
pub use rate_limiter::*;
pub use identity::*;
pub use dht::*;

/// Filesystem server implementation
pub mod server;

/// WASM translator system
pub mod wasm;

/// Virtual settrans system
pub mod settrans;

/// Auto-mount daemon
pub mod auto_mount;

/// Synthetic filesystem support (extension)
pub mod synth;

// Re-exports
pub use server::*;
pub use wasm::*;
pub use settrans::*;
pub use auto_mount::*;
pub use synth::*;

/// Network configuration and primitives
pub mod network;

/// Server configuration
pub mod config;

/// Namespace management
pub mod namespace_manager;

/// Mesh network integration
pub mod mesh;

/// GPU acceleration support
pub mod gpu;

/// SYCL integration
pub mod sycl;

/// Compute control
pub mod compute_control;

/// Fog computing for distributed job execution
pub mod fog;

/// Consensus control
pub mod consensus_control;

/// Mesh control
pub mod mesh_control;

/// Statistics
pub mod stats;

/// Error types
pub mod error;

// Re-exports for newly added modules
pub use network::*;
pub use config::*;
pub use namespace_manager::*;
pub use mesh::*;
pub use gpu::*;
pub use sycl::*;
pub use compute_control::*;
pub use fog::*;
pub use consensus_control::*;
pub use mesh_control::*;
pub use stats::*;
pub use error::*;
pub mod traits;
pub use traits::*;

/// 9P Client implementation
pub mod client;
pub use client::*;

pub mod compute_adapter;
pub use compute_adapter::*;
pub mod storage_adapter;
pub use storage_adapter::*;
pub mod wasm_adapter;
pub use wasm_adapter::*;
pub mod ipc;
pub use ipc::*;
#[cfg(feature = "fuse")]
pub mod fuse_mount;
#[cfg(feature = "fuse")]
pub use fuse_mount::*;

/// CLI module
pub mod cli;
pub use cli::*;
