//! Node resource advertisement and discovery
//!
//! Tracks and advertises compute resources for fog routing decisions.

use crate::identity::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Compute resources available on a node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeResources {
    /// Node identifier
    pub node_id: NodeId,
    /// Network address for job submission
    pub address: SocketAddr,
    /// Number of CPU cores
    pub cpu_cores: u32,
    /// CPU architecture (x86_64, aarch64, etc.)
    pub cpu_arch: String,
    /// Number of GPUs
    pub gpu_count: u32,
    /// GPU information per device
    pub gpus: Vec<GpuInfo>,
    /// Total system memory in MB
    pub memory_mb: u64,
    /// Available memory in MB
    pub available_memory_mb: u64,
    /// Total storage in GB
    pub storage_gb: u64,
    /// Available storage in GB
    pub available_storage_gb: u64,
    /// Network bandwidth estimate in Mbps
    pub network_bandwidth_mbps: u32,
    /// Current number of running jobs
    pub current_jobs: u32,
    /// Maximum concurrent jobs this node accepts
    pub max_concurrent_jobs: u32,
    /// Operations this node supports
    pub supported_operations: Vec<String>,
    /// WASM translators available
    pub available_translators: Vec<String>,
    /// Data IDs stored locally on this node
    pub local_data: Vec<String>,
    /// Timestamp of this resource report
    pub timestamp: u64,
    /// Node uptime in seconds
    pub uptime_secs: u64,
    /// Average job completion time in ms (rolling average)
    pub avg_job_time_ms: u64,
    /// Job success rate (0.0 - 1.0)
    pub success_rate: f32,
}

/// GPU device information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    /// Device index
    pub index: u32,
    /// Device name/model
    pub name: String,
    /// GPU vendor (Intel, NVIDIA, AMD)
    pub vendor: String,
    /// Total VRAM in MB
    pub vram_mb: u64,
    /// Available VRAM in MB
    pub available_vram_mb: u64,
    /// Compute capability or driver version
    pub compute_capability: String,
    /// Whether device supports specific features
    pub features: Vec<String>,
}

impl NodeResources {
    /// Create a new resource report for the local node
    pub fn local(node_id: NodeId, address: SocketAddr) -> Self {
        // Get CPU count - fallback to 4 if unavailable
        let cpu_count = std::thread::available_parallelism()
            .map(|p| p.get() as u32)
            .unwrap_or(4);

        Self {
            node_id,
            address,
            cpu_cores: cpu_count,
            cpu_arch: std::env::consts::ARCH.to_string(),
            gpu_count: 0,
            gpus: Vec::new(),
            memory_mb: 0,
            available_memory_mb: 0,
            storage_gb: 0,
            available_storage_gb: 0,
            network_bandwidth_mbps: 1000, // Default assumption
            current_jobs: 0,
            max_concurrent_jobs: cpu_count,
            supported_operations: vec![
                "vector_add".to_string(),
                "matrix_multiply".to_string(),
                "reduce_sum".to_string(),
            ],
            available_translators: Vec::new(),
            local_data: Vec::new(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            uptime_secs: 0,
            avg_job_time_ms: 0,
            success_rate: 1.0,
        }
    }

    /// Check if node can handle a job with given requirements
    pub fn can_handle(&self, requirements: &ResourceQuery) -> bool {
        // Check operation support
        if let Some(ref op) = requirements.operation {
            if !self.supported_operations.contains(op) {
                return false;
            }
        }

        // Check GPU requirements
        if requirements.requires_gpu && self.gpu_count == 0 {
            return false;
        }

        // Check VRAM requirements
        if let Some(vram) = requirements.min_vram_mb {
            let available: u64 = self.gpus.iter().map(|g| g.available_vram_mb).sum();
            if available < vram {
                return false;
            }
        }

        // Check memory requirements
        if let Some(mem) = requirements.min_memory_mb {
            if self.available_memory_mb < mem {
                return false;
            }
        }

        // Check job capacity
        if self.current_jobs >= self.max_concurrent_jobs {
            return false;
        }

        // Check data locality
        for data_id in &requirements.required_data {
            if !self.local_data.contains(data_id) {
                // Data not local - may still work but suboptimal
                // Don't reject, let router handle scoring
            }
        }

        true
    }

    /// Calculate a score for this node handling a job (higher is better)
    pub fn score(&self, requirements: &ResourceQuery) -> f64 {
        let mut score = 100.0;

        // Penalize high load
        let load_ratio = self.current_jobs as f64 / self.max_concurrent_jobs.max(1) as f64;
        score -= load_ratio * 30.0;

        // Bonus for GPU if needed
        if requirements.requires_gpu && self.gpu_count > 0 {
            score += 20.0;
            // Extra bonus for more available VRAM
            let vram: u64 = self.gpus.iter().map(|g| g.available_vram_mb).sum();
            score += (vram as f64 / 1024.0).min(20.0); // Up to 20 points for VRAM
        }

        // Bonus for data locality
        let local_data_count = requirements
            .required_data
            .iter()
            .filter(|d| self.local_data.contains(d))
            .count();
        score += local_data_count as f64 * 15.0;

        // Factor in historical performance
        score *= self.success_rate as f64;

        // Slight penalty for high average job time
        if self.avg_job_time_ms > 0 {
            score -= (self.avg_job_time_ms as f64 / 1000.0).min(10.0);
        }

        score.max(0.0)
    }

    /// Check if this resource report is stale
    pub fn is_stale(&self, max_age_secs: u64) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now.saturating_sub(self.timestamp) > max_age_secs
    }
}

