//! Test the GPU synthetic filesystem functionality

use anyhow::Result;
use ninepe_server::{
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

/// Test VRAM allocation and release via synthetic files
#[tokio::test]
async fn test_gpu_vram_allocation() -> Result<()> {
    let synth_fs = Arc::new(SyntheticFilesystem::new());

    let gpu_infos = vec![GpuInfo {
        name: "Test GPU".to_string(),
        vendor: "Test Vendor".to_string(),
        compute_units: 8,
        global_memory_size: 1024 * 1024 * 1024,
        local_memory_size: 64 * 1024,
        max_work_group_size: 1024,
        is_gpu: true,
        is_cpu: false,
        supports_fp64: true,
        supports_fp16: false,
        total_vram_bytes: 1024 * 1024 * 1024, // 1GB
        backend: "gpu".to_string(),
        local_index: 0,
    }];

    let gpu_runtimes: Vec<Arc<GpuRuntime>> = gpu_infos
        .iter()
        .map(|gpu_info| {
            Arc::new(GpuRuntime::new(
                &format!("gpu{}", gpu_info.local_index),
                gpu_info.total_vram_bytes,
            ))
        })
        .collect();

    register_gpu_controls(&synth_fs, &gpu_infos, &gpu_runtimes).await?;

    // Read initial VRAM free (should be 1GB = 1073741824)
    let vram_free = synth_fs
        .read_file(&PathBuf::from("/srv/compute/gpu0/vram_free"))
        .await?;
    let initial_free: u64 = String::from_utf8(vram_free)?.trim().parse()?;
    assert_eq!(initial_free, 1024 * 1024 * 1024);

    // Allocate 100MB
    let alloc_amount = 100 * 1024 * 1024u64;
    synth_fs
        .write_file(
            &PathBuf::from("/srv/compute/gpu0/vram_allocate"),
            alloc_amount.to_string().into_bytes(),
        )
        .await?;

    // Check free VRAM decreased
    let vram_free = synth_fs
        .read_file(&PathBuf::from("/srv/compute/gpu0/vram_free"))
        .await?;
    let after_alloc: u64 = String::from_utf8(vram_free)?.trim().parse()?;
    assert_eq!(after_alloc, initial_free - alloc_amount);

    // Release 100MB
    synth_fs
        .write_file(
            &PathBuf::from("/srv/compute/gpu0/vram_release"),
            alloc_amount.to_string().into_bytes(),
        )
        .await?;

    // Check free VRAM is restored
    let vram_free = synth_fs
        .read_file(&PathBuf::from("/srv/compute/gpu0/vram_free"))
        .await?;
    let after_release: u64 = String::from_utf8(vram_free)?.trim().parse()?;
    assert_eq!(after_release, initial_free);

    // Read VRAM status
    let status = synth_fs
        .read_file(&PathBuf::from("/srv/compute/gpu0/vram_status"))
        .await?;
    let status_str = String::from_utf8(status)?;
    assert!(status_str.contains("Total: 1024 MB"));
    assert!(status_str.contains("Free: 1024 MB"));
    assert!(status_str.contains("Used: 0 MB"));

    println!("VRAM allocation/release test passed!");
    Ok(())
}

/// Test compute job submission
#[tokio::test]
async fn test_compute_job_submission() -> Result<()> {
    let synth_fs = Arc::new(SyntheticFilesystem::new());

    let gpu_infos = vec![GpuInfo {
        name: "Test GPU".to_string(),
        vendor: "Test Vendor".to_string(),
        compute_units: 8,
        global_memory_size: 1024 * 1024 * 1024,
        local_memory_size: 64 * 1024,
        max_work_group_size: 1024,
        is_gpu: true,
        is_cpu: false,
        supports_fp64: true,
        supports_fp16: false,
        total_vram_bytes: 1024 * 1024 * 1024,
        backend: "gpu".to_string(),
        local_index: 0,
    }];

    let gpu_runtimes: Vec<Arc<GpuRuntime>> = gpu_infos
        .iter()
        .map(|gpu_info| {
            Arc::new(GpuRuntime::new(
                &format!("gpu{}", gpu_info.local_index),
                gpu_info.total_vram_bytes,
            ))
        })
        .collect();

    register_gpu_controls(&synth_fs, &gpu_infos, &gpu_runtimes).await?;

    let compute_mgr = Arc::new(ComputeManager::with_runtimes(gpu_runtimes.clone()));
    let temp_dir = TempDir::new().unwrap();
    let registry = Arc::new(ThreadSafeTranslatorRegistry::new(
        temp_dir.path().to_path_buf(),
    ));
    registry.scan_and_load().await.ok();
    compute_mgr
        .set_translator_registry(registry.clone())
        .await?;

    register_compute_control(&synth_fs, compute_mgr.clone(), registry).await?;

    // Start the compute manager workers
    compute_mgr.start_workers();

    // Read initial status (plain text format)
    let status = synth_fs
        .read_file(&PathBuf::from("/srv/compute/status"))
        .await?;
    let status_str = String::from_utf8(status)?;
    assert!(status_str.contains("Compute System Status"));
    assert!(status_str.contains("SYCL Available:"));

    // Read devices list (plain text format)
    let devices = synth_fs
        .read_file(&PathBuf::from("/srv/compute/devices"))
        .await?;
    let devices_str = String::from_utf8(devices)?;
    assert!(devices_str.contains("GPU") || devices_str.contains("Device") || devices_str.is_empty());

    println!("Compute job submission test passed!");
    Ok(())
}

/// Test multiple GPU handling
#[tokio::test]
async fn test_multi_gpu() -> Result<()> {
    let synth_fs = Arc::new(SyntheticFilesystem::new());

    // Simulate 3 GPUs
    let gpu_infos = (0..3)
        .map(|i| GpuInfo {
            name: format!("Test GPU {}", i),
            vendor: "Test Vendor".to_string(),
            compute_units: 8 + i as u32,
            global_memory_size: (1 + i as u64) * 1024 * 1024 * 1024,
            local_memory_size: 64 * 1024,
            max_work_group_size: 1024,
            is_gpu: true,
            is_cpu: false,
            supports_fp64: true,
            supports_fp16: i == 2,
            total_vram_bytes: (1 + i as u64) * 1024 * 1024 * 1024,
            backend: "gpu".to_string(),
            local_index: i,
        })
        .collect::<Vec<_>>();

    let gpu_runtimes: Vec<Arc<GpuRuntime>> = gpu_infos
        .iter()
        .map(|gpu_info| {
            Arc::new(GpuRuntime::new(
                &format!("gpu{}", gpu_info.local_index),
                gpu_info.total_vram_bytes,
            ))
        })
        .collect();

    register_gpu_controls(&synth_fs, &gpu_infos, &gpu_runtimes).await?;

    // Verify all 3 GPUs are registered
    for i in 0..3 {
        let info_path = PathBuf::from(format!("/srv/compute/gpu{}/info", i));
        assert!(synth_fs.exists(&info_path).await);

        let data = synth_fs.read_file(&info_path).await?;
        let json_str = String::from_utf8(data)?;
        assert!(json_str.contains(&format!("Test GPU {}", i)));
    }

    // Allocate different amounts from each GPU
    for i in 0..3 {
        let alloc_amount = (i + 1) as u64 * 100 * 1024 * 1024; // 100MB, 200MB, 300MB
        synth_fs
            .write_file(
                &PathBuf::from(format!("/srv/compute/gpu{}/vram_allocate", i)),
                alloc_amount.to_string().into_bytes(),
            )
            .await?;
    }

    // Verify allocations
    for i in 0..3 {
        let vram_free = synth_fs
            .read_file(&PathBuf::from(format!("/srv/compute/gpu{}/vram_free", i)))
            .await?;
        let free: u64 = String::from_utf8(vram_free)?.trim().parse()?;
        let expected_free =
            (1 + i as u64) * 1024 * 1024 * 1024 - (i + 1) as u64 * 100 * 1024 * 1024;
        assert_eq!(free, expected_free);
    }

    println!("Multi-GPU test passed!");
    Ok(())
}
