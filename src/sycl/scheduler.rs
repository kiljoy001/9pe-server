//! SYCL Job Scheduler
//!
//! Provides priority-based job scheduling, timeout enforcement, and job lifecycle management
//! for GPU compute operations.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock, Notify};
use tokio::time::timeout;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Job priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobPriority {
    /// Critical jobs - system operations, should never be preempted
    Critical = 0,
    /// High priority - interactive user operations
    High = 1,
    /// Normal priority - standard compute jobs
    Normal = 2,
    /// Low priority - batch operations, background tasks
    Low = 3,
    /// Idle - only runs when no other jobs are pending
    Idle = 4,
}

impl Default for JobPriority {
    fn default() -> Self {
        JobPriority::Normal
    }
}

impl From<u8> for JobPriority {
    fn from(value: u8) -> Self {
        match value {
            0 => JobPriority::Critical,
            1 => JobPriority::High,
            2 => JobPriority::Normal,
            3 => JobPriority::Low,
            _ => JobPriority::Idle,
        }
    }
}

/// Job execution status
#[derive(Debug, Clone)]
pub enum ScheduledJobStatus {
    /// Job is queued waiting for execution
    Queued,
    /// Job is currently running
    Running {
        started_at: Instant,
        device_id: String,
    },
    /// Job completed successfully
    Completed {
        result: Vec<u8>,
        duration: Duration,
    },
    /// Job failed with error
    Failed {
        error: String,
        duration: Option<Duration>,
    },
    /// Job was cancelled
    Cancelled {
        reason: String,
    },
    /// Job timed out
    TimedOut {
        elapsed: Duration,
    },
}

/// A scheduled compute job with priority and metadata
#[derive(Debug, Clone)]
pub struct ScheduledJob {
    /// Unique job identifier
    pub id: String,
    /// Job priority level
    pub priority: JobPriority,
    /// Job type (sycl, wasm, etc.)
    pub job_type: String,
    /// Operation to perform
    pub operation: String,
    /// Input data
    pub input: Vec<u8>,
    /// Current status
    pub status: ScheduledJobStatus,
    /// When the job was submitted
    pub submitted_at: Instant,
    /// Maximum execution time before timeout
    pub timeout: Duration,
    /// Preferred device (if any)
    pub device_hint: Option<String>,
    /// Required VRAM in bytes
    pub required_vram: u64,
    /// Sequence number for FIFO ordering within same priority
    pub sequence: u64,
    /// Optional callback channel for completion notification
    pub completion_notify: Option<Arc<Notify>>,
}

/// Priority queue entry that compares jobs by priority then sequence
struct PriorityEntry {
    job: ScheduledJob,
}

impl PartialEq for PriorityEntry {
    fn eq(&self, other: &Self) -> bool {
        self.job.priority == other.job.priority && self.job.sequence == other.job.sequence
    }
}

impl Eq for PriorityEntry {}

impl PartialOrd for PriorityEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PriorityEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Lower priority value = higher priority (Critical=0 is highest)
        // For same priority, lower sequence = submitted earlier = higher priority
        match (other.job.priority as u8).cmp(&(self.job.priority as u8)) {
            Ordering::Equal => other.job.sequence.cmp(&self.job.sequence),
            other => other,
        }
    }
}

/// Job submission request
#[derive(Debug, Clone)]
pub struct JobSubmitRequest {
    pub job_type: String,
    pub operation: String,
    pub input: Vec<u8>,
    pub priority: JobPriority,
    pub timeout: Duration,
    pub device_hint: Option<String>,
    pub required_vram: u64,
}

impl Default for JobSubmitRequest {
    fn default() -> Self {
        Self {
            job_type: "sycl".to_string(),
            operation: String::new(),
            input: Vec::new(),
            priority: JobPriority::Normal,
            timeout: Duration::from_secs(300), // 5 minute default timeout
            device_hint: None,
            required_vram: 0,
        }
    }
}