/// Query for finding suitable nodes
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceQuery {
    /// Required operation type
    pub operation: Option<String>,
    /// Whether GPU is required
    pub requires_gpu: bool,
    /// Minimum VRAM in MB
    pub min_vram_mb: Option<u64>,
    /// Minimum system memory in MB
    pub min_memory_mb: Option<u64>,
    /// Required data locality
    pub required_data: Vec<String>,
    /// Maximum acceptable load ratio (0.0 - 1.0)
    pub max_load_ratio: Option<f32>,
    /// Required translator
    pub required_translator: Option<String>,
}

/// Resource advertisement broadcast
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceAdvertisement {
    /// The resources being advertised
    pub resources: NodeResources,
    /// Signature from the node's sovereign identity
    pub signature: Vec<u8>,
    /// TTL for this advertisement in seconds
    pub ttl_secs: u64,
}

/// Tracks resources across the fog network
pub struct ResourceRegistry {
    /// Known node resources indexed by node ID
    nodes: Arc<RwLock<HashMap<String, (NodeResources, Instant)>>>,
    /// Local node's resources
    local: Arc<RwLock<Option<NodeResources>>>,
    /// Maximum age for resource entries before considered stale
    max_age: Duration,
    /// Resource update interval
    update_interval: Duration,
}

