//! Fog Job Router
//!
//! Intelligent routing of compute jobs across fog network nodes based on:
//! - Node capabilities (GPU, memory, supported operations)
//! - Current load and availability
//! - Data locality
//! - Network latency
//! - QoS requirements

use crate::fog::protocol::{FogJobSpec, FogMessage, RemoteJobHandle, RemoteJobStatus, WorkReceipt};
use crate::fog::resources::{NodeResources, ResourceQuery, ResourceRegistry};
use crate::fog::{DataId, FogError, FogJobResult, FogOptions, QoSPolicy};
use crate::identity::NodeId;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Decision about where to execute a job
#[derive(Debug, Clone)]
pub enum RoutingDecision {
    /// Execute locally
    Local,
    /// Forward to a specific remote node
    Remote { node: NodeId, score: f64 },
    /// Split across multiple nodes
    Split { nodes: Vec<(NodeId, f64)> },
    /// Cannot route - no suitable nodes
    NoRoute { reason: String },
}

/// Job placement result
#[derive(Debug, Clone)]
pub struct JobPlacement {
    /// The routing decision made
    pub decision: RoutingDecision,
    /// Estimated execution time
    pub estimated_time_ms: u64,
    /// Data that needs to be transferred
    pub data_transfer_required: Vec<DataId>,
    /// Score/confidence in this placement
    pub confidence: f64,
}

/// Trait for fog routing implementations
#[async_trait]
pub trait FogRouter: Send + Sync {
    /// Decide where to route a job
    async fn route(&self, spec: &FogJobSpec) -> Result<JobPlacement, FogError>;

    /// Submit a job for fog execution
    async fn submit(&self, spec: FogJobSpec) -> Result<String, FogError>;

    /// Get status of a submitted job
    async fn get_status(&self, job_id: &str) -> Option<RemoteJobStatus>;

    /// Wait for job completion
    async fn wait_for_completion(&self, job_id: &str, timeout: Duration)
        -> Result<FogJobResult, FogError>;

    /// Cancel a job
    async fn cancel(&self, job_id: &str, reason: &str) -> Result<(), FogError>;
}

/// Pending job waiting for completion
struct PendingJob {
    spec: FogJobSpec,
    handle: RemoteJobHandle,
    submitted_at: Instant,
    completion_tx: Option<oneshot::Sender<Result<FogJobResult, FogError>>>,
    retries_remaining: u8,
}

/// Default fog router implementation
pub struct DefaultFogRouter {
    /// Local node ID
    local_node: NodeId,
    /// Resource registry for node discovery
    resources: Arc<ResourceRegistry>,
    /// Channel for sending messages to mesh network
    mesh_tx: mpsc::UnboundedSender<(NodeId, FogMessage)>,
    /// Pending jobs awaiting results
    pending_jobs: Arc<RwLock<HashMap<String, PendingJob>>>,
    /// Completed job results (cached briefly)
    completed_jobs: Arc<RwLock<HashMap<String, FogJobResult>>>,
    /// Local execution handler
    local_executor: Arc<dyn LocalExecutor>,
    /// Configuration
    config: RouterConfig,
}

/// Configuration for the fog router
#[derive(Debug, Clone)]
pub struct RouterConfig {
    /// Prefer local execution if load below this threshold
    pub local_preference_threshold: f32,
    /// Minimum score difference to prefer remote over local
    pub remote_score_margin: f64,
    /// Maximum pending jobs per remote node
    pub max_jobs_per_node: u32,
    /// Default timeout for job offers
    pub offer_timeout: Duration,
    /// How long to cache completed results
    pub result_cache_ttl: Duration,
    /// Whether to automatically retry failed jobs
    pub auto_retry: bool,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            local_preference_threshold: 0.7,
            remote_score_margin: 10.0,
            max_jobs_per_node: 10,
            offer_timeout: Duration::from_secs(10),
            result_cache_ttl: Duration::from_secs(300),
            auto_retry: true,
        }
    }
}

/// Trait for local job execution
#[async_trait]
pub trait LocalExecutor: Send + Sync {
    /// Execute a job locally
    async fn execute(&self, spec: &FogJobSpec) -> Result<Vec<u8>, String>;

    /// Check if local node can handle the job
    fn can_handle(&self, spec: &FogJobSpec) -> bool;

    /// Get current local load (0.0 - 1.0)
    async fn current_load(&self) -> f32;
}

