//! Test the GPU synthetic filesystem functionality

use anyhow::Result;
use ninep_server::{
    compute_control::{register_compute_control, ComputeManager},
    gpu::{synthetic::register_gpu_controls, GpuInfo, GpuRuntime},
    synth::SyntheticFilesystem,
    wasm::ThreadSafeTranslatorRegistry,
};
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

#[tokio::test]
async fn test_gpu_synthetic_files() -> Result<()> {
    // Create synthetic filesystem
    let synth_fs = Arc::new(SyntheticFilesystem::new());

    // Create a mock GPU info for testing (without requiring actual GPU hardware)
    let gpu_infos = vec![GpuInfo {
        name: "Test GPU".to_string(),
        vendor: "Test Vendor".to_string(),
        compute_units: 8,
        global_memory_size: 1024 * 1024 * 1024, // 1GB
        local_memory_size: 64 * 1024,           // 64KB
        max_work_group_size: 1024,
        is_gpu: true,
        is_cpu: false,
        supports_fp64: true,
        supports_fp16: false,
        total_vram_bytes: 1024 * 1024 * 1024, // 1GB
        backend: "gpu".to_string(),
        local_index: 0,
    }];

    // Create GPU runtimes for each discovered device
    let gpu_runtimes: Vec<Arc<GpuRuntime>> = gpu_infos
        .iter()
        .map(|gpu_info| {
            Arc::new(GpuRuntime::new(
                &format!("gpu{}", gpu_info.local_index),
                gpu_info.total_vram_bytes,
            ))
        })
        .collect();

    // Register GPU synthetic files (info, vram_* etc.)
    register_gpu_controls(&synth_fs, &gpu_infos, &gpu_runtimes).await?;

    // Create compute manager and register compute control files
    let compute_mgr = Arc::new(ComputeManager::with_runtimes(gpu_runtimes.clone()));
    let temp_dir = TempDir::new().unwrap();
    let registry = Arc::new(ThreadSafeTranslatorRegistry::new(
        temp_dir.path().to_path_buf(),
    ));
    registry.scan_and_load().await.ok();

    register_compute_control(&synth_fs, compute_mgr.clone(), registry).await?;

    // Test that GPU info file exists and is readable
    let info_path = PathBuf::from("/srv/compute/gpu0/info");
    assert!(synth_fs.exists(&info_path).await);

    let data = synth_fs.read_file(&info_path).await?;
    let json_str = String::from_utf8(data)?;
    assert!(json_str.contains("Test GPU"));
    assert!(json_str.contains("Test Vendor"));

    // Test that VRAM files exist
    let vram_free_path = PathBuf::from("/srv/compute/gpu0/vram_free");
    assert!(synth_fs.exists(&vram_free_path).await);

    let vram_release_path = PathBuf::from("/srv/compute/gpu0/vram_release");
    assert!(synth_fs.exists(&vram_release_path).await);

    let vram_status_path = PathBuf::from("/srv/compute/gpu0/vram_status");
    assert!(synth_fs.exists(&vram_status_path).await);

    // Test that compute control files exist
    let submit_path = PathBuf::from("/srv/compute/submit");
    assert!(synth_fs.exists(&submit_path).await);

    let jobs_path = PathBuf::from("/srv/compute/jobs");
    assert!(synth_fs.exists(&jobs_path).await);

    let devices_path = PathBuf::from("/srv/compute/devices");
    assert!(synth_fs.exists(&devices_path).await);

    let status_path = PathBuf::from("/srv/compute/status");
    assert!(synth_fs.exists(&status_path).await);

    println!("All GPU synthetic files registered and accessible!");

    Ok(())
}
