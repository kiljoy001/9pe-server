//! Compatibility shim for old FFI interface
//!
//! This provides the same FFI function signatures as before,
//! but routes calls through the dynamic backend loader.
//!
//! This allows existing code to work without changes while we
//! gradually migrate to the new backend-aware API.

use std::sync::{Arc, Mutex};
use once_cell::sync::Lazy;
use super::backend_loader::{SyclBackendManager, SyclBackendLib};
use super::ffi::{SyclDevice, SyclQueue, SyclBuffer, SyclEvent, SyclError};
use std::os::raw::{c_char, c_void};

/// Global backend manager instance
static BACKEND_MANAGER: Lazy<Arc<Mutex<Option<SyclBackendManager>>>> =
    Lazy::new(|| Arc::new(Mutex::new(None)));

/// Global default backend (uses first available: Intel > AdaptiveCpp)
static DEFAULT_BACKEND: Lazy<Arc<Mutex<Option<Arc<SyclBackendLib>>>>> =
    Lazy::new(|| Arc::new(Mutex::new(None)));

/// Initialize the backend manager
fn ensure_backend_initialized() {
    let mut manager_guard = BACKEND_MANAGER.lock().unwrap();
    if manager_guard.is_none() {
        let manager = SyclBackendManager::new();

        // Select default backend (prefer Intel)
        let default_backend = manager.intel_backend()
            .or_else(|| manager.adaptive_backend());

        *DEFAULT_BACKEND.lock().unwrap() = default_backend;
        *manager_guard = Some(manager);
    }
}

/// Get the default backend
fn get_default_backend() -> Option<Arc<SyclBackendLib>> {
    ensure_backend_initialized();
    DEFAULT_BACKEND.lock().unwrap().clone()
}

// Export the same C-compatible functions as before, but route through dynamic backends

#[no_mangle]
pub extern "C" fn sycl_discover_devices() -> SyclError {
    match get_default_backend() {
        Some(backend) => unsafe { (backend.discover_devices)() },
        None => SyclError::InvalidDevice,
    }
}

#[no_mangle]
pub extern "C" fn sycl_get_device_count(count: *mut u32) -> SyclError {
    match get_default_backend() {
        Some(backend) => unsafe { (backend.get_device_count)(count) },
        None => SyclError::InvalidDevice,
    }
}

#[no_mangle]
pub extern "C" fn sycl_get_device(index: u32, device: *mut SyclDevice) -> SyclError {
    match get_default_backend() {
        Some(backend) => unsafe { (backend.get_device)(index, device) },
        None => SyclError::InvalidDevice,
    }
}

#[no_mangle]
pub extern "C" fn sycl_get_device_info(
    device: SyclDevice,
    name: *mut c_char,
    name_size: usize,
    backend_type: *mut i32,
) -> SyclError {
    match get_default_backend() {
        Some(backend) => unsafe { (backend.get_device_info)(device, name, name_size, backend_type) },
        None => SyclError::InvalidDevice,
    }
}

#[no_mangle]
pub extern "C" fn sycl_release_device(device: SyclDevice) -> SyclError {
    match get_default_backend() {
        Some(backend) => unsafe { (backend.release_device)(device) },
        None => SyclError::InvalidDevice,
    }
}

#[no_mangle]
pub extern "C" fn sycl_create_queue(device: SyclDevice, queue: *mut SyclQueue) -> SyclError {
    match get_default_backend() {
        Some(backend) => unsafe { (backend.create_queue)(device, queue) },
        None => SyclError::InvalidQueue,
    }
}

#[no_mangle]
pub extern "C" fn sycl_queue_wait(queue: SyclQueue) -> SyclError {
    match get_default_backend() {
        Some(backend) => unsafe { (backend.queue_wait)(queue) },
        None => SyclError::InvalidQueue,
    }
}

#[no_mangle]
pub extern "C" fn sycl_release_queue(queue: SyclQueue) -> SyclError {
    match get_default_backend() {
        Some(backend) => unsafe { (backend.release_queue)(queue) },
        None => SyclError::InvalidQueue,
    }
}

#[no_mangle]
pub extern "C" fn sycl_create_buffer(
    queue: SyclQueue,
    size: usize,
    buffer: *mut SyclBuffer,
) -> SyclError {
    match get_default_backend() {
        Some(backend) => unsafe { (backend.create_buffer)(queue, size, buffer) },
        None => SyclError::InvalidBuffer,
    }
}

#[no_mangle]
pub extern "C" fn sycl_buffer_write(
    queue: SyclQueue,
    buffer: SyclBuffer,
    data: *const c_void,
    offset: usize,
    size: usize,
) -> SyclError {
    match get_default_backend() {
        Some(backend) => unsafe { (backend.buffer_write)(queue, buffer, data, offset, size) },
        None => SyclError::InvalidBuffer,
    }
}

#[no_mangle]
pub extern "C" fn sycl_buffer_read(
    queue: SyclQueue,
    buffer: SyclBuffer,
    data: *mut c_void,
    offset: usize,
    size: usize,
) -> SyclError {
    match get_default_backend() {
        Some(backend) => unsafe { (backend.buffer_read)(queue, buffer, data, offset, size) },
        None => SyclError::InvalidBuffer,
    }
}

#[no_mangle]
pub extern "C" fn sycl_release_buffer(buffer: SyclBuffer) -> SyclError {
    match get_default_backend() {
        Some(backend) => unsafe { (backend.release_buffer)(buffer) },
        None => SyclError::InvalidBuffer,
    }
}

#[no_mangle]
pub extern "C" fn sycl_release_event(event: SyclEvent) -> SyclError {
    match get_default_backend() {
        Some(backend) => unsafe { (backend.release_event)(event) },
        None => SyclError::InvalidHandle,
    }
}

#[no_mangle]
pub extern "C" fn sycl_get_kernel_time(
    event: SyclEvent,
    start_ns: *mut u64,
    end_ns: *mut u64,
) -> SyclError {
    match get_default_backend() {
        Some(backend) => unsafe { (backend.get_kernel_time)(event, start_ns, end_ns) },
        None => SyclError::InvalidHandle,
    }
}

#[no_mangle]
pub extern "C" fn sycl_matmul_f32_async(
    queue: SyclQueue,
    a: SyclBuffer,
    b: SyclBuffer,
    c: SyclBuffer,
    m: u32,
    n: u32,
    k: u32,
    event: *mut SyclEvent,
) -> SyclError {
    match get_default_backend() {
        Some(backend) => unsafe { (backend.matmul_f32_async)(queue, a, b, c, m, n, k, event) },
        None => SyclError::ExecutionFailed,
    }
}

#[no_mangle]
pub extern "C" fn sycl_ternary_matmul_async(
    queue: SyclQueue,
    a: SyclBuffer,
    b: SyclBuffer,
    c: SyclBuffer,
    m: u32,
    n: u32,
    k: u32,
    event: *mut SyclEvent,
) -> SyclError {
    match get_default_backend() {
        Some(backend) => unsafe { (backend.ternary_matmul_async)(queue, a, b, c, m, n, k, event) },
        None => SyclError::ExecutionFailed,
    }
}

#[no_mangle]
pub extern "C" fn sycl_get_last_error() -> *const c_char {
    match get_default_backend() {
        Some(backend) => unsafe { (backend.get_last_error)() },
        None => std::ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn sycl_clear_error() {
    if let Some(backend) = get_default_backend() {
        unsafe { (backend.clear_error)() }
    }
}
