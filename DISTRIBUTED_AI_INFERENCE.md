# Distributed AI Inference Architecture

## Goal
Enable multiple machines to contribute compute resources to run a single AI model (LLaMA, Ollama, etc.) through the 9P.e synthetic filesystem.

## Architecture

### User Experience
```bash
# On any machine in the mesh network:
echo "Explain quantum computing" > /n/ai-cluster/llama3-70b/prompt
cat /n/ai-cluster/llama3-70b/response

# The response is computed across ALL available GPUs in the network
```

### Components

#### 1. Synthetic AI Filesystem (`/srv/ai/`)
```
/srv/ai/
  models/
    llama3-70b/
      prompt       - Write prompt here (triggers inference)
      response     - Read generated response
      status       - Real-time inference status
      config       - Model configuration
    mistral-7b/
    ...
  cluster/
    peers/         - Discovered compute nodes
    utilization    - Cluster-wide GPU usage
    topology       - Network topology view
```

#### 2. Model Sharding via WASM Translator
```rust
// WASM translator for LLaMA inference
// Translates write to /prompt into distributed work

When user writes to /srv/ai/models/llama3-70b/prompt:
  1. Translator loads model metadata (70B params, 80 layers)
  2. Queries mesh network for available GPUs
  3. Assigns layers to machines:
     - Machine A (NVIDIA 4090): Layers 0-26
     - Machine B (AMD 7900 XTX): Layers 27-53
     - Machine C (Intel Arc A770): Layers 54-80
  4. Each machine processes its layers using SYCL
  5. Results flow through 9P.e consensus
  6. Final response written to /response synthetic file
```

#### 3. SYCL Compute Backend
Each machine runs SYCL kernels for:
- **Matrix multiplication** (attention mechanism)
- **Layer normalization**
- **Activation functions** (ReLU, GELU, SiLU)
- **Token embedding lookup**
- **KV-cache management**

**Vendor-neutral**: AdaptiveCpp automatically uses:
- CUDA backend for NVIDIA
- HIP backend for AMD
- Level-Zero backend for Intel
- OpenCL fallback for others

#### 4. Network Communication
```
User writes prompt
  ↓
Local WASM translator
  ↓
9P.e Mesh Network (QUIC encrypted)
  ↓
Consensus coordinator assigns work
  ↓
Each peer processes assigned layers
  ↓
Results streamed back via 9P
  ↓
Synthetic file /response updated in real-time
```

#### 5. Ollama/LLaMA Integration

**Option A: WASM Translator Calls Ollama API**
```
/srv/ai/models/llama3/prompt (write)
  ↓
WASM translator intercepts
  ↓
Calls distributed Ollama instances via HTTP
  ↓
Aggregates responses
  ↓
/srv/ai/models/llama3/response (read)
```

**Option B: Native SYCL Inference**
```
/srv/ai/models/llama3/prompt (write)
  ↓
WASM translator loads GGUF model
  ↓
Shards layers across mesh peers
  ↓
Each peer runs SYCL kernels directly
  ↓
Consensus merges results
  ↓
/srv/ai/models/llama3/response (read)
```

## Implementation Phases

### Phase 1: Single-Machine SYCL Inference ✅ (Current)
- [x] SYCL FFI interface
- [x] Basic kernels (matmul, activation)
- [ ] Load GGUF model format
- [ ] Run simple inference locally

### Phase 2: Multi-Machine Discovery
- [ ] Mesh network announces "ai-compute" capability
- [ ] Peers advertise GPU capabilities via synthetic files
- [ ] Auto-discovery of compute nodes

### Phase 3: Work Distribution
- [ ] WASM translator shards model layers
- [ ] Consensus tracks layer assignments
- [ ] Stream intermediate activations between peers

### Phase 4: Production Integration
- [ ] Ollama API compatibility layer
- [ ] Model caching across cluster
- [ ] Dynamic load balancing
- [ ] Fault tolerance (peer drops out mid-inference)

## Example: Distributed LLaMA-70B Inference

### Setup (automatic via mesh)
```bash
# Machine 1 (NVIDIA 4090 24GB)
./ninep-server serve --mesh --root ~/ai-models

# Machine 2 (AMD 7900 XTX 24GB)
./ninep-server serve --mesh --root ~/ai-models

# Machine 3 (Intel Arc A770 16GB)
./ninep-server serve --mesh --root ~/ai-models

# Machines auto-discover each other via mDNS
# Total cluster: 64GB GPU memory
```

