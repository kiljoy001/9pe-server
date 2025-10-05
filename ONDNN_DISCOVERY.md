# oneDNN Discovery: The Good News

## You Were Right - It's All There!

**oneDNN on GitHub:** https://github.com/oneapi-src/oneDNN
**License:** Apache 2.0 (open source!)

## What I Found

### The Source Code IS Available

```
/tmp/oneDNN/
├── src/gpu/intel/
│   ├── matmul/
│   │   ├── ref.cl              # Reference OpenCL kernel
│   │   ├── gemm.cpp            # GEMM-based matmul
│   │   └── sparse_ref.cl       # Sparse matmul
│   └── gemm/
│       └── jit/
│           ├── generator/      # JIT code generator
│           │   ├── pieces/
│           │   │   ├── gemm_microkernel.cxx  # THE GOOD STUFF
│           │   │   ├── matrix_multiply.cxx
│           │   │   └── monolithic_k_loop_dpasw.cxx  # DPAS = XMX!
│           │   └── generator.cpp
│           └── include/gemmstone/  # GEMM generator framework
```

### The XMX Code Is There

**File:** `src/gpu/intel/gemm/jit/generator/pieces/gemm_microkernel.cxx`
- Contains actual DPAS (XMX tensor core) instructions
- JIT-compiled assembly generation
- Arc-specific optimizations

### The Problem: It's Not Simple OpenCL

**What I expected:**
```c
// Simple OpenCL kernel
__kernel void matmul(...) {
    acc = intel_sub_group_bf16_matrix_mad(a, b, acc);
}
```

**What Intel actually does:**
```cpp
// C++ code that GENERATES assembly at runtime
dpasw(8|M0, acc0.xw, A0_regs[k].sub(boffset), B_regs0[k]);
```

**It's a JIT compiler** that generates GPU assembly code on-the-fly!

## The Architecture

### Intel's Approach

```
oneDNN API
    ↓
Gemmstone (JIT framework)
    ↓
ngen (code generator)
    ↓
Generate GPU assembly (dpasw instructions)
    ↓
Compile to binary kernel
    ↓
Execute on GPU
```

### Why They Do This

**Advantages:**
- Can optimize for exact matrix sizes at runtime
- Handle different data types (FP32, BF16, INT8)
- Tune for specific GPU (Xe, Xe-HP, Xe-HPC)
- Register allocation optimized per problem

**Disadvantages:**
- Can't just copy the kernel
- Need the entire JIT infrastructure
- Compilation overhead at first call

## What We Can Extract

### 1. The Algorithms (✅ Easy)

**From the source, we can learn:**
- Tiling strategies
- Memory access patterns
- XMX usage patterns
- Register allocation strategies

**Example from** `gemm_microkernel.cxx`:
- Uses 8x8 tiles for DPAS
- Loads A/B into registers
- Accumulates in FP32
- Writes back to C

### 2. The Reference Kernels (✅ Available)

**File:** `src/gpu/intel/matmul/ref.cl`
- Actual OpenCL code (not JIT)
- Simple implementation
- Shows basic algorithm
- We can use as starting point!

### 3. The JIT Generator (❌ Complex)

**Could we use it?**
- Gemmstone is Apache 2.0 licensed
- Could link against it
- Would need ngen library
- Would need to understand the API

**Should we use it?**
- Probably not (too complex)
- We want simple, not JIT
- Better to write own kernels

## The Plan Forward

### Option 1: Use oneDNN Reference Kernel

**Pros:**
- Already works
- Apache 2.0 licensed
- Can modify it
- Good starting point

**Cons:**
- Doesn't use XMX (slow)
- Needs optimization

**Status:** Can do this NOW

### Option 2: Learn From JIT Generator

**Pros:**
- See how Intel does XMX
- Learn tiling strategies
- Understand Arc optimizations

**Cons:**
- Have to read C++ JIT code
- Can't copy-paste

**Status:** Study and reimplement

### Option 3: Write Our Own From Scratch

**Pros:**
- Clean implementation
- Understand every line
- Optimized for our use case

**Cons:**
- Takes longer
- Might miss optimizations

**Status:** Viable but slow

## Recommended Approach

**Phase 1: Use Reference Kernel (Week 1)**
```bash
# Copy Intel's ref.cl
cp /tmp/oneDNN/src/gpu/intel/matmul/ref.cl \
   /home/scott/Repo/9pe-server/gpu/kernels/matmul_ref.cl

# Compile to SPIR-V
clang -cl-std=CL3.0 -target spirv64 matmul_ref.cl -o matmul_ref.spv

# Works immediately, gets us started
```

**Phase 2: Add XMX (Weeks 2-3)**
- Study `gemm_microkernel.cxx`
- Learn DPAS patterns
- Write our own XMX kernel
- Use OpenCL subgroup intrinsics

**Phase 3: Optimize (Weeks 4-6)**
- Better tiling
- Memory coalescing
- Register optimization
- Match Intel's performance

## Code We Can Actually Use

### Intel's Reference Kernel

```c
// From src/gpu/intel/matmul/ref.cl
__kernel void ref_matmul(
    __global SRC_DATA_T *A,
    __global WEI_DATA_T *B,
    __global DST_DATA_T *C,
    ...
) {
    // Actual working OpenCL code
    // Apache 2.0 licensed
    // We can use/modify this!
}
```

**This gives us:**
- Working matmul kernel
- Proper memory access patterns
- Broadcasting support
- Post-ops handling

### What We Need to Add

```c
// Our XMX optimization
#pragma OPENCL EXTENSION cl_intel_subgroup_matrix_multiply_accumulate : enable

__kernel void matmul_xmx(...) {
    // Take Intel's ref.cl structure
    // Add XMX intrinsics
    // Profit!

    acc = intel_sub_group_bf16_bf16_matrix_mad_k16(a, b, acc);
}
```

## The Bottom Line

**Q: Is oneDNN source code useful?**

**A: YES! But not in the way I thought.**

**What's useful:**
- ✅ Reference OpenCL kernels (can copy)
- ✅ Algorithm documentation
- ✅ Tiling strategies to learn from
- ✅ Problem decomposition

**What's not directly usable:**
- ❌ JIT generator (too complex)
- ❌ Gemmstone framework (overkill)
- ❌ ngen assembly (requires infrastructure)

**Our path:**
1. Copy `ref.cl` as starting point (Apache 2.0 allows this)
2. Study `gemm_microkernel.cxx` for XMX patterns
3. Rewrite XMX version using OpenCL intrinsics
4. Optimize based on Intel's strategies

**Timeline:**
- Week 1: Working matmul (from ref.cl)
- Week 2-3: XMX optimization
- Week 4-6: Performance tuning

**We CAN use Intel's code. Just not the way I expected!**
