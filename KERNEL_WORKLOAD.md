# The Real Work: GPU Kernels

## The Hard Truth

**GPU API (easy):**
- Open `/dev/dri/renderD128`
- `ioctl()` to allocate memory
- `ioctl()` to submit work
- `ioctl()` to wait
- **Total: 500 lines, 2 days**

**ML Kernels (hard):**
- MatMul with XMX tensor cores
- Conv2D with im2col transform
- Attention (Q @ K^T @ V)
- Softmax, LayerNorm, ReLU, GELU
- Optimize for Arc GPU architecture
- **Total: 5000+ lines, 2-6 months**

**The API is 10% of the work. Kernels are 90%.**

## Why Kernels Are Hard

### 1. XMX Matrix Units Are Picky

**XMX wants:**
- 8x8 tiles in BF16
- 2D block layout (not linear)
- Subgroup size = 16
- Specific memory alignment

**Get it wrong:**
- Falls back to scalar ALUs
- 25 TFLOPS → 2 TFLOPS
- You wasted the tensor cores

**Example:**
```c
// Wrong: Scalar (2 TFLOPS)
for (int i = 0; i < 8; i++) {
    for (int j = 0; j < 8; j++) {
        sum += A[i][j] * B[i][j];  // One at a time
    }
}

// Right: XMX (25 TFLOPS)
acc = intel_sub_group_bf16_bf16_matrix_mad_k16(
    a_tile, b_tile, acc  // 64 ops in one instruction
);
```

**One wrong memory access pattern = no tensor cores.**

### 2. Memory Coalescing

**Arc GPU memory:**
- 512 GB/s bandwidth (good)
- But only if accesses are coalesced
- Random access: 50 GB/s (10x slower)

**Bad access pattern:**
```c
// Each thread reads different location (scattered)
float val = input[thread_id * 1000];  // Cache misses
```

**Good access pattern:**
```c
// Adjacent threads read adjacent memory (coalesced)
float val = input[thread_id];  // One cache line
```

**Same algorithm, 10x performance difference.**

### 3. Occupancy

**Arc B50 specs:**
- 160 execution units (EUs)
- Each EU can run 7 threads
- Need 1120 active threads for full utilization

**Kernel with 64 threads:**
- Only 64/1120 = 5.7% GPU utilization
- Wasting 94% of the hardware

**Need to tile work correctly to hit 100% occupancy.**

### 4. Register Pressure

**Each EU has:**
- 128 registers per thread
- Use >128 = spill to memory (slow)

**Complex kernel:**
```c
float acc[16];      // 16 registers
float a_tile[8];    // 8 registers
float b_tile[8];    // 8 registers
float temp[32];     // 32 registers
// Total: 64 registers (OK)
```

**Too complex:**
```c
float big_array[200];  // 200 registers - SPILLS!
// Now running at 1/10th speed
```

**Have to manually count registers like it's 1990.**

## The Kernel Workload

### Tier 1: Critical Path (Must Have)

These run 90% of ML compute:

**1. MatMul (Matrix Multiply)**
- Used by: Every linear layer, every attention layer
- Complexity: ★★★★☆
- Impact: ★★★★★
- Time: 2-3 weeks
- Variants needed:
  - FP32 (baseline)
  - BF16 + XMX (fast)
  - INT8 + XMX (fastest)
  - Batched (multiple matrices)

**2. Conv2D (Convolution)**
- Used by: CNNs (ResNet, EfficientNet)
- Complexity: ★★★★★
- Impact: ★★★★☆
- Time: 2-3 weeks
- Approach: im2col + MatMul (reuse XMX)

**3. Attention (Scaled Dot-Product)**
- Used by: Transformers (every LLM)
- Complexity: ★★★★★
- Impact: ★★★★★
- Time: 2 weeks
- Components:
  - Q @ K^T (matmul)
  - Softmax
  - @ V (matmul)
  - FlashAttention optimization

### Tier 2: Activation Functions (Need for Training)

**4. ReLU / GELU**
- Complexity: ★☆☆☆☆
- Time: 1 day each
- Elementwise, easy

**5. Softmax**
- Complexity: ★★★☆☆
- Time: 3 days
- Reduction across dimension, needs sync

**6. LayerNorm**
- Complexity: ★★★☆☆
- Time: 3 days
- Mean/variance reduction + normalize

### Tier 3: Utilities (Quality of Life)

**7. Reduce (sum/max/mean)**
- Complexity: ★★☆☆☆
- Time: 2 days
- Parallel reduction patterns

**8. Elementwise (add/mul/div)**
- Complexity: ★☆☆☆☆
- Time: 1 day
- Trivial but needed everywhere

