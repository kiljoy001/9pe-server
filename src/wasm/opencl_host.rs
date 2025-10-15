//! OpenCL host functions for WASM transformers
//!
//! This module provides OpenCL access to WASM transformers through WASI-style host functions.

use anyhow::{Result, Context};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wasmtime::{Caller, Linker};
use tracing::{debug, error, warn, info};
use once_cell::sync::Lazy;

use opencl3::platform::{Platform, get_platforms};
use opencl3::device::{Device, get_device_ids, CL_DEVICE_TYPE_GPU, CL_DEVICE_TYPE_CPU};
use opencl3::context::Context as OpenCLContext;
use opencl3::command_queue::CommandQueue;
use opencl3::memory::{Buffer, CL_MEM_READ_ONLY, CL_MEM_WRITE_ONLY, CL_MEM_READ_WRITE};
use opencl3::program::Program;
use opencl3::kernel::Kernel;
use opencl3::types::cl_float;

/// Global OpenCL state shared across WASM instances
static OPENCL_STATE: Lazy<Arc<Mutex<OpenCLState>>> = Lazy::new(|| {
    Arc::new(Mutex::new(OpenCLState::new()))
});

/// OpenCL state management
#[allow(dead_code)]
struct OpenCLState {
    platforms: Vec<Platform>,
    devices: HashMap<u32, Device>,
    contexts: HashMap<u32, OpenCLContext>,
    queues: HashMap<u32, CommandQueue>,
    buffers: HashMap<u32, Buffer<cl_float>>,
    programs: HashMap<u32, Program>,
    kernels: HashMap<u32, Kernel>,
    next_id: u32,
}

impl OpenCLState {
    fn new() -> Self {
        Self {
            platforms: Vec::new(),
            devices: HashMap::new(),
            contexts: HashMap::new(),
            queues: HashMap::new(),
            buffers: HashMap::new(),
            programs: HashMap::new(),
            kernels: HashMap::new(),
            next_id: 1,
        }
    }

    fn get_next_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn initialize(&mut self) -> Result<()> {
        if self.platforms.is_empty() {
            self.platforms = get_platforms().context("Failed to get OpenCL platforms")?;
            info!("Initialized {} OpenCL platforms", self.platforms.len());
        }
        Ok(())
    }
}

/// Add OpenCL host functions to the WASM linker
pub fn add_opencl_functions<T>(linker: &mut Linker<T>) -> Result<()>
where
    T: 'static,
{
    // Platform and device discovery
    linker.func_wrap("opencl", "get_platform_count", opencl_get_platform_count)?;
    linker.func_wrap("opencl", "get_platforms", opencl_get_platforms)?;
    linker.func_wrap("opencl", "get_device_count", opencl_get_device_count)?;
    linker.func_wrap("opencl", "get_devices", opencl_get_devices)?;

    // Context and queue management
    linker.func_wrap("opencl", "create_context", opencl_create_context)?;
    linker.func_wrap("opencl", "create_queue", opencl_create_queue)?;

    // Buffer management
    linker.func_wrap("opencl", "create_buffer", opencl_create_buffer)?;
    linker.func_wrap("opencl", "write_buffer", opencl_write_buffer)?;
    linker.func_wrap("opencl", "read_buffer", opencl_read_buffer)?;
    linker.func_wrap("opencl", "release_buffer", opencl_release_buffer)?;

    // Program and kernel management
    linker.func_wrap("opencl", "create_program", opencl_create_program)?;
    linker.func_wrap("opencl", "build_program", opencl_build_program)?;
    linker.func_wrap("opencl", "create_kernel", opencl_create_kernel)?;
    linker.func_wrap("opencl", "set_kernel_arg", opencl_set_kernel_arg)?;

    // Execution
    linker.func_wrap("opencl", "enqueue_kernel", opencl_enqueue_kernel)?;
    linker.func_wrap("opencl", "finish", opencl_finish)?;

    Ok(())
}

// Host function implementations

fn opencl_get_platform_count<T>(_caller: Caller<'_, T>) -> i32 {
    match OPENCL_STATE.lock() {
        Ok(mut state) => {
            if let Err(e) = state.initialize() {
                error!("Failed to initialize OpenCL: {}", e);
                return -1;
            }
            state.platforms.len() as i32
        }
        Err(e) => {
            error!("Failed to lock OpenCL state: {}", e);
            -1
        }
    }
}

fn opencl_get_platforms<T>(_caller: Caller<'_, T>, _buffer_ptr: i32, buffer_size: i32) -> i32 {
    match OPENCL_STATE.lock() {
        Ok(state) => {
            let count = std::cmp::min(state.platforms.len(), buffer_size as usize);
            // In a real implementation, we'd write platform IDs to WASM memory
            // For now, just return the count
            count as i32
        }
        Err(e) => {
            error!("Failed to lock OpenCL state: {}", e);
            -1
        }
    }
}

fn opencl_get_device_count<T>(_caller: Caller<'_, T>, platform_id: i32, device_type: i32) -> i32 {
    match OPENCL_STATE.lock() {
        Ok(state) => {
            if let Some(platform) = state.platforms.get(platform_id as usize) {
                let cl_device_type = match device_type {
                    0 => CL_DEVICE_TYPE_GPU,
                    1 => CL_DEVICE_TYPE_CPU,
                    _ => CL_DEVICE_TYPE_GPU | CL_DEVICE_TYPE_CPU,
                };

                match get_device_ids(platform.id(), cl_device_type) {
                    Ok(device_ids) => device_ids.len() as i32,
                    Err(e) => {
                        warn!("Failed to get devices for platform {}: {}", platform_id, e);
                        0
                    }
                }
            } else {
                error!("Invalid platform ID: {}", platform_id);
                -1
            }
        }
        Err(e) => {
            error!("Failed to lock OpenCL state: {}", e);
            -1
        }
    }
}

fn opencl_get_devices<T>(_caller: Caller<'_, T>, platform_id: i32, device_type: i32) -> i32 {
    match OPENCL_STATE.lock() {
        Ok(mut state) => {
            if let Some(platform) = state.platforms.get(platform_id as usize) {
                let cl_device_type = match device_type {
                    0 => CL_DEVICE_TYPE_GPU,
                    1 => CL_DEVICE_TYPE_CPU,
                    _ => CL_DEVICE_TYPE_GPU | CL_DEVICE_TYPE_CPU,
                };

                match get_device_ids(platform.id(), cl_device_type) {
                    Ok(device_ids) => {
                        let mut ids = Vec::new();
                        for device_id in device_ids {
                            let device = Device::new(device_id);
                            let id = state.get_next_id();
                            state.devices.insert(id, device);
                            ids.push(id);
                        }
                        ids.len() as i32
                    }
                    Err(e) => {
                        warn!("Failed to get devices for platform {}: {}", platform_id, e);
                        0
                    }
                }
            } else {
                error!("Invalid platform ID: {}", platform_id);
                -1
            }
        }
        Err(e) => {
            error!("Failed to lock OpenCL state: {}", e);
            -1
        }
    }
}

fn opencl_create_context<T>(_caller: Caller<'_, T>, device_id: i32) -> i32 {
    match OPENCL_STATE.lock() {
        Ok(mut state) => {
            if let Some(device) = state.devices.get(&(device_id as u32)) {
                match OpenCLContext::from_device(device) {
                    Ok(context) => {
                        let context_id = state.get_next_id();
                        state.contexts.insert(context_id, context);
                        debug!("Created OpenCL context {} for device {}", context_id, device_id);
                        context_id as i32
                    }
                    Err(e) => {
                        error!("Failed to create context for device {}: {}", device_id, e);
                        -1
                    }
                }
            } else {
                error!("Invalid device ID: {}", device_id);
                -1
            }
        }
        Err(e) => {
            error!("Failed to lock OpenCL state: {}", e);
            -1
        }
    }
}

fn opencl_create_queue<T>(_caller: Caller<'_, T>, context_id: i32, device_id: i32) -> i32 {
    match OPENCL_STATE.lock() {
        Ok(mut state) => {
            if let (Some(context), Some(_device)) = (
                state.contexts.get(&(context_id as u32)),
                state.devices.get(&(device_id as u32))
            ) {
                #[allow(deprecated)]
                match CommandQueue::create_default(context, 0) {
                    Ok(queue) => {
                        let queue_id = state.get_next_id();
                        state.queues.insert(queue_id, queue);
                        debug!("Created OpenCL queue {} for context {} device {}", queue_id, context_id, device_id);
                        queue_id as i32
                    }
                    Err(e) => {
                        error!("Failed to create queue: {}", e);
                        -1
                    }
                }
            } else {
                error!("Invalid context ID {} or device ID {}", context_id, device_id);
                -1
            }
        }
        Err(e) => {
            error!("Failed to lock OpenCL state: {}", e);
            -1
        }
    }
}

fn opencl_create_buffer<T>(_caller: Caller<'_, T>, context_id: i32, flags: i32, size: i32) -> i32 {
    match OPENCL_STATE.lock() {
        Ok(mut state) => {
            if let Some(context) = state.contexts.get(&(context_id as u32)) {
                let cl_flags = match flags {
                    0 => CL_MEM_READ_ONLY,
                    1 => CL_MEM_WRITE_ONLY,
                    2 => CL_MEM_READ_WRITE,
                    _ => CL_MEM_READ_WRITE,
                };

                match unsafe { Buffer::<cl_float>::create(context, cl_flags, size as usize, std::ptr::null_mut()) } {
                    Ok(buffer) => {
                        let buffer_id = state.get_next_id();
                        state.buffers.insert(buffer_id, buffer);
                        debug!("Created OpenCL buffer {} with size {} bytes", buffer_id, size);
                        buffer_id as i32
                    }
                    Err(e) => {
                        error!("Failed to create buffer: {}", e);
                        -1
                    }
                }
            } else {
                error!("Invalid context ID: {}", context_id);
                -1
            }
        }
        Err(e) => {
            error!("Failed to lock OpenCL state: {}", e);
            -1
        }
    }
}

fn opencl_write_buffer<T>(_caller: Caller<'_, T>, queue_id: i32, buffer_id: i32, _data_ptr: i32, size: i32) -> i32 {
    // In a real implementation, we'd read data from WASM memory at data_ptr
    // and write it to the OpenCL buffer
    debug!("Write buffer {} on queue {} (size: {})", buffer_id, queue_id, size);
    0 // Success
}

fn opencl_read_buffer<T>(_caller: Caller<'_, T>, queue_id: i32, buffer_id: i32, _data_ptr: i32, size: i32) -> i32 {
    // In a real implementation, we'd read from OpenCL buffer
    // and write to WASM memory at data_ptr
    debug!("Read buffer {} on queue {} (size: {})", buffer_id, queue_id, size);
    0 // Success
}

fn opencl_release_buffer<T>(_caller: Caller<'_, T>, buffer_id: i32) -> i32 {
    match OPENCL_STATE.lock() {
        Ok(mut state) => {
            if state.buffers.remove(&(buffer_id as u32)).is_some() {
                debug!("Released OpenCL buffer {}", buffer_id);
                0
            } else {
                error!("Invalid buffer ID: {}", buffer_id);
                -1
            }
        }
        Err(e) => {
            error!("Failed to lock OpenCL state: {}", e);
            -1
        }
    }
}

fn opencl_create_program<T>(_caller: Caller<'_, T>, context_id: i32, _source_ptr: i32, source_len: i32) -> i32 {
    // In a real implementation, we'd read source code from WASM memory
    // For now, return a mock program ID
    debug!("Create program for context {} (source length: {})", context_id, source_len);
    42 // Mock program ID
}

fn opencl_build_program<T>(_caller: Caller<'_, T>, program_id: i32, device_id: i32) -> i32 {
    debug!("Build program {} for device {}", program_id, device_id);
    0 // Success
}

fn opencl_create_kernel<T>(_caller: Caller<'_, T>, program_id: i32, _kernel_name_ptr: i32, kernel_name_len: i32) -> i32 {
    // In a real implementation, we'd read kernel name from WASM memory
    debug!("Create kernel for program {} (name length: {})", program_id, kernel_name_len);
    123 // Mock kernel ID
}

fn opencl_set_kernel_arg<T>(_caller: Caller<'_, T>, kernel_id: i32, arg_index: i32, arg_value: i32) -> i32 {
    debug!("Set kernel {} arg {} to value {}", kernel_id, arg_index, arg_value);
    0 // Success
}

fn opencl_enqueue_kernel<T>(_caller: Caller<'_, T>, queue_id: i32, kernel_id: i32, work_dim: i32, global_size: i32) -> i32 {
    debug!("Enqueue kernel {} on queue {} (work_dim: {}, global_size: {})", kernel_id, queue_id, work_dim, global_size);
    0 // Success
}

fn opencl_finish<T>(_caller: Caller<'_, T>, queue_id: i32) -> i32 {
    debug!("Finish queue {}", queue_id);
    0 // Success
}

/// Initialize OpenCL host functions - call this when setting up WASM runtime
pub fn initialize_opencl() -> Result<()> {
    match OPENCL_STATE.lock() {
        Ok(mut state) => {
            state.initialize().context("Failed to initialize OpenCL state")?;
            info!("OpenCL host functions initialized successfully");
            Ok(())
        }
        Err(e) => {
            anyhow::bail!("Failed to lock OpenCL state during initialization: {}", e);
        }
    }
}

/// Get OpenCL device information for diagnostics
pub fn get_opencl_info() -> Result<String> {
    match OPENCL_STATE.lock() {
        Ok(mut state) => {
            state.initialize()?;

            let mut info = String::new();
            info.push_str(&format!("OpenCL Platforms: {}\n", state.platforms.len()));

            for (i, platform) in state.platforms.iter().enumerate() {
                if let Ok(name) = platform.name() {
                    info.push_str(&format!("  Platform {}: {}\n", i, name));
                }

                if let Ok(device_ids) = get_device_ids(platform.id(), CL_DEVICE_TYPE_GPU | CL_DEVICE_TYPE_CPU) {
                    for (j, device_id) in device_ids.iter().enumerate() {
                        let device = Device::new(*device_id);
                        if let Ok(device_name) = device.name() {
                            info.push_str(&format!("    Device {}: {}\n", j, device_name));
                        }
                    }
                }
            }

            Ok(info)
        }
        Err(e) => {
            anyhow::bail!("Failed to lock OpenCL state: {}", e);
        }
    }
}
