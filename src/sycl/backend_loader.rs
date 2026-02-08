//! Dynamic SYCL backend loader
//!
//! This module provides runtime selection of SYCL backends:
//! - Intel oneAPI for Intel GPUs (optimized, preferred)
//! - AdaptiveCpp for NVIDIA/AMD GPUs (universal fallback)
//!
//! The loader uses dlopen to load the appropriate backend .so file
//! based on detected GPU vendor.

use libloading::{Library, Symbol};
use std::ffi::CStr;
use std::os::raw::{c_char, c_void};
use std::path::PathBuf;
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use tracing::{info, warn, debug, error};
use uuid::{Uuid, Builder};
use std::time::{SystemTime, UNIX_EPOCH};
use rand::RngCore;
use crate::ipc::SharedMemoryHandle;
use crate::identity::Capability;
use std::sync::atomic::{AtomicU64, Ordering};

use super::ffi::{SyclBackend, SyclBuffer, SyclDevice, SyclError, SyclEvent, SyclQueue};

/// SYCL backend type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendType {
    /// Intel oneAPI DPC++ (optimized for Intel GPUs)
    IntelOneAPI,
    /// AdaptiveCpp (universal: NVIDIA, AMD, Intel)
    AdaptiveCpp,
}

impl std::fmt::Display for BackendType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendType::IntelOneAPI => write!(f, "Intel oneAPI"),
            BackendType::AdaptiveCpp => write!(f, "AdaptiveCpp"),
        }
    }
}

/// Dynamically loaded SYCL backend
pub struct SyclBackendLib {
    _lib: Arc<Library>,
    backend_type: BackendType,

    // Function pointers to SYCL C API
    pub discover_devices: Symbol<'static, unsafe extern "C" fn() -> SyclError>,
    pub get_device_count: Symbol<'static, unsafe extern "C" fn(*mut u32) -> SyclError>,
    pub get_device: Symbol<'static, unsafe extern "C" fn(u32, *mut SyclDevice) -> SyclError>,
    pub get_device_info: Symbol<'static, unsafe extern "C" fn(SyclDevice, *mut c_char, usize, *mut i32) -> SyclError>,
    pub release_device: Symbol<'static, unsafe extern "C" fn(SyclDevice) -> SyclError>,
    pub create_queue: Symbol<'static, unsafe extern "C" fn(SyclDevice, *mut SyclQueue) -> SyclError>,
    pub queue_wait: Symbol<'static, unsafe extern "C" fn(SyclQueue) -> SyclError>,
    pub release_queue: Symbol<'static, unsafe extern "C" fn(SyclQueue) -> SyclError>,
    pub create_buffer: Symbol<'static, unsafe extern "C" fn(SyclQueue, usize, *mut SyclBuffer) -> SyclError>,
    pub buffer_write: Symbol<'static, unsafe extern "C" fn(SyclQueue, SyclBuffer, *const c_void, usize, usize) -> SyclError>,
    pub buffer_read: Symbol<'static, unsafe extern "C" fn(SyclQueue, SyclBuffer, *mut c_void, usize, usize) -> SyclError>,
    pub release_buffer: Symbol<'static, unsafe extern "C" fn(SyclBuffer) -> SyclError>,
    pub release_event: Symbol<'static, unsafe extern "C" fn(SyclEvent) -> SyclError>,
    pub get_kernel_time: Symbol<'static, unsafe extern "C" fn(SyclEvent, *mut u64, *mut u64) -> SyclError>,
    pub matmul_f32_async: Symbol<'static, unsafe extern "C" fn(SyclQueue, SyclBuffer, SyclBuffer, SyclBuffer, u32, u32, u32, *mut SyclEvent) -> SyclError>,
    pub ternary_matmul_async: Symbol<'static, unsafe extern "C" fn(SyclQueue, SyclBuffer, SyclBuffer, SyclBuffer, u32, u32, u32, *mut SyclEvent) -> SyclError>,
    pub get_last_error: Symbol<'static, unsafe extern "C" fn() -> *const c_char>,
    pub clear_error: Symbol<'static, unsafe extern "C" fn()>,
}