**9. Transpose / Reshape**
- Complexity: ★★☆☆☆
- Time: 2 days
- Memory layout transforms

### Tier 4: Advanced (Nice to Have)

**10. Embedding Lookup**
- Complexity: ★★★☆☆
- Time: 1 week

**11. Dropout**
- Complexity: ★★☆☆☆
- Time: 2 days

**12. BatchNorm**
- Complexity: ★★★☆☆
- Time: 3 days

## Work Estimate

### Minimum Viable Product (MVP)
**Kernels: MatMul, Conv2D, ReLU, Softmax**
- Can run: ResNet inference
- Can run: Small transformer inference
- Time: **6-8 weeks**

### Production Ready
**Add: Attention, LayerNorm, all activations**
- Can run: Any transformer (BERT, GPT, Llama)
- Can run: Any CNN
- Time: **3-4 months**

### Optimized
**Add: FlashAttention, kernel fusion, INT8**
- Competitive with CUDA performance
- Training support
- Time: **6 months**

## The Optimization Rabbit Hole

**Each kernel needs:**

### 1. Naive implementation (1 day)
```c
// Works but slow
for (int i...) for (int j...) sum += A[i]*B[j];
```

### 2. Tiled implementation (3 days)
```c
// Use shared memory, 2x faster
for (int tile...) {
    load_tile_shared();
    compute_tile();
}
```

### 3. XMX implementation (1 week)
```c
// Use tensor cores, 10x faster
acc = intel_sub_group_bf16_matrix_mad(a, b, acc);
```

### 4. Fully optimized (2 weeks)
```c
// XMX + coalescing + occupancy + register optimization
// 25x faster than naive
```

**Each kernel goes through this cycle.**

## Why We Can't Just Steal Kernels

### NVIDIA CUDA Kernels
- Won't compile for Intel (different ISA)
- CUDA intrinsics don't exist on Arc
- Can study algorithms, but rewrite from scratch

### Intel oneDNN Kernels
- Buried in 10GB of oneAPI spaghetti
- Tightly coupled to DPC++ runtime
- Licensing unclear (probably proprietary)
- Would need to extract and rewrite anyway

### OpenCL Reference Kernels
- Usually naive implementations
- No XMX usage (generic)
- Good for learning, not production

**We have to write them ourselves.**

## The Strategy

### Phase 1: Get Something Working (2 weeks)
- Naive MatMul (FP32, no XMX)
- Naive Conv2D (im2col approach)
- Basic ReLU
- **Goal: Run a simple model end-to-end**

### Phase 2: Add Tensor Cores (4 weeks)
- MatMul with XMX (BF16)
- Conv2D with XMX
- 10x speedup achieved
- **Goal: Competitive with CUDA on simple models**

### Phase 3: Transformer Support (4 weeks)
- Attention kernel
- Softmax
- LayerNorm
- **Goal: Run Llama 7B inference**

### Phase 4: Optimize Everything (8 weeks)
- Kernel fusion (ReLU+MatMul, etc)
- INT8 quantization
- FlashAttention
- **Goal: Match CUDA performance**

**Total: 4-6 months of kernel work**

## Can We Shortcut This?

### Option 1: Compile PyTorch IR
- PyTorch → ONNX → SPIR-V → GPU
- Auto-generate kernels
- **Problem:** Won't use XMX (too generic)

### Option 2: Use AI to Generate Kernels
- Give LLM the XMX spec
- Generate optimized kernels
- **Problem:** LLMs are bad at optimization

### Option 3: Port from Intel's Examples
- Intel has XMX examples in docs
- Rewrite for our API
- **Best option:** Study examples, write ourselves

### Option 4: Minimal Kernel Set
- MatMul with XMX (1 month)
- Everything else via MatMul (Conv2D = im2col + MatMul)
- **Trade-off:** Slower but faster to ship

**Probably: Option 3 + Option 4 hybrid**

## Why This Matters

**The GPU API is commodity:**
- CUDA does it
- ROCm does it
- Metal does it
- Ours will too

**The kernels are differentiation:**
- CUDA has 15 years of optimized kernels
- We need 6 months to match
- But we only need 80% performance to win (Arc is cheaper)

**The real work:**
- Week 1: Build API (easy)
- Months 2-6: Write kernels (hard)
- Month 7: Release and watch Intel's market share grow

## Bottom Line

**You asked: "The real work becomes the kernels, correct?"**

**Answer: Yes. The API is a weekend project. The kernels are a career.**

But here's the thing: **We only need 10-20 kernels to run 90% of ML models.**

CUDA has thousands of kernels. We need a dozen good ones.

**MatMul with XMX is 60% of the work. Get that right, everything else follows.**