/// Local executor that wraps a ComputeManager for local job execution
pub struct ComputeLocalExecutor {
    compute_manager: Arc<crate::compute_control::ComputeManager>,
    supported_operations: Vec<String>,
}

impl ComputeLocalExecutor {
    /// Create a new compute local executor
    pub fn new(compute_manager: Arc<crate::compute_control::ComputeManager>) -> Self {
        Self {
            compute_manager,
            supported_operations: vec![
                "matmul".to_string(),
                "vector_add".to_string(),
                "reduce_sum".to_string(),
                "relu".to_string(),
                "sycl".to_string(),
                "wasm".to_string(),
            ],
        }
    }

    /// Add supported operation
    pub fn with_operation(mut self, op: impl Into<String>) -> Self {
        self.supported_operations.push(op.into());
        self
    }
}

#[async_trait]
impl LocalExecutor for ComputeLocalExecutor {
    async fn execute(&self, spec: &FogJobSpec) -> Result<Vec<u8>, String> {
        use crate::compute_control::{FogJobOptions, JobPriority, JobSubmission};

        let submission = JobSubmission {
            job_type: spec.job_type.clone(),
            operation: spec.operation.clone(),
            payload: spec.input.clone(),
            requested_vram: spec.required_vram,
            device_hint: spec.device_hint,
            shm_handle: None,
            priority: JobPriority::from(spec.priority),
            timeout_secs: spec.timeout.as_secs(),
            fog: FogJobOptions {
                allow_remote: false, // Local execution only
                data_locality: vec![],
                max_hops: 0,
                assigned_node: None,
                max_retries: 0,
            },
        };

        // Submit and wait for completion
        let job_id = self.compute_manager.submit_job(submission).await
            .map_err(|e| format!("Failed to submit job: {}", e))?;

        // Poll for completion with timeout
        let deadline = std::time::Instant::now() + spec.timeout;
        loop {
            if std::time::Instant::now() > deadline {
                return Err("Job timed out".to_string());
            }

            match self.compute_manager.get_job_status(&job_id).await {
                Some(crate::compute_control::JobStatus::Completed(result)) => {
                    return Ok(result);
                }
                Some(crate::compute_control::JobStatus::Failed(err)) => {
                    return Err(err);
                }
                Some(crate::compute_control::JobStatus::Pending | crate::compute_control::JobStatus::Running) => {
                    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                }
                None => {
                    return Err("Job not found".to_string());
                }
            }
        }
    }

    fn can_handle(&self, spec: &FogJobSpec) -> bool {
        // Check if we support the job type and operation
        let type_ok = spec.job_type == "sycl" || spec.job_type == "wasm" || spec.job_type == "raw";
        let op_ok = self.supported_operations.iter().any(|op| {
            op == &spec.operation || spec.operation.is_empty()
        });
        type_ok && op_ok
    }

    async fn current_load(&self) -> f32 {
        // Estimate load based on pending jobs
        let jobs = self.compute_manager.list_jobs().await;
        let active = jobs.iter().filter(|j| {
            matches!(j.status, crate::compute_control::JobStatus::Pending | crate::compute_control::JobStatus::Running)
        }).count();

        // Simple load calculation: assume 10 concurrent jobs = full load
        (active as f32 / 10.0).min(1.0)
    }
}

impl DefaultFogRouter {
    /// Create a new fog router
    pub fn new(
        local_node: NodeId,
        resources: Arc<ResourceRegistry>,
        mesh_tx: mpsc::UnboundedSender<(NodeId, FogMessage)>,
        local_executor: Arc<dyn LocalExecutor>,
    ) -> Self {
        Self {
            local_node,
            resources,
            mesh_tx,
            pending_jobs: Arc::new(RwLock::new(HashMap::new())),
            completed_jobs: Arc::new(RwLock::new(HashMap::new())),
            local_executor,
            config: RouterConfig::default(),
        }
    }

    /// Create with custom configuration
    pub fn with_config(mut self, config: RouterConfig) -> Self {
        self.config = config;
        self
    }

