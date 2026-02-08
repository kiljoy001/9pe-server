use ninepe_server::sycl::backend_loader::{SyclBackendManager, BackendType};
use ninepe_server::traits::{ComputeBackend, ComputeJob, JobStatus};
use uuid::Uuid;
use serde_json::json;

#[tokio::test]
async fn test_sycl_backend_full_lifecycle() {
    // 1. Initialize backend manager
    let manager = SyclBackendManager::new();
    if !manager.has_any_backend() {
        println!("No SYCL backends available, skipping test");
        return;
    }

    // 2. Discover devices
    let devices = manager.discover_devices().expect("Failed to discover devices");
    assert!(!devices.is_empty(), "No devices discovered");
    let device_id = &devices[0].id;
    println!("Testing on device: {}", device_id);

    // 3. Submit a matrix multiply job
    let m = 2u32;
    let n = 2u32;
    let k = 2u32;
    let a = vec![1.0f32, 2.0, 3.0, 4.0];
    let b = vec![5.0f32, 6.0, 7.0, 8.0];
    
    let job = ComputeJob {
        id: device_id.clone(),
        job_type: "sycl".to_string(),
        operation: "matrix_multiply".to_string(),
        params: serde_json::to_vec(&json!({
            "a": a,
            "b": b,
            "m": m,
            "n": n,
            "k": k
        })).unwrap(),
        shm_handle: None,
    };

    let job_id = manager.submit_job(job).await.expect("Failed to submit job");
    println!("Submitted job ID: {}", job_id);

    // 4. Verify Job ID is UUID v8 and check capabilities
    let uuid = Uuid::parse_str(&job_id).expect("Invalid UUID format");
    assert_eq!(uuid.get_version_num(), 8, "Expected UUID v8");
    
    // Check capability flags in UUID (custom bits)
    let uuid_bytes = uuid.as_bytes();
    // Reconstruct capability from UUID bytes (byte 7 + bits from 6 and 8)
    // Low 4 bits of byte 6 = bits 12-15 of capability
    // Byte 7 = bits 4-11 of capability
    // High 4 bits of byte 8 (shifted) = bits 0-3 of capability
    // Actually, logic in backend_loader was:
    // byte 6: 0x80 | ((cap >> 12) & 0x0F)
    // byte 7: (cap >> 4) & 0xFF
    // byte 8: 0x80 | ((cap & 0x0F) << 2)
    
    let cap_high = (uuid_bytes[6] & 0x0F) as u16;
    let cap_mid = uuid_bytes[7] as u16;
    let cap_low = (uuid_bytes[8] >> 2) & 0x0F;
    let extracted_cap = (cap_high << 12) | (cap_mid << 4) | (cap_low as u16);
    
    println!("Extracted capability from UUID: 0x{:04x}", extracted_cap);
    // BasicCompute is 1
    assert_eq!(extracted_cap & 1, 1, "Expected BasicCompute capability in UUID");

    // 5. Poll for completion
    let mut status = JobStatus::Pending;
    for _ in 0..10 {
        if let Some(s) = manager.get_job_status(&job_id).await {
            status = s;
            match status {
                JobStatus::Completed(_) | JobStatus::Failed(_) => break,
                _ => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
            }
        }
    }

    // 6. Verify result
    match status {
        JobStatus::Completed(result_bytes) => {
            let result: serde_json::Value = serde_json::from_slice(&result_bytes).expect("Invalid result JSON");
            let values = result.get("values").and_then(|v| v.as_array()).expect("Missing values in result");
            assert_eq!(values.len(), 4);
            // Result of [1,2;3,4] * [5,6;7,8] is [19,22;43,50]
            assert_eq!(values[0].as_f64().unwrap(), 19.0);
            assert_eq!(values[1].as_f64().unwrap(), 22.0);
            assert_eq!(values[2].as_f64().unwrap(), 43.0);
            assert_eq!(values[3].as_f64().unwrap(), 50.0);
            println!("Job completed successfully with correct results");
        }
        JobStatus::Failed(err) => panic!("Job failed: {}", err),
        _ => panic!("Job timed out or stayed pending"),
    }
}
