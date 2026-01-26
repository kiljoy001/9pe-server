use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::ffi::CStr;

use crate::sycl::ffi::{
    sycl_discover_devices, sycl_get_device, sycl_get_device_count, sycl_get_device_info,
    sycl_release_device,
};

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
    unsafe {
        // Step 1: Discover devices
        sycl_discover_devices()
            .to_result()
            .context("SYCL device discovery failed")?;

        // Step 2: Get device count
        let mut count: u32 = 0;
        sycl_get_device_count(&mut count)
            .to_result()
            .context("Failed to get device count")?;

        let mut infos = Vec::new();

        // Step 3: Query each device
        for i in 0..count {
            let mut device = std::ptr::null_mut();
            if sycl_get_device(i, &mut device).to_result().is_err() {
                continue;
            }

            let mut name_buf = vec![0i8; 256];
            let mut backend: i32 = 0;

            if sycl_get_device_info(device, name_buf.as_mut_ptr(), name_buf.len(), &mut backend)
                .to_result()
                .is_ok()
            {
                let name = CStr::from_ptr(name_buf.as_ptr())
                    .to_str()
                    .unwrap_or("Unknown")
                    .to_string();

                let backend_str = match backend {
                    0 => "OpenCL",
                    1 => "CUDA",
                    2 => "HIP",
                    3 => "Level-Zero",
                    4 => "CPU",
                    _ => "Unknown",
                };

                // For now, use placeholder values for fields not in new API
                // These could be queried via additional SYCL calls if needed
                infos.push(GpuInfo {
                    name,
                    vendor: "Unknown".to_string(), // Could add vendor query to C++ layer
                    compute_units: 0,               // Could add CU query
                    global_memory_size: 8 * 1024 * 1024 * 1024, // Placeholder: 8GB
                    local_memory_size: 64 * 1024,   // Placeholder: 64KB
                    max_work_group_size: 256,       // Placeholder
                    is_gpu: backend != 4,
                    is_cpu: backend == 4,
                    supports_fp64: false, // Could query via device capabilities
                    supports_fp16: false, // Could query via device capabilities
                    local_index: i as usize,
                    backend: backend_str.to_string(),
                    total_vram_bytes: 8 * 1024 * 1024 * 1024, // Placeholder: 8GB
                });
            }

            sycl_release_device(device);
        }

        Ok(infos)
    }
}
