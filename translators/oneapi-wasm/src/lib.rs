//! oneAPI/Level Zero WASM Translator for 9P.e
//!
//! This translator exposes Intel oneAPI compute through synthetic files.
//!
//! Policy (9P files):
//!   /intel/
//!     devices/          - List available devices
//!     compute/
//!       submit          - Submit compute job (write)
//!       results/        - Read results
//!     kernels/          - Available kernels
//!     buffers/          - Memory buffers
//!
//! Mechanism (host functions):
//!   - Server provides Level Zero/SYCL access via WASM host functions
//!   - Translator translates file operations to compute operations

use wasm_bindgen::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use web_sys::console;

/// Device information from Level Zero
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DeviceInfo {
    pub id: u32,
    pub name: String,
    pub vendor: String,
    pub compute_units: u32,
    pub max_memory: u64,
    pub device_type: String, // "gpu", "cpu", "accelerator"
}

/// Compute buffer
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BufferDescriptor {
    pub id: String,
    pub size: usize,
    pub device_id: u32,
    pub data_type: String, // "f32", "i32", "u32"
}

/// Compute job submission
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ComputeJob {
    pub kernel: String,
    pub device_id: u32,
    pub global_work_size: Vec<usize>,
    pub local_work_size: Option<Vec<usize>>,
    pub buffers: HashMap<String, String>, // arg_name -> buffer_id
    pub scalars: HashMap<String, f32>,
}

/// Job result
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct JobResult {
    pub job_id: String,
    pub status: String, // "queued", "running", "completed", "failed"
    pub output_buffers: HashMap<String, Vec<f32>>,
    pub error: Option<String>,
    pub execution_time_ms: Option<f64>,
}

/// oneAPI translator state
#[wasm_bindgen]
pub struct OneAPITranslator {
    devices: Vec<DeviceInfo>,
    buffers: HashMap<String, BufferDescriptor>,
    jobs: HashMap<String, JobResult>,
    next_buffer_id: u64,
    next_job_id: u64,
}

#[wasm_bindgen]
impl OneAPITranslator {
    /// Initialize translator - calls host function to discover devices
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        console::log_1(&"🚀 oneAPI WASM Translator initialized".into());

        Self {
            devices: Vec::new(),
            buffers: HashMap::new(),
            jobs: HashMap::new(),
            next_buffer_id: 0,
            next_job_id: 0,
        }
    }

    /// Read /intel/devices - list available devices
    #[wasm_bindgen]
    pub fn read_devices(&self) -> String {
        // In real implementation, this calls host function:
        // let devices = host_get_level_zero_devices();

        serde_json::to_string(&self.devices).unwrap_or_else(|_| "[]".to_string())
    }

    /// Write to /intel/buffers/new - create new buffer
    #[wasm_bindgen]
    pub fn create_buffer(&mut self, size: usize, device_id: u32, data_type: &str) -> String {
        let id = format!("buf_{}", self.next_buffer_id);
        self.next_buffer_id += 1;

        let descriptor = BufferDescriptor {
            id: id.clone(),
            size,
            device_id,
            data_type: data_type.to_string(),
        };

        // In real implementation: host_create_level_zero_buffer(&descriptor)

        self.buffers.insert(id.clone(), descriptor);

        console::log_1(&format!("Created buffer: {} ({} bytes on device {})", id, size, device_id).into());
        id
    }

    /// Write to /intel/buffers/{id} - write data to buffer
    #[wasm_bindgen]
    pub fn write_buffer(&mut self, buffer_id: &str, data: Vec<f32>) -> bool {
        if !self.buffers.contains_key(buffer_id) {
            console::log_1(&format!("Buffer not found: {}", buffer_id).into());
            return false;
        }

        // In real implementation: host_write_level_zero_buffer(buffer_id, data)

        console::log_1(&format!("Wrote {} floats to buffer {}", data.len(), buffer_id).into());
        true
    }

    /// Read from /intel/buffers/{id} - read data from buffer
    #[wasm_bindgen]
    pub fn read_buffer(&self, buffer_id: &str) -> Vec<f32> {
        if !self.buffers.contains_key(buffer_id) {
            console::log_1(&format!("Buffer not found: {}", buffer_id).into());
            return vec![];
        }

        // In real implementation: host_read_level_zero_buffer(buffer_id)

        vec![] // Placeholder
    }

    /// Write to /intel/compute/submit - submit compute job
    #[wasm_bindgen]
    pub fn submit_job(&mut self, job_json: &str) -> String {
        let job: ComputeJob = match serde_json::from_str(job_json) {
            Ok(j) => j,
            Err(e) => {
                let error = JobResult {
                    job_id: String::new(),
                    status: "failed".to_string(),
                    output_buffers: HashMap::new(),
                    error: Some(format!("Invalid job JSON: {}", e)),
                    execution_time_ms: None,
                };
                return serde_json::to_string(&error).unwrap();
            }
        };

        let job_id = format!("job_{}", self.next_job_id);
        self.next_job_id += 1;

        // In real implementation: host_submit_level_zero_kernel(&job)

        let result = JobResult {
            job_id: job_id.clone(),
            status: "queued".to_string(),
            output_buffers: HashMap::new(),
            error: None,
            execution_time_ms: None,
        };

        self.jobs.insert(job_id.clone(), result.clone());

        console::log_1(&format!("Submitted job: {} (kernel: {})", job_id, job.kernel).into());

        serde_json::to_string(&result).unwrap()
    }

    /// Read from /intel/compute/results/{job_id} - get job result
    #[wasm_bindgen]
    pub fn read_job_result(&self, job_id: &str) -> String {
        match self.jobs.get(job_id) {
            Some(result) => serde_json::to_string(result).unwrap(),
            None => {
                let error = JobResult {
                    job_id: job_id.to_string(),
                    status: "not_found".to_string(),
                    output_buffers: HashMap::new(),
                    error: Some("Job not found".to_string()),
                    execution_time_ms: None,
                };
                serde_json::to_string(&error).unwrap()
            }
        }
    }

    /// List available kernels
    #[wasm_bindgen]
    pub fn list_kernels(&self) -> String {
        // Standard ML kernels
        let kernels = vec![
            "matmul_f32",
            "vector_add_f32",
            "reduce_sum_f32",
            "softmax_f32",
            "relu_f32",
            "conv2d_f32",
        ];

        serde_json::to_string(&kernels).unwrap()
    }
}

/// Export functions for 9P.e server integration
#[wasm_bindgen]
pub fn initialize_oneapi_translator() -> OneAPITranslator {
    console::log_1(&"Initializing oneAPI translator for 9P.e".into());
    OneAPITranslator::new()
}