impl SyclBackendLib {
    /// Load a SYCL backend library from path
    pub fn load(backend_type: BackendType) -> Result<Self, String> {
        let lib_name = match backend_type {
            BackendType::IntelOneAPI => "libsycl_ffi_intel.so",
            BackendType::AdaptiveCpp => "libsycl_ffi_adaptive.so",
        };

        // Try to find the library
        let lib_path = Self::find_library(lib_name)?;

        info!("Loading {} backend from {}", backend_type, lib_path.display());

        // Load the library
        let lib = unsafe {
            Library::new(&lib_path)
                .map_err(|e| format!("Failed to load {}: {}", lib_path.display(), e))?
        };

        let lib = Arc::new(lib);

        // Load all function symbols
        // Safety: We transmute the Symbol to 'static lifetime
        // This is safe because the Library is stored in Arc and kept alive
        unsafe {
            let discover_devices = std::mem::transmute(lib.get::<unsafe extern "C" fn() -> SyclError>(b"sycl_discover_devices\0")
                .map_err(|e| format!("Failed to load sycl_discover_devices: {}", e))?);
            let get_device_count = std::mem::transmute(lib.get::<unsafe extern "C" fn(*mut u32) -> SyclError>(b"sycl_get_device_count\0")
                .map_err(|e| format!("Failed to load sycl_get_device_count: {}", e))?);
            let get_device = std::mem::transmute(lib.get::<unsafe extern "C" fn(u32, *mut SyclDevice) -> SyclError>(b"sycl_get_device\0")
                .map_err(|e| format!("Failed to load sycl_get_device: {}", e))?);
            let get_device_info = std::mem::transmute(lib.get::<unsafe extern "C" fn(SyclDevice, *mut c_char, usize, *mut i32) -> SyclError>(b"sycl_get_device_info\0")
                .map_err(|e| format!("Failed to load sycl_get_device_info: {}", e))?);
            let release_device = std::mem::transmute(lib.get::<unsafe extern "C" fn(SyclDevice) -> SyclError>(b"sycl_release_device\0")
                .map_err(|e| format!("Failed to load sycl_release_device: {}", e))?);
            let create_queue = std::mem::transmute(lib.get::<unsafe extern "C" fn(SyclDevice, *mut SyclQueue) -> SyclError>(b"sycl_create_queue\0")
                .map_err(|e| format!("Failed to load sycl_create_queue: {}", e))?);
            let queue_wait = std::mem::transmute(lib.get::<unsafe extern "C" fn(SyclQueue) -> SyclError>(b"sycl_queue_wait\0")
                .map_err(|e| format!("Failed to load sycl_queue_wait: {}", e))?);
            let release_queue = std::mem::transmute(lib.get::<unsafe extern "C" fn(SyclQueue) -> SyclError>(b"sycl_release_queue\0")
                .map_err(|e| format!("Failed to load sycl_release_queue: {}", e))?);
            let create_buffer = std::mem::transmute(lib.get::<unsafe extern "C" fn(SyclQueue, usize, *mut SyclBuffer) -> SyclError>(b"sycl_create_buffer\0")
                .map_err(|e| format!("Failed to load sycl_create_buffer: {}", e))?);
            let buffer_write = std::mem::transmute(lib.get::<unsafe extern "C" fn(SyclQueue, SyclBuffer, *const c_void, usize, usize) -> SyclError>(b"sycl_buffer_write\0")
                .map_err(|e| format!("Failed to load sycl_buffer_write: {}", e))?);
            let buffer_read = std::mem::transmute(lib.get::<unsafe extern "C" fn(SyclQueue, SyclBuffer, *mut c_void, usize, usize) -> SyclError>(b"sycl_buffer_read\0")
                .map_err(|e| format!("Failed to load sycl_buffer_read: {}", e))?);
            let release_buffer = std::mem::transmute(lib.get::<unsafe extern "C" fn(SyclBuffer) -> SyclError>(b"sycl_release_buffer\0")
                .map_err(|e| format!("Failed to load sycl_release_buffer: {}", e))?);
            let release_event = std::mem::transmute(lib.get::<unsafe extern "C" fn(SyclEvent) -> SyclError>(b"sycl_release_event\0")
                .map_err(|e| format!("Failed to load sycl_release_event: {}", e))?);
            let get_kernel_time = std::mem::transmute(lib.get::<unsafe extern "C" fn(SyclEvent, *mut u64, *mut u64) -> SyclError>(b"sycl_get_kernel_time\0")
                .map_err(|e| format!("Failed to load sycl_get_kernel_time: {}", e))?);
            let matmul_f32_async = std::mem::transmute(lib.get::<unsafe extern "C" fn(SyclQueue, SyclBuffer, SyclBuffer, SyclBuffer, u32, u32, u32, *mut SyclEvent) -> SyclError>(b"sycl_matmul_f32_async\0")
                .map_err(|e| format!("Failed to load sycl_matmul_f32_async: {}", e))?);
            let ternary_matmul_async = std::mem::transmute(lib.get::<unsafe extern "C" fn(SyclQueue, SyclBuffer, SyclBuffer, SyclBuffer, u32, u32, u32, *mut SyclEvent) -> SyclError>(b"sycl_ternary_matmul_async\0")
                .map_err(|e| format!("Failed to load sycl_ternary_matmul_async: {}", e))?);
            let get_last_error = std::mem::transmute(lib.get::<unsafe extern "C" fn() -> *const c_char>(b"sycl_get_last_error\0")
                .map_err(|e| format!("Failed to load sycl_get_last_error: {}", e))?);
            let clear_error = std::mem::transmute(lib.get::<unsafe extern "C" fn()>(b"sycl_clear_error\0")
                .map_err(|e| format!("Failed to load sycl_clear_error: {}", e))?);

            Ok(Self {
                _lib: lib,
                backend_type,
                discover_devices,
                get_device_count,
                get_device,
                get_device_info,
                release_device,
                create_queue,
                queue_wait,
                release_queue,
                create_buffer,
                buffer_write,
                buffer_read,
                release_buffer,
                release_event,
                get_kernel_time,
                matmul_f32_async,
                ternary_matmul_async,
                get_last_error,
                clear_error,
            })
        }
    }

