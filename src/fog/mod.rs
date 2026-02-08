//! Fog Computing Support
//!
//! Enables distributed job execution across mesh network nodes with:
//! - Intelligent job routing based on node capabilities
//! - Data locality awareness
//! - Fault tolerance with automatic reassignment
//! - Resource advertisement and discovery

pub mod router;
pub mod resources;
pub mod protocol;

pub use router::{FogRouter, RoutingDecision, JobPlacement, LocalExecutor, DefaultFogRouter, ComputeLocalExecutor};
pub use resources::{NodeResources, ResourceAdvertisement, ResourceQuery};
pub use protocol::{FogMessage, FogJobSpec, RemoteJobHandle, JobCheckpoint};

use crate::identity::NodeId;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Data identifier for locality-aware scheduling
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataId(pub String);

impl DataId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for DataId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for DataId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Quality of Service policy for fog jobs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QoSPolicy {
    /// Maximum acceptable latency in milliseconds
    pub max_latency_ms: u64,
    /// Minimum required throughput (operations per second)
    pub min_throughput_ops: Option<u64>,
    /// Required reliability percentage (e.g., 99.9)
    pub reliability_percent: f32,
    /// Geographic/network constraints for data residency
    pub data_residency: Option<Vec<String>>,
    /// Prefer nodes with specific capabilities
    pub preferred_capabilities: Vec<String>,
}

impl Default for QoSPolicy {
    fn default() -> Self {
        Self {
            max_latency_ms: 30_000, // 30 seconds default
            min_throughput_ops: None,
            reliability_percent: 99.0,
            data_residency: None,
            preferred_capabilities: Vec::new(),
        }
    }
}

/// Fog job execution options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FogOptions {
    /// Allow job to execute on remote nodes
    pub allow_remote: bool,
    /// Data IDs this job needs access to
    pub data_locality: Vec<DataId>,
    /// Maximum delegation hops (prevents infinite forwarding)
    pub max_hops: u8,
    /// QoS requirements
    pub qos: QoSPolicy,
    /// Whether job can be split across multiple nodes
    pub allow_splitting: bool,
    /// Minimum chunk size if splitting is allowed
    pub min_split_size: Option<u64>,
    /// Timeout for remote execution (includes network latency)
    pub remote_timeout: Duration,
    /// Number of retry attempts on failure
    pub max_retries: u8,
    /// Checkpoint interval for long-running jobs
    pub checkpoint_interval: Option<Duration>,
}

impl Default for FogOptions {
    fn default() -> Self {
        Self {
            allow_remote: false, // Default to local-only for safety
            data_locality: Vec::new(),
            max_hops: 3,
            qos: QoSPolicy::default(),
            allow_splitting: false,
            min_split_size: None,
            remote_timeout: Duration::from_secs(300),
            max_retries: 2,
            checkpoint_interval: None,
        }
    }
}

/// Result of a fog job execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FogJobResult {
    /// Job identifier
    pub job_id: String,
    /// Node that executed the job
    pub executed_by: NodeId,
    /// Execution result data
    pub data: Vec<u8>,
    /// Total execution time including network
    pub total_time_ms: u64,
    /// Pure compute time on remote node
    pub compute_time_ms: u64,
    /// Number of hops the job traveled
    pub hops: u8,
    /// Whether result came from cache
    pub from_cache: bool,
}

/// Error types for fog computing operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FogError {
    /// No suitable node found for job
    NoSuitableNode { reason: String },
    /// Job exceeded maximum hops
    MaxHopsExceeded { hops: u8 },
    /// Remote node rejected the job
    JobRejected { node: NodeId, reason: String },
    /// Remote execution failed
    ExecutionFailed { node: NodeId, error: String },
    /// Network error during job transfer
    NetworkError { error: String },
    /// Timeout waiting for result
    Timeout { elapsed_ms: u64 },
    /// QoS requirements not met
    QoSViolation { policy: String, actual: String },
    /// Data not available at any reachable node
    DataNotFound { data_id: DataId },
    /// Authentication failed with remote node
    AuthenticationFailed { node: NodeId },
}

impl std::fmt::Display for FogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuitableNode { reason } => write!(f, "No suitable node: {}", reason),
            Self::MaxHopsExceeded { hops } => write!(f, "Max hops exceeded: {}", hops),
            Self::JobRejected { node, reason } => {
                write!(f, "Job rejected by {}: {}", node.as_str(), reason)
            }
            Self::ExecutionFailed { node, error } => {
                write!(f, "Execution failed on {}: {}", node.as_str(), error)
            }
            Self::NetworkError { error } => write!(f, "Network error: {}", error),
            Self::Timeout { elapsed_ms } => write!(f, "Timeout after {}ms", elapsed_ms),
            Self::QoSViolation { policy, actual } => {
                write!(f, "QoS violation: expected {}, got {}", policy, actual)
            }
            Self::DataNotFound { data_id } => write!(f, "Data not found: {}", data_id.as_str()),
            Self::AuthenticationFailed { node } => {
                write!(f, "Auth failed with {}", node.as_str())
            }
        }
    }
}

impl std::error::Error for FogError {}
