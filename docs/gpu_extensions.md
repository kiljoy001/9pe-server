# 9P.e GPU Compute Extensions

This document describes the enhanced GPU compute capabilities available through the extended 9P protocol (9P.e).

## Overview

While GPU compute functionality is available through synthetic files (e.g., `/srv/compute/gpu0/info`), the 9P.e extensions provide a more efficient and type-safe interface for GPU operations.

## Available Extensions

### 1. GPU Info Query (`GPUInfo`)

Query detailed information about a GPU device.

**Request:**
```
GPUInfo {
    device: u32,  // GPU device index
}
```

**Response:**
```
ComputeResponse {
    job_id: "info_{device}",
    success: true,
    result: [JSON bytes with GPU info],
    error_msg: "",
}
```

### 2. VRAM Allocation (`VRAMAllocate`)

Allocate memory on a GPU device.

**Request:**
```
VRAMAllocate {
    device: u32,  // GPU device index
    bytes: u64,   // Number of bytes to allocate
}
```

**Response:**
```
ComputeResponse {
    job_id: "vram_{device}",
    success: true,
    result: [8 bytes containing allocated size],
    error_msg: "",
}
```

### 3. Compute Job Submission (`ComputeSubmit`)

Submit a compute job to be executed on a GPU.

**Request:**
```
ComputeSubmit {
    job_type: String,          // "sycl", "wasm", "opencl"
    kernel_name: String,       // Name of kernel/function to execute
    data: Vec<u8>,             // Input data for computation
    device_hint: Option<u32>,  // Preferred device (optional)
}
```

**Response:**
```
ComputeResponse {
    job_id: "{uuid}",
    success: true,
    result: [],  // Empty for submission (use ComputeStatus for results)
    error_msg: "",
}
```

### 4. Compute Job Status (`ComputeStatus`)

Query the status and results of a compute job.

**Request:**
```
ComputeStatus {
    job_id: String,  // UUID of the job
}
```

**Response:**
```
ComputeResponse {
    job_id: "{uuid}",
    success: true,
    result: [output data bytes],
    error_msg: "",
}
```

## Benefits

### vs. Traditional File-based Interface

1. **Performance**: Direct binary protocol eliminates file I/O overhead
2. **Type Safety**: Structured data instead of text parsing
3. **Efficiency**: Single message vs. multiple file operations
4. **Extensibility**: New GPU operations can be added easily

### Example Performance Comparison

```bash
# Traditional file-based approach:
echo '{"type":"sycl","op":"vector_add"}' > /srv/compute/submit
cat /srv/compute/jobs | grep "completed"  # Polling required

# Enhanced 9P.e approach:
9p-e compute-submit sycl vector_add [binary_data] 0
9p-e compute-status [job_uuid]  # Direct query
```

## Implementation Status

Currently implemented in the protocol structures and handler framework. Integration with actual GPU compute functionality is pending.

## Demo

Run the demo to see how these extensions work:

```bash
cargo run --bin gpu_ninepee_demo
```

## Future Extensions

Planned extensions include:

- `GPUStats`: Real-time GPU utilization statistics
- `ComputeStream`: Streaming compute results
- `KernelUpload`: Dynamic kernel compilation and upload