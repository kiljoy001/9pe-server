//! Fog computing protocol messages
//!
//! Protocol messages for distributed job execution across mesh nodes.

use crate::fog::{DataId, FogOptions, QoSPolicy};
use crate::identity::NodeId;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Fog computing protocol messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FogMessage {
    // === Job Lifecycle ===
    /// Offer a job to a remote node
    JobOffer {
        /// Unique job identifier
        job_id: String,
        /// Job specification
        spec: FogJobSpec,
        /// Offered reward/credits (optional incentive system)
        reward: u64,
        /// Current hop count
        hops: u8,
        /// Original requester node
        origin: NodeId,
    },

    /// Accept a job offer
    JobAccept {
        /// Job identifier being accepted
        job_id: String,
        /// Estimated completion time in milliseconds
        estimated_time_ms: u64,
    },

    /// Reject a job offer
    JobReject {
        /// Job identifier being rejected
        job_id: String,
        /// Reason for rejection
        reason: String,
        /// Alternative nodes that might handle it (if known)
        alternatives: Vec<NodeId>,
    },

    // === Job Execution ===
    /// Send input data for a job
    JobInput {
        /// Job identifier
        job_id: String,
        /// Input data (may be chunked for large payloads)
        data: Vec<u8>,
        /// Chunk index (0 for single-chunk jobs)
        chunk_index: u32,
        /// Total chunks expected
        total_chunks: u32,
        /// Shared memory handle if using zero-copy
        shm_id: Option<String>,
    },

    /// Report job progress
    JobProgress {
        /// Job identifier
        job_id: String,
        /// Completion percentage (0-100)
        percent: u8,
        /// Status message
        status: String,
        /// Bytes processed so far
        bytes_processed: u64,
    },

    /// Return job result
    JobResult {
        /// Job identifier
        job_id: String,
        /// Output data
        output: Vec<u8>,
        /// Execution time on the remote node (ms)
        execution_time_ms: u64,
        /// Work receipt for verification/payment
        receipt: Option<WorkReceipt>,
    },

    /// Report job failure
    JobFailure {
        /// Job identifier
        job_id: String,
        /// Error description
        error: String,
        /// Whether the job can be retried
        retriable: bool,
        /// Partial result if any
        partial_result: Option<Vec<u8>>,
    },

    // === Job Control ===
    /// Cancel a running job
    JobCancel {
        /// Job identifier to cancel
        job_id: String,
        /// Reason for cancellation
        reason: String,
    },

    /// Acknowledge job cancellation
    JobCancelAck {
        /// Job identifier
        job_id: String,
        /// Whether cancellation succeeded
        success: bool,
    },

    /// Request job status
    JobStatusQuery {
        /// Job identifier
        job_id: String,
    },

    /// Job status response
    JobStatusResponse {
        /// Job identifier
        job_id: String,
        /// Current status
        status: RemoteJobStatus,
    },

    // === Checkpointing ===
    /// Checkpoint data for fault tolerance
    JobCheckpoint {
        /// Job identifier
        job_id: String,
        /// Checkpoint data
        checkpoint: JobCheckpoint,
    },

    /// Request to resume from checkpoint
    JobResume {
        /// Job identifier
        job_id: String,
        /// Checkpoint to resume from
        checkpoint_id: String,
    },

    // === Data Transfer ===
    /// Request data prefetch to a node
    DataPrefetch {
        /// Data identifier
        data_id: DataId,
        /// Target node to prefetch to
        target_node: NodeId,
        /// Priority (higher = more urgent)
        priority: u8,
    },

    /// Notify that data is available
    DataAvailable {
        /// Data identifier
        data_id: DataId,
        /// Size in bytes
        size: u64,
        /// Content checksum
        checksum: [u8; 32],
        /// TTL in seconds
        ttl_secs: u64,
    },

    /// Request data transfer
    DataRequest {
        /// Data identifier
        data_id: DataId,
        /// Byte offset for partial transfer
        offset: u64,
        /// Number of bytes requested (0 = all)
        length: u64,
    },

    /// Data transfer response
    DataResponse {
        /// Data identifier
        data_id: DataId,
        /// Data chunk
        data: Vec<u8>,
        /// Offset of this chunk
        offset: u64,
        /// Whether this is the final chunk
        is_final: bool,
    },

    // === Resource Discovery ===
    /// Query for nodes with specific capabilities
    ResourceQuery {
        /// Query identifier for response correlation
        query_id: String,
        /// Required operation
        operation: Option<String>,
        /// Whether GPU is required
        requires_gpu: bool,
        /// Minimum VRAM in MB
        min_vram_mb: Option<u64>,
        /// Required data locality
        required_data: Vec<DataId>,
    },

    /// Response to resource query
    ResourceResponse {
        /// Query identifier
        query_id: String,
        /// Node resources
        resources: crate::fog::resources::NodeResources,
    },
}

/// Specification for a fog job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FogJobSpec {
    /// Job type (sycl, wasm, etc.)
    pub job_type: String,
    /// Operation to perform
    pub operation: String,
    /// Input data (may be empty if using shm or data locality)
    pub input: Vec<u8>,
    /// Required VRAM in bytes
    pub required_vram: u64,
    /// Preferred device index
    pub device_hint: Option<usize>,
    /// Fog execution options
    pub fog_options: FogOptions,
    /// Priority level (0 = highest)
    pub priority: u8,
    /// Timeout for execution
    pub timeout: Duration,
    /// Data IDs that must be available
    pub required_data: Vec<DataId>,
    /// Parameters for the operation (JSON-encoded)
    pub params: Vec<u8>,
}

impl Default for FogJobSpec {
    fn default() -> Self {
        Self {
            job_type: "sycl".to_string(),
            operation: String::new(),
            input: Vec::new(),
            required_vram: 0,
            device_hint: None,
            fog_options: FogOptions::default(),
            priority: 2, // Normal
            timeout: Duration::from_secs(300),
            required_data: Vec::new(),
            params: Vec::new(),
        }
    }
}

/// Handle for tracking a remote job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteJobHandle {
    /// Job identifier
    pub job_id: String,
    /// Node executing the job
    pub executing_node: NodeId,
    /// When the job was submitted
    pub submitted_at: u64,
    /// Expected completion time
    pub expected_completion: u64,
    /// Current status
    pub status: RemoteJobStatus,
    /// Number of hops to reach executor
    pub hops: u8,
}

/// Status of a remote job
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RemoteJobStatus {
    /// Job offer sent, waiting for accept/reject
    Pending,
    /// Job accepted, waiting for input transfer
    Accepted,
    /// Input data being transferred
    Transferring { progress_percent: u8 },
    /// Job is executing
    Executing { progress_percent: u8 },
    /// Job completed successfully
    Completed,
    /// Job failed
    Failed { error: String },
    /// Job was cancelled
    Cancelled { reason: String },
    /// Job timed out
    TimedOut,
}

/// Checkpoint for job resumption
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobCheckpoint {
    /// Unique checkpoint identifier
    pub checkpoint_id: String,
    /// Job identifier
    pub job_id: String,
    /// Checkpoint timestamp
    pub timestamp: u64,
    /// Progress percentage at checkpoint
    pub progress_percent: u8,
    /// State data for resumption
    pub state: Vec<u8>,
    /// Intermediate results (if any)
    pub intermediate_results: Option<Vec<u8>>,
    /// Hash of input data for verification
    pub input_hash: [u8; 32],
}

/// Work receipt for verification and optional payment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkReceipt {
    /// Job identifier
    pub job_id: String,
    /// Worker node that executed the job
    pub worker: NodeId,
    /// Requester node that submitted the job
    pub requester: NodeId,
    /// Hash of the work performed
    pub work_hash: [u8; 32],
    /// Hash of the input data
    pub input_hash: [u8; 32],
    /// Hash of the output data
    pub output_hash: [u8; 32],
    /// Execution time in milliseconds
    pub execution_time_ms: u64,
    /// Timestamp of completion
    pub timestamp: u64,
    /// Signature from the worker node
    pub signature: Vec<u8>,
}

impl WorkReceipt {
    /// Create a new work receipt (unsigned)
    pub fn new(
        job_id: String,
        worker: NodeId,
        requester: NodeId,
        input: &[u8],
        output: &[u8],
        execution_time_ms: u64,
    ) -> Self {
        use blake3::Hasher;

        let input_hash: [u8; 32] = *blake3::hash(input).as_bytes();
        let output_hash: [u8; 32] = *blake3::hash(output).as_bytes();

        let mut hasher = Hasher::new();
        hasher.update(&input_hash);
        hasher.update(&output_hash);
        hasher.update(&execution_time_ms.to_le_bytes());
        let work_hash: [u8; 32] = *hasher.finalize().as_bytes();

        Self {
            job_id,
            worker,
            requester,
            work_hash,
            input_hash,
            output_hash,
            execution_time_ms,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            signature: Vec::new(),
        }
    }

    /// Verify the receipt hashes match
    pub fn verify_hashes(&self, input: &[u8], output: &[u8]) -> bool {
        let expected_input: [u8; 32] = *blake3::hash(input).as_bytes();
        let expected_output: [u8; 32] = *blake3::hash(output).as_bytes();
        self.input_hash == expected_input && self.output_hash == expected_output
    }
}

/// Serialization helpers
impl FogMessage {
    /// Serialize to bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    /// Deserialize from bytes
    pub fn from_bytes(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }

    /// Get message type as string for logging
    pub fn message_type(&self) -> &'static str {
        match self {
            FogMessage::JobOffer { .. } => "JobOffer",
            FogMessage::JobAccept { .. } => "JobAccept",
            FogMessage::JobReject { .. } => "JobReject",
            FogMessage::JobInput { .. } => "JobInput",
            FogMessage::JobProgress { .. } => "JobProgress",
            FogMessage::JobResult { .. } => "JobResult",
            FogMessage::JobFailure { .. } => "JobFailure",
            FogMessage::JobCancel { .. } => "JobCancel",
            FogMessage::JobCancelAck { .. } => "JobCancelAck",
            FogMessage::JobStatusQuery { .. } => "JobStatusQuery",
            FogMessage::JobStatusResponse { .. } => "JobStatusResponse",
            FogMessage::JobCheckpoint { .. } => "JobCheckpoint",
            FogMessage::JobResume { .. } => "JobResume",
            FogMessage::DataPrefetch { .. } => "DataPrefetch",
            FogMessage::DataAvailable { .. } => "DataAvailable",
            FogMessage::DataRequest { .. } => "DataRequest",
            FogMessage::DataResponse { .. } => "DataResponse",
            FogMessage::ResourceQuery { .. } => "ResourceQuery",
            FogMessage::ResourceResponse { .. } => "ResourceResponse",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fog_message_serialization() {
        let msg = FogMessage::JobOffer {
            job_id: "test-123".to_string(),
            spec: FogJobSpec::default(),
            reward: 100,
            hops: 0,
            origin: NodeId::new("origin-node".to_string()),
        };

        let bytes = msg.to_bytes().unwrap();
        let decoded = FogMessage::from_bytes(&bytes).unwrap();

        match decoded {
            FogMessage::JobOffer { job_id, .. } => assert_eq!(job_id, "test-123"),
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_work_receipt() {
        let input = b"test input data";
        let output = b"test output data";

        let receipt = WorkReceipt::new(
            "job-1".to_string(),
            NodeId::new("worker".to_string()),
            NodeId::new("requester".to_string()),
            input,
            output,
            1000,
        );

        assert!(receipt.verify_hashes(input, output));
        assert!(!receipt.verify_hashes(b"wrong input", output));
    }
}
