//! Work distribution system for GHOSTDAG consensus
//!
//! Manages distributed job execution across the network using consensus
//! to ensure fair work allocation and reliable result collection.

use anyhow::Result;
use serde::{Serialize, Deserialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use super::crypto::WorkProof;
use super::ghostdag::WorkResult;

/// Work distributor manages job scheduling and execution
pub struct WorkDistributor {
    node_id: String,
    pending_jobs: Arc<RwLock<HashMap<String, JobRequest>>>,
    active_jobs: Arc<RwLock<HashMap<String, ActiveJob>>>,
    completed_jobs: Arc<RwLock<HashMap<String, WorkResult>>>,
    node_capabilities: Arc<RwLock<HashMap<String, NodeCapabilities>>>,
    job_scheduler: JobScheduler,
    result_collector: ResultCollector,
}

impl WorkDistributor {
    pub fn new(node_id: String) -> Self {
        Self {
            node_id: node_id.clone(),
            pending_jobs: Arc::new(RwLock::new(HashMap::new())),
            active_jobs: Arc::new(RwLock::new(HashMap::new())),
            completed_jobs: Arc::new(RwLock::new(HashMap::new())),
            node_capabilities: Arc::new(RwLock::new(HashMap::new())),
            job_scheduler: JobScheduler::new(node_id.clone()),
            result_collector: ResultCollector::new(),
        }
    }

    /// Submit a new job for distributed execution
    pub async fn submit_job(&self, mut job: JobRequest) -> Result<String> {
        let job_id = format!("job_{}_{}", self.node_id, uuid::Uuid::new_v4());
        job.id = job_id.clone();
        job.submitted_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        // Validate job requirements
        self.validate_job_requirements(&job).await?;

        // Add to pending jobs
        {
            let mut pending = self.pending_jobs.write().await;
            pending.insert(job_id.clone(), job.clone());
        }

        // Schedule the job
        self.job_scheduler.schedule_job(job).await?;

        info!("Submitted job {} for distributed execution", job_id);
        Ok(job_id)
    }

    /// Get job result if completed
    pub async fn get_result(&self, job_id: &str) -> Result<Option<WorkResult>> {
        let completed = self.completed_jobs.read().await;
        Ok(completed.get(job_id).cloned())
    }

    /// Get job status
    pub async fn get_job_status(&self, job_id: &str) -> Option<JobStatus> {
        // Check completed jobs first
        {
            let completed = self.completed_jobs.read().await;
            if completed.contains_key(job_id) {
                return Some(JobStatus::Completed);
            }
        }

        // Check active jobs
        {
            let active = self.active_jobs.read().await;
            if let Some(active_job) = active.get(job_id) {
                return Some(active_job.status.clone());
            }
        }

        // Check pending jobs
        {
            let pending = self.pending_jobs.read().await;
            if pending.contains_key(job_id) {
                return Some(JobStatus::Pending);
            }
        }

        None
    }

    /// Register node capabilities
    pub async fn register_node_capabilities(&self, node_id: String, capabilities: NodeCapabilities) {
        let mut caps = self.node_capabilities.write().await;
        caps.insert(node_id, capabilities);
    }

    /// Process work assignment from the network
    pub async fn assign_work(&self, assignment: WorkAssignment) -> Result<()> {
        let active_job = ActiveJob {
            request: assignment.job,
            status: JobStatus::InProgress,
            assigned_nodes: assignment.assigned_nodes.clone(),
            started_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
            deadline: assignment.deadline,
        };

        {
            let mut active = self.active_jobs.write().await;
            active.insert(assignment.job_id.clone(), active_job);
        }

        // Remove from pending if it was there
        {
            let mut pending = self.pending_jobs.write().await;
            pending.remove(&assignment.job_id);
        }

        info!("Assigned work {} to {} nodes", assignment.job_id, assignment.assigned_nodes.len());
        Ok(())
    }

    /// Complete a job with results
    pub async fn complete_job(&self, job_id: String, result: WorkResult) -> Result<()> {
        // Move from active to completed
        {
            let mut active = self.active_jobs.write().await;
            active.remove(&job_id);
        }

        {
            let mut completed = self.completed_jobs.write().await;
            completed.insert(job_id.clone(), result);
        }

        info!("Completed job {}", job_id);
        Ok(())
    }

    async fn validate_job_requirements(&self, job: &JobRequest) -> Result<()> {
        // Check if we have enough capable nodes
        let capabilities = self.node_capabilities.read().await;
        let mut suitable_nodes = 0;

        for (_, node_caps) in capabilities.iter() {
            if self.node_meets_requirements(node_caps, &job.requirements) {
                suitable_nodes += 1;
            }
        }

        if suitable_nodes < job.requirements.min_nodes {
            anyhow::bail!(
                "Insufficient nodes: need {}, have {}",
                job.requirements.min_nodes,
                suitable_nodes
            );
        }

        Ok(())
    }

    fn node_meets_requirements(&self, caps: &NodeCapabilities, req: &JobRequirements) -> bool {
        // Check CPU requirements
        if let Some(min_cpu) = req.min_cpu_cores {
            if caps.cpu_cores < min_cpu {
                return false;
            }
        }

        // Check memory requirements
        if let Some(min_memory) = req.min_memory_gb {
            if caps.memory_gb < min_memory {
                return false;
            }
        }

        // Check GPU requirements
        if req.requires_gpu && !caps.has_gpu {
            return false;
        }

        // Check required capabilities
        for required_cap in &req.required_capabilities {
            if !caps.capabilities.contains(required_cap) {
                return false;
            }
        }

        true
    }
}

/// Job scheduler manages work allocation across nodes
pub struct JobScheduler {
    node_id: String,
    scheduling_queue: Arc<RwLock<VecDeque<JobRequest>>>,
    node_workload: Arc<RwLock<HashMap<String, f64>>>,
}

impl JobScheduler {
    pub fn new(node_id: String) -> Self {
        Self {
            node_id,
            scheduling_queue: Arc::new(RwLock::new(VecDeque::new())),
            node_workload: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn schedule_job(&self, job: JobRequest) -> Result<()> {
        let mut queue = self.scheduling_queue.write().await;

        // Insert job based on priority
        let insert_pos = queue.iter().position(|existing_job| {
            existing_job.priority < job.priority
        }).unwrap_or(queue.len());

        queue.insert(insert_pos, job);
        Ok(())
    }

    pub async fn get_next_job(&self) -> Option<JobRequest> {
        let mut queue = self.scheduling_queue.write().await;
        queue.pop_front()
    }

    pub async fn select_nodes_for_job(&self, job: &JobRequest, available_nodes: &[NodeInfo]) -> Vec<String> {
        let mut suitable_nodes: Vec<_> = available_nodes.iter()
            .filter(|node| self.node_meets_job_requirements(node, job))
            .collect();

        // Sort by workload (ascending)
        suitable_nodes.sort_by(|a, b| a.current_workload.partial_cmp(&b.current_workload).unwrap());

        // Select required number of nodes
        suitable_nodes.iter()
            .take(job.requirements.min_nodes as usize)
            .map(|node| node.node_id.clone())
            .collect()
    }

    fn node_meets_job_requirements(&self, node: &NodeInfo, job: &JobRequest) -> bool {
        // Check resource requirements
        if let Some(min_cpu) = job.requirements.min_cpu_cores {
            if node.capabilities.cpu_cores < min_cpu {
                return false;
            }
        }

        // Check workload capacity
        if node.current_workload > 0.8 {
            return false;
        }

        // Check required capabilities
        for req_cap in &job.requirements.required_capabilities {
            if !node.capabilities.capabilities.contains(req_cap) {
                return false;
            }
        }

        true
    }
}

/// Result collector manages work result aggregation
pub struct ResultCollector {
    partial_results: Arc<RwLock<HashMap<String, Vec<PartialResult>>>>,
}

impl Default for ResultCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl ResultCollector {
    pub fn new() -> Self {
        Self {
            partial_results: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn collect_partial_result(&self, job_id: String, result: PartialResult) -> Result<Option<WorkResult>> {
        let mut results = self.partial_results.write().await;
        let job_results = results.entry(job_id.clone()).or_insert_with(Vec::new);
        job_results.push(result);

        // Check if we have enough results to aggregate
        if self.can_aggregate_results(&job_id, job_results).await? {
            let aggregated = self.aggregate_results(job_id.clone(), job_results).await?;
            results.remove(&job_id);
            Ok(Some(aggregated))
        } else {
            Ok(None)
        }
    }

    async fn can_aggregate_results(&self, _job_id: &str, results: &[PartialResult]) -> Result<bool> {
        // Simple majority consensus - need at least 2/3 of expected results
        // In real implementation, this would be more sophisticated
        Ok(results.len() >= 2)
    }

    async fn aggregate_results(&self, job_id: String, results: &[PartialResult]) -> Result<WorkResult> {
        // Simple aggregation - in real implementation, this would be more sophisticated
        let mut aggregated_data = Vec::new();
        let mut total_execution_time = 0;

        for result in results {
            aggregated_data.extend_from_slice(&result.data);
            total_execution_time += result.execution_time_ms;
        }

        // Use the first result's proof as template
        let computation_proof = results[0].proof.clone();

        Ok(WorkResult {
            work_id: job_id,
            result_data: aggregated_data,
            computation_proof,
            executor_node: "aggregated".to_string(),
            execution_time_ms: total_execution_time / results.len() as u64,
        })
    }
}

// Data structures

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRequest {
    pub id: String,
    pub work_type: String,
    pub input_data: Vec<u8>,
    pub requirements: JobRequirements,
    pub priority: u32,
    pub timeout_seconds: u64,
    pub submitted_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRequirements {
    pub min_nodes: u32,
    pub min_cpu_cores: Option<u32>,
    pub min_memory_gb: Option<u32>,
    pub requires_gpu: bool,
    pub required_capabilities: Vec<String>,
    pub geographic_constraints: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCapabilities {
    pub cpu_cores: u32,
    pub memory_gb: u32,
    pub has_gpu: bool,
    pub gpu_memory_gb: Option<u32>,
    pub storage_gb: u32,
    pub capabilities: Vec<String>,
    pub geographic_region: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub node_id: String,
    pub capabilities: NodeCapabilities,
    pub current_workload: f64, // 0.0 to 1.0
    pub reputation_score: f64,
    pub last_seen: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkAssignment {
    pub job_id: String,
    pub job: JobRequest,
    pub assigned_nodes: Vec<String>,
    pub deadline: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveJob {
    pub request: JobRequest,
    pub status: JobStatus,
    pub assigned_nodes: Vec<String>,
    pub started_at: u64,
    pub deadline: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobStatus {
    Pending,
    Scheduled,
    InProgress,
    Completed,
    Failed(String),
    Timeout,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialResult {
    pub job_id: String,
    pub node_id: String,
    pub data: Vec<u8>,
    pub proof: WorkProof,
    pub execution_time_ms: u64,
    pub timestamp: u64,
}

/// Work capacity information for network planning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkCapacity {
    pub total_nodes: u32,
    pub available_cpu_cores: u32,
    pub available_memory_gb: u32,
    pub nodes_with_gpu: u32,
    pub average_workload: f64,
    pub capabilities: HashMap<String, u32>, // capability -> node count
}

impl NetworkCapacity {
    pub fn calculate_from_nodes(nodes: &[NodeInfo]) -> Self {
        let total_nodes = nodes.len() as u32;
        let mut available_cpu_cores = 0;
        let mut available_memory_gb = 0;
        let mut nodes_with_gpu = 0;
        let mut total_workload = 0.0;
        let mut capabilities = HashMap::new();

        for node in nodes {
            available_cpu_cores += node.capabilities.cpu_cores;
            available_memory_gb += node.capabilities.memory_gb;
            if node.capabilities.has_gpu {
                nodes_with_gpu += 1;
            }
            total_workload += node.current_workload;

            // Count capabilities
            for cap in &node.capabilities.capabilities {
                *capabilities.entry(cap.clone()).or_insert(0) += 1;
            }
        }

        let average_workload = if total_nodes > 0 {
            total_workload / total_nodes as f64
        } else {
            0.0
        };

        Self {
            total_nodes,
            available_cpu_cores,
            available_memory_gb,
            nodes_with_gpu,
            average_workload,
            capabilities,
        }
    }

    pub fn can_handle_job(&self, job: &JobRequest) -> bool {
        if self.total_nodes < job.requirements.min_nodes {
            return false;
        }

        if let Some(min_cpu) = job.requirements.min_cpu_cores {
            if self.available_cpu_cores < min_cpu * job.requirements.min_nodes {
                return false;
            }
        }

        if let Some(min_memory) = job.requirements.min_memory_gb {
            if self.available_memory_gb < min_memory * job.requirements.min_nodes {
                return false;
            }
        }

        if job.requirements.requires_gpu && self.nodes_with_gpu < job.requirements.min_nodes {
            return false;
        }

        // Check if we have enough nodes with required capabilities
        for req_cap in &job.requirements.required_capabilities {
            if self.capabilities.get(req_cap).unwrap_or(&0) < &job.requirements.min_nodes {
                return false;
            }
        }

        true
    }
}