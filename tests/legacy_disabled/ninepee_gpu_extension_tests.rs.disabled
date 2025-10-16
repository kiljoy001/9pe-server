//! Tests for 9P.e GPU extension messages
//!
//! Tests the new GPU compute extension messages:
//! - ComputeSubmit
//! - ComputeStatus
//! - VRAMAllocate
//! - GPUInfo
//! - ComputeResponse

use ninep_server::protocol::NinePeeMessage;
use serde_json;

#[test]
fn test_compute_submit_message() {
    let job_data = vec![1, 2, 3, 4, 5];
    let message = NinePeeMessage::ComputeSubmit {
        job_type: "sycl".to_string(),
        kernel_name: "vector_add".to_string(),
        data: job_data.clone(),
        device_hint: Some(0),
    };

    // Test serialization
    let serialized = bincode::serialize(&message).unwrap();
    let deserialized: NinePeeMessage = bincode::deserialize(&serialized).unwrap();

    match deserialized {
        NinePeeMessage::ComputeSubmit {
            job_type,
            kernel_name,
            data,
            device_hint,
        } => {
            assert_eq!(job_type, "sycl");
            assert_eq!(kernel_name, "vector_add");
            assert_eq!(data, job_data);
            assert_eq!(device_hint, Some(0));
        }
        _ => panic!("Deserialized to wrong message type"),
    }

    // Test JSON serialization
    let json = serde_json::to_string(&message).unwrap();
    let from_json: NinePeeMessage = serde_json::from_str(&json).unwrap();

    match from_json {
        NinePeeMessage::ComputeSubmit {
            job_type,
            kernel_name,
            data,
            device_hint,
        } => {
            assert_eq!(job_type, "sycl");
            assert_eq!(kernel_name, "vector_add");
            assert_eq!(data, job_data);
            assert_eq!(device_hint, Some(0));
        }
        _ => panic!("JSON deserialized to wrong message type"),
    }
}

#[test]
fn test_compute_status_message() {
    let job_id = "550e8400-e29b-41d4-a716-446655440000".to_string();
    let message = NinePeeMessage::ComputeStatus {
        job_id: job_id.clone(),
    };

    // Test serialization
    let serialized = bincode::serialize(&message).unwrap();
    let deserialized: NinePeeMessage = bincode::deserialize(&serialized).unwrap();

    match deserialized {
        NinePeeMessage::ComputeStatus { job_id: id } => {
            assert_eq!(id, job_id);
        }
        _ => panic!("Deserialized to wrong message type"),
    }

    // Test JSON serialization
    let json = serde_json::to_string(&message).unwrap();
    let from_json: NinePeeMessage = serde_json::from_str(&json).unwrap();

    match from_json {
        NinePeeMessage::ComputeStatus { job_id: id } => {
            assert_eq!(id, job_id);
        }
        _ => panic!("JSON deserialized to wrong message type"),
    }
}

#[test]
fn test_vram_allocate_message() {
    let message = NinePeeMessage::VRAMAllocate {
        device: 1,
        bytes: 1024 * 1024 * 256, // 256MB
    };

    // Test serialization
    let serialized = bincode::serialize(&message).unwrap();
    let deserialized: NinePeeMessage = bincode::deserialize(&serialized).unwrap();

    match deserialized {
        NinePeeMessage::VRAMAllocate { device, bytes } => {
            assert_eq!(device, 1);
            assert_eq!(bytes, 1024 * 1024 * 256);
        }
        _ => panic!("Deserialized to wrong message type"),
    }

    // Test JSON serialization
    let json = serde_json::to_string(&message).unwrap();
    let from_json: NinePeeMessage = serde_json::from_str(&json).unwrap();

    match from_json {
        NinePeeMessage::VRAMAllocate { device, bytes } => {
            assert_eq!(device, 1);
            assert_eq!(bytes, 1024 * 1024 * 256);
        }
        _ => panic!("JSON deserialized to wrong message type"),
    }
}

#[test]
fn test_gpu_info_message() {
    let message = NinePeeMessage::GPUInfo { device: 2 };

    // Test serialization
    let serialized = bincode::serialize(&message).unwrap();
    let deserialized: NinePeeMessage = bincode::deserialize(&serialized).unwrap();

    match deserialized {
        NinePeeMessage::GPUInfo { device } => {
            assert_eq!(device, 2);
        }
        _ => panic!("Deserialized to wrong message type"),
    }

    // Test JSON serialization
    let json = serde_json::to_string(&message).unwrap();
    let from_json: NinePeeMessage = serde_json::from_str(&json).unwrap();

    match from_json {
        NinePeeMessage::GPUInfo { device } => {
            assert_eq!(device, 2);
        }
        _ => panic!("JSON deserialized to wrong message type"),
    }
}

#[test]
fn test_compute_response_message() {
    let result_data = vec![10, 20, 30, 40];
    let message = NinePeeMessage::ComputeResponse {
        job_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
        success: true,
        result: result_data.clone(),
        error_msg: "".to_string(),
    };

    // Test serialization
    let serialized = bincode::serialize(&message).unwrap();
    let deserialized: NinePeeMessage = bincode::deserialize(&serialized).unwrap();

    match deserialized {
        NinePeeMessage::ComputeResponse {
            job_id,
            success,
            result,
            error_msg,
        } => {
            assert_eq!(job_id, "550e8400-e29b-41d4-a716-446655440000");
            assert_eq!(success, true);
            assert_eq!(result, result_data);
            assert_eq!(error_msg, "");
        }
        _ => panic!("Deserialized to wrong message type"),
    }

    // Test JSON serialization
    let json = serde_json::to_string(&message).unwrap();
    let from_json: NinePeeMessage = serde_json::from_str(&json).unwrap();

    match from_json {
        NinePeeMessage::ComputeResponse {
            job_id,
            success,
            result,
            error_msg,
        } => {
            assert_eq!(job_id, "550e8400-e29b-41d4-a716-446655440000");
            assert_eq!(success, true);
            assert_eq!(result, result_data);
            assert_eq!(error_msg, "");
        }
        _ => panic!("JSON deserialized to wrong message type"),
    }

    // Test error case
    let error_message = NinePeeMessage::ComputeResponse {
        job_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
        success: false,
        result: vec![],
        error_msg: "Kernel execution failed".to_string(),
    };

    let json = serde_json::to_string(&error_message).unwrap();
    let from_json: NinePeeMessage = serde_json::from_str(&json).unwrap();

    match from_json {
        NinePeeMessage::ComputeResponse {
            job_id,
            success,
            result,
            error_msg,
        } => {
            assert_eq!(job_id, "550e8400-e29b-41d4-a716-446655440000");
            assert_eq!(success, false);
            assert_eq!(result, Vec::<u8>::new());
            assert_eq!(error_msg, "Kernel execution failed");
        }
        _ => panic!("JSON deserialized to wrong message type"),
    }
}

#[test]
fn test_gpu_extension_message_identification() {
    // Test that GPU extension messages are properly identified
    let compute_submit = NinePeeMessage::ComputeSubmit {
        job_type: "sycl".to_string(),
        kernel_name: "test".to_string(),
        data: vec![],
        device_hint: None,
    };

    let compute_status = NinePeeMessage::ComputeStatus {
        job_id: "test".to_string(),
    };

    let vram_allocate = NinePeeMessage::VRAMAllocate {
        device: 0,
        bytes: 1024,
    };

    let gpu_info = NinePeeMessage::GPUInfo { device: 0 };

    let compute_response = NinePeeMessage::ComputeResponse {
        job_id: "test".to_string(),
        success: true,
        result: vec![],
        error_msg: "".to_string(),
    };

    // These should NOT be identified as errors
    assert!(!compute_submit.is_error());
    assert!(!compute_status.is_error());
    assert!(!vram_allocate.is_error());
    assert!(!gpu_info.is_error());
    assert!(!compute_response.is_error());

    // These should NOT be identified as basic extensions
    // (They're GPU extensions, not the original translator/consensus extensions)
    assert!(!compute_submit.is_extension());
    assert!(!compute_status.is_extension());
    assert!(!vram_allocate.is_extension());
    assert!(!gpu_info.is_extension());
    assert!(!compute_response.is_extension());
}

#[test]
fn test_gpu_extension_message_fid_handling() {
    // GPU extension messages don't have FIDs, so they should return None
    let compute_submit = NinePeeMessage::ComputeSubmit {
        job_type: "sycl".to_string(),
        kernel_name: "test".to_string(),
        data: vec![],
        device_hint: None,
    };

    let compute_status = NinePeeMessage::ComputeStatus {
        job_id: "test".to_string(),
    };

    let vram_allocate = NinePeeMessage::VRAMAllocate {
        device: 0,
        bytes: 1024,
    };

    let gpu_info = NinePeeMessage::GPUInfo { device: 0 };

    let compute_response = NinePeeMessage::ComputeResponse {
        job_id: "test".to_string(),
        success: true,
        result: vec![],
        error_msg: "".to_string(),
    };

    assert_eq!(compute_submit.fid(), None);
    assert_eq!(compute_status.fid(), None);
    assert_eq!(vram_allocate.fid(), None);
    assert_eq!(gpu_info.fid(), None);
    assert_eq!(compute_response.fid(), None);
}
