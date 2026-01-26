//! Rust FFI bindings for SYCL C++ wrapper
//!
//! This module provides safe Rust bindings to the AdaptiveCpp SYCL backend.

use std::ffi::CStr;
use std::os::raw::{c_char, c_void};

// Opaque handle types
pub type SyclDevice = *mut c_void;
pub type SyclQueue = *mut c_void;
pub type SyclBuffer = *mut c_void;
pub type SyclKernel = *mut c_void;
pub type SyclEvent = *mut c_void;

// Device information structure
#[repr(C)]
#[derive(Debug, Clone)]
pub struct SyclDeviceInfo {
    pub name: [c_char; 256],
    pub vendor: [c_char; 128],
    pub compute_units: u32,
    pub global_memory_size: u64,
    pub local_memory_size: u64,
    pub max_work_group_size: u32,
    pub is_gpu: bool,
    pub is_cpu: bool,
    pub supports_fp64: bool,
    pub supports_fp16: bool,
}

impl SyclDeviceInfo {
    pub fn name_str(&self) -> &str {
        unsafe {
            CStr::from_ptr(self.name.as_ptr())
                .to_str()
                .unwrap_or("Unknown")
        }
    }

    pub fn vendor_str(&self) -> &str {
        unsafe {
            CStr::from_ptr(self.vendor.as_ptr())
                .to_str()
                .unwrap_or("Unknown")
        }
    }
}

// Error codes
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyclError {
    Success = 0,
    InvalidDevice = 1,
    InvalidQueue = 2,
    InvalidBuffer = 3,
    ExecutionFailed = 4,
    OutOfMemory = 5,
    InvalidHandle = 6,
}

impl SyclError {
    pub fn is_ok(&self) -> bool {
        *self == SyclError::Success
    }

    pub fn is_err(&self) -> bool {
        *self != SyclError::Success
    }

    pub fn to_result(self) -> Result<(), SyclError> {
        if self.is_ok() {
            Ok(())
        } else {
            Err(self)
        }
    }

    pub fn ok(self) -> Option<()> {
        if self.is_ok() {
            Some(())
        } else {
            None
        }
    }
}

impl std::fmt::Display for SyclError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyclError::Success => write!(f, "Success"),
            SyclError::InvalidDevice => write!(f, "Invalid device"),
            SyclError::InvalidQueue => write!(f, "Invalid queue"),
            SyclError::InvalidBuffer => write!(f, "Invalid buffer"),
            SyclError::ExecutionFailed => write!(f, "Execution failed"),
            SyclError::OutOfMemory => write!(f, "Out of memory"),
            SyclError::InvalidHandle => write!(f, "Invalid handle"),
        }
    }
}

impl std::error::Error for SyclError {}

// Backend type
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyclBackend {
    OpenCL = 0,
    CUDA = 1,
    HIP = 2,
    LevelZero = 3,
    CPU = 4,
}

impl std::fmt::Display for SyclBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyclBackend::OpenCL => write!(f, "OpenCL"),
            SyclBackend::CUDA => write!(f, "CUDA (NVIDIA)"),
            SyclBackend::HIP => write!(f, "HIP (AMD)"),
            SyclBackend::LevelZero => write!(f, "Level-Zero (Intel)"),
            SyclBackend::CPU => write!(f, "CPU"),
        }
    }
}