/// Scheduler statistics
#[derive(Debug, Clone, Default)]
pub struct SchedulerStats {
    pub jobs_submitted: u64,
    pub jobs_completed: u64,
    pub jobs_failed: u64,
    pub jobs_cancelled: u64,
    pub jobs_timed_out: u64,
    pub jobs_reassigned: u64,
    pub jobs_queued: usize,
    pub jobs_running: usize,
    pub avg_wait_time_ms: f64,
    pub avg_execution_time_ms: f64,
}

/// Node heartbeat tracking for fault tolerance
#[derive(Debug, Clone)]
pub struct NodeHeartbeat {
    pub node_id: String,
    pub last_seen: Instant,
    pub jobs_assigned: Vec<String>,
}

/// Checkpoint data for job recovery
#[derive(Debug, Clone)]
pub struct JobCheckpointData {
    pub job_id: String,
    pub checkpoint_id: String,
    pub timestamp: Instant,
    pub progress_percent: u8,
    pub state: Vec<u8>,
    pub intermediate_results: Option<Vec<u8>>,
}

/// Job scheduler with priority queue and fault tolerance
pub struct JobScheduler {
    /// Priority queue for pending jobs
    queue: RwLock<BinaryHeap<PriorityEntry>>,
    /// Currently running jobs
    running: RwLock<HashMap<String, ScheduledJob>>,
    /// Completed/failed job history (limited size)
    history: RwLock<Vec<ScheduledJob>>,
    /// Maximum history size
    max_history: usize,
    /// Sequence counter for FIFO ordering
    sequence: AtomicU64,
    /// Channel for job execution requests
    execution_tx: mpsc::UnboundedSender<ScheduledJob>,
    /// Notify when new jobs are available
    job_available: Arc<Notify>,
    /// Statistics
    stats: RwLock<SchedulerStats>,
    /// Shutdown flag
    shutdown: Arc<RwLock<bool>>,
    /// Node heartbeat tracking for fault tolerance
    node_heartbeats: RwLock<HashMap<String, NodeHeartbeat>>,
    /// Job checkpoints for recovery
    checkpoints: RwLock<HashMap<String, JobCheckpointData>>,
    /// Heartbeat timeout threshold
    heartbeat_timeout: Duration,
    /// Jobs awaiting reassignment
    reassignment_queue: RwLock<Vec<ScheduledJob>>,
}

impl JobScheduler {
    /// Create a new job scheduler
    pub fn new() -> (Arc<Self>, mpsc::UnboundedReceiver<ScheduledJob>) {
        Self::with_heartbeat_timeout(Duration::from_secs(60))
    }

    /// Create a new job scheduler with custom heartbeat timeout
    pub fn with_heartbeat_timeout(
        heartbeat_timeout: Duration,
    ) -> (Arc<Self>, mpsc::UnboundedReceiver<ScheduledJob>) {
        let (execution_tx, execution_rx) = mpsc::unbounded_channel();

        let scheduler = Arc::new(Self {
            queue: RwLock::new(BinaryHeap::new()),
            running: RwLock::new(HashMap::new()),
            history: RwLock::new(Vec::new()),
            max_history: 1000,
            sequence: AtomicU64::new(0),
            execution_tx,
            job_available: Arc::new(Notify::new()),
            stats: RwLock::new(SchedulerStats::default()),
            shutdown: Arc::new(RwLock::new(false)),
            node_heartbeats: RwLock::new(HashMap::new()),
            checkpoints: RwLock::new(HashMap::new()),
            heartbeat_timeout,
            reassignment_queue: RwLock::new(Vec::new()),
        });

        (scheduler, execution_rx)
    }

