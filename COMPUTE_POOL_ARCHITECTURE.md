# Distributed Compute Pool Architecture

## Philosophy: Infrastructure, Not Application

**What 9P.e Provides:**
- Distributed GPU compute pool
- Distributed memory/KV cache
- Network-transparent buffer management
- Work scheduling and distribution

**What 9P.e Does NOT Provide:**
- AI model inference directly
- Model format parsing
- Prompt engineering
- Token generation

**Users run Ollama/llama.cpp/etc, which USES our compute pool**

## Architecture

### Synthetic Filesystem Interface

```
/srv/compute/
  pool/
    devices/                 - List all GPUs in mesh
      device_0/
        info                 - GPU capabilities (CUDA/HIP/Level-Zero)
        memory_total         - Total VRAM
        memory_free          - Available VRAM
        utilization          - Current usage %
        backend              - SYCL backend type
      device_1/
        ...

    buffers/                 - Distributed GPU buffers
      create               - Write to create buffer
      list                 - List all buffers
      buffer_abc123/
        data               - Read/write buffer data
        info               - Buffer metadata
        pin                - Pin to specific device

    kv_cache/              - Distributed KV cache for transformers
      allocate             - Allocate KV cache slice
      read                 - Read KV cache data
      write                - Write KV cache data
      evict                - Evict cache entries

    jobs/                  - Compute job queue
      submit               - Submit compute job (matmul, etc)
      queue/               - Queued jobs
      running/             - Running jobs
      completed/           - Completed jobs
        job_xyz/
          status           - Job status
          result           - Job result data
          timing           - Profiling info
```

### Integration Example: Ollama

#### Current Ollama (Single GPU):
```
Ollama → GPU 0 → VRAM full → OOM
```

#### Ollama + 9P.e Pool:
```bash
# Ollama plugin/hook configuration
export OLLAMA_COMPUTE_BACKEND=9pe
export OLLAMA_9PE_MOUNT=/srv/compute

# Ollama now uses distributed pool:
Ollama → 9P.e /srv/compute/pool/submit
       ↓
  9P.e distributes work:
    - Matmul layer 0-20 → Machine 1 (NVIDIA 4090)
    - Matmul layer 21-40 → Machine 2 (AMD 7900 XTX)
    - KV cache → Machine 3 (16GB Intel Arc)
       ↓
  Results merged via 9P
       ↓
  Ollama receives response
```

### Key Operations Exposed

#### 1. Buffer Management
```bash
# Create GPU buffer
echo '{"size": 134217728, "type": "float32"}' > /srv/compute/pool/buffers/create
# Returns: buffer_abc123

# Write data to buffer
cat tensor.bin > /srv/compute/pool/buffers/buffer_abc123/data

# Read from buffer
cat /srv/compute/pool/buffers/buffer_abc123/data > result.bin
```

#### 2. Matrix Operations (SYCL)
```bash
# Submit matrix multiplication
cat <<EOF > /srv/compute/pool/jobs/submit
{
  "operation": "matmul_f32",
  "buffer_a": "buffer_abc123",
  "buffer_b": "buffer_def456",
  "buffer_c": "buffer_ghi789",
  "m": 4096, "n": 4096, "k": 11008
}
EOF
# Returns: job_xyz

# Check status
cat /srv/compute/pool/jobs/completed/job_xyz/status
# Output: {"state": "completed", "duration_ms": 45}
```

#### 3. KV Cache Management
```bash
# Allocate KV cache for model
cat <<EOF > /srv/compute/pool/kv_cache/allocate
{
  "model_id": "llama3-70b",
  "num_layers": 80,
  "heads": 8,
  "head_dim": 128,
  "max_seq_len": 4096
}
EOF
# Returns: kv_cache_id

# Write KV cache for specific layer/position
echo $KV_DATA > /srv/compute/pool/kv_cache/${kv_cache_id}/layer_10/write

# Read KV cache
cat /srv/compute/pool/kv_cache/${kv_cache_id}/layer_10/read > kv.bin
```

## How Existing Software Integrates

### Option 1: llama.cpp Backend
Modify llama.cpp's `ggml-backend.c`:
```c
// Instead of local GPU calls:
cuda_matmul(a, b, c);

// Use 9P.e pool:
write_file("/srv/compute/pool/jobs/submit", job_json);
read_file("/srv/compute/pool/jobs/completed/job_xyz/result", result);
```

### Option 2: Ollama Plugin
Create Ollama compute plugin:
```go
// Ollama calls plugin for heavy ops
func (p *NinePPlugin) MatMul(a, b Matrix) Matrix {
    jobID := submitTo9Pe("/srv/compute/pool/jobs/submit", job)
    return waitForResult(jobID)
}
```

### Option 3: Python Library
```python
import ninep_compute

# Automatically uses distributed pool
pool = ninep_compute.Pool("/srv/compute/pool")

# Allocate buffer on best available GPU
buf_a = pool.create_buffer(size=1024*1024, dtype='float32')
buf_b = pool.create_buffer(size=1024*1024, dtype='float32')
buf_c = pool.create_buffer(size=1024*1024, dtype='float32')

# Write data
buf_a.write(tensor_a.numpy())
buf_b.write(tensor_b.numpy())

# Submit compute (automatically distributed)
job = pool.matmul(buf_a, buf_b, buf_c, m=1024, n=1024, k=1024)
job.wait()

# Read result
result = buf_c.read()
```

## Benefits

### 1. Software Ecosystem Compatibility
- Ollama works unchanged (with plugin)
- llama.cpp works unchanged (with backend)
- vLLM could use it
- TGI could use it
- ANY compute framework

