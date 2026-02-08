# Hybrid SYCL Strategy: Intel oneAPI + AdaptiveCpp

## Philosophy

**Use the right tool for each vendor:**
- Intel hardware → Intel oneAPI DPC++ (native, optimized, oneMKL included)
- NVIDIA/AMD/Others → AdaptiveCpp (cross-vendor compatibility)

## Architecture

```
┌─────────────────────────────────────────────────┐
│           Rust FFI Layer (sycl_ffi.rs)          │
└─────────────────────┬───────────────────────────┘
                      │
          ┌───────────┴───────────┐
          │                       │
┌─────────▼──────────┐   ┌────────▼─────────────┐
│  Intel oneAPI Path │   │ AdaptiveCpp Path     │
│  (libsycl_intel.so)│   │ (libsycl_adaptive.so)│
└────────────────────┘   └──────────────────────┘
          │                       │
┌─────────▼──────────┐   ┌────────▼─────────────┐
│ Intel DPC++ Compiler│   │ AdaptiveCpp Compiler │
│ + oneMKL + Level-Zero│  │ CUDA/HIP/OpenCL     │
└────────────────────┘   └──────────────────────┘
```

## Implementation Plan

### 1. Detection at Build Time

```rust
// In build.rs
fn main() {
    // Detect Intel oneAPI
    let has_oneapi = Path::new("/opt/intel/oneapi/compiler").exists();

    // Detect AdaptiveCpp
    let has_adaptivecpp = Path::new("/opt/adaptivecpp/bin/acpp").exists();

    if has_oneapi {
        println!("cargo:rustc-cfg=feature=\"intel_oneapi\"");
        build_intel_backend();
    }

    if has_adaptivecpp {
        println!("cargo:rustc-cfg=feature=\"adaptive_sycl\"");
        build_adaptive_backend();
    }
}
```

### 2. Runtime Device Selection

```cpp
// In sycl_ffi.cpp
SyclError sycl_discover_devices() {
    auto platforms = sycl::platform::get_platforms();

    for (const auto& platform : platforms) {
        std::string platform_name = platform.get_info<sycl::info::platform::name>();

        if (platform_name.find("Intel") != std::string::npos) {
            #ifdef USE_INTEL_ONEAPI
                // Use Intel-optimized path
                discover_intel_devices_oneapi(platform);
            #else
                // Fall back to AdaptiveCpp
                discover_devices_adaptive(platform);
            #endif
        } else {
            #ifdef USE_ADAPTIVE_CPP
                // NVIDIA/AMD via AdaptiveCpp
                discover_devices_adaptive(platform);
            #endif
        }
    }
}
```

### 3. Dual Build System

**Makefile changes:**

```makefile
# Intel oneAPI backend (if available)
intel-backend: sycl_intel.cpp
	@if [ -f /opt/intel/oneapi/compiler/latest/bin/icpx ]; then \
		echo "Building Intel oneAPI backend..."; \
		source /opt/intel/oneapi/setvars.sh && \
		icpx -fsycl -fPIC -shared -O3 \
			-DUSE_INTEL_ONEAPI \
			sycl_intel.cpp \
			-o libsycl_intel.so \
			-lmkl_sycl -lmkl_intel_ilp64 -lmkl_tbb_thread -lmkl_core; \
	else \
		echo "Intel oneAPI not found, skipping"; \
	fi

# AdaptiveCpp backend (cross-vendor)
adaptive-backend: sycl_adaptive.cpp
	@if [ -f /opt/adaptivecpp/bin/acpp ]; then \
		echo "Building AdaptiveCpp backend..."; \
		/opt/adaptivecpp/bin/acpp -std=c++17 -O3 -fPIC \
			-DUSE_ADAPTIVE_CPP \
			sycl_adaptive.cpp \
			-shared -o libsycl_adaptive.so; \
	else \
		echo "AdaptiveCpp not found, skipping"; \
	fi

all: intel-backend adaptive-backend
```

## Expected Performance

### Intel Hardware (with oneAPI)
- **oneMKL matmul**: 100x faster than naive SYCL
- **Level-Zero backend**: Direct GPU access, lowest latency
- **Zero-copy iGPU**: Shared memory with CPU (2-3x faster)
- **XMX tensor cores**: Full utilization on Arc GPUs

### NVIDIA/AMD Hardware (with AdaptiveCpp)
- **CUDA backend**: Native NVIDIA support
- **HIP backend**: Native AMD support
- **Standards compliance**: SYCL 2020 portable code

## Installation Guide

### Install Intel oneAPI (for Intel hardware)

```bash
# Add Intel repository
wget https://apt.repos.intel.com/intel-gpg-keys/GPG-PUB-KEY-INTEL-SW-PRODUCTS.PUB
sudo apt-key add GPG-PUB-KEY-INTEL-SW-PRODUCTS.PUB
echo "deb https://apt.repos.intel.com/oneapi all main" | \
  sudo tee /etc/apt/sources.list.d/oneAPI.list

# Install oneAPI with oneMKL
sudo apt update
sudo apt install \
  intel-oneapi-compiler-dpcpp-cpp \
  intel-oneapi-mkl-devel \
  intel-oneapi-tbb-devel

# Source environment
source /opt/intel/oneapi/setvars.sh
```

### Keep AdaptiveCpp (for cross-vendor support)

Already installed at `/opt/adaptivecpp` - no changes needed.

## Cargo Features

```toml
[features]
# Intel-optimized path (requires oneAPI)
intel-full = ["intel-oneapi", "intel-mkl"]
intel-oneapi = []
intel-mkl = []

# Cross-vendor support (requires AdaptiveCpp)
adaptive-sycl = []
cuda = ["adaptive-sycl"]
rocm = ["adaptive-sycl"]

# Default: try both, use what's available
default = ["intel-full", "adaptive-sycl"]
```

## Code Organization

```
9pe-server/
├── sycl_ffi.hpp              # Common FFI header
├── sycl_intel.cpp            # Intel oneAPI implementation
├── sycl_adaptive.cpp         # AdaptiveCpp implementation
├── sycl_common.cpp           # Shared utilities
└── src/
    └── sycl/
        ├── ffi.rs            # Rust FFI bindings
        └── backend.rs        # Runtime backend selection
```

## Why This Works Better

**Current problem with AdaptiveCpp-only:**
- ❌ Hanging on Intel GPU device enumeration
- ❌ Missing LLVM library version issues
- ❌ No oneMKL integration (100x slower matmul)
- ❌ Sub-optimal Intel GPU support

**With Intel oneAPI for Intel hardware:**
- ✅ Native Intel GPU support (no hangs)
- ✅ No library dependency issues
- ✅ oneMKL included (100x faster)
- ✅ Full Level-Zero backend support

**With AdaptiveCpp for others:**
- ✅ NVIDIA CUDA support
- ✅ AMD ROCm/HIP support
- ✅ Standards-compliant fallback
- ✅ Future-proof for new vendors

## Migration Path

1. **Phase 1** (immediate): Install Intel oneAPI, build Intel backend
2. **Phase 2**: Test Intel devices with oneAPI path (should work perfectly)
3. **Phase 3**: Keep AdaptiveCpp for future NVIDIA/AMD support
4. **Phase 4**: Add runtime device → backend mapping

## Decision Points

**When to use Intel path:**
- Device vendor is Intel
- Device platform name contains "Intel"
- User explicitly requests Intel optimization

**When to use AdaptiveCpp path:**
- Device vendor is NVIDIA/AMD/other
- Intel oneAPI not installed
- Fallback/compatibility mode

## Summary

**NO TENSOR LEFT BEHIND** - but use the right tool for each tensor!

- Intel tensors → Intel oneAPI DPC++ + oneMKL
- NVIDIA tensors → AdaptiveCpp + CUDA
- AMD tensors → AdaptiveCpp + ROCm
- Future tensors → AdaptiveCpp + [new backend]

This gives you **best performance** AND **maximum compatibility**.