    /// Build resource query from job spec
    fn build_query(&self, spec: &FogJobSpec) -> ResourceQuery {
        ResourceQuery {
            operation: Some(spec.operation.clone()),
            requires_gpu: spec.required_vram > 0 || spec.job_type == "sycl",
            min_vram_mb: if spec.required_vram > 0 {
                Some(spec.required_vram / (1024 * 1024))
            } else {
                None
            },
            min_memory_mb: None,
            required_data: spec.required_data.iter().map(|d| d.0.clone()).collect(),
            max_load_ratio: Some(0.9),
            required_translator: if spec.job_type == "wasm" {
                Some(spec.operation.clone())
            } else {
                None
            },
        }
    }

    /// Score local execution
    async fn score_local(&self, spec: &FogJobSpec) -> f64 {
        if !self.local_executor.can_handle(spec) {
            return 0.0;
        }

        let load = self.local_executor.current_load().await;
        let mut score = 100.0;

        // Penalize high load
        score -= load as f64 * 40.0;

        // Bonus for data already being local
        let local_resources = self.resources.get_local().await;
        if let Some(res) = local_resources {
            let local_data_count = spec
                .required_data
                .iter()
                .filter(|d| res.local_data.contains(&d.0))
                .count();
            score += local_data_count as f64 * 20.0;
        }

        // Bonus for avoiding network latency
        score += 15.0;

        score.max(0.0)
    }

    /// Find best remote node for the job
    async fn find_best_remote(&self, spec: &FogJobSpec) -> Option<(NodeId, f64)> {
        let query = self.build_query(spec);
        let candidates = self.resources.find_nodes(&query).await;

        // Filter out local node and nodes at capacity
        let pending = self.pending_jobs.read().await;
        let jobs_per_node: HashMap<String, u32> = pending
            .values()
            .map(|p| p.handle.executing_node.as_str().to_string())
            .fold(HashMap::new(), |mut acc, node| {
                *acc.entry(node).or_insert(0) += 1;
                acc
            });

        for candidate in candidates {
            let node_id = candidate.node_id.as_str();

            // Skip local node
            if node_id == self.local_node.as_str() {
                continue;
            }

            // Skip overloaded nodes
            let jobs_to_node = jobs_per_node.get(node_id).copied().unwrap_or(0);
            if jobs_to_node >= self.config.max_jobs_per_node {
                continue;
            }

            let score = candidate.score(&query);
            return Some((candidate.node_id.clone(), score));
        }

        None
    }

    /// Send a message to a remote node
    fn send_message(&self, target: &NodeId, message: FogMessage) -> Result<(), FogError> {
        self.mesh_tx
            .send((target.clone(), message))
            .map_err(|_| FogError::NetworkError {
                error: "Mesh channel closed".to_string(),
            })
    }

    /// Handle incoming fog message
    pub async fn handle_message(&self, from: NodeId, message: FogMessage) -> Result<(), FogError> {
        match message {
            FogMessage::JobAccept {
                job_id,
                estimated_time_ms,
            } => {
                self.handle_job_accept(&job_id, from, estimated_time_ms)
                    .await
            }

            FogMessage::JobReject {
                job_id,
                reason,
                alternatives,
            } => {
                self.handle_job_reject(&job_id, reason, alternatives).await
            }

            FogMessage::JobProgress {
                job_id,
                percent,
                status,
                ..
            } => self.handle_job_progress(&job_id, percent, &status).await,

            FogMessage::JobResult {
                job_id,
                output,
                execution_time_ms,
                receipt,
            } => {
                self.handle_job_result(&job_id, from, output, execution_time_ms, receipt)
                    .await
            }

            FogMessage::JobFailure {
                job_id,
                error,
                retriable,
                partial_result,
            } => {
                self.handle_job_failure(&job_id, error, retriable, partial_result)
                    .await
            }

            FogMessage::JobOffer {
                job_id,
                spec,
                reward,
                hops,
                origin,
            } => {
                self.handle_incoming_job_offer(job_id, spec, reward, hops, origin, from)
                    .await
            }

            _ => {
                debug!("Unhandled fog message type: {}", message.message_type());
                Ok(())
            }
        }
    }

    async fn handle_job_accept(
        &self,
        job_id: &str,
        from: NodeId,
        estimated_time_ms: u64,
    ) -> Result<(), FogError> {
        let mut pending = self.pending_jobs.write().await;
        if let Some(job) = pending.get_mut(job_id) {
            job.handle.status = RemoteJobStatus::Accepted;
            job.handle.expected_completion = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64
                + estimated_time_ms;
            info!(
                "Job {} accepted by {}, ETA {}ms",
                job_id,
                from.as_str(),
                estimated_time_ms
            );

            // Send input data
            let input_msg = FogMessage::JobInput {
                job_id: job_id.to_string(),
                data: job.spec.input.clone(),
                chunk_index: 0,
                total_chunks: 1,
                shm_id: None,
            };
            self.send_message(&from, input_msg)?;
        }
        Ok(())
    }

