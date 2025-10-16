use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::sycl::ffi::{sycl_discover_devices, SyclDeviceInfo};

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

/// Discover available GPU devices through the SYCL runtime and convert them to `GpuInfo`.
pub fn discover_gpus() -> Result<Vec<GpuInfo>> {
    // Allocate a reasonable upper bound for device discovery; SYCL will overwrite `count`
    // with the actual number of enumerated devices.
    let mut devices = vec![
        SyclDeviceInfo {
            name: [0; 256],
            vendor: [0; 128],
            compute_units: 0,
            global_memory_size: 0,
            local_memory_size: 0,
            max_work_group_size: 0,
            is_gpu: false,
            is_cpu: false,
            supports_fp64: false,
            supports_fp16: false,
        };
        16
    ];

    let mut count: usize = devices.len();
    unsafe {
        sycl_discover_devices(devices.as_mut_ptr(), &mut count as *mut usize)
            .to_result()
            .context("SYCL device discovery failed")?;
    }

    devices.truncate(count);

    let infos = devices
        .into_iter()
        .enumerate()
        .map(|(local_index, dev)| GpuInfo {
            name: dev.name_str().to_string(),
            vendor: dev.vendor_str().to_string(),
            compute_units: dev.compute_units,
            global_memory_size: dev.global_memory_size,
            local_memory_size: dev.local_memory_size,
            max_work_group_size: dev.max_work_group_size,
            is_gpu: dev.is_gpu,
            is_cpu: dev.is_cpu,
            supports_fp64: dev.supports_fp64,
            supports_fp16: dev.supports_fp16,
            local_index,
            backend: if dev.is_gpu {
                "gpu".to_string()
            } else if dev.is_cpu {
                "cpu".to_string()
            } else {
                "unknown".to_string()
            },
            total_vram_bytes: dev.global_memory_size,
        })
        .collect();

    Ok(infos)
}
