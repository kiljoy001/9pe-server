//! OpenCL GPU Compute WASM Transformer for 9P.e
//!
//! This transformer exposes GPU compute capabilities through the 9P.e filesystem,
//! allowing programs to interact with GPUs as synthetic files.

use std::collections::HashMap;
use serde::{Serialize, Deserialize};

// WASM-exposed memory management
static mut MEMORY: Vec<u8> = Vec::new();
static mut RESULT_BUFFER: Vec<u8> = Vec::new();

/// Directory structure for the GPU compute filesystem
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GpuFileSystem {
    devices: Vec<DeviceInfo>,
    kernels: HashMap<String, KernelInfo>,
    buffers: HashMap<String, BufferInfo>,
    jobs: HashMap<String, JobInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeviceInfo {
    id: String,
    name: String,
    vendor: String,
    device_type: String,
    compute_units: u32,
    max_work_group_size: usize,
    global_mem_size: u64,
    local_mem_size: u64,
    capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KernelInfo {
    name: String,
    source: String,
    compiled: bool,
    parameters: Vec<KernelParameter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KernelParameter {
    name: String,
    param_type: String,
    direction: String, // "in", "out", "inout"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
const MATRIX_MULTIPLY_KERNEL: &str = r#"
__kernel void matrix_multiply(
    __global const float* A,
    __global const float* B,
    __global float* C,
    const int M,
    const int N,
    const int K
) {
    int row = get_global_id(0);
    int col = get_global_id(1);

    if (row < M && col < N) {
        float sum = 0.0f;
        for (int k = 0; k < K; k++) {
            sum += A[row * K + k] * B[k * N + col];
        }
        C[row * N + col] = sum;
    }
}
"#;

const VECTOR_ADD_KERNEL: &str = r#"
__kernel void vector_add(
    __global const float* a,
    __global const float* b,
    __global float* c,
    const int n
) {
    int id = get_global_id(0);
    if (id < n) {
        c[id] = a[id] + b[id];
    }
}
"#;

const FFT_KERNEL: &str = r#"
__kernel void fft_radix2(
    __global float2* data,
    const int n,
    const int log2n
) {
    int id = get_global_id(0);
    // Simplified FFT implementation
    // Full implementation would be much longer
}
"#;

const REDUCTION_KERNEL: &str = r#"
__kernel void reduce_sum(
    __global const float* input,
    __global float* output,
    __local float* scratch,
    const int n
) {
    int global_id = get_global_id(0);
    int local_id = get_local_id(0);
    int group_size = get_local_size(0);

    // Load data to local memory
    scratch[local_id] = (global_id < n) ? input[global_id] : 0;
    barrier(CLK_LOCAL_MEM_FENCE);

    // Reduction in local memory
    for (int stride = group_size / 2; stride > 0; stride >>= 1) {
        if (local_id < stride) {
            scratch[local_id] += scratch[local_id + stride];
        }
        barrier(CLK_LOCAL_MEM_FENCE);
    }

    // Write result
    if (local_id == 0) {
        output[get_group_id(0)] = scratch[0];
    }
}
"#;

/// WASM export: Initialize the transformer
#[no_mangle]
pub extern "C" fn init() -> i32 {
    unsafe {
        MEMORY.clear();
        RESULT_BUFFER.clear();
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
            size: 1024,
            is_dir: false,
            name: "info".to_string(),
        },
        "/gpu/compute/submit" => StatInfo {
            mode: 0o100644,
            size: 0,
            is_dir: false,
            name: "submit".to_string(),
        },
        _ => {
            // Check for kernel files
            if path.starts_with("/gpu/kernels/") {
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

/// WASM export: Handle read request
#[no_mangle]
pub extern "C" fn handle_read(
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
            // Mock device information - in real implementation would query OpenCL
            serde_json::json!({
                "devices": [
                    {
                        "id": "gpu0",
                        "name": "NVIDIA RTX 4090",
                        "vendor": "NVIDIA",
                        "type": "GPU",
                        "compute_units": 128,
                        "max_work_group_size": 1024,
                        "global_mem_size": 24576000000u64,
                        "local_mem_size": 49152,
                        "capabilities": ["fp64", "atomics", "image"]
                    },
                    {
                        "id": "gpu1",
                        "name": "AMD RX 7900 XTX",
                        "vendor": "AMD",
                        "type": "GPU",
                        "compute_units": 96,
                        "max_work_group_size": 256,
                        "global_mem_size": 24576000000u64,
                        "local_mem_size": 65536,
                        "capabilities": ["fp64", "atomics"]
                    }
                ]
            }).to_string().into_bytes()
        },
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

/// WASM export: Handle write request (for job submission)
#[no_mangle]
pub extern "C" fn handle_write(
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

                // In real implementation, this would queue the job for OpenCL execution
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

/// WASM export: Handle directory listing
#[no_mangle]
pub extern "C" fn handle_readdir(path_ptr: *const u8, path_len: usize) -> i32 {
    let path = unsafe {
        std::str::from_utf8(std::slice::from_raw_parts(path_ptr, path_len))
            .unwrap_or("")
    };

    let entries = match path {
        "/" => vec!["gpu"],
        "/gpu" => vec!["devices", "kernels", "buffers", "jobs", "compute"],
        "/gpu/devices" => vec!["info", "gpu0", "gpu1"],
        "/gpu/kernels" => vec!["matrix_multiply", "vector_add", "fft", "reduce", "custom"],
        "/gpu/buffers" => vec!["create", "list"],
        "/gpu/jobs" => vec!["submit", "status", "results"],
        "/gpu/compute" => vec!["submit"],
        _ => vec![],
    };

    let dir_entries: Vec<DirEntry> = entries
        .into_iter()
        .map(|name| DirEntry {
            name: name.to_string(),
            is_dir: !name.contains('.'),
        })
        .collect();

    let serialized = bincode::serialize(&dir_entries).unwrap_or_default();
    let len = serialized.len() as i32;
    unsafe {
        RESULT_BUFFER = serialized;
    }

    len
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
                // In real implementation, would execute OpenCL kernel
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
                // Return detailed device information
                let device_info = serde_json::json!({
                    "extensions": ["cl_khr_fp64", "cl_khr_global_int32_base_atomics"],
                    "profile": "FULL_PROFILE",
                    "version": "OpenCL 3.0"
                });

                let serialized = serde_json::to_vec(&device_info).unwrap_or_default();
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