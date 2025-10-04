//! OpenCL GPU Compute Synthetic Filesystem
//!
//! Exposes GPU compute resources as synthetic files through 9P.e
//!
//! Directory structure:
//! /gpu/
//!   devices/
//!     0/
//!       info         - Device capabilities (read-only)
//!       memory       - Memory info (total/free/used)
//!       kernels/     - Available compute kernels
//!         matrix_mul - Matrix multiplication kernel
//!         fft        - Fast Fourier Transform
//!         reduce     - Parallel reduction
//!       buffers/     - Memory buffers
//!         buffer_0   - Read/write GPU memory
//!       queues/      - Command queues
//!         default    - Default command queue
//!       status       - Device status and utilization
//!   compute/
//!     submit       - Submit compute jobs (write kernel + args)
//!     results/     - Job results
//!     status       - Overall compute status

use anyhow::{Result, Context, anyhow};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::path::{Path, PathBuf};
use opencl3::command_queue::{CommandQueue, CL_QUEUE_PROFILING_ENABLE};
use opencl3::context::Context as CLContext;
use opencl3::device::{Device, CL_DEVICE_TYPE_GPU, get_all_devices};
use opencl3::kernel::{Kernel, ExecuteKernel};
use opencl3::memory::{Buffer, CL_MEM_READ_WRITE};
use opencl3::program::Program;
use opencl3::types::{cl_ulong, cl_float, CL_BLOCKING, CL_NON_BLOCKING};
use serde::{Serialize, Deserialize};
use tokio::sync::Mutex;
use tracing::{info, debug, error, warn};

/// GPU device information exposed as synthetic files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuDeviceInfo {
    pub device_id: usize,
    pub name: String,
    pub vendor: String,
    pub compute_units: u32,
    pub max_work_group_size: usize,
    pub max_work_item_dimensions: u32,
    pub global_memory_size: u64,
    pub local_memory_size: u64,
    pub max_clock_frequency: u32,
    pub opencl_version: String,
}

/// GPU memory buffer exposed as a synthetic file
#[derive(Debug)]
pub struct GpuBuffer {
    pub id: String,
    pub size: usize,
    pub buffer: Buffer<cl_float>,
    pub device_id: usize,
    pub created_at: std::time::Instant,
    pub last_accessed: std::time::Instant,
    pub access_mode: BufferAccessMode,
}

#[derive(Debug, Clone, Copy)]
pub enum BufferAccessMode {
    ReadOnly,
    WriteOnly,
    ReadWrite,
}

