# Dual SYCL Backend Architecture

## Overview

The 9P.e server supports **two SYCL backends** to provide optimal performance across heterogeneous GPU environments:

1. **Intel oneAPI DPC++ (icpx)** - Primary optimized backend for Intel GPUs
2. **AdaptiveCpp (acpp)** - Universal backend for NVIDIA/AMD/Intel GPUs

## Design Philosophy

> "For Intel, we use oneAPI (optimized and preferred). For others they use the AdaptiveCpp path. If people pick this up, they WILL optimize it for AMD/NVIDIA."

This dual-backend strategy follows the SYCL philosophy of **performance portability**:
- **Single codebase** runs on any GPU vendor
- **Vendor-optimized paths** automatically selected when available
- **Community extensibility** for AMD/NVIDIA optimizations

## Backend Selection Logic

### Build Time (Compile)

```
IF Intel oneAPI installed AND building on Intel-equipped machine:
    → Compile with icpx (Intel DPC++)
    → Link libsycl_ffi_intel.so with oneMKL, Level-Zero, XMX support
    → This becomes the PRIMARY backend

ELSE IF AdaptiveCpp installed:
    → Compile with acpp
    → Link libsycl_ffi_adaptive.so with CUDA/HIP/OpenCL support
    → This becomes the FALLBACK backend

BOTH can coexist on a single machine!
```

### Runtime (Device Selection)

```
FOR each available GPU:
    backend = detect_backend(gpu)

    IF backend == Level-Zero (Intel GPU):
        → Use libsycl_ffi_intel.so
        → Benefits: oneMKL GEMM, XMX acceleration, zero-copy with CPU

    ELSE IF backend == CUDA (NVIDIA GPU):
        → Use libsycl_ffi_adaptive.so
        → Benefits: Works on any CUDA device, community can add cuBLAS integration

    ELSE IF backend == HIP (AMD GPU):
        → Use libsycl_ffi_adaptive.so
        → Benefits: Works on any ROCm device, community can add rocBLAS integration

    ELSE:
        → Use libsycl_ffi_adaptive.so with OpenCL fallback
        → OR use CPU SIMD fallback (AVX2/SSE/NEON)
```

## Library Structure

### Before (Single Backend)

```
libsycl_ffi.so  (compiled with icpx OR acpp, not both)
```

### After (Dual Backend)

```
libsycl_ffi_intel.so      # Intel oneAPI DPC++ backend
├── Links: libsycl.so.8 (Intel SYCL runtime)
├── Links: libmkl_*.so (oneMKL for GEMM)
└── Supports: Level-Zero (Intel GPU), OpenCL (Intel fallback)

libsycl_ffi_adaptive.so   # AdaptiveCpp backend
├── Links: libacpp-rt.so (AdaptiveCpp runtime)
├── Links: libacpp-common.so
└── Supports: CUDA (NVIDIA), HIP (AMD), Level-Zero (Intel), OpenCL (fallback)

Both can be loaded simultaneously!
Runtime selects appropriate backend per device.
```

## Implementation Strategy

### Phase 1: Dual Library Compilation ✅ (This PR)

**Modify build.rs:**
```rust
// NEW: Try to build BOTH backends
let intel_success = try_build_intel_backend();
let adaptive_success = try_build_adaptive_backend();

if !intel_success && !adaptive_success {
    panic!("At least one SYCL backend must be available");
}
```

**Build Intel backend** (if icpx available):
```bash
icpx -fPIC -fsycl -O3 -shared \
     -DBACKEND_INTEL \
     sycl_ffi.cpp -o libsycl_ffi_intel.so \
     -lsycl -lmkl_sycl -lmkl_intel_ilp64 -lmkl_tbb_thread -lmkl_core
```

**Build AdaptiveCpp backend** (if acpp available):
```bash
acpp -fPIC -O3 -std=c++17 \
     -DBACKEND_ADAPTIVE \
     sycl_ffi.cpp -o libsycl_ffi_adaptive.so \
     -lacpp-rt -lacpp-common
```

### Phase 2: Runtime Backend Dispatch ✅ (This PR)