    async fn handle_job_reject(
        &self,
        job_id: &str,
        reason: String,
        alternatives: Vec<NodeId>,
    ) -> Result<(), FogError> {
        warn!("Job {} rejected: {}", job_id, reason);

        let mut pending = self.pending_jobs.write().await;
        if let Some(job) = pending.get_mut(job_id) {
            // Try alternatives if available and retries remaining
            if !alternatives.is_empty() && job.retries_remaining > 0 {
                job.retries_remaining -= 1;
                let alt_node = &alternatives[0];
                info!(
                    "Retrying job {} with alternative node {}",
                    job_id,
                    alt_node.as_str()
                );

                let offer = FogMessage::JobOffer {
                    job_id: job_id.to_string(),
                    spec: job.spec.clone(),
                    reward: 0,
                    hops: job.handle.hops + 1,
                    origin: self.local_node.clone(),
                };
                self.send_message(alt_node, offer)?;
                job.handle.executing_node = alt_node.clone();
                return Ok(());
            }

            // No alternatives or no retries - fail the job
            if let Some(tx) = job.completion_tx.take() {
                let _ = tx.send(Err(FogError::JobRejected {
                    node: job.handle.executing_node.clone(),
                    reason,
                }));
            }
        }
        pending.remove(job_id);
        Ok(())
    }

    async fn handle_job_progress(
        &self,
        job_id: &str,
        percent: u8,
        status: &str,
    ) -> Result<(), FogError> {
        let mut pending = self.pending_jobs.write().await;
        if let Some(job) = pending.get_mut(job_id) {
            job.handle.status = RemoteJobStatus::Executing {
                progress_percent: percent,
            };
            debug!("Job {} progress: {}% - {}", job_id, percent, status);
        }
        Ok(())
    }

    async fn handle_job_result(
        &self,
        job_id: &str,
        from: NodeId,
        output: Vec<u8>,
        execution_time_ms: u64,
        _receipt: Option<WorkReceipt>,
    ) -> Result<(), FogError> {
        let mut pending = self.pending_jobs.write().await;
        if let Some(job) = pending.remove(job_id) {
            let total_time = job.submitted_at.elapsed().as_millis() as u64;

            let result = FogJobResult {
                job_id: job_id.to_string(),
                executed_by: from,
                data: output,
                total_time_ms: total_time,
                compute_time_ms: execution_time_ms,
                hops: job.handle.hops,
                from_cache: false,
            };

            info!(
                "Job {} completed: {}ms total, {}ms compute",
                job_id, total_time, execution_time_ms
            );

            // Cache result
            {
                let mut completed = self.completed_jobs.write().await;
                completed.insert(job_id.to_string(), result.clone());
            }

            // Notify waiter
            if let Some(tx) = job.completion_tx {
                let _ = tx.send(Ok(result));
            }
        }
        Ok(())
    }

    async fn handle_job_failure(
        &self,
        job_id: &str,
        error: String,
        retriable: bool,
        _partial_result: Option<Vec<u8>>,
    ) -> Result<(), FogError> {
        let mut pending = self.pending_jobs.write().await;
        if let Some(mut job) = pending.remove(job_id) {
            // Try retry if allowed
            if retriable && self.config.auto_retry && job.retries_remaining > 0 {
                job.retries_remaining -= 1;
                warn!(
                    "Job {} failed (retriable), {} retries remaining",
                    job_id, job.retries_remaining
                );

                // Find another node
                drop(pending);
                if let Some((new_node, _)) = self.find_best_remote(&job.spec).await {
                    let offer = FogMessage::JobOffer {
                        job_id: job_id.to_string(),
                        spec: job.spec.clone(),
                        reward: 0,
                        hops: job.handle.hops,
                        origin: self.local_node.clone(),
                    };
                    self.send_message(&new_node, offer)?;
                    job.handle.executing_node = new_node;
                    job.handle.status = RemoteJobStatus::Pending;

                    let mut pending = self.pending_jobs.write().await;
                    pending.insert(job_id.to_string(), job);
                    return Ok(());
                }
            }

            // Failed definitively
            error!("Job {} failed: {}", job_id, error);
            if let Some(tx) = job.completion_tx {
                let _ = tx.send(Err(FogError::ExecutionFailed {
                    node: job.handle.executing_node,
                    error,
                }));
            }
        }
        Ok(())
    }

