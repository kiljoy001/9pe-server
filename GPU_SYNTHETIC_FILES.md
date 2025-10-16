# GPU Compute via Synthetic Files

The 9P.e server exposes GPU compute capabilities through the virtual filesystem using the "everything is a file" principle. This allows users to interact with GPUs using standard file operations. The built-in SYCL system translator under `/system/sycl` mirrors these resources with richer metadata and kernel sources for accelerator-aware tools.

## File Structure

All GPU-related files are located under `/srv/compute/`:

```
/srv/compute/
├── gpu0/                 # First GPU device
│   ├── info              # GPU information (JSON)
│   ├── vram_free         # Free VRAM in bytes (read-only)
│   ├── vram_allocate     # Allocate VRAM by writing size in bytes
│   ├── vram_release      # Release VRAM back to the pool
│   └── vram_status       # VRAM usage statistics
├── gpu1/                 # Second GPU device (if available)
│   └── ...               # Same structure as gpu0/
├── submit               # Submit compute jobs (write JSON)
├── jobs                 # List all compute jobs
│   └── <job_id>/
│       ├── status        # Structured status JSON for the job
│       └── result        # Raw job result bytes (JSON for SYCL jobs)
├── devices              # List available GPU devices
└── status               # Compute system status
```

## Usage Examples

### Reading GPU Information

```bash
# Get information about the first GPU
cat /srv/compute/gpu0/info

# Check free VRAM on GPU 0
cat /srv/compute/gpu0/vram_free

# Get detailed VRAM status
cat /srv/compute/gpu0/vram_status
```

### Allocating VRAM

```bash
# Allocate 100MB of VRAM on GPU 0
echo "104857600" > /srv/compute/gpu0/vram_allocate

# Check that allocation was successful
cat /srv/compute/gpu0/vram_status

# Release VRAM once the job completes
echo "104857600" > /srv/compute/gpu0/vram_release
```

### Submitting Compute Jobs

```bash
# Submit a SYCL compute job
echo '{
  "type": "sycl",
  "operation": "vector_add",
  "data": "base64-encoded-input"
}' > /srv/compute/submit

# Check job status
cat /srv/compute/jobs
```

### Checking System Status

```bash
# Get compute system status
cat /srv/compute/status

# List available devices
cat /srv/compute/devices
```

## Job Submission Format

To submit compute jobs, write a JSON object to `/srv/compute/submit`:

```json
{
  "type": "sycl" | "wasm" | "opencl",
  "operation": "vector_add" | "matrix_multiply" | "custom",
  "data": "base64-encoded-input",
  "vram_bytes": 104857600,     // optional hint, defaults to payload size
  "device": 0                  // optional GPU index hint
}

> SYCL is the preferred backend now; `"opencl"` remains supported for legacy agents.
```

## VRAM Management

VRAM is managed through atomic operations:

1. Check free VRAM: `cat /srv/compute/gpu{N}/vram_free`
2. Allocate VRAM: `echo "size_in_bytes" > /srv/compute/gpu{N}/vram_allocate`
3. Release VRAM: `echo "size_in_bytes" > /srv/compute/gpu{N}/vram_release`
4. Monitor usage: `cat /srv/compute/gpu{N}/vram_status`

### Payload Formats

Encode the following JSON objects as base64 before writing them to `/srv/compute/submit`:

```json
// Vector add
{
  "a": [1.0, 2.0, 3.0],
  "b": [4.0, 5.0, 6.0]
}

// Matrix multiply (m x k) * (k x n)
{
  "a": [1.0, 0.0, 0.0, 1.0],
  "b": [1.0, 2.0, 3.0, 4.0],
  "m": 2,
  "n": 2,
  "k": 2
}
```

After submission a per-job directory appears under `/srv/compute/jobs/<job_id>/`:

- `status` returns a JSON document describing the current state, device assignment, and a preview of any result payload.
- `result` yields the raw result bytes (the SYCL translators emit JSON).

The system automatically tracks VRAM allocation and prevents over-allocation.

## Supported Operations

- **SYCL**: Cross-platform GPU compute using AdaptiveCpp
- **WASM**: WebAssembly compute with GPU acceleration
- **OpenCL**: Direct OpenCL operations

Each GPU backend supports multiple compute operations with automatic device selection.