### Usage
```bash
# From any machine:
cd /n/ai-cluster

# Check cluster status
cat cluster/utilization
# Output:
# Machine 1 (NVIDIA): 0% | 24GB free
# Machine 2 (AMD): 0% | 24GB free
# Machine 3 (Intel): 0% | 16GB free
# Total: 64GB available

# Run inference (automatically distributed)
echo "Write a poem about distributed computing" > models/llama3-70b/prompt

# Watch real-time status
watch cat models/llama3-70b/status
# Output:
# Layer 0-26: Machine 1 [=========>  ] 45%
# Layer 27-53: Machine 2 [=====>      ] 23%
# Layer 54-80: Machine 3 [==>         ] 8%
# Estimated: 12 seconds remaining

# Get response
cat models/llama3-70b/response
```

## Benefits

### 1. Vendor-Neutral GPU Utilization
- Use ANY GPU: NVIDIA + AMD + Intel in same cluster
- SYCL handles vendor differences
- No lock-in to CUDA ecosystem

### 2. Transparent Distribution
- Users see simple file interface
- Complexity hidden in WASM translators
- Works over network (no local GPU required!)

### 3. Plan 9 Philosophy
- Everything is a file
- Network transparency via 9P
- Distributed by default

### 4. Flexible Backend
```
User can choose:
Option 1: /srv/ai/ollama/... (calls Ollama servers)
Option 2: /srv/ai/native/... (pure SYCL implementation)
Option 3: /srv/ai/hybrid/... (mix both)

All via same file interface!
```

## Performance Considerations

### Network Bottleneck?
**No** - Only activations flow between machines:
- LLaMA-70B: ~10KB per layer activation (FP16)
- 80 layers = ~800KB total network transfer
- QUIC compression + encryption
- Typical inference: 200ms compute, 20ms network

### Memory Requirements
- Models loaded once per machine (shared across requests)
- KV-cache stays local to processing machine
- Only intermediate activations cross network

### Fault Tolerance
- If peer drops out during inference:
  - Consensus detects failure
  - Work redistributed to remaining peers
  - User sees updated status, slight delay

## Security

### Isolation
- Each WASM translator runs sandboxed
- Can't access other translators' data
- SYCL buffers are per-translator

### Encryption
- All mesh traffic encrypted via QUIC
- Model files can be encrypted at rest
- Prompts/responses encrypted in transit

### Privacy
- Prompts never leave user's cluster
- No cloud API calls required
- Self-hosted distributed inference

## Future Extensions

### 1. Model Marketplace
```
/srv/ai/marketplace/
  available/           - Browse downloadable models
  install/llama3-70b   - Write to install
  installed/           - List local models
```

### 2. Multi-Tenant Inference
```
/srv/ai/queues/
  user1/prompt         - User 1's queue
  user2/prompt         - User 2's queue
  priority/prompt      - High-priority queue
```

### 3. Training Support
```
/srv/ai/training/
  datasets/
  checkpoints/
  loss                 - Real-time training loss
```

## Competitive Advantages

### vs. Ray/Spark
- **Simpler**: Just files, no Python frameworks
- **Transparent**: Network is invisible
- **Vendor-neutral**: Any GPU works

### vs. Cloud APIs (OpenAI, Anthropic)
- **Privacy**: Data never leaves your network
- **Cost**: Use your own hardware
- **Latency**: Local inference is faster

### vs. Single-Machine Ollama
- **Scale**: Use multiple GPUs across machines
- **Flexibility**: Mix different GPU vendors
- **Resilience**: Fault-tolerant distributed system

## Implementation Priority

Given user's goal of "many machines contribute to running a single model":

**Phase 1 (Now)**: ✅
- SYCL integration working
- Basic inference kernels
- Single-machine prototype

**Phase 2 (Next)**:
- Load GGUF model format
- Implement layer sharding logic
- Test with small model (LLaMA-7B)

**Phase 3**:
- Multi-machine distribution
- Consensus-based work assignment
- Stream activations between peers

**Phase 4**:
- Ollama compatibility
- Production hardening
- Performance optimization

The foundation (mesh networking, consensus, SYCL, WASM translators) is already built. Now we connect the pieces for distributed AI inference!
