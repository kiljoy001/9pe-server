use serde::{Deserialize, Serialize};

/// Information about a discovered GPU device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub name: String,
    pub vendor: String,
    pub compute_units: u32,
    pub global_memory_size: u64,
    pub local_memory_size: u64,
    pub max_work_group_size: u32,
    pub is_gpu: bool,
    pub is_cpu: bool,
    pub supports_fp64: bool,
    pub supports_fp16: bool,
    pub local_index: usize,
    pub backend: String,
    pub total_vram_bytes: u64,
}