### 2. Transparent Distribution
```
User perspective:
  ollama run llama3:70b

Behind the scenes:
  - Ollama hooks into /srv/compute/pool
  - 9P.e distributes across 5 machines
  - User sees single response
```

### 3. Heterogeneous Hardware
```
Pool discovers:
- Machine 1: NVIDIA 4090 → SYCL/CUDA backend
- Machine 2: AMD 7900 XTX → SYCL/HIP backend
- Machine 3: Intel Arc A770 → SYCL/Level-Zero backend

Ollama doesn't care - just submits jobs!
```

### 4. Flexible Storage
```
KV cache too big for one GPU?
  → Distribute across machines
  → 9P.e handles network transparency
  → Ollama sees single unified cache
```

## Implementation

### Phase 1: GPU Device Enumeration ✅
```
/srv/compute/pool/devices/
  device_0/info
  device_1/info
```
- Already have SYCL FFI
- Just expose via synthetic filesystem

### Phase 2: Buffer Management
```
/srv/compute/pool/buffers/
  create
  buffer_*/data
```
- Wrap SYCL buffer operations
- Handle network transfer

### Phase 3: Basic Compute Operations
```
/srv/compute/pool/jobs/submit
```
- Matmul, vector ops, activations
- Queue and execute via consensus

### Phase 4: KV Cache Distribution
```
/srv/compute/pool/kv_cache/
```
- Distributed cache allocation
- Transparent read/write

### Phase 5: Plugin/Backend for Existing Software
- Ollama plugin
- llama.cpp backend
- Python library

## Competitive Advantages

### vs. Ray Serve
**Them**: Python framework, complex API, cloud-oriented
**You**: File interface, zero coding, works with existing tools

### vs. NVIDIA NIM
**Them**: NVIDIA GPUs only, cloud deployment, $$$
**You**: Any GPU, self-hosted, free

### vs. vLLM Distributed
**Them**: Python-specific, homogeneous GPUs
**You**: Language-agnostic (files!), heterogeneous GPUs

### vs. DeepSpeed/Megatron
**Them**: Researcher tool, complex setup, identical GPUs
**You**: Production-ready, auto-discovery, mixed GPUs

## Real-World Scenarios

### Scenario 1: Home Lab
```
You: RTX 3090 (24GB)
Friend: RX 7900 XTX (24GB)
Neighbor: Arc A770 (16GB)

Total pool: 64GB

Run Ollama with llama3:70b (fits in pool!)
Each person contributes compute
Everyone benefits from larger pool
```

### Scenario 2: Small Business
```
5 employee workstations (various GPUs)
Total: 80GB GPU memory pool

Deploy Ollama with 9P.e backend
Employees use AI assistant
Compute distributed automatically
$0 cloud costs
```

### Scenario 3: Research Lab
```
20 researcher machines (heterogeneous)
Automatic pooling during off-hours
Researchers submit via Ollama/llama.cpp
9P.e handles distribution
Papers cite "distributed via 9P.e"
```

## Security & Isolation

### Buffer Isolation
- Each buffer has owner
- Cross-machine access requires auth
- Consensus tracks ownership

### KV Cache Privacy
- Per-user KV cache namespaces
- Encrypted network transfer (QUIC)
- No cross-contamination

### Compute Quotas
```
/srv/compute/pool/quotas/user1
  max_memory: 8GB
  max_gpus: 2
  priority: normal
```

## Integration Code Examples

### llama.cpp Integration
```c
// In ggml-backend.c
#ifdef GGML_USE_9PE
static void ggml_9pe_matmul(
    struct ggml_tensor * dst,
    const struct ggml_tensor * src0,
    const struct ggml_tensor * src1) {

    // Write buffers to 9P.e pool
    write_buffer("/srv/compute/pool/buffers/buf_a/data",
                 src0->data, ggml_nbytes(src0));
    write_buffer("/srv/compute/pool/buffers/buf_b/data",
                 src1->data, ggml_nbytes(src1));

    // Submit job
    char job[1024];
    snprintf(job, sizeof(job),
        "{\"operation\":\"matmul_f32\","
        "\"buffer_a\":\"buf_a\","
        "\"buffer_b\":\"buf_b\","
        "\"buffer_c\":\"buf_c\","
        "\"m\":%d,\"n\":%d,\"k\":%d}",
        dst->ne[1], dst->ne[0], src0->ne[0]);

    write_file("/srv/compute/pool/jobs/submit", job);

    // Read result
    read_buffer("/srv/compute/pool/buffers/buf_c/data",
                dst->data, ggml_nbytes(dst));
}
#endif
```

### Ollama Plugin (Hypothetical)
```go
package ninep

type NinePBackend struct {
    poolPath string
}

func (b *NinePBackend) Forward(input Tensor) Tensor {
    // Upload input
    bufID := b.createBuffer(input.Size())
    b.writeBuffer(bufID, input.Data())

    // Submit compute
    job := Job{
        Operation: "forward_pass",
        InputBuffer: bufID,
        Model: "llama3-70b",
    }
    jobID := b.submitJob(job)

    // Wait and return
    result := b.waitForJob(jobID)
    return result
}
```

## Summary

**9P.e provides the infrastructure, not the application.**

Users run:
- Ollama (for chat interfaces)
- llama.cpp (for local inference)
- vLLM (for serving)
- Custom Python scripts

9P.e provides:
- Distributed GPU pool
- Transparent buffer management
- Work distribution via consensus
- Network-transparent KV cache
- Vendor-neutral compute (SYCL)

**Result**: Existing AI software gets distributed, heterogeneous GPU support for free!