    /// Submit a new job to the scheduler
    pub async fn submit(&self, request: JobSubmitRequest) -> Result<String, String> {
        let job_id = Uuid::new_v4().to_string();
        let sequence = self.sequence.fetch_add(1, AtomicOrdering::SeqCst);

        let job = ScheduledJob {
            id: job_id.clone(),
            priority: request.priority,
            job_type: request.job_type,
            operation: request.operation,
            input: request.input,
            status: ScheduledJobStatus::Queued,
            submitted_at: Instant::now(),
            timeout: request.timeout,
            device_hint: request.device_hint,
            required_vram: request.required_vram,
            sequence,
            completion_notify: None,
        };

        {
            let mut queue = self.queue.write().await;
            queue.push(PriorityEntry { job });
        }

        {
            let mut stats = self.stats.write().await;
            stats.jobs_submitted += 1;
            stats.jobs_queued += 1;
        }

        // Notify that a job is available
        self.job_available.notify_one();

        debug!("Job {} submitted with priority {:?}", job_id, request.priority);
        Ok(job_id)
    }

    /// Submit a job and wait for completion
    pub async fn submit_and_wait(&self, request: JobSubmitRequest) -> Result<Vec<u8>, String> {
        let notify = Arc::new(Notify::new());
        let job_id = Uuid::new_v4().to_string();
        let sequence = self.sequence.fetch_add(1, AtomicOrdering::SeqCst);
        let job_timeout = request.timeout;

        let job = ScheduledJob {
            id: job_id.clone(),
            priority: request.priority,
            job_type: request.job_type,
            operation: request.operation,
            input: request.input,
            status: ScheduledJobStatus::Queued,
            submitted_at: Instant::now(),
            timeout: request.timeout,
            device_hint: request.device_hint,
            required_vram: request.required_vram,
            sequence,
            completion_notify: Some(notify.clone()),
        };

        {
            let mut queue = self.queue.write().await;
            queue.push(PriorityEntry { job });
        }

        {
            let mut stats = self.stats.write().await;
            stats.jobs_submitted += 1;
            stats.jobs_queued += 1;
        }

        self.job_available.notify_one();

        // Wait for completion with timeout
        match timeout(job_timeout, notify.notified()).await {
            Ok(()) => {
                // Check job status in history
                let history = self.history.read().await;
                if let Some(job) = history.iter().find(|j| j.id == job_id) {
                    match &job.status {
                        ScheduledJobStatus::Completed { result, .. } => Ok(result.clone()),
                        ScheduledJobStatus::Failed { error, .. } => Err(error.clone()),
                        ScheduledJobStatus::Cancelled { reason } => Err(format!("Cancelled: {}", reason)),
                        ScheduledJobStatus::TimedOut { elapsed } => {
                            Err(format!("Timed out after {:?}", elapsed))
                        }
                        _ => Err("Job completed with unexpected status".to_string()),
                    }
                } else {
                    Err("Job completed but not found in history".to_string())
                }
            }
            Err(_) => {
                // Timeout - try to cancel the job
                let _ = self.cancel(&job_id, "Client timeout").await;
                Err(format!("Job timed out after {:?}", job_timeout))
            }
        }
    }

    /// Get the next job to execute (called by worker)
    pub async fn next_job(&self) -> Option<ScheduledJob> {
        loop {
            // Check shutdown
            if *self.shutdown.read().await {
                return None;
            }

            // Try to get a job from the queue
            {
                let mut queue = self.queue.write().await;
                if let Some(entry) = queue.pop() {
                    let mut job = entry.job;
                    job.status = ScheduledJobStatus::Running {
                        started_at: Instant::now(),
                        device_id: "pending".to_string(),
                    };

                    // Move to running map
                    {
                        let mut running = self.running.write().await;
                        running.insert(job.id.clone(), job.clone());
                    }

                    {
                        let mut stats = self.stats.write().await;
                        stats.jobs_queued = stats.jobs_queued.saturating_sub(1);
                        stats.jobs_running += 1;
                    }

                    return Some(job);
                }
            }

            // Wait for a job to be available
            self.job_available.notified().await;
        }
    }