    async fn handle_incoming_job_offer(
        &self,
        job_id: String,
        spec: FogJobSpec,
        _reward: u64,
        hops: u8,
        origin: NodeId,
        from: NodeId,
    ) -> Result<(), FogError> {
        // Check hop limit
        if hops >= spec.fog_options.max_hops {
            let reject = FogMessage::JobReject {
                job_id,
                reason: "Max hops exceeded".to_string(),
                alternatives: vec![],
            };
            return self.send_message(&from, reject);
        }

        // Check if we can handle it
        if !self.local_executor.can_handle(&spec) {
            // Find alternatives to suggest
            let query = self.build_query(&spec);
            let alternatives: Vec<NodeId> = self
                .resources
                .find_nodes(&query)
                .await
                .into_iter()
                .take(3)
                .map(|r| r.node_id)
                .collect();

            let reject = FogMessage::JobReject {
                job_id,
                reason: "Cannot handle job type".to_string(),
                alternatives,
            };
            return self.send_message(&from, reject);
        }

        // Check current load
        let load = self.local_executor.current_load().await;
        if load > 0.95 {
            let reject = FogMessage::JobReject {
                job_id,
                reason: "Node at capacity".to_string(),
                alternatives: vec![],
            };
            return self.send_message(&from, reject);
        }

        // Accept the job
        info!(
            "Accepting job {} from {} (origin: {})",
            job_id,
            from.as_str(),
            origin.as_str()
        );

        let accept = FogMessage::JobAccept {
            job_id: job_id.clone(),
            estimated_time_ms: 5000, // Estimate
        };
        self.send_message(&from, accept)?;

        // Execute in background
        let executor = Arc::clone(&self.local_executor);
        let mesh_tx = self.mesh_tx.clone();
        let local_node = self.local_node.clone();

        tokio::spawn(async move {
            let start = Instant::now();
            match executor.execute(&spec).await {
                Ok(output) => {
                    let execution_time_ms = start.elapsed().as_millis() as u64;
                    let receipt = WorkReceipt::new(
                        job_id.clone(),
                        local_node.clone(),
                        origin,
                        &spec.input,
                        &output,
                        execution_time_ms,
                    );

                    let result = FogMessage::JobResult {
                        job_id,
                        output,
                        execution_time_ms,
                        receipt: Some(receipt),
                    };
                    let _ = mesh_tx.send((from, result));
                }
                Err(error) => {
                    let failure = FogMessage::JobFailure {
                        job_id,
                        error,
                        retriable: true,
                        partial_result: None,
                    };
                    let _ = mesh_tx.send((from, failure));
                }
            }
        });

        Ok(())
    }

    /// Start background tasks (cleanup, monitoring)
    pub fn start_background_tasks(self: &Arc<Self>) {
        // Cleanup completed jobs cache
        let router = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                let mut completed = router.completed_jobs.write().await;
                // Just clear old entries periodically
                if completed.len() > 1000 {
                    completed.clear();
                }
            }
        });

        // Monitor pending jobs for timeouts
        let router = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(10));
            loop {
                interval.tick().await;
                let mut pending = router.pending_jobs.write().await;
                let mut timed_out = Vec::new();

                for (job_id, job) in pending.iter() {
                    if job.submitted_at.elapsed() > job.spec.timeout {
                        timed_out.push(job_id.clone());
                    }
                }

                for job_id in timed_out {
                    if let Some(job) = pending.remove(&job_id) {
                        warn!("Job {} timed out", job_id);
                        if let Some(tx) = job.completion_tx {
                            let _ = tx.send(Err(FogError::Timeout {
                                elapsed_ms: job.submitted_at.elapsed().as_millis() as u64,
                            }));
                        }
                    }
                }
            }
        });
    }
}

#[async_trait]
impl FogRouter for DefaultFogRouter {
    async fn route(&self, spec: &FogJobSpec) -> Result<JobPlacement, FogError> {
        // If remote execution not allowed, must be local
        if !spec.fog_options.allow_remote {
            if self.local_executor.can_handle(spec) {
                return Ok(JobPlacement {
                    decision: RoutingDecision::Local,
                    estimated_time_ms: 1000,
                    data_transfer_required: vec![],
                    confidence: 1.0,
                });
            } else {
                return Err(FogError::NoSuitableNode {
                    reason: "Local execution required but cannot handle job".to_string(),
                });
            }
        }

        // Score local execution
        let local_score = self.score_local(spec).await;

        // Find best remote
        let remote_option = self.find_best_remote(spec).await;

        match remote_option {
            Some((remote_node, remote_score)) => {
                // Compare scores with margin
                if remote_score > local_score + self.config.remote_score_margin {
                    Ok(JobPlacement {
                        decision: RoutingDecision::Remote {
                            node: remote_node,
                            score: remote_score,
                        },
                        estimated_time_ms: 5000,
                        data_transfer_required: spec.required_data.clone(),
                        confidence: remote_score / 100.0,
                    })
                } else if local_score > 0.0 {
                    Ok(JobPlacement {
                        decision: RoutingDecision::Local,
                        estimated_time_ms: 1000,
                        data_transfer_required: vec![],
                        confidence: local_score / 100.0,
                    })
                } else {
                    Err(FogError::NoSuitableNode {
                        reason: "No suitable node found".to_string(),
                    })
                }
            }
            None => {
                if local_score > 0.0 {
                    Ok(JobPlacement {
                        decision: RoutingDecision::Local,
                        estimated_time_ms: 1000,
                        data_transfer_required: vec![],
                        confidence: local_score / 100.0,
                    })
                } else {
                    Err(FogError::NoSuitableNode {
                        reason: "No nodes available".to_string(),
                    })
                }
            }
        }
    }

    async fn submit(&self, spec: FogJobSpec) -> Result<String, FogError> {
        let placement = self.route(&spec).await?;
        let job_id = Uuid::new_v4().to_string();

        match placement.decision {
            RoutingDecision::Local => {
                // Execute locally in background
                let executor = Arc::clone(&self.local_executor);
                let completed_jobs = Arc::clone(&self.completed_jobs);
                let local_node = self.local_node.clone();
                let job_id_clone = job_id.clone();
                let spec_clone = spec.clone();

                tokio::spawn(async move {
                    let start = Instant::now();
                    match executor.execute(&spec_clone).await {
                        Ok(output) => {
                            let result = FogJobResult {
                                job_id: job_id_clone.clone(),
                                executed_by: local_node,
                                data: output,
                                total_time_ms: start.elapsed().as_millis() as u64,
                                compute_time_ms: start.elapsed().as_millis() as u64,
                                hops: 0,
                                from_cache: false,
                            };
                            let mut completed = completed_jobs.write().await;
                            completed.insert(job_id_clone, result);
                        }
                        Err(e) => {
                            error!("Local execution failed: {}", e);
                        }
                    }
                });

                Ok(job_id)
            }

            RoutingDecision::Remote { node, score: _ } => {
                let offer = FogMessage::JobOffer {
                    job_id: job_id.clone(),
                    spec: spec.clone(),
                    reward: 0,
                    hops: 0,
                    origin: self.local_node.clone(),
                };

                self.send_message(&node, offer)?;

                let handle = RemoteJobHandle {
                    job_id: job_id.clone(),
                    executing_node: node,
                    submitted_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    expected_completion: 0,
                    status: RemoteJobStatus::Pending,
                    hops: 0,
                };

                let pending = PendingJob {
                    spec,
                    handle,
                    submitted_at: Instant::now(),
                    completion_tx: None,
                    retries_remaining: 2,
                };

                let mut pending_jobs = self.pending_jobs.write().await;
                pending_jobs.insert(job_id.clone(), pending);

                Ok(job_id)
            }

            RoutingDecision::Split { .. } => {
                // Job splitting not yet implemented
                Err(FogError::NoSuitableNode {
                    reason: "Job splitting not implemented".to_string(),
                })
            }

            RoutingDecision::NoRoute { reason } => Err(FogError::NoSuitableNode { reason }),
        }
    }

    async fn get_status(&self, job_id: &str) -> Option<RemoteJobStatus> {
        // Check completed first
        {
            let completed = self.completed_jobs.read().await;
            if completed.contains_key(job_id) {
                return Some(RemoteJobStatus::Completed);
            }
        }

        // Check pending
        let pending = self.pending_jobs.read().await;
        pending.get(job_id).map(|j| j.handle.status.clone())
    }