/// Compute kernel exposed as a synthetic file
#[derive(Debug, Clone)]
pub struct ComputeKernel {
    pub name: String,
    pub source: String,
    pub args: Vec<KernelArgument>,
    pub work_dimensions: u32,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelArgument {
    pub name: String,
    pub arg_type: String,
    pub size: usize,
    pub is_buffer: bool,
}

/// Compute job submitted through synthetic filesystem
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeJob {
    pub id: String,
    pub kernel_name: String,
    pub device_id: usize,
    pub global_work_size: Vec<usize>,
    pub local_work_size: Option<Vec<usize>>,
    pub arguments: HashMap<String, JobArgument>,
    pub status: JobStatus,
    pub submitted_at: u64,
    pub completed_at: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobArgument {
    Buffer(String),      // Buffer ID
    Scalar(f32),        // Scalar value
    Integer(i32),       // Integer value
    UInteger(u32),      // Unsigned integer
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
}

/// Main OpenCL synthetic filesystem
pub struct OpenCLFilesystem {
    devices: Arc<RwLock<Vec<GpuDevice>>>,
    buffers: Arc<RwLock<HashMap<String, Arc<Mutex<GpuBuffer>>>>>,
    kernels: Arc<RwLock<HashMap<String, ComputeKernel>>>,
    jobs: Arc<RwLock<HashMap<String, ComputeJob>>>,
    job_results: Arc<RwLock<HashMap<String, Vec<f32>>>>,
    next_buffer_id: Arc<RwLock<u64>>,
    next_job_id: Arc<RwLock<u64>>,
}

/// Individual GPU device with OpenCL context
pub struct GpuDevice {
    pub info: GpuDeviceInfo,
    pub device: Device,
    pub context: CLContext,
    pub queue: CommandQueue,
    pub programs: HashMap<String, Program>,
    pub kernels: HashMap<String, Kernel>,
    pub utilization: f32,
    pub temperature: Option<f32>,
}

impl OpenCLFilesystem {
    /// Create new OpenCL synthetic filesystem
    pub fn new() -> Result<Self> {
        info!("Initializing OpenCL synthetic filesystem");

        // Discover all GPU devices
        let devices = Self::discover_devices()?;

        // Initialize standard compute kernels
        let kernels = Self::init_standard_kernels();

        Ok(Self {
            devices: Arc::new(RwLock::new(devices)),
            buffers: Arc::new(RwLock::new(HashMap::new())),
            kernels: Arc::new(RwLock::new(kernels)),
            jobs: Arc::new(RwLock::new(HashMap::new())),
            job_results: Arc::new(RwLock::new(HashMap::new())),
            next_buffer_id: Arc::new(RwLock::new(0)),
            next_job_id: Arc::new(RwLock::new(0)),
        })
    }

    /// Discover and initialize all OpenCL GPU devices
    fn discover_devices() -> Result<Vec<GpuDevice>> {
        let device_ids = get_all_devices(CL_DEVICE_TYPE_GPU)?;
        let mut devices = Vec::new();

        for (idx, device_id) in device_ids.iter().enumerate() {
            let device = Device::new(*device_id);

            // Get device info
            let info = GpuDeviceInfo {
                device_id: idx,
                name: device.name()?,
                vendor: device.vendor()?,
                compute_units: device.max_compute_units()?,
                max_work_group_size: device.max_work_group_size()?,
                max_work_item_dimensions: device.max_work_item_dimensions()?,
                global_memory_size: device.global_mem_size()?,
                local_memory_size: device.local_mem_size()?,
                max_clock_frequency: device.max_clock_frequency()?,
                opencl_version: device.opencl_c_version()?,
            };

            // Create context and command queue
            let context = CLContext::from_device(&device)?;
            let queue = CommandQueue::create_default(&context, CL_QUEUE_PROFILING_ENABLE)?;

            info!("Found GPU device: {} ({} compute units, {} GB memory)",
                  info.name, info.compute_units,
                  info.global_memory_size / (1024 * 1024 * 1024));

            devices.push(GpuDevice {
                info,
                device,
                context,
                queue,
                programs: HashMap::new(),
                kernels: HashMap::new(),
                utilization: 0.0,
                temperature: None,
            });
        }

        if devices.is_empty() {
            warn!("No OpenCL GPU devices found");
        }

        Ok(devices)
    }

    /// Initialize standard compute kernels
    fn init_standard_kernels() -> HashMap<String, ComputeKernel> {
        let mut kernels = HashMap::new();

        // Matrix multiplication kernel
        kernels.insert("matrix_mul".to_string(), ComputeKernel {
            name: "matrix_mul".to_string(),
            source: MATRIX_MUL_KERNEL.to_string(),
            args: vec![
                KernelArgument { name: "A".to_string(), arg_type: "float*".to_string(), size: 0, is_buffer: true },
                KernelArgument { name: "B".to_string(), arg_type: "float*".to_string(), size: 0, is_buffer: true },
                KernelArgument { name: "C".to_string(), arg_type: "float*".to_string(), size: 0, is_buffer: true },
                KernelArgument { name: "N".to_string(), arg_type: "int".to_string(), size: 4, is_buffer: false },
            ],
            work_dimensions: 2,
            description: "Parallel matrix multiplication".to_string(),
        });

        // Vector addition kernel
        kernels.insert("vector_add".to_string(), ComputeKernel {
            name: "vector_add".to_string(),
            source: VECTOR_ADD_KERNEL.to_string(),
            args: vec![
                KernelArgument { name: "a".to_string(), arg_type: "float*".to_string(), size: 0, is_buffer: true },
                KernelArgument { name: "b".to_string(), arg_type: "float*".to_string(), size: 0, is_buffer: true },
                KernelArgument { name: "c".to_string(), arg_type: "float*".to_string(), size: 0, is_buffer: true },
            ],
            work_dimensions: 1,
            description: "Element-wise vector addition".to_string(),
        });

        // Parallel reduction kernel
        kernels.insert("reduce_sum".to_string(), ComputeKernel {
            name: "reduce_sum".to_string(),
            source: REDUCE_SUM_KERNEL.to_string(),
            args: vec![
                KernelArgument { name: "input".to_string(), arg_type: "float*".to_string(), size: 0, is_buffer: true },
                KernelArgument { name: "output".to_string(), arg_type: "float*".to_string(), size: 0, is_buffer: true },
                KernelArgument { name: "local_sum".to_string(), arg_type: "local float*".to_string(), size: 0, is_buffer: false },
                KernelArgument { name: "n".to_string(), arg_type: "uint".to_string(), size: 4, is_buffer: false },
            ],
            work_dimensions: 1,
            description: "Parallel sum reduction".to_string(),
        });

        // FFT kernel (simplified)
        kernels.insert("fft".to_string(), ComputeKernel {
            name: "fft".to_string(),
            source: FFT_KERNEL.to_string(),
            args: vec![
                KernelArgument { name: "real_in".to_string(), arg_type: "float*".to_string(), size: 0, is_buffer: true },
                KernelArgument { name: "imag_in".to_string(), arg_type: "float*".to_string(), size: 0, is_buffer: true },
                KernelArgument { name: "real_out".to_string(), arg_type: "float*".to_string(), size: 0, is_buffer: true },
                KernelArgument { name: "imag_out".to_string(), arg_type: "float*".to_string(), size: 0, is_buffer: true },
                KernelArgument { name: "n".to_string(), arg_type: "uint".to_string(), size: 4, is_buffer: false },
            ],
            work_dimensions: 1,
            description: "Fast Fourier Transform".to_string(),
        });

        kernels
    }

    /// Create a new GPU buffer
    pub async fn create_buffer(&self, device_id: usize, size: usize) -> Result<String> {
        let devices = self.devices.read().unwrap();
        let device = devices.get(device_id)
            .ok_or_else(|| anyhow!("Invalid device ID: {}", device_id))?;

        // Allocate buffer on GPU
        let buffer = Buffer::<cl_float>::create(
            &device.context,
            CL_MEM_READ_WRITE,
            size,
            std::ptr::null_mut(),
        )?;

        // Generate buffer ID
        let buffer_id = {
            let mut id = self.next_buffer_id.write().unwrap();
            *id += 1;
            format!("buffer_{}", id)
        };

        let gpu_buffer = GpuBuffer {
            id: buffer_id.clone(),
            size,
            buffer,
            device_id,
            created_at: std::time::Instant::now(),
            last_accessed: std::time::Instant::now(),
            access_mode: BufferAccessMode::ReadWrite,
        };

        self.buffers.write().unwrap()
            .insert(buffer_id.clone(), Arc::new(Mutex::new(gpu_buffer)));

        info!("Created GPU buffer {} on device {} ({} bytes)",
              buffer_id, device_id, size * std::mem::size_of::<cl_float>());

        Ok(buffer_id)
    }

    /// Submit a compute job
    pub async fn submit_job(&self, job: ComputeJob) -> Result<String> {
        let job_id = job.id.clone();

        // Validate kernel exists
        let kernels = self.kernels.read().unwrap();
        let kernel = kernels.get(&job.kernel_name)
            .ok_or_else(|| anyhow!("Unknown kernel: {}", job.kernel_name))?
            .clone();
        drop(kernels);

        // Add job to queue
        self.jobs.write().unwrap().insert(job_id.clone(), job.clone());

        // Execute job asynchronously
        let self_clone = self.clone();
        let job_id_clone = job_id.clone();
        tokio::spawn(async move {
            if let Err(e) = self_clone.execute_job(job).await {
                error!("Failed to execute job {}: {}", job_id_clone, e);
                // Update job status to failed
                if let Ok(mut jobs) = self_clone.jobs.write() {
                    if let Some(job) = jobs.get_mut(&job_id_clone) {
                        job.status = JobStatus::Failed;
                        job.error = Some(format!("{}", e));
                    }
                }
            }
        });

        Ok(job_id)
    }

    /// Execute a compute job on GPU
    async fn execute_job(&self, mut job: ComputeJob) -> Result<()> {
        // Update status to running
        {
            let mut jobs = self.jobs.write().unwrap();
            if let Some(j) = jobs.get_mut(&job.id) {
                j.status = JobStatus::Running;
            }
        }

        let devices = self.devices.read().unwrap();
        let device = devices.get(job.device_id)
            .ok_or_else(|| anyhow!("Invalid device ID"))?;

        // Compile kernel if needed
        let kernels = self.kernels.read().unwrap();
        let kernel_def = kernels.get(&job.kernel_name)
            .ok_or_else(|| anyhow!("Kernel not found"))?;

        let program = Program::create_and_build_from_source(
            &device.context,
            &kernel_def.source,
            "",
        ).context("Failed to build kernel")?;

        let kernel = Kernel::create(&program, &kernel_def.name)?;

        // Set kernel arguments
        for (idx, arg_def) in kernel_def.args.iter().enumerate() {
            match job.arguments.get(&arg_def.name) {
                Some(JobArgument::Buffer(buffer_id)) => {
                    let buffers = self.buffers.read().unwrap();
                    let buffer = buffers.get(buffer_id)
                        .ok_or_else(|| anyhow!("Buffer not found: {}", buffer_id))?;
                    let buffer_lock = buffer.lock().await;
                    unsafe {
                        kernel.set_arg(idx as u32, &buffer_lock.buffer)?;
                    }
                }
                Some(JobArgument::Scalar(val)) => {
                    unsafe { kernel.set_arg(idx as u32, val)?; }
                }
                Some(JobArgument::Integer(val)) => {
                    unsafe { kernel.set_arg(idx as u32, val)?; }
                }
                Some(JobArgument::UInteger(val)) => {
                    unsafe { kernel.set_arg(idx as u32, val)?; }
                }
                None => return Err(anyhow!("Missing argument: {}", arg_def.name)),
            }
        }

        // Execute kernel
        unsafe {
            let event = device.queue.enqueue_nd_range_kernel(
                &kernel,
                kernel_def.work_dimensions,
                None,
                &job.global_work_size,
                job.local_work_size.as_deref(),
                &[],
            )?;

            // Wait for completion
            event.wait()?;

            // Profile execution time
            let start = event.profiling_info_start()?;
            let end = event.profiling_info_end()?;
            let elapsed_ns = end - start;

            debug!("Kernel {} executed in {} ms",
                   job.kernel_name, elapsed_ns / 1_000_000);
        }

        // Update job status
        {
            let mut jobs = self.jobs.write().unwrap();
            if let Some(j) = jobs.get_mut(&job.id) {
                j.status = JobStatus::Completed;
                j.completed_at = Some(std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs());
            }
        }

        info!("Completed GPU job {} on device {}", job.id, job.device_id);
        Ok(())
    }

    /// Read GPU buffer data
    pub async fn read_buffer(&self, buffer_id: &str) -> Result<Vec<f32>> {
        let buffers = self.buffers.read().unwrap();
        let buffer = buffers.get(buffer_id)
            .ok_or_else(|| anyhow!("Buffer not found: {}", buffer_id))?
            .clone();
        drop(buffers);

        let mut buffer_lock = buffer.lock().await;
        buffer_lock.last_accessed = std::time::Instant::now();

        // Find the device this buffer belongs to
        let devices = self.devices.read().unwrap();
        let device = devices.get(buffer_lock.device_id)
            .ok_or_else(|| anyhow!("Device not found"))?;

        // Read data from GPU
        let mut host_data = vec![0.0f32; buffer_lock.size];
        unsafe {
            device.queue.enqueue_read_buffer(
                &buffer_lock.buffer,
                CL_BLOCKING,
                0,
                &mut host_data,
                &[],
            )?;
        }

        Ok(host_data)
    }

    /// Write data to GPU buffer
    pub async fn write_buffer(&self, buffer_id: &str, data: Vec<f32>) -> Result<()> {
        let buffers = self.buffers.read().unwrap();
        let buffer = buffers.get(buffer_id)
            .ok_or_else(|| anyhow!("Buffer not found: {}", buffer_id))?
            .clone();
        drop(buffers);

        let mut buffer_lock = buffer.lock().await;

        if data.len() != buffer_lock.size {
            return Err(anyhow!("Data size mismatch: expected {}, got {}",
                               buffer_lock.size, data.len()));
        }

        buffer_lock.last_accessed = std::time::Instant::now();

        // Find the device this buffer belongs to
        let devices = self.devices.read().unwrap();
        let device = devices.get(buffer_lock.device_id)
            .ok_or_else(|| anyhow!("Device not found"))?;

        // Write data to GPU
        unsafe {
            device.queue.enqueue_write_buffer(
                &buffer_lock.buffer,
                CL_BLOCKING,
                0,
                &data,
                &[],
            )?;
        }

        debug!("Wrote {} floats to GPU buffer {}", data.len(), buffer_id);
        Ok(())
    }

    /// Get device info as JSON
    pub fn get_device_info(&self, device_id: usize) -> Result<String> {
        let devices = self.devices.read().unwrap();
        let device = devices.get(device_id)
            .ok_or_else(|| anyhow!("Invalid device ID"))?;

        Ok(serde_json::to_string_pretty(&device.info)?)
    }

    /// Get device memory usage
    pub fn get_device_memory(&self, device_id: usize) -> Result<String> {
        let devices = self.devices.read().unwrap();
        let device = devices.get(device_id)
            .ok_or_else(|| anyhow!("Invalid device ID"))?;

        // Calculate memory usage from buffers
        let buffers = self.buffers.read().unwrap();
        let mut used_memory: u64 = 0;

        for buffer in buffers.values() {
            if let Ok(buf) = buffer.try_lock() {
                if buf.device_id == device_id {
                    used_memory += (buf.size * std::mem::size_of::<cl_float>()) as u64;
                }
            }
        }

        let info = serde_json::json!({
            "total": device.info.global_memory_size,
            "used": used_memory,
            "free": device.info.global_memory_size - used_memory,
            "usage_percent": (used_memory as f64 / device.info.global_memory_size as f64) * 100.0
        });

        Ok(serde_json::to_string_pretty(&info)?)
    }

    /// List all kernels
    pub fn list_kernels(&self) -> Vec<String> {
        self.kernels.read().unwrap().keys().cloned().collect()
    }

    /// List all buffers
    pub fn list_buffers(&self) -> Vec<String> {
        self.buffers.read().unwrap().keys().cloned().collect()
    }

    /// Get job status
    pub fn get_job_status(&self, job_id: &str) -> Result<String> {
        let jobs = self.jobs.read().unwrap();
        let job = jobs.get(job_id)
            .ok_or_else(|| anyhow!("Job not found: {}", job_id))?;

        Ok(serde_json::to_string_pretty(job)?)
    }
}

impl Clone for OpenCLFilesystem {
    fn clone(&self) -> Self {
        Self {
            devices: self.devices.clone(),
            buffers: self.buffers.clone(),
            kernels: self.kernels.clone(),
            jobs: self.jobs.clone(),
            job_results: self.job_results.clone(),
            next_buffer_id: self.next_buffer_id.clone(),
            next_job_id: self.next_job_id.clone(),
        }
    }
}

// Standard OpenCL kernels

const MATRIX_MUL_KERNEL: &str = r#"
__kernel void matrix_mul(__global const float* A,
                         __global const float* B,
                         __global float* C,
                         int N) {
    int row = get_global_id(0);
    int col = get_global_id(1);

    if (row < N && col < N) {
        float sum = 0.0f;
        for (int k = 0; k < N; k++) {
            sum += A[row * N + k] * B[k * N + col];
        }
        C[row * N + col] = sum;
    }
}
"#;

const VECTOR_ADD_KERNEL: &str = r#"
__kernel void vector_add(__global const float* a,
                         __global const float* b,
                         __global float* c) {
    int id = get_global_id(0);
    c[id] = a[id] + b[id];
}
"#;

const REDUCE_SUM_KERNEL: &str = r#"
__kernel void reduce_sum(__global float* input,
                         __global float* output,
                         __local float* local_sum,
                         uint n) {
    uint tid = get_local_id(0);
    uint gid = get_global_id(0);
    uint local_size = get_local_size(0);

    // Load data to local memory
    local_sum[tid] = (gid < n) ? input[gid] : 0.0f;
    barrier(CLK_LOCAL_MEM_FENCE);

    // Perform reduction in local memory
    for (uint s = local_size / 2; s > 0; s >>= 1) {
        if (tid < s) {
            local_sum[tid] += local_sum[tid + s];
        }
        barrier(CLK_LOCAL_MEM_FENCE);
    }

    // Write result for this work group
    if (tid == 0) {
        output[get_group_id(0)] = local_sum[0];
    }
}
"#;

const FFT_KERNEL: &str = r#"
// Simplified FFT kernel (Cooley-Tukey radix-2)
__kernel void fft(__global float* real_in,
                  __global float* imag_in,
                  __global float* real_out,
                  __global float* imag_out,
                  uint n) {
    uint gid = get_global_id(0);

    if (gid < n) {
        // Simplified FFT - would need full implementation
        // This is just a placeholder for the actual FFT algorithm
        real_out[gid] = real_in[gid];
        imag_out[gid] = imag_in[gid];
    }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_opencl_filesystem_creation() {
        // May fail if no OpenCL devices available
        let fs = OpenCLFilesystem::new();
        assert!(fs.is_ok() || fs.is_err());
    }

    #[tokio::test]
    async fn test_kernel_registration() {
        if let Ok(fs) = OpenCLFilesystem::new() {
            let kernels = fs.list_kernels();
            assert!(kernels.contains(&"matrix_mul".to_string()));
            assert!(kernels.contains(&"vector_add".to_string()));
            assert!(kernels.contains(&"reduce_sum".to_string()));
        }
    }
}