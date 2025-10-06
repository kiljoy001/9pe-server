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
    DeviceNotFound = 1,
    OutOfMemory = 2,
    InvalidKernel = 3,
    InvalidBuffer = 4,
    RuntimeError = 5,
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
}

impl std::fmt::Display for SyclError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyclError::Success => write!(f, "Success"),
            SyclError::DeviceNotFound => write!(f, "Device not found"),
            SyclError::OutOfMemory => write!(f, "Out of memory"),
            SyclError::InvalidKernel => write!(f, "Invalid kernel"),
            SyclError::InvalidBuffer => write!(f, "Invalid buffer"),
            SyclError::RuntimeError => write!(f, "Runtime error"),
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
    pub fn sycl_discover_devices(
        device_info: *mut SyclDeviceInfo,
        device_count: *mut usize,
    ) -> SyclError;

    pub fn sycl_get_device(device_index: u32, device: *mut SyclDevice) -> SyclError;

    pub fn sycl_get_device_backend(device: SyclDevice, backend: *mut SyclBackend) -> SyclError;

    pub fn sycl_release_device(device: SyclDevice);

    // Queue management
    pub fn sycl_create_queue(device: SyclDevice, queue: *mut SyclQueue) -> SyclError;

    pub fn sycl_queue_wait(queue: SyclQueue) -> SyclError;

    pub fn sycl_release_queue(queue: SyclQueue);

    // Buffer management
    pub fn sycl_create_buffer(
        queue: SyclQueue,
        size_bytes: usize,
        buffer: *mut SyclBuffer,
    ) -> SyclError;

    pub fn sycl_write_buffer(
        queue: SyclQueue,
        buffer: SyclBuffer,
        data: *const c_void,
        size_bytes: usize,
        offset: usize,
    ) -> SyclError;

    pub fn sycl_read_buffer(
        queue: SyclQueue,
        buffer: SyclBuffer,
        data: *mut c_void,
        size_bytes: usize,
        offset: usize,
    ) -> SyclError;

    pub fn sycl_release_buffer(buffer: SyclBuffer);

    // Standard AI kernels
    pub fn sycl_matmul_f32(
        queue: SyclQueue,
        buffer_a: SyclBuffer,
        buffer_b: SyclBuffer,
        buffer_c: SyclBuffer,
        m: u32,
        n: u32,
        k: u32,
    ) -> SyclError;

    pub fn sycl_vector_add_f32(
        queue: SyclQueue,
        buffer_a: SyclBuffer,
        buffer_b: SyclBuffer,
        buffer_c: SyclBuffer,
        length: usize,
    ) -> SyclError;

    pub fn sycl_relu_f32(
        queue: SyclQueue,
        buffer_in: SyclBuffer,
        buffer_out: SyclBuffer,
        length: usize,
    ) -> SyclError;

    pub fn sycl_conv2d_f32(
        queue: SyclQueue,
        input: SyclBuffer,
        kernel: SyclBuffer,
        output: SyclBuffer,
        batch: u32,
        in_channels: u32,
        out_channels: u32,
        height: u32,
        width: u32,
        kernel_h: u32,
        kernel_w: u32,
        stride: u32,
        padding: u32,
    ) -> SyclError;

    // Custom kernel compilation
    pub fn sycl_compile_kernel(
        device: SyclDevice,
        source: *const c_char,
        kernel_name: *const c_char,
        kernel: *mut SyclKernel,
    ) -> SyclError;

    pub fn sycl_set_kernel_arg_buffer(
        kernel: SyclKernel,
        arg_index: u32,
        buffer: SyclBuffer,
    ) -> SyclError;

    pub fn sycl_set_kernel_arg_scalar(
        kernel: SyclKernel,
        arg_index: u32,
        value: *const c_void,
        size: usize,
    ) -> SyclError;

    pub fn sycl_execute_kernel(
        queue: SyclQueue,
        kernel: SyclKernel,
        global_work_size: *const usize,
        local_work_size: *const usize,
        work_dim: u32,
    ) -> SyclError;

    pub fn sycl_release_kernel(kernel: SyclKernel);

    // Profiling and diagnostics
    pub fn sycl_get_kernel_time(event: SyclEvent, nanoseconds: *mut u64) -> SyclError;

    pub fn sycl_get_device_utilization(device: SyclDevice, utilization: *mut f32) -> SyclError;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discover_devices() {
        let mut devices = vec![SyclDeviceInfo {
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
        }; 16];

        let mut count: usize = 16;

        unsafe {
            let err = sycl_discover_devices(devices.as_mut_ptr(), &mut count as *mut usize);
            println!("Discovered {} SYCL devices (error: {:?})", count, err);

            for i in 0..count {
                println!(
                    "Device {}: {} ({}) - {} CUs, {} GB memory",
                    i,
                    devices[i].name_str(),
                    devices[i].vendor_str(),
                    devices[i].compute_units,
                    devices[i].global_memory_size / (1024 * 1024 * 1024)
                );
            }
        }
    }
}