    async fn wait_for_completion(
        &self,
        job_id: &str,
        timeout: Duration,
    ) -> Result<FogJobResult, FogError> {
        // Check if already completed
        {
            let completed = self.completed_jobs.read().await;
            if let Some(result) = completed.get(job_id) {
                return Ok(result.clone());
            }
        }

        // Set up completion channel
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending_jobs.write().await;
            if let Some(job) = pending.get_mut(job_id) {
                job.completion_tx = Some(tx);
            } else {
                return Err(FogError::NoSuitableNode {
                    reason: "Job not found".to_string(),
                });
            }
        }

        // Wait with timeout
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(FogError::NetworkError {
                error: "Channel closed".to_string(),
            }),
            Err(_) => Err(FogError::Timeout {
                elapsed_ms: timeout.as_millis() as u64,
            }),
        }
    }

    async fn cancel(&self, job_id: &str, reason: &str) -> Result<(), FogError> {
        let mut pending = self.pending_jobs.write().await;
        if let Some(job) = pending.remove(job_id) {
            let cancel = FogMessage::JobCancel {
                job_id: job_id.to_string(),
                reason: reason.to_string(),
            };
            self.send_message(&job.handle.executing_node, cancel)?;

            if let Some(tx) = job.completion_tx {
                let _ = tx.send(Err(FogError::JobRejected {
                    node: job.handle.executing_node,
                    reason: format!("Cancelled: {}", reason),
                }));
            }
            Ok(())
        } else {
            Err(FogError::NoSuitableNode {
                reason: "Job not found".to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    struct MockExecutor {
        can_handle: bool,
        load: f32,
    }

    #[async_trait]
    impl LocalExecutor for MockExecutor {
        async fn execute(&self, _spec: &FogJobSpec) -> Result<Vec<u8>, String> {
            Ok(vec![1, 2, 3, 4])
        }

        fn can_handle(&self, _spec: &FogJobSpec) -> bool {
            self.can_handle
        }

        async fn current_load(&self) -> f32 {
            self.load
        }
    }

    #[tokio::test]
    async fn test_local_routing() {
        let (mesh_tx, _mesh_rx) = mpsc::unbounded_channel();
        let resources = Arc::new(ResourceRegistry::new());
        let executor = Arc::new(MockExecutor {
            can_handle: true,
            load: 0.3,
        });

        let router = DefaultFogRouter::new(
            NodeId::new("local".to_string()),
            resources,
            mesh_tx,
            executor,
        );

        let spec = FogJobSpec {
            fog_options: FogOptions {
                allow_remote: false,
                ..Default::default()
            },
            ..Default::default()
        };

        let placement = router.route(&spec).await.unwrap();
        assert!(matches!(placement.decision, RoutingDecision::Local));
    }

    #[tokio::test]
    async fn test_remote_routing() {
        let (mesh_tx, _mesh_rx) = mpsc::unbounded_channel();
        let resources = Arc::new(ResourceRegistry::new());

        // Add a remote node with better score
        let mut remote_resources = crate::fog::resources::NodeResources::local(
            NodeId::new("remote".to_string()),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9999),
        );
        remote_resources.gpu_count = 2;
        remote_resources.gpus.push(crate::fog::resources::GpuInfo {
            index: 0,
            name: "Test GPU".to_string(),
            vendor: "Test".to_string(),
            vram_mb: 16384,
            available_vram_mb: 16000,
            compute_capability: "1.0".to_string(),
            features: vec![],
        });
        remote_resources
            .supported_operations
            .push("test_op".to_string());
        resources.register(remote_resources).await;

        let executor = Arc::new(MockExecutor {
            can_handle: true,
            load: 0.9, // High local load
        });

        let router = DefaultFogRouter::new(
            NodeId::new("local".to_string()),
            resources,
            mesh_tx,
            executor,
        );

        let spec = FogJobSpec {
            operation: "test_op".to_string(),
            fog_options: FogOptions {
                allow_remote: true,
                ..Default::default()
            },
            ..Default::default()
        };

        let placement = router.route(&spec).await.unwrap();
        assert!(matches!(placement.decision, RoutingDecision::Remote { .. }));
    }
}