    /// Find library in common locations
    fn find_library(lib_name: &str) -> Result<PathBuf, String> {
        // Search paths in order of preference
        let search_paths = vec![
            // Current directory (for development)
            std::env::current_dir().ok(),
            // Executable directory
            std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf())),
            // System library paths
            Some(PathBuf::from("/usr/local/lib")),
            Some(PathBuf::from("/usr/lib")),
        ];

        for maybe_path in search_paths {
            if let Some(path) = maybe_path {
                let lib_path = path.join(lib_name);
                if lib_path.exists() {
                    return Ok(lib_path);
                }
            }
        }

        Err(format!("{} not found in search paths", lib_name))
    }

    /// Get backend type
    pub fn backend_type(&self) -> BackendType {
        self.backend_type
    }

    /// Discover devices using this backend
    pub fn discover_devices(&self) -> Result<Vec<DeviceInfo>, SyclError> {
        unsafe {
            let err = (self.discover_devices)();
            if err != SyclError::Success {
                return Err(err);
            }

            let mut count = 0u32;
            let err = (self.get_device_count)(&mut count);
            if err != SyclError::Success {
                return Err(err);
            }

            let mut devices = Vec::new();
            for i in 0..count {
                let mut device: SyclDevice = std::ptr::null_mut();
                let err = (self.get_device)(i, &mut device);
                if err != SyclError::Success {
                    continue;
                }

                let mut name = vec![0i8; 256];
                let mut backend: i32 = 0;
                let err = (self.get_device_info)(
                    device,
                    name.as_mut_ptr(),
                    name.len(),
                    &mut backend,
                );

                let device_name = if err == SyclError::Success {
                    CStr::from_ptr(name.as_ptr())
                        .to_string_lossy()
                        .to_string()
                } else {
                    "Unknown".to_string()
                };

                devices.push(DeviceInfo {
                    index: i,
                    name: device_name,
                    backend: match backend {
                        0 => SyclBackend::OpenCL,
                        1 => SyclBackend::CUDA,
                        2 => SyclBackend::HIP,
                        3 => SyclBackend::LevelZero,
                        4 => SyclBackend::CPU,
                        _ => SyclBackend::OpenCL,
                    },
                    backend_lib: self.backend_type,
                });

                let _ = (self.release_device)(device);
            }

            Ok(devices)
        }
    }
}

/// Device information from backend discovery
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub index: u32,
    pub name: String,
    pub backend: SyclBackend,
    pub backend_lib: BackendType,
}


/// Wrapper for raw SYCL handles to make them Send/Sync
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyclHandle(pub *mut c_void);
unsafe impl Send for SyclHandle {}
unsafe impl Sync for SyclHandle {}

