use anyhow::Result;
use std::sync::Arc;

use super::ServerConfig;
use crate::gpu::{GpuInfo, GpuRuntime, synthetic::register_gpu_controls};
use crate::compute_control::{register_compute_control, ComputeManager};
use crate::namespace_manager::register_namespace_controls;
use crate::synth::SyntheticFilesystem;

/// Core server struct that holds the synthetic filesystem and compute manager.
#[derive(Debug)]
pub struct Server {
    pub config: ServerConfig,
    pub synth_fs: Arc<SyntheticFilesystem>,
    pub compute_mgr: Arc<ComputeManager>,
    pub gpu_infos: Vec<GpuInfo>,
    address: String, // Store the address for the address() method
}

impl Server {
    /// Create a new server instance with the given configuration
    pub async fn new(config: ServerConfig) -> Result<Self> {
        // 1️⃣ Synthetic filesystem
        let synth_fs = Arc::new(SyntheticFilesystem::new());
        
        // 2️⃣ Register namespace controls
        let _namespace_mgr = register_namespace_controls(&synth_fs).await?;
        
        // 3️⃣ Discover GPUs via SYCL wrapper
        let gpu_infos = discover_gpus()?;
        
        // 4️⃣ Create GPU runtimes for each discovered device
        let gpu_runtimes: Vec<std::sync::Arc<GpuRuntime>> = gpu_infos
            .iter()
            .map(|gpu_info| {
                let id = format!("gpu{}", gpu_info.local_index);
                Arc::new(GpuRuntime::new(&id, gpu_info.total_vram_bytes))
            })
            .collect();
        
        // 5️⃣ Register GPU synthetic files (info, vram_* etc.)
        register_gpu_controls(&synth_fs, &gpu_infos, &gpu_runtimes).await?;
        
        // 6️⃣ Create compute manager and register compute control files
        let compute_mgr = Arc::new(ComputeManager::with_runtimes(gpu_runtimes.clone()));
        register_compute_control(&synth_fs, compute_mgr.clone(), translator_registry.clone()).await?;
        
        // Extract address from config for the address() method
        let address = config.listen_addr.clone();
        
        Ok(Server {
            config,
            synth_fs,
            compute_mgr,
            gpu_infos,
            address,
        })
    }
    
    /// Constructs a new server instance, sets up synthetic GPU files, compute control, and mounts the compute namespace.
    pub async fn new_with_gpu_support() -> Result<Self> {
        // Create a default server config
        let config = ServerConfig {
            listen_addr: "0.0.0.0:5640".to_string(),
            node_id: "default".to_string(),
            network: crate::network::NetworkConfig::default(),
            transport: crate::transport::TransportType::default(),
            root_directory: std::path::PathBuf::from("."),
            max_message_size: 8 * 1024 * 1024,
            worker_threads: None,
            mesh_enabled: true,
            mesh_port: 9650,
            metrics_enabled: true,
            metrics_port: 9090,
            translator_directory: std::path::PathBuf::from(".9pe/translators"),
            settrans_directory: std::path::PathBuf::from(".9pe/settrans"),
            auto_mount_enabled: true,
            consensus_config: None,
            auth_config: crate::auth::AuthConfig::default(),
        };
        
        Self::new(config).await
    }
    
    /// Get the server address
    pub fn address(&self) -> &str {
        &self.address
    }
    
    /// Run the server (placeholder implementation)
    pub async fn run(&self) -> Result<()> {
        println!("Server running on {}", self.address());
        // In a real implementation, this would start the actual server
        Ok(())
    }
}

/// Helper function that discovers GPUs using the SYCL FFI and converts them into `GpuInfo` structs.
fn discover_gpus() -> Result<Vec<GpuInfo>> {
    use crate::sycl::ffi::{sycl_discover_devices, SyclDeviceInfo};
    // Prepare a buffer for a single device – the SYCL discovery routine will tell us how many exist.
    let mut devices: Vec<SyclDeviceInfo> = vec![SyclDeviceInfo {
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
    }];
    let mut count: usize = devices.len();
    let err = unsafe { sycl_discover_devices(devices.as_mut_ptr(), &mut count as *mut usize) };
    err.to_result().map_err(|e| anyhow::anyhow!("SYCL discovery error: {:?}", e))?;

    // Resize vector to the actual count returned by SYCL.
    devices.resize(count, SyclDeviceInfo {
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
    });

    // Convert each SyclDeviceInfo into our internal GpuInfo.
    let mut infos = Vec::new();
    for (idx, dev) in devices.iter().enumerate() {
        let name = unsafe {
            String::from_utf8_lossy(std::slice::from_raw_parts(dev.name.as_ptr() as *const u8, dev.name.len())).trim_end_matches('\0').to_string()
        };
        let vendor = unsafe {
            String::from_utf8_lossy(std::slice::from_raw_parts(dev.vendor.as_ptr() as *const u8, dev.vendor.len())).trim_end_matches('\0').to_string()
        };
        let info = GpuInfo {
            name,
            vendor,
            compute_units: dev.compute_units,
            global_memory_size: dev.global_memory_size,
            local_memory_size: dev.local_memory_size,
            max_work_group_size: dev.max_work_group_size,
            is_gpu: dev.is_gpu,
            is_cpu: dev.is_cpu,
            supports_fp64: dev.supports_fp64,
            supports_fp16: dev.supports_fp16,
            total_vram_bytes: dev.global_memory_size,
            backend: if dev.is_gpu { "gpu".to_string() } else { "cpu".to_string() },
            local_index: idx,
        };
        infos.push(info);
    }
    Ok(infos)
}

impl Server {
    /// Get information about discovered GPU devices
    pub fn gpu_info(&self) -> &[crate::gpu::GpuInfo] {
        &self.gpu_infos
    }
}