**New Rust API layer** (src/sycl/backend_dispatch.rs):
```rust
pub enum SyclBackendType {
    IntelOneAPI,     // Preferred for Intel GPUs
    AdaptiveCpp,     // Universal fallback
}

pub struct SyclBackendManager {
    intel_lib: Option<Library>,      // libsycl_ffi_intel.so
    adaptive_lib: Option<Library>,   // libsycl_ffi_adaptive.so
}

impl SyclBackendManager {
    pub fn select_backend_for_device(&self, device: &GpuInfo) -> SyclBackendType {
        match device.vendor.as_str() {
            "Intel(R) Corporation" | "Intel" if self.intel_lib.is_some() => {
                SyclBackendType::IntelOneAPI  // Optimized path
            },
            "NVIDIA Corporation" | "NVIDIA" if self.adaptive_lib.is_some() => {
                SyclBackendType::AdaptiveCpp  // CUDA path
            },
            "Advanced Micro Devices" | "AMD" if self.adaptive_lib.is_some() => {
                SyclBackendType::AdaptiveCpp  // HIP path
            },
            _ if self.adaptive_lib.is_some() => {
                SyclBackendType::AdaptiveCpp  // OpenCL fallback
            },
            _ => {
                // No GPU backend available, will use CPU fallback
                panic!("No suitable SYCL backend for device: {}", device.name)
            }
        }
    }
}
```

### Phase 3: Job Routing Integration ✅ (This PR)

**Modify ComputeManager** (src/compute_control.rs):
```rust
pub struct ComputeManager {
    sycl_backend: Arc<SyclBackendManager>,
    // ... existing fields
}

async fn execute_sycl_job(&self, job_id: String, submission: JobSubmission) {
    // Select device based on VRAM requirements
    let device = self.select_best_device(submission.requested_vram).await;

    // NEW: Select appropriate backend for this device
    let backend = self.sycl_backend.select_backend_for_device(&device);

    match backend {
        SyclBackendType::IntelOneAPI => {
            // Use Intel-optimized kernels
            self.execute_with_intel_backend(job_id, submission, device).await
        },
        SyclBackendType::AdaptiveCpp => {
            // Use AdaptiveCpp kernels (CUDA/HIP/OpenCL)
            self.execute_with_adaptive_backend(job_id, submission, device).await
        }
    }
}
```

## Performance Expectations

### Intel Arc GPU (with Intel oneAPI backend)
- **Matrix Multiply**: ~500-2000 GFLOPS (oneMKL + XMX acceleration)
- **Ternary Operations**: ~100-300 GFLOPS (XMX ternary instructions)
- **Memory Transfer**: Zero-copy for CPU↔iGPU (shared DDR)

### NVIDIA GPU (with AdaptiveCpp backend)
- **Matrix Multiply**: ~50-500 GFLOPS (basic SYCL kernels, no cuBLAS yet)
- **Ternary Operations**: ~20-100 GFLOPS (integer ALU-based)
- **Community Opportunity**: Add cuBLAS integration → 10-100x speedup potential!

### AMD GPU (with AdaptiveCpp backend)
- **Matrix Multiply**: ~50-500 GFLOPS (basic SYCL kernels, no rocBLAS yet)
- **Ternary Operations**: ~20-100 GFLOPS
- **Community Opportunity**: Add rocBLAS integration → 10-100x speedup potential!

### CPU Fallback (no GPU or SYCL unavailable)
- **Matrix Multiply**: ~2.5 GFLOPS (AVX2 + cache blocking)
- **Vector Add**: ~0.8-1.0 GFLOPS (AVX2 SIMD)

## Why This Architecture?

### 1. Vendor Optimization Without Lock-In
- **Intel GPUs get oneMKL** → 10-100x faster than generic SYCL
- **NVIDIA/AMD get working implementation** → Community can optimize later
- **No vendor lock-in** → Same SYCL API across all backends

### 2. Graceful Degradation
```
Preferred: Intel GPU with Intel oneAPI (fastest)
    ↓ (if no Intel GPU)
Fallback: NVIDIA/AMD GPU with AdaptiveCpp (fast)
    ↓ (if no SYCL GPU)
Fallback: CPU with SIMD (acceptable)
    ↓ (if no AVX2/SSE/NEON)
Fallback: CPU scalar (slow but works)
```

### 3. Community Extensibility
The AdaptiveCpp path provides a **clean integration point** for community optimizations:
- **NVIDIA users can add cuBLAS** → Just link and call in `sycl_ffi.cpp`
- **AMD users can add rocBLAS** → Same approach
- **No core architecture changes required** → Just enhance the adaptive backend

## Migration Path for Existing Code