/// Internal struct to track active SYCL jobs and their associated resources for cleanup
struct ActiveJob {
    backend_lib: Arc<SyclBackendLib>,
    queue: SyclHandle,
    buffers: Vec<SyclHandle>,
    event: SyclHandle,
    device: SyclHandle,
    output_size: usize,
    operation: String,
    capability: Capability,
}

unsafe impl Send for ActiveJob {}
unsafe impl Sync for ActiveJob {}

/// SYCL backend manager - selects appropriate backend per device
pub struct SyclBackendManager {
    intel_backend: Option<Arc<SyclBackendLib>>,
    adaptive_backend: Option<Arc<SyclBackendLib>>,
    jobs: Arc<RwLock<HashMap<String, ActiveJob>>>,
    job_counter: AtomicU64,
}

impl SyclBackendManager {
    /// Create new backend manager
    pub fn new() -> Self {
        // Filter backends based on NINEPE_SYCL_BACKENDS env var
        // Example: NINEPE_SYCL_BACKENDS=intel,adaptive
        let enabled_backends = std::env::var("NINEPE_SYCL_BACKENDS")
            .unwrap_or_else(|_| "intel,adaptive".to_string())
            .to_lowercase();
        
        let intel_enabled = enabled_backends.contains("intel") || enabled_backends.contains("all");
        let adaptive_enabled = enabled_backends.contains("adaptive") || enabled_backends.contains("all");

        // Try to load backends if enabled
        let intel_backend = if intel_enabled {
            match SyclBackendLib::load(BackendType::IntelOneAPI) {
                Ok(lib) => {
                    info!("Intel oneAPI backend loaded successfully");
                    Some(Arc::new(lib))
                }
                Err(e) => {
                    warn!("Intel oneAPI backend not available: {}", e);
                    None
                }
            }
        } else {
            info!("Intel oneAPI backend disabled via NINEPE_SYCL_BACKENDS");
            None
        };

        let adaptive_backend = if adaptive_enabled {
            match SyclBackendLib::load(BackendType::AdaptiveCpp) {
                Ok(lib) => {
                    info!("AdaptiveCpp backend loaded successfully");
                    Some(Arc::new(lib))
                }
                Err(e) => {
                    warn!("AdaptiveCpp backend not available: {}", e);
                    None
                }
            }
        } else {
            info!("AdaptiveCpp backend disabled via NINEPE_SYCL_BACKENDS");
            None
        };

        if intel_backend.is_none() && adaptive_backend.is_none() {
            warn!("No SYCL backends available - GPU acceleration disabled");
        }

        Self {
            intel_backend,
            adaptive_backend,
            jobs: Arc::new(RwLock::new(HashMap::new())),
            job_counter: AtomicU64::new(0),
        }
    }

    /// Discover all devices across all backends
    pub fn discover_all_devices(&self) -> Vec<DeviceInfo> {
        let mut all_devices = Vec::new();

        // Discover Intel devices
        if let Some(ref backend) = self.intel_backend {
            if let Ok(devices) = backend.discover_devices() {
                all_devices.extend(devices);
            }
        }

        // Discover AdaptiveCpp devices
        if let Some(ref backend) = self.adaptive_backend {
            if let Ok(devices) = backend.discover_devices() {
                all_devices.extend(devices);
            }
        }

        all_devices
    }