impl ResourceRegistry {
    /// Create a new resource registry
    pub fn new() -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
            local: Arc::new(RwLock::new(None)),
            max_age: Duration::from_secs(120), // 2 minutes
            update_interval: Duration::from_secs(30),
        }
    }

    /// Set local node resources
    pub async fn set_local(&self, resources: NodeResources) {
        let node_id = resources.node_id.as_str().to_string();
        *self.local.write().await = Some(resources.clone());

        // Also add to nodes map
        let mut nodes = self.nodes.write().await;
        nodes.insert(node_id, (resources, Instant::now()));
    }

    /// Update local resource metrics (jobs, memory, etc.)
    pub async fn update_local_metrics(&self, current_jobs: u32, available_memory_mb: u64) {
        let mut local = self.local.write().await;
        if let Some(ref mut resources) = *local {
            resources.current_jobs = current_jobs;
            resources.available_memory_mb = available_memory_mb;
            resources.timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
        }
    }

    /// Register a remote node's resources
    pub async fn register(&self, resources: NodeResources) {
        let node_id = resources.node_id.as_str().to_string();
        debug!("Registering resources for node {}", node_id);

        let mut nodes = self.nodes.write().await;
        nodes.insert(node_id, (resources, Instant::now()));
    }

    /// Get resources for a specific node
    pub async fn get(&self, node_id: &str) -> Option<NodeResources> {
        let nodes = self.nodes.read().await;
        nodes.get(node_id).map(|(r, _)| r.clone())
    }

    /// Find nodes matching a query, sorted by score (best first)
    pub async fn find_nodes(&self, query: &ResourceQuery) -> Vec<NodeResources> {
        let nodes = self.nodes.read().await;
        let mut candidates: Vec<_> = nodes
            .values()
            .filter(|(r, received)| {
                // Filter stale entries
                received.elapsed() < self.max_age && r.can_handle(query)
            })
            .map(|(r, _)| r.clone())
            .collect();

        // Sort by score descending
        candidates.sort_by(|a, b| {
            let score_a = a.score(query);
            let score_b = b.score(query);
            score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
        });

        candidates
    }

    /// Get the best node for a job
    pub async fn find_best_node(&self, query: &ResourceQuery) -> Option<NodeResources> {
        self.find_nodes(query).await.into_iter().next()
    }

    /// Remove stale entries
    pub async fn cleanup_stale(&self) {
        let mut nodes = self.nodes.write().await;
        let before = nodes.len();
        nodes.retain(|_, (_, received)| received.elapsed() < self.max_age);
        let removed = before - nodes.len();
        if removed > 0 {
            info!("Cleaned up {} stale resource entries", removed);
        }
    }

    /// Get all known nodes
    pub async fn all_nodes(&self) -> Vec<NodeResources> {
        let nodes = self.nodes.read().await;
        nodes.values().map(|(r, _)| r.clone()).collect()
    }

    /// Get local node resources
    pub async fn get_local(&self) -> Option<NodeResources> {
        self.local.read().await.clone()
    }

    /// Get count of known nodes
    pub async fn node_count(&self) -> usize {
        self.nodes.read().await.len()
    }

    /// Start periodic cleanup task
    pub fn start_cleanup_task(self: &Arc<Self>) {
        let registry = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                registry.cleanup_stale().await;
            }
        });
    }
}

impl Default for ResourceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn test_node(id: &str, gpu_count: u32, current_jobs: u32) -> NodeResources {
        let mut resources = NodeResources::local(
            NodeId::new(id.to_string()),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9999),
        );
        resources.gpu_count = gpu_count;
        resources.current_jobs = current_jobs;
        if gpu_count > 0 {
            resources.gpus.push(GpuInfo {
                index: 0,
                name: "Test GPU".to_string(),
                vendor: "Test".to_string(),
                vram_mb: 8192,
                available_vram_mb: 6000,
                compute_capability: "1.0".to_string(),
                features: vec![],
            });
        }
        resources
    }

    #[tokio::test]
    async fn test_resource_scoring() {
        let node_with_gpu = test_node("node1", 1, 0);
        let node_without_gpu = test_node("node2", 0, 0);
        let node_loaded = test_node("node3", 1, 4);

        let query = ResourceQuery {
            requires_gpu: true,
            ..Default::default()
        };

        let score1 = node_with_gpu.score(&query);
        let score2 = node_without_gpu.score(&query);
        let score3 = node_loaded.score(&query);

        // GPU node should score higher
        assert!(score1 > score2);
        // Unloaded node should score higher than loaded
        assert!(score1 > score3);
    }

    #[tokio::test]
    async fn test_find_best_node() {
        let registry = ResourceRegistry::new();

        registry.register(test_node("node1", 0, 0)).await;
        registry.register(test_node("node2", 1, 0)).await;
        registry.register(test_node("node3", 1, 3)).await;

        let query = ResourceQuery {
            requires_gpu: true,
            ..Default::default()
        };

        let best = registry.find_best_node(&query).await;
        assert!(best.is_some());
        // node2 should be best (has GPU, not loaded)
        assert_eq!(best.unwrap().node_id.as_str(), "node2");
    }
}