    /// Mark a job as completed
    pub async fn complete(&self, job_id: &str, result: Vec<u8>) {
        let job = {
            let mut running = self.running.write().await;
            running.remove(job_id)
        };

        if let Some(mut job) = job {
            let duration = job.submitted_at.elapsed();
            job.status = ScheduledJobStatus::Completed {
                result,
                duration,
            };

            // Notify waiters
            if let Some(notify) = &job.completion_notify {
                notify.notify_one();
            }

            // Add to history
            self.add_to_history(job).await;

            {
                let mut stats = self.stats.write().await;
                stats.jobs_completed += 1;
                stats.jobs_running = stats.jobs_running.saturating_sub(1);

                // Update average execution time
                let new_avg = (stats.avg_execution_time_ms * (stats.jobs_completed - 1) as f64
                    + duration.as_secs_f64() * 1000.0)
                    / stats.jobs_completed as f64;
                stats.avg_execution_time_ms = new_avg;
            }

            debug!("Job {} completed in {:?}", job_id, duration);
        }
    }

    /// Mark a job as failed
    pub async fn fail(&self, job_id: &str, error: String) {
        let job = {
            let mut running = self.running.write().await;
            running.remove(job_id)
        };

        if let Some(mut job) = job {
            let duration = Some(job.submitted_at.elapsed());
            job.status = ScheduledJobStatus::Failed { error, duration };

            if let Some(notify) = &job.completion_notify {
                notify.notify_one();
            }

            self.add_to_history(job).await;

            {
                let mut stats = self.stats.write().await;
                stats.jobs_failed += 1;
                stats.jobs_running = stats.jobs_running.saturating_sub(1);
            }

            warn!("Job {} failed", job_id);
        }
    }

    /// Cancel a job
    pub async fn cancel(&self, job_id: &str, reason: &str) -> Result<(), String> {
        // Try to remove from queue first
        {
            let mut queue = self.queue.write().await;
            let jobs: Vec<_> = std::mem::take(&mut *queue).into_vec();
            let mut found = false;

            for entry in jobs {
                if entry.job.id == job_id {
                    found = true;
                    let mut job = entry.job;
                    job.status = ScheduledJobStatus::Cancelled {
                        reason: reason.to_string(),
                    };

                    if let Some(notify) = &job.completion_notify {
                        notify.notify_one();
                    }

                    self.add_to_history(job).await;

                    {
                        let mut stats = self.stats.write().await;
                        stats.jobs_cancelled += 1;
                        stats.jobs_queued = stats.jobs_queued.saturating_sub(1);
                    }
                } else {
                    queue.push(entry);
                }
            }

            if found {
                info!("Job {} cancelled from queue: {}", job_id, reason);
                return Ok(());
            }
        }

        // Try to cancel running job (mark for cancellation - actual stop depends on executor)
        {
            let mut running = self.running.write().await;
            if let Some(mut job) = running.remove(job_id) {
                job.status = ScheduledJobStatus::Cancelled {
                    reason: reason.to_string(),
                };

                if let Some(notify) = &job.completion_notify {
                    notify.notify_one();
                }

                self.add_to_history(job).await;

                {
                    let mut stats = self.stats.write().await;
                    stats.jobs_cancelled += 1;
                    stats.jobs_running = stats.jobs_running.saturating_sub(1);
                }

                info!("Job {} cancelled while running: {}", job_id, reason);
                return Ok(());
            }
        }

        Err(format!("Job {} not found", job_id))
    }

    /// Get job status
    pub async fn get_status(&self, job_id: &str) -> Option<ScheduledJobStatus> {
        // Check running
        {
            let running = self.running.read().await;
            if let Some(job) = running.get(job_id) {
                return Some(job.status.clone());
            }
        }

        // Check queue
        {
            let queue = self.queue.read().await;
            for entry in queue.iter() {
                if entry.job.id == job_id {
                    return Some(entry.job.status.clone());
                }
            }
        }

        // Check history
        {
            let history = self.history.read().await;
            if let Some(job) = history.iter().find(|j| j.id == job_id) {
                return Some(job.status.clone());
            }
        }

        None
    }

    /// Get scheduler statistics
    pub async fn get_stats(&self) -> SchedulerStats {
        self.stats.read().await.clone()
    }

    /// List all queued jobs
    pub async fn list_queued(&self) -> Vec<ScheduledJob> {
        let queue = self.queue.read().await;
        queue.iter().map(|e| e.job.clone()).collect()
    }

    /// List all running jobs
    pub async fn list_running(&self) -> Vec<ScheduledJob> {
        let running = self.running.read().await;
        running.values().cloned().collect()
    }

    /// Get job history
    pub async fn get_history(&self, limit: usize) -> Vec<ScheduledJob> {
        let history = self.history.read().await;
        history.iter().rev().take(limit).cloned().collect()
    }

    /// Check for timed out jobs and mark them
    pub async fn check_timeouts(&self) {
        let mut timed_out = Vec::new();

        {
            let running = self.running.read().await;
            for (id, job) in running.iter() {
                if job.submitted_at.elapsed() > job.timeout {
                    timed_out.push(id.clone());
                }
            }
        }

        for job_id in timed_out {
            let job = {
                let mut running = self.running.write().await;
                running.remove(&job_id)
            };

            if let Some(mut job) = job {
                let elapsed = job.submitted_at.elapsed();
                job.status = ScheduledJobStatus::TimedOut { elapsed };

                if let Some(notify) = &job.completion_notify {
                    notify.notify_one();
                }

                self.add_to_history(job).await;

                {
                    let mut stats = self.stats.write().await;
                    stats.jobs_timed_out += 1;
                    stats.jobs_running = stats.jobs_running.saturating_sub(1);
                }

                warn!("Job {} timed out after {:?}", job_id, elapsed);
            }
        }
    }

    /// Shutdown the scheduler
    pub async fn shutdown(&self) {
        *self.shutdown.write().await = true;
        self.job_available.notify_waiters();
    }

    /// Add a job to history, maintaining max size
    async fn add_to_history(&self, job: ScheduledJob) {
        let mut history = self.history.write().await;
        history.push(job);

        // Trim if over max size
        while history.len() > self.max_history {
            history.remove(0);
        }
    }

    // === Fault Tolerance Methods ===

    /// Record a heartbeat from a node
    pub async fn record_heartbeat(&self, node_id: &str, job_ids: Vec<String>) {
        let mut heartbeats = self.node_heartbeats.write().await;
        heartbeats.insert(
            node_id.to_string(),
            NodeHeartbeat {
                node_id: node_id.to_string(),
                last_seen: Instant::now(),
                jobs_assigned: job_ids,
            },
        );
    }

    /// Check for failed nodes and reassign their jobs
    pub async fn check_node_health(&self) -> Vec<String> {
        let mut failed_nodes = Vec::new();
        let mut jobs_to_reassign = Vec::new();

        {
            let heartbeats = self.node_heartbeats.read().await;
            let running = self.running.read().await;

            for (node_id, heartbeat) in heartbeats.iter() {
                if heartbeat.last_seen.elapsed() > self.heartbeat_timeout {
                    failed_nodes.push(node_id.clone());

                    // Find jobs assigned to this node
                    for job_id in &heartbeat.jobs_assigned {
                        if let Some(job) = running.get(job_id) {
                            jobs_to_reassign.push(job.clone());
                        }
                    }
                }
            }
        }

        // Process failed nodes
        for node_id in &failed_nodes {
            warn!("Node {} failed heartbeat check, reassigning jobs", node_id);

            // Remove from heartbeats
            let mut heartbeats = self.node_heartbeats.write().await;
            heartbeats.remove(node_id);
        }

        // Reassign jobs
        for job in jobs_to_reassign {
            self.reassign_job(&job.id).await;
        }

        failed_nodes
    }