    /// Select best backend for a GPU based on vendor/type
    pub fn select_backend_for_device(&self, device_name: &str) -> Option<Arc<SyclBackendLib>> {
        let device_lower = device_name.to_lowercase();

        // Intel GPU detection
        if device_lower.contains("intel") || device_lower.contains("arc") || device_lower.contains("iris") {
            if let Some(ref backend) = self.intel_backend {
                info!("Selected Intel oneAPI backend for Intel GPU: {}", device_name);
                return Some(Arc::clone(backend));
            }
            // Fallback to AdaptiveCpp if Intel backend not available
            if let Some(ref backend) = self.adaptive_backend {
                warn!("Intel GPU detected but oneAPI backend not available, using AdaptiveCpp fallback");
                return Some(Arc::clone(backend));
            }
        }

        // NVIDIA GPU detection
        if device_lower.contains("nvidia") || device_lower.contains("geforce") || device_lower.contains("quadro") || device_lower.contains("tesla") {
            if let Some(ref backend) = self.adaptive_backend {
                info!("Selected AdaptiveCpp backend for NVIDIA GPU: {}", device_name);
                return Some(Arc::clone(backend));
            }
        }

        // AMD GPU detection
        if device_lower.contains("amd") || device_lower.contains("radeon") || device_lower.contains("instinct") {
            if let Some(ref backend) = self.adaptive_backend {
                info!("Selected AdaptiveCpp backend for AMD GPU: {}", device_name);
                return Some(Arc::clone(backend));
            }
        }

        // Default fallback: prefer Intel, then AdaptiveCpp
        if let Some(ref backend) = self.intel_backend {
            warn!("Unknown GPU type, defaulting to Intel oneAPI backend: {}", device_name);
            return Some(Arc::clone(backend));
        }
        if let Some(ref backend) = self.adaptive_backend {
            warn!("Unknown GPU type, defaulting to AdaptiveCpp backend: {}", device_name);
            return Some(Arc::clone(backend));
        }

        warn!("No suitable SYCL backend available for device: {}", device_name);
        None
    }

    /// Get Intel backend if available
    pub fn intel_backend(&self) -> Option<Arc<SyclBackendLib>> {
        self.intel_backend.as_ref().map(Arc::clone)
    }

    /// Get AdaptiveCpp backend if available
    pub fn adaptive_backend(&self) -> Option<Arc<SyclBackendLib>> {
        self.adaptive_backend.as_ref().map(Arc::clone)
    }

    /// Check if any backend is available
    pub fn has_any_backend(&self) -> bool {
        self.intel_backend.is_some() || self.adaptive_backend.is_some()
    }
}

use crate::traits::{ComputeBackend, ComputeJob, JobStatus, DeviceInfo as GenericDeviceInfo};
use async_trait::async_trait;
use anyhow::Result;

#[async_trait]
impl ComputeBackend for SyclBackendManager {
    fn discover_devices(&self) -> Result<Vec<GenericDeviceInfo>> {
        let devices = self.discover_all_devices();
        Ok(devices.into_iter().map(|d| GenericDeviceInfo {
            id: format!("{:?}_{}", d.backend_lib, d.index),
            name: d.name,
            is_gpu: match d.backend {
                crate::sycl::SyclBackend::OpenCL |
                crate::sycl::SyclBackend::CUDA |
                crate::sycl::SyclBackend::HIP |
                crate::sycl::SyclBackend::LevelZero => true,
                crate::sycl::SyclBackend::CPU => false,
            },
            memory: 0, // Need to implement memory detection in backend_loader
        }).collect())
    }