// External C functions from SYCL wrapper
extern "C" {
    // Device management
    pub fn sycl_discover_devices() -> SyclError;

    pub fn sycl_get_device_count(count: *mut u32) -> SyclError;

    pub fn sycl_get_device(device_index: u32, device: *mut SyclDevice) -> SyclError;

    pub fn sycl_get_device_info(
        device: SyclDevice,
        name: *mut c_char,
        name_size: usize,
        backend: *mut i32,
    ) -> SyclError;

    pub fn sycl_release_device(device: SyclDevice) -> SyclError;

    // Queue management
    pub fn sycl_create_queue(device: SyclDevice, queue: *mut SyclQueue) -> SyclError;

    pub fn sycl_queue_wait(queue: SyclQueue) -> SyclError;

    pub fn sycl_release_queue(queue: SyclQueue) -> SyclError;

    // Buffer management
    pub fn sycl_create_buffer(
        queue: SyclQueue,
        size_bytes: usize,
        buffer: *mut SyclBuffer,
    ) -> SyclError;

    // FIXED: Now takes queue parameter
    pub fn sycl_buffer_write(
        queue: SyclQueue,
        buffer: SyclBuffer,
        data: *const c_void,
        offset: usize,
        size_bytes: usize,
    ) -> SyclError;

    pub fn sycl_buffer_read(
        queue: SyclQueue,
        buffer: SyclBuffer,
        data: *mut c_void,
        offset: usize,
        size_bytes: usize,
    ) -> SyclError;

    pub fn sycl_release_buffer(buffer: SyclBuffer) -> SyclError;

    // Event management
    pub fn sycl_release_event(event: SyclEvent) -> SyclError;

    pub fn sycl_get_kernel_time(
        event: SyclEvent,
        start_ns: *mut u64,
        end_ns: *mut u64,
    ) -> SyclError;

    // Compute operations (async, return event)
    pub fn sycl_matmul_f32_async(
        queue: SyclQueue,
        buffer_a: SyclBuffer,
        buffer_b: SyclBuffer,
        buffer_c: SyclBuffer,
        m: u32,
        n: u32,
        k: u32,
        event: *mut SyclEvent,
    ) -> SyclError;

    pub fn sycl_ternary_matmul_async(
        queue: SyclQueue,
        buffer_a: SyclBuffer,
        buffer_b: SyclBuffer,
        buffer_c: SyclBuffer,
        m: u32,
        n: u32,
        k: u32,
        event: *mut SyclEvent,
    ) -> SyclError;

    // Error handling
    pub fn sycl_get_last_error() -> *const c_char;

    pub fn sycl_clear_error();

    // Handle management
    pub fn sycl_cleanup_unused_handles() -> SyclError;

    pub fn sycl_get_active_handle_count(
        devices: *mut u32,
        queues: *mut u32,
        buffers: *mut u32,
        events: *mut u32,
    ) -> SyclError;
}

/// Helper to get the last error as a Rust String
pub fn get_last_error_message() -> Option<String> {
    unsafe {
        let ptr = sycl_get_last_error();
        if ptr.is_null() {
            None
        } else {
            CStr::from_ptr(ptr).to_str().ok().map(|s| s.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn test_discover_devices() {
        unsafe {
            let err = sycl_discover_devices();
            println!("Device discovery result: {:?}", err);

            let mut count: u32 = 0;
            let err = sycl_get_device_count(&mut count);
            println!("Discovered {} SYCL devices (error: {:?})", count, err);

            for i in 0..count {
                let mut device: SyclDevice = std::ptr::null_mut();
                let err = sycl_get_device(i, &mut device);
                if err != SyclError::Success {
                    println!("Failed to get device {}: {:?}", i, err);
                    continue;
                }

                let mut name = vec![0i8; 256];
                let mut backend: i32 = 0;
                let err = sycl_get_device_info(
                    device,
                    name.as_mut_ptr(),
                    name.len(),
                    &mut backend,
                );

                if err == SyclError::Success {
                    let name_str = CStr::from_ptr(name.as_ptr())
                        .to_str()
                        .unwrap_or("Unknown");
                    println!("Device {}: {} (backend: {})", i, name_str, backend);
                }

                sycl_release_device(device);
            }
        }
    }

    #[test]
    fn test_error_handling() {
        unsafe {
            sycl_clear_error();

            // Should have no error initially
            assert!(sycl_get_last_error().is_null());

            // Trigger an error by passing null
            let err = sycl_create_buffer(
                std::ptr::null_mut(),
                1024,
                std::ptr::null_mut(),
            );

            assert_ne!(err, SyclError::Success);

            if let Some(msg) = get_last_error_message() {
                println!("Error message: {}", msg);
                assert!(!msg.is_empty());
            }

            sycl_clear_error();
            assert!(sycl_get_last_error().is_null());
        }
    }
}