    /// Reassign a job from a failed node
    pub async fn reassign_job(&self, job_id: &str) -> bool {
        let job = {
            let mut running = self.running.write().await;
            running.remove(job_id)
        };

        if let Some(mut job) = job {
            info!("Reassigning job {} due to node failure", job_id);

            // Check if we have a checkpoint to resume from
            let checkpoint = {
                let checkpoints = self.checkpoints.read().await;
                checkpoints.get(job_id).cloned()
            };

            if let Some(cp) = checkpoint {
                // Resume from checkpoint
                info!(
                    "Resuming job {} from checkpoint at {}%",
                    job_id, cp.progress_percent
                );
                // Store intermediate results in job if available
                if let Some(intermediate) = cp.intermediate_results {
                    // Could be used by executor to resume
                    debug!("Checkpoint has {} bytes of intermediate data", intermediate.len());
                }
            }

            // Reset job status and requeue with higher priority
            job.status = ScheduledJobStatus::Queued;
            // Boost priority for reassigned jobs
            job.priority = match job.priority {
                JobPriority::Idle => JobPriority::Low,
                JobPriority::Low => JobPriority::Normal,
                JobPriority::Normal => JobPriority::High,
                JobPriority::High | JobPriority::Critical => JobPriority::Critical,
            };
            job.sequence = self.sequence.fetch_add(1, AtomicOrdering::SeqCst);

            // Re-queue the job
            {
                let mut queue = self.queue.write().await;
                queue.push(PriorityEntry { job });
            }

            {
                let mut stats = self.stats.write().await;
                stats.jobs_reassigned += 1;
                stats.jobs_running = stats.jobs_running.saturating_sub(1);
                stats.jobs_queued += 1;
            }

            self.job_available.notify_one();
            true
        } else {
            false
        }
    }

    /// Save a checkpoint for a running job
    pub async fn save_checkpoint(&self, checkpoint: JobCheckpointData) {
        debug!(
            "Saving checkpoint for job {} at {}%",
            checkpoint.job_id, checkpoint.progress_percent
        );

        let mut checkpoints = self.checkpoints.write().await;
        checkpoints.insert(checkpoint.job_id.clone(), checkpoint);
    }

    /// Get checkpoint for a job
    pub async fn get_checkpoint(&self, job_id: &str) -> Option<JobCheckpointData> {
        let checkpoints = self.checkpoints.read().await;
        checkpoints.get(job_id).cloned()
    }

    /// Clear checkpoint after job completion
    pub async fn clear_checkpoint(&self, job_id: &str) {
        let mut checkpoints = self.checkpoints.write().await;
        checkpoints.remove(job_id);
    }

    /// Assign a job to a specific node for tracking
    pub async fn assign_job_to_node(&self, job_id: &str, node_id: &str) {
        let mut heartbeats = self.node_heartbeats.write().await;
        if let Some(heartbeat) = heartbeats.get_mut(node_id) {
            if !heartbeat.jobs_assigned.contains(&job_id.to_string()) {
                heartbeat.jobs_assigned.push(job_id.to_string());
            }
        } else {
            // Create new heartbeat entry for the node
            heartbeats.insert(
                node_id.to_string(),
                NodeHeartbeat {
                    node_id: node_id.to_string(),
                    last_seen: Instant::now(),
                    jobs_assigned: vec![job_id.to_string()],
                },
            );
        }
    }

    /// Remove job assignment from a node
    pub async fn unassign_job_from_node(&self, job_id: &str, node_id: &str) {
        let mut heartbeats = self.node_heartbeats.write().await;
        if let Some(heartbeat) = heartbeats.get_mut(node_id) {
            heartbeat.jobs_assigned.retain(|id| id != job_id);
        }
    }