    async fn submit_job(&self, job: ComputeJob) -> Result<String> {
        let device_id = job.id.clone(); 
        
        // Extract capability from job metadata if present, else default
        // In a real Lux9 system, this would be validated against a ProcessVault
        let required_capability = Capability::BasicCompute; 
        
        // Parse device_id to find backend and index
        let parts: Vec<&str> = device_id.split('_').collect();
        if parts.len() < 2 {
            anyhow::bail!("Invalid device ID: {}", device_id);
        }
        
        let backend_type_str = parts[0];
        let device_index: u32 = parts[1].parse().map_err(|_| anyhow::anyhow!("Invalid device index: {}", parts[1]))?;
        
        let backend_lib = match backend_type_str {
            "IntelOneAPI" => self.intel_backend.as_ref().ok_or_else(|| anyhow::anyhow!("Intel backend requested but not available"))?,
            "AdaptiveCpp" => self.adaptive_backend.as_ref().ok_or_else(|| anyhow::anyhow!("AdaptiveCpp backend requested but not available"))?,
            _ => anyhow::bail!("Unknown backend type: {}", backend_type_str),
        };
        
        let backend_lib = Arc::clone(backend_lib);
        
        let result: anyhow::Result<(String, ActiveJob)> = unsafe {
            let mut device_ptr: SyclDevice = std::ptr::null_mut();
            let err = (backend_lib.get_device)(device_index, &mut device_ptr);
            if err != SyclError::Success {
                anyhow::bail!("Failed to get SYCL device {}: {}", device_index, err);
            }
            let device = SyclHandle(device_ptr);
            
            let mut queue_ptr: SyclQueue = std::ptr::null_mut();
            let err = (backend_lib.create_queue)(device.0, &mut queue_ptr);
            if err != SyclError::Success {
                let _ = (backend_lib.release_device)(device.0);
                anyhow::bail!("Failed to create SYCL queue: {}", err);
            }
            let queue = SyclHandle(queue_ptr);
            
            // Handle different job operations
            let (rust_job_id, active_job) = match job.operation.as_str() {
                "matrix_multiply" | "ternary_matmul" => {
                    let value: serde_json::Value = serde_json::from_slice(&job.params)?;
                    let a_vals = value.get("a").and_then(|v| v.as_array()).ok_or_else(|| anyhow::anyhow!("Missing 'a'"))?;
                    let b_vals = value.get("b").and_then(|v| v.as_array()).ok_or_else(|| anyhow::anyhow!("Missing 'b'"))?;
                    let m = value.get("m").and_then(|v| v.as_u64()).ok_or_else(|| anyhow::anyhow!("Missing 'm'"))? as u32;
                    let n = value.get("n").and_then(|v| v.as_u64()).ok_or_else(|| anyhow::anyhow!("Missing 'n'"))? as u32;
                    let k = value.get("k").and_then(|v| v.as_u64()).ok_or_else(|| anyhow::anyhow!("Missing 'k'"))? as u32;
                    
                    let is_ternary = job.operation == "ternary_matmul";
                    
                    let (bytes_a, bytes_b, bytes_c) = if is_ternary {
                        ((m * k) as usize, (k * n) as usize, (m * n * 4) as usize)
                    } else {
                        ((m * k * 4) as usize, (k * n * 4) as usize, (m * n * 4) as usize)
                    };
                    
                    let mut buf_a_ptr: SyclBuffer = std::ptr::null_mut();
                    let mut buf_b_ptr: SyclBuffer = std::ptr::null_mut();
                    let mut buf_c_ptr: SyclBuffer = std::ptr::null_mut();
                    
                    (backend_lib.create_buffer)(queue.0, bytes_a, &mut buf_a_ptr);
                    (backend_lib.create_buffer)(queue.0, bytes_b, &mut buf_b_ptr);
                    (backend_lib.create_buffer)(queue.0, bytes_c, &mut buf_c_ptr);
                    
                    let buf_a = SyclHandle(buf_a_ptr);
                    let buf_b = SyclHandle(buf_b_ptr);
                    let buf_c = SyclHandle(buf_c_ptr);
                    
                    // Write data
                    if let Some(ref shm) = job.shm_handle {
                        // ZERO-COPY: Write directly from shared memory handle to SYCL buffers
                        let data = shm.as_slice();
                        if data.len() < bytes_a + bytes_b {
                            anyhow::bail!("Shared memory region too small for matrices (needs {}, has {})", bytes_a + bytes_b, data.len());
                        }
                        
                        (backend_lib.buffer_write)(queue.0, buf_a.0, data.as_ptr() as *const c_void, 0, bytes_a);
                        (backend_lib.buffer_write)(queue.0, buf_b.0, unsafe { data.as_ptr().add(bytes_a) } as *const c_void, 0, bytes_b);
                    } else if is_ternary {
                        let a: Vec<i8> = a_vals.iter().filter_map(|v| v.as_i64().map(|i| i as i8)).collect();
                        let b: Vec<i8> = b_vals.iter().filter_map(|v| v.as_i64().map(|i| i as i8)).collect();
                        (backend_lib.buffer_write)(queue.0, buf_a.0, a.as_ptr() as *const c_void, 0, bytes_a);
                        (backend_lib.buffer_write)(queue.0, buf_b.0, b.as_ptr() as *const c_void, 0, bytes_b);
                    } else {
                        let a: Vec<f32> = a_vals.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect();
                        let b: Vec<f32> = b_vals.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect();
                        (backend_lib.buffer_write)(queue.0, buf_a.0, a.as_ptr() as *const c_void, 0, bytes_a);
                        (backend_lib.buffer_write)(queue.0, buf_b.0, b.as_ptr() as *const c_void, 0, bytes_b);
                    }
                    
                    let mut event_ptr: SyclEvent = std::ptr::null_mut();
                    let err = if is_ternary {
                        (backend_lib.ternary_matmul_async)(queue.0, buf_a.0, buf_b.0, buf_c.0, m, n, k, &mut event_ptr)
                    } else {
                        (backend_lib.matmul_f32_async)(queue.0, buf_a.0, buf_b.0, buf_c.0, m, n, k, &mut event_ptr)
                    };
                    
                    if err != SyclError::Success {
                        (backend_lib.release_buffer)(buf_a.0);
                        (backend_lib.release_buffer)(buf_b.0);
                        (backend_lib.release_buffer)(buf_c.0);
                        (backend_lib.release_queue)(queue.0);
                        (backend_lib.release_device)(device.0);
                        anyhow::bail!("SYCL operation failed: {}", err);
                    }
                    
                    let event = SyclHandle(event_ptr);
                    
                    // Use UUID v8 to encode capability and monotonic counter
                    let counter = self.job_counter.fetch_add(1, Ordering::SeqCst);
                    let mut uuid_bytes = [0u8; 16];
                    // Use 48 bits of the counter for the timestamp/sequence field
                    uuid_bytes[0..6].copy_from_slice(&counter.to_be_bytes()[2..8]);
                    uuid_bytes[6] = 0x80 | ((required_capability as u16 >> 12) as u8 & 0x0F);
                    uuid_bytes[7] = (required_capability as u16 >> 4) as u8;
                    uuid_bytes[8] = 0x80 | (((required_capability as u16) as u8 & 0x0F) << 2);
                    let mut entropy = [0u8; 7];
                    rand::thread_rng().fill_bytes(&mut entropy);
                    uuid_bytes[9..16].copy_from_slice(&entropy);
                    
                    let rust_job_id = Uuid::from_bytes(uuid_bytes).to_string();
                    let active_job = ActiveJob {
                        backend_lib,
                        queue,
                        buffers: vec![buf_a, buf_b, buf_c],
                        event,
                        device,
                        output_size: bytes_c,
                        operation: job.operation.clone(),
                        capability: required_capability,
                    };
                    
                    (rust_job_id, active_job)
                }
                other => {
                    (backend_lib.release_queue)(queue.0);
                    (backend_lib.release_device)(device.0);
                    anyhow::bail!("Unsupported SYCL operation: {}", other)
                }
            };
            
            Ok((rust_job_id, active_job))
        };

        let (rust_job_id, active_job) = result?;
        self.jobs.write().await.insert(rust_job_id.clone(), active_job);
        Ok(rust_job_id)
    }