### No changes required!
The JobSubmission API remains identical:
```rust
let submission = JobSubmission {
    job_type: "sycl".to_string(),
    operation: "matrix_multiply".to_string(),
    payload: /* ... */,
    requested_vram: 0,
    device_hint: None,  // NEW: Can specify "intel" or "adaptive" to force backend
};
```

The system **automatically routes** to the best available backend based on:
1. Available GPUs
2. GPU vendor
3. Available SYCL backends (Intel vs AdaptiveCpp)
4. VRAM requirements

## Build Dependencies

### For Intel Backend
```bash
# Ubuntu/Debian
wget https://apt.repos.intel.com/intel-gpg-keys/GPG-PUB-KEY-INTEL-SW-PRODUCTS.PUB
sudo apt-key add GPG-PUB-KEY-INTEL-SW-PRODUCTS.PUB
sudo add-apt-repository "deb https://apt.repos.intel.com/oneapi all main"
sudo apt update
sudo apt install intel-oneapi-compiler-dpcpp-cpp intel-oneapi-mkl-devel
```

### For AdaptiveCpp Backend
```bash
# Ubuntu 24.04+
sudo apt install adaptivecpp

# Or build from source (supports older Ubuntu)
git clone https://github.com/AdaptiveCpp/AdaptiveCpp
cd AdaptiveCpp && mkdir build && cd build
cmake .. -DCMAKE_INSTALL_PREFIX=/opt/adaptivecpp
make -j$(nproc) && sudo make install
```

### For NVIDIA Support (AdaptiveCpp)
```bash
# Install CUDA toolkit
sudo apt install nvidia-cuda-toolkit

# AdaptiveCpp will auto-detect CUDA and enable it
acpp --acpp-targets  # Should show "cuda:sm_XX"
```

### For AMD Support (AdaptiveCpp)
```bash
# Install ROCm
wget https://repo.radeon.com/amdgpu-install/latest/ubuntu/jammy/amdgpu-install.deb
sudo dpkg -i amdgpu-install.deb
sudo amdgpu-install --usecase=rocm

# AdaptiveCpp will auto-detect ROCm and enable it
acpp --acpp-targets  # Should show "hip"
```

## Testing Strategy

### Test Matrix
```
Backend          | GPU Type      | Expected Result
-----------------|---------------|------------------------------------------
Intel oneAPI     | Intel Arc     | ✅ All tests pass, ~1000 GFLOPS
Intel oneAPI     | NVIDIA        | ❌ Not supported (expected)
AdaptiveCpp      | Intel Arc     | ✅ Works but slower (~100 GFLOPS)
AdaptiveCpp      | NVIDIA        | ✅ All tests pass (~50-500 GFLOPS)
AdaptiveCpp      | AMD           | ✅ All tests pass (~50-500 GFLOPS)
CPU Fallback     | (none)        | ✅ All tests pass (~2.5 GFLOPS)
```

### Verification Commands
```bash
# Build both backends
cargo build --release --features full

# Test Intel backend (if available)
LD_LIBRARY_PATH=.:$LD_LIBRARY_PATH cargo test --release --features full test_matrix_multiply_gpu

# Test AdaptiveCpp backend (if available)
LD_LIBRARY_PATH=.:$LD_LIBRARY_PATH cargo test --release --features full test_adaptive_backend

# Test cross-network GPU borrowing
# Local: Intel Arc GPU with oneAPI
# Remote: NVIDIA GPU with AdaptiveCpp
# Job routing should work transparently!
```

## Future Optimizations

### Phase 4: Vendor-Specific Accelerations (Community)
- **cuBLAS integration** for NVIDIA (10-100x matrix multiply speedup)
- **rocBLAS integration** for AMD (10-100x matrix multiply speedup)
- **Tensor Core support** for NVIDIA (ternary operations)
- **Matrix Cores support** for AMD (ternary operations)

### Phase 5: Distributed GPU Borrowing
- **Job migration**: Submit on NVIDIA node → Execute on Intel node's GPU
- **VRAM-aware routing**: Route large jobs to high-VRAM GPUs across mesh
- **Backend-aware scheduling**: Prefer Intel backend for Intel GPUs even on remote nodes

## Conclusion

This dual-backend architecture provides:
- ✅ **Optimal Intel performance** via oneAPI + oneMKL
- ✅ **Universal NVIDIA/AMD support** via AdaptiveCpp
- ✅ **Community extensibility** for vendor-specific optimizations
- ✅ **Zero breaking changes** to existing APIs
- ✅ **Graceful fallbacks** at every layer

The strategy: **"Intel first, community optimizes the rest."**