    /// Start periodic health check task
    pub fn start_health_check_task(self: &Arc<Self>, interval: Duration) {
        let scheduler = Arc::clone(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                if *scheduler.shutdown.read().await {
                    break;
                }
                let failed = scheduler.check_node_health().await;
                if !failed.is_empty() {
                    warn!("Health check found {} failed nodes", failed.len());
                }
                scheduler.check_timeouts().await;
            }
        });
    }

    /// Get node health status
    pub async fn get_node_health(&self) -> HashMap<String, (Duration, usize)> {
        let heartbeats = self.node_heartbeats.read().await;
        heartbeats
            .iter()
            .map(|(id, hb)| {
                (
                    id.clone(),
                    (hb.last_seen.elapsed(), hb.jobs_assigned.len()),
                )
            })
            .collect()
    }
}

// Note: JobScheduler doesn't implement Default because it returns a tuple
// Use JobScheduler::new() directly to get (Arc<JobScheduler>, Receiver)

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_priority_ordering() {
        let (scheduler, _rx) = JobScheduler::new();

        // Submit jobs in reverse priority order
        scheduler
            .submit(JobSubmitRequest {
                priority: JobPriority::Low,
                operation: "low".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();

        scheduler
            .submit(JobSubmitRequest {
                priority: JobPriority::High,
                operation: "high".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();

        scheduler
            .submit(JobSubmitRequest {
                priority: JobPriority::Critical,
                operation: "critical".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();

        // Should get critical first
        let job = scheduler.next_job().await.unwrap();
        assert_eq!(job.operation, "critical");

        // Then high
        scheduler.complete(&job.id, vec![]).await;
        let job = scheduler.next_job().await.unwrap();
        assert_eq!(job.operation, "high");

        // Then low
        scheduler.complete(&job.id, vec![]).await;
        let job = scheduler.next_job().await.unwrap();
        assert_eq!(job.operation, "low");
    }

    #[tokio::test]
    async fn test_fifo_within_priority() {
        let (scheduler, _rx) = JobScheduler::new();

        // Submit multiple jobs with same priority
        scheduler
            .submit(JobSubmitRequest {
                priority: JobPriority::Normal,
                operation: "first".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();

        scheduler
            .submit(JobSubmitRequest {
                priority: JobPriority::Normal,
                operation: "second".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();

        scheduler
            .submit(JobSubmitRequest {
                priority: JobPriority::Normal,
                operation: "third".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();

        // Should get them in FIFO order
        let job = scheduler.next_job().await.unwrap();
        assert_eq!(job.operation, "first");

        scheduler.complete(&job.id, vec![]).await;
        let job = scheduler.next_job().await.unwrap();
        assert_eq!(job.operation, "second");

        scheduler.complete(&job.id, vec![]).await;
        let job = scheduler.next_job().await.unwrap();
        assert_eq!(job.operation, "third");
    }

    #[tokio::test]
    async fn test_job_cancellation() {
        let (scheduler, _rx) = JobScheduler::new();

        let job_id = scheduler
            .submit(JobSubmitRequest {
                operation: "cancel_me".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();

        // Cancel the job
        scheduler.cancel(&job_id, "test").await.unwrap();

        // Should be in history as cancelled
        let status = scheduler.get_status(&job_id).await.unwrap();
        assert!(matches!(status, ScheduledJobStatus::Cancelled { .. }));
    }

    #[tokio::test]
    async fn test_stats_tracking() {
        let (scheduler, _rx) = JobScheduler::new();

        // Submit and complete a job
        let job_id = scheduler
            .submit(JobSubmitRequest::default())
            .await
            .unwrap();

        let stats = scheduler.get_stats().await;
        assert_eq!(stats.jobs_submitted, 1);
        assert_eq!(stats.jobs_queued, 1);

        let job = scheduler.next_job().await.unwrap();
        let stats = scheduler.get_stats().await;
        assert_eq!(stats.jobs_running, 1);
        assert_eq!(stats.jobs_queued, 0);

        scheduler.complete(&job.id, vec![1, 2, 3]).await;
        let stats = scheduler.get_stats().await;
        assert_eq!(stats.jobs_completed, 1);
        assert_eq!(stats.jobs_running, 0);
    }
}