    async fn get_job_status(&self, job_id: &str) -> Option<JobStatus> {
        let mut jobs = self.jobs.write().await;
        let job = jobs.get_mut(job_id)?;
        
        unsafe {
            let err = (job.backend_lib.queue_wait)(job.queue.0);
            if err != SyclError::Success {
                let reason = format!("Queue wait failed: {}", err);
                for buf in job.buffers.drain(..) {
                    (job.backend_lib.release_buffer)(buf.0);
                }
                (job.backend_lib.release_event)(job.event.0);
                (job.backend_lib.release_queue)(job.queue.0);
                (job.backend_lib.release_device)(job.device.0);
                
                return Some(JobStatus::Failed(reason));
            }
            
            let result_buf = *job.buffers.last().unwrap();
            let mut result_data = vec![0u8; job.output_size];
            let err = (job.backend_lib.buffer_read)(job.queue.0, result_buf.0, result_data.as_mut_ptr() as *mut c_void, 0, job.output_size);
            
            if err != SyclError::Success {
                let reason = format!("Buffer read failed: {}", err);
                for buf in job.buffers.drain(..) {
                    (job.backend_lib.release_buffer)(buf.0);
                }
                (job.backend_lib.release_event)(job.event.0);
                (job.backend_lib.release_queue)(job.queue.0);
                (job.backend_lib.release_device)(job.device.0);
                return Some(JobStatus::Failed(reason));
            }
            
            // Success! 
            for buf in job.buffers.drain(..) {
                (job.backend_lib.release_buffer)(buf.0);
            }
            (job.backend_lib.release_event)(job.event.0);
            (job.backend_lib.release_queue)(job.queue.0);
            (job.backend_lib.release_device)(job.device.0);
            
            let floats: Vec<f32> = result_data.chunks_exact(4).map(|chunk| {
                f32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
            }).collect();
            
            let result_json = serde_json::to_vec(&serde_json::json!({
                "values": floats
            })).unwrap_or_default();
            
            Some(JobStatus::Completed(result_json))
        }
    }
}
