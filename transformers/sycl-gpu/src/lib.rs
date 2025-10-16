//! SYCL GPU Compute WASM Transformer for 9P.e
//!
//! This transformer exposes SYCL compute capabilities through the 9P.e filesystem,
//! allowing programs to interact with accelerators as synthetic files.

use std::collections::HashMap;
use serde::{Serialize, Deserialize};

// WASM-exposed memory management
static mut MEMORY: Vec<u8> = Vec::new();
static mut RESULT_BUFFER: Vec<u8> = Vec::new();
static mut GPU_FS: Option<GpuFileSystem> = None;

/// Directory structure for the GPU compute filesystem
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct GpuFileSystem {
    devices: Vec<DeviceInfo>,
    kernels: HashMap<String, KernelInfo>,
    buffers: HashMap<String, BufferInfo>,
    jobs: HashMap<String, JobInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DeviceInfo {
    id: String,
    name: String,
    vendor: String,
    backend: String,
    device_type: String,
    compute_units: u32,
    max_work_group_size: usize,
    global_mem_size: u64,
    local_mem_size: u64,
    capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct KernelInfo {
    name: String,
    source: String,
    compiled: bool,
    parameters: Vec<KernelParameter>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct KernelParameter {
    name: String,
    param_type: String,
    direction: String, // "in", "out", "inout"
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct BufferInfo {
    id: String,
    size: usize,
    device_id: String,
    flags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JobInfo {
    id: String,
    kernel: String,
    status: JobStatus,
    device_id: String,
    work_dims: Vec<usize>,
    execution_time_ns: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed(String),
}

/// Standard kernels provided by the transformer
const MATRIX_MULTIPLY_KERNEL: &str = r#"#include <sycl/sycl.hpp>

void matrix_multiply(sycl::queue& q,
                     sycl::buffer<float>& a,
                     sycl::buffer<float>& b,
                     sycl::buffer<float>& c,
                     int M, int N, int K) {
    q.submit([&](sycl::handler& h) {
        auto acc_a = a.get_access<sycl::access::mode::read>(h);
        auto acc_b = b.get_access<sycl::access::mode::read>(h);
        auto acc_c = c.get_access<sycl::access::mode::write>(h);
        h.parallel_for(sycl::range<2>(M, N), [=](sycl::id<2> id) {
            int row = id[0];
            int col = id[1];
            float sum = 0.0f;
            for (int k = 0; k < K; ++k) {
                sum += acc_a[row * K + k] * acc_b[k * N + col];
            }
            acc_c[row * N + col] = sum;
        });
    });
}
"#;

const VECTOR_ADD_KERNEL: &str = r#"#include <sycl/sycl.hpp>

void vector_add(sycl::queue& q,
                sycl::buffer<float>& a,
                sycl::buffer<float>& b,
                sycl::buffer<float>& c,
                int n) {
    q.submit([&](sycl::handler& h) {
        auto acc_a = a.get_access<sycl::access::mode::read>(h);
        auto acc_b = b.get_access<sycl::access::mode::read>(h);
        auto acc_c = c.get_access<sycl::access::mode::write>(h);
        h.parallel_for(sycl::range<1>(n), [=](sycl::id<1> idx) {
            acc_c[idx] = acc_a[idx] + acc_b[idx];
        });
    });
}
"#;

const FFT_KERNEL: &str = r#"#include <sycl/sycl.hpp>
#include <complex>

void fft_radix2(sycl::queue& q,
                sycl::buffer<std::complex<float>>& data,
                int n) {
    q.submit([&](sycl::handler& h) {
        auto acc = data.get_access<sycl::access::mode::read_write>(h);
        h.parallel_for(sycl::range<1>(n), [=](sycl::id<1> idx) {
            // Placeholder: real implementation would perform the butterfly operations
            acc[idx] = std::complex<float>(acc[idx].real(), acc[idx].imag());
        });
    });
}
"#;

const REDUCTION_KERNEL: &str = r#"#include <sycl/sycl.hpp>

void reduce_sum(sycl::queue& q,
                sycl::buffer<float>& input,
                sycl::buffer<float>& output,
                int n) {
    q.submit([&](sycl::handler& h) {
        auto in = input.get_access<sycl::access::mode::read>(h);
        auto out = output.get_access<sycl::access::mode::write>(h);
        sycl::local_accessor<float, 1> scratch(sycl::range<1>(h.get_local_range().size()), h);

        h.parallel_for(sycl::nd_range<1>(sycl::range<1>(n), sycl::range<1>(256)),
                       [=](sycl::nd_item<1> item) {
            size_t lid = item.get_local_id(0);
            size_t gid = item.get_global_id(0);
            scratch[lid] = (gid < n) ? in[gid] : 0.0f;
            item.barrier(sycl::access::fence_space::local_space);

            for (size_t stride = item.get_local_range(0) / 2; stride > 0; stride >>= 1) {
                if (lid < stride) {
                    scratch[lid] += scratch[lid + stride];
                }
                item.barrier(sycl::access::fence_space::local_space);
            }

            if (lid == 0) {
                out[item.get_group(0)] = scratch[0];
            }
        });
    });
}
"#;

/// WASM export: Initialize the transformer
#[no_mangle]
pub extern "C" fn init() -> i32 {
    unsafe {
        MEMORY.clear();
        RESULT_BUFFER.clear();
        if GPU_FS.is_none() {
            GPU_FS = Some(default_gpu_filesystem());
        }
    }
    0
}

/// WASM export: Handle stat request for a path
#[no_mangle]
pub extern "C" fn handle_stat(path_ptr: *const u8, path_len: usize) -> i32 {
    let path = unsafe {
        std::str::from_utf8(std::slice::from_raw_parts(path_ptr, path_len))
            .unwrap_or("")
    };

    let stat_info = match path {
        "/" => StatInfo {
            mode: 0o040755, // Directory
            size: 0,
            is_dir: true,
            name: "/".to_string(),
        },
        "/gpu" => StatInfo {
            mode: 0o040755,
            size: 0,
            is_dir: true,
            name: "gpu".to_string(),
        },
        "/gpu/devices" => StatInfo {
            mode: 0o040755,
            size: 0,
            is_dir: true,
            name: "devices".to_string(),
        },
        "/gpu/kernels" => StatInfo {
            mode: 0o040755,
            size: 0,
            is_dir: true,
            name: "kernels".to_string(),
        },
        "/gpu/buffers" => StatInfo {
            mode: 0o040755,
            size: 0,
            is_dir: true,
            name: "buffers".to_string(),
        },
        "/gpu/jobs" => StatInfo {
            mode: 0o040755,
            size: 0,
            is_dir: true,
            name: "jobs".to_string(),
        },
        "/gpu/devices/info" => StatInfo {
            mode: 0o100644,
            size: 0,
            is_dir: false,
            name: "info".to_string(),
        },
        "/gpu/devices/register" | "/gpu/devices/remove" => StatInfo {
            mode: 0o100644,
            size: 0,
            is_dir: false,
            name: path.trim_start_matches("/gpu/devices/").to_string(),
        },
        "/gpu/compute/submit" => StatInfo {
            mode: 0o100644,
            size: 0,
            is_dir: false,
            name: "submit".to_string(),
        },
        _ => {
            if let Some(device_id) = path.strip_prefix("/gpu/devices/") {
                let size = with_fs(|fs| {
                    fs.devices
                        .iter()
                        .find(|d| d.id == device_id)
                        .map(|d| serde_json::to_string(d).unwrap_or_default().len() as u64)
                        .unwrap_or(0)
                });
                if size > 0 {
                    StatInfo {
                        mode: 0o100644,
                        size,
                        is_dir: false,
                        name: device_id.to_string(),
                    }
                } else {
                    return -1;
                }
            } else if path.starts_with("/gpu/kernels/") {
                let kernel_name = path.trim_start_matches("/gpu/kernels/");
                StatInfo {
                    mode: 0o100644,
                    size: get_kernel_size(kernel_name),
                    is_dir: false,
                    name: kernel_name.to_string(),
                }
            } else {
                return -1; // Not found
            }
        }
    };

    // Serialize stat info to result buffer
    let serialized = bincode::serialize(&stat_info).unwrap_or_default();
    unsafe {
        RESULT_BUFFER = serialized;
    }

    0
}

/// WASM export: Read file request
#[no_mangle]
pub extern "C" fn read_file(
    path_ptr: *const u8,
    path_len: usize,
    offset: u64,
    count: u32
) -> i32 {
    let path = unsafe {
        std::str::from_utf8(std::slice::from_raw_parts(path_ptr, path_len))
            .unwrap_or("")
    };

    let content = match path {
        "/gpu/devices/info" => {
            let payload = with_fs(|fs| serde_json::json!({ "devices": fs.devices }));
            payload.to_string().into_bytes()
        }
        _ if path.starts_with("/gpu/devices/") => {
            let device_id = path.trim_start_matches("/gpu/devices/");
            match with_fs(|fs| fs.devices.iter().find(|d| d.id == device_id).cloned()) {
                Some(device) => serde_json::to_vec(&device).unwrap_or_default(),
                None => return -1,
            }
        }
        "/gpu/kernels/matrix_multiply" => MATRIX_MULTIPLY_KERNEL.as_bytes().to_vec(),
        "/gpu/kernels/vector_add" => VECTOR_ADD_KERNEL.as_bytes().to_vec(),
        "/gpu/kernels/fft" => FFT_KERNEL.as_bytes().to_vec(),
        "/gpu/kernels/reduce" => REDUCTION_KERNEL.as_bytes().to_vec(),
        _ => return -1,
    };

    // Apply offset and count
    let start = offset as usize;
    let end = std::cmp::min(start + count as usize, content.len());

    if start >= content.len() {
        unsafe {
            RESULT_BUFFER.clear();
        }
        return 0;
    }

    unsafe {
        RESULT_BUFFER = content[start..end].to_vec();
    }

    unsafe { RESULT_BUFFER.len() as i32 }
}

/// WASM export: Write file request (for job submission)
#[no_mangle]
pub extern "C" fn write_file(
    path_ptr: *const u8,
    path_len: usize,
    data_ptr: *const u8,
    data_len: usize,
    _offset: u64
) -> i32 {
    let path = unsafe {
        std::str::from_utf8(std::slice::from_raw_parts(path_ptr, path_len))
            .unwrap_or("")
    };

    let data = unsafe {
        std::slice::from_raw_parts(data_ptr, data_len)
    };

    match path {
        "/gpu/compute/submit" => {
            // Parse job submission request
            if let Ok(job_request) = serde_json::from_slice::<JobRequest>(data) {
                let job_id = format!("job_{}", generate_job_id());

                with_fs_mut(|fs| {
                    fs.jobs.insert(
                        job_id.clone(),
                        JobInfo {
                            id: job_id.clone(),
                            kernel: job_request.kernel.clone(),
                            status: JobStatus::Pending,
                            device_id: job_request.device_id.clone(),
                            work_dims: job_request.work_dims.clone(),
                            execution_time_ns: None,
                        },
                    );
                });

                // In a real implementation, this would queue the job for SYCL execution
                let response = JobResponse {
                    job_id: job_id.clone(),
                    status: "submitted".to_string(),
                    message: format!("Job {} queued for execution on {}", job_id, job_request.device_id),
                };

                let serialized = serde_json::to_vec(&response).unwrap_or_default();
                let len = serialized.len() as i32;
                unsafe {
                    RESULT_BUFFER = serialized;
                }

                return len;
            }
        },
        "/gpu/devices/register" => {
            if let Ok(device) = serde_json::from_slice::<DeviceInfo>(data) {
                with_fs_mut(|fs| {
                    if let Some(existing) = fs.devices.iter_mut().find(|d| d.id == device.id) {
                        *existing = device.clone();
                    } else {
                        fs.devices.push(device.clone());
                    }
                });

                let response = serde_json::json!({
                    "status": "registered"
                });
                unsafe {
                    RESULT_BUFFER = response.to_string().into_bytes();
                }
                unsafe {
                    return RESULT_BUFFER.len() as i32;
                }
            }
        },
        "/gpu/devices/remove" => {
            if let Ok(payload) = serde_json::from_slice::<serde_json::Value>(data) {
                if let Some(id) = payload.get("id").and_then(|v| v.as_str()) {
                    with_fs_mut(|fs| {
                        fs.devices.retain(|d| d.id != id);
                    });
                    let response = serde_json::json!({
                        "status": "removed",
                        "id": id
                    });
                    unsafe {
                        RESULT_BUFFER = response.to_string().into_bytes();
                    }
                    unsafe {
                        return RESULT_BUFFER.len() as i32;
                    }
                }
            }
        },
        "/gpu/buffers/create" => {
            // Handle buffer creation
            if let Ok(buffer_request) = serde_json::from_slice::<BufferRequest>(data) {
                let buffer_id = format!("buf_{}", generate_buffer_id());

                let response = BufferResponse {
                    buffer_id: buffer_id.clone(),
                    allocated_size: buffer_request.size,
                    device: buffer_request.device_id,
                };

                let serialized = serde_json::to_vec(&response).unwrap_or_default();
                let len = serialized.len() as i32;
                unsafe {
                    RESULT_BUFFER = serialized;
                }

                return len;
            }
        },
        _ => {}
    }

    -1
}

/// WASM export: List files in a directory
#[no_mangle]
pub extern "C" fn list_files(path_ptr: *const u8, path_len: usize) -> i32 {
    let path = unsafe {
        std::str::from_utf8(std::slice::from_raw_parts(path_ptr, path_len))
            .unwrap_or("")
    };

    match path {
        "/" => write_dir_entries(vec!["gpu".to_string()]),
        "/gpu" => write_dir_entries(vec![
            "devices".to_string(),
            "kernels".to_string(),
            "buffers".to_string(),
            "jobs".to_string(),
            "compute".to_string(),
        ]),
        "/gpu/devices" => {
            let mut entries = vec!["info".to_string(), "register".to_string(), "remove".to_string()];
            with_fs(|fs| {
                for device in &fs.devices {
                    entries.push(device.id.clone());
                }
            });
            write_dir_entries(entries)
        }
        "/gpu/kernels" => write_dir_entries(vec![
            "matrix_multiply".to_string(),
            "vector_add".to_string(),
            "fft".to_string(),
            "reduce".to_string(),
            "custom".to_string(),
        ]),
        "/gpu/buffers" => write_dir_entries(vec!["create".to_string(), "list".to_string()]),
        "/gpu/jobs" => write_dir_entries(vec![
            "submit".to_string(),
            "status".to_string(),
            "results".to_string(),
        ]),
        "/gpu/compute" => write_dir_entries(vec!["submit".to_string()]),
        _ => write_dir_entries(Vec::new()),
    }
}

/// WASM export: Get result buffer pointer
#[no_mangle]
pub extern "C" fn get_result_ptr() -> *const u8 {
    unsafe {
        RESULT_BUFFER.as_ptr()
    }
}

/// WASM export: Get result buffer length
#[no_mangle]
pub extern "C" fn get_result_len() -> usize {
    unsafe {
        RESULT_BUFFER.len()
    }
}

/// WASM export: Allocate memory for data transfer
#[no_mangle]
pub extern "C" fn alloc(size: usize) -> *mut u8 {
    unsafe {
        MEMORY.clear();
        MEMORY.reserve(size);
        MEMORY.set_len(size);
        MEMORY.as_mut_ptr()
    }
}

/// WASM export: Free allocated memory
#[no_mangle]
pub extern "C" fn dealloc(_ptr: *mut u8, _size: usize) {
    // Memory is managed by the MEMORY static
    unsafe {
        MEMORY.clear();
    }
}

// Helper structures for serialization

#[derive(Serialize, Deserialize)]
struct StatInfo {
    mode: u32,
    size: u64,
    is_dir: bool,
    name: String,
}

#[derive(Serialize, Deserialize)]
struct DirEntry {
    name: String,
    is_dir: bool,
}

#[derive(Serialize, Deserialize)]
struct JobRequest {
    kernel: String,
    device_id: String,
    work_dims: Vec<usize>,
    arguments: Vec<ArgumentData>,
}

#[derive(Serialize, Deserialize)]
struct ArgumentData {
    name: String,
    buffer_id: Option<String>,
    value: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize)]
struct JobResponse {
    job_id: String,
    status: String,
    message: String,
}

#[derive(Serialize, Deserialize)]
struct BufferRequest {
    size: usize,
    device_id: String,
    flags: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct BufferResponse {
    buffer_id: String,
    allocated_size: usize,
    device: String,
}

// Helper functions

fn get_kernel_size(kernel_name: &str) -> u64 {
    match kernel_name {
        "matrix_multiply" => MATRIX_MULTIPLY_KERNEL.len() as u64,
        "vector_add" => VECTOR_ADD_KERNEL.len() as u64,
        "fft" => FFT_KERNEL.len() as u64,
        "reduce" => REDUCTION_KERNEL.len() as u64,
        _ => 0,
    }
}

fn generate_job_id() -> u64 {
    // Simple incrementing counter for demo
    static mut COUNTER: u64 = 0;
    unsafe {
        COUNTER += 1;
        COUNTER
    }
}

fn generate_buffer_id() -> u64 {
    static mut COUNTER: u64 = 0;
    unsafe {
        COUNTER += 1;
        COUNTER
    }
}

fn default_gpu_filesystem() -> GpuFileSystem {
    GpuFileSystem {
        devices: default_devices(),
        ..GpuFileSystem::default()
    }
}

fn default_devices() -> Vec<DeviceInfo> {
    vec![
        DeviceInfo {
            id: "gpu0".to_string(),
            name: "SYCL Sample NVIDIA Adapter".to_string(),
            vendor: "NVIDIA".to_string(),
            backend: "cuda".to_string(),
            device_type: "GPU".to_string(),
            compute_units: 128,
            max_work_group_size: 1024,
            global_mem_size: 24_576_000_000u64,
            local_mem_size: 48_192,
            capabilities: vec!["fp64".to_string(), "atomics".to_string(), "images".to_string()],
        },
        DeviceInfo {
            id: "gpu1".to_string(),
            name: "SYCL Sample Intel Adapter".to_string(),
            vendor: "Intel".to_string(),
            backend: "level-zero".to_string(),
            device_type: "GPU".to_string(),
            compute_units: 96,
            max_work_group_size: 512,
            global_mem_size: 24_576_000_000u64,
            local_mem_size: 65_536,
            capabilities: vec!["fp64".to_string(), "matrix".to_string()],
        },
    ]
}

fn with_fs_mut<R>(f: impl FnOnce(&mut GpuFileSystem) -> R) -> R {
    unsafe {
        if GPU_FS.is_none() {
            GPU_FS = Some(default_gpu_filesystem());
        }
        f(GPU_FS.as_mut().unwrap())
    }
}

fn with_fs<R>(f: impl FnOnce(&GpuFileSystem) -> R) -> R {
    with_fs_mut(|fs| f(fs))
}

fn write_dir_entries(entries: Vec<String>) -> i32 {
    let dir_entries: Vec<DirEntry> = entries
        .into_iter()
        .map(|name| DirEntry {
            is_dir: !name.contains('.'),
            name,
        })
        .collect();

    let serialized = bincode::serialize(&dir_entries).unwrap_or_default();
    let len = serialized.len() as i32;
    unsafe {
        RESULT_BUFFER = serialized;
    }
    len
}

/// WASM export: Handle special operations (like GPU execution)
#[no_mangle]
pub extern "C" fn handle_special_op(op_ptr: *const u8, op_len: usize) -> i32 {
    let op_data = unsafe {
        std::slice::from_raw_parts(op_ptr, op_len)
    };

    // Parse operation request
    if let Ok(op) = serde_json::from_slice::<SpecialOperation>(op_data) {
        match op.op_type.as_str() {
            "execute_kernel" => {
                // In a real implementation, this would execute the requested SYCL kernel
                if let Some(job_id) = op.params.get("job_id").and_then(|v| v.as_str()) {
                    with_fs_mut(|fs| {
                        if let Some(job) = fs.jobs.get_mut(job_id) {
                            job.status = JobStatus::Completed;
                            job.execution_time_ns = Some(1_000_000);
                        }
                    });
                }

                let result = ExecutionResult {
                    success: true,
                    execution_time_ns: 1000000, // 1ms mock time
                    output: "Kernel executed successfully".to_string(),
                };

                let serialized = serde_json::to_vec(&result).unwrap_or_default();
                unsafe {
                    RESULT_BUFFER = serialized;
                }
                return 0;
            },
            "query_device" => {
                // Return illustrative device information
                let payload = if let Some(id) = op.params.get("id").and_then(|v| v.as_str()) {
                    with_fs(|fs| {
                        fs.devices
                            .iter()
                            .find(|d| d.id == id)
                            .map(|d| serde_json::to_value(d).unwrap_or_default())
                    })
                    .unwrap_or_else(|| serde_json::json!({ "error": "unknown device" }))
                } else {
                    serde_json::json!({
                        "extensions": ["sycl_ext_oneapi_bfloat16", "sycl_ext_oneapi_matrix"],
                        "profile": "FULL_PROFILE",
                        "version": "SYCL 2020"
                    })
                };

                let serialized = serde_json::to_vec(&payload).unwrap_or_default();
                unsafe {
                    RESULT_BUFFER = serialized;
                }
                return 0;
            },
            _ => {}
        }
    }

    -1
}

#[derive(Serialize, Deserialize)]
struct SpecialOperation {
    op_type: String,
    params: serde_json::Value,
}

#[derive(Serialize, Deserialize)]
struct ExecutionResult {
    success: bool,
    execution_time_ns: u64,
    output: String,
}
