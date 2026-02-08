// Build script for compiling SYCL C++ wrapper with AdaptiveCpp
use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    // Only build SYCL if gpu feature is enabled
    #[cfg(not(feature = "gpu"))]
    {
        println!("cargo:warning=Skipping SYCL build (gpu feature disabled)");
        return;
    }

    #[cfg(feature = "gpu")]
    {
    println!("cargo:rerun-if-changed=sycl_ffi.cpp");
    println!("cargo:rerun-if-changed=sycl_ffi.hpp");

    // NEW DUAL-BACKEND STRATEGY:
    // Try to build BOTH Intel oneAPI and AdaptiveCpp backends
    // At runtime, we'll select the appropriate backend per device

    let mut backends_available = vec![];

    // Try Intel oneAPI backend first
    let intel_success = try_build_intel_backend();
    if intel_success {
        backends_available.push("Intel oneAPI");
    }

    // Try AdaptiveCpp backend
    let adaptive_success = try_build_adaptive_backend();
    if adaptive_success {
        backends_available.push("AdaptiveCpp");
    }

    if backends_available.is_empty() {
        println!("cargo:warning=No SYCL backends available - SYCL support will be disabled");
        println!("cargo:warning=Install Intel oneAPI for Intel GPU support");
        println!("cargo:warning=Install AdaptiveCpp for NVIDIA/AMD GPU support");
        create_stub_library();
        return;
    }

    println!("cargo:warning=SYCL backends available: {}", backends_available.join(", "));

    // RUNTIME DYNAMIC LOADING STRATEGY:
    // Both backends are compiled as separate .so files
    // At RUNTIME, we use dlopen (via libloading) to load the appropriate backend
    // based on detected GPU vendor
    //
    // NO static linking! This allows:
    // - Loading both backends simultaneously (no symbol conflicts)
    // - Per-device backend selection (Intel GPU → Intel oneAPI, NVIDIA → AdaptiveCpp)
    // - Easy deployment (just ship both .so files)
    //
    // The backend_loader.rs module handles dynamic loading at runtime

    println!("cargo:warning=Both backends built as separate shared libraries");
    println!("cargo:warning=Runtime will dynamically load appropriate backend per GPU");
    println!("cargo:warning=No static linking - using dlopen via libloading crate");

    } // End of #[cfg(feature = "gpu")] block
}

/// Try to build Intel oneAPI backend
/// Returns true if successful, false otherwise
fn try_build_intel_backend() -> bool {
    // Check if Intel oneAPI compiler (icpx) is available
    let icpx_available = Command::new("icpx").arg("--version").output().is_ok()
        || Command::new("/opt/intel/oneapi/compiler/latest/bin/icpx")
            .arg("--version")
            .output()
            .is_ok();

    if !icpx_available {
        println!("cargo:warning=Intel oneAPI compiler (icpx) not found - Intel backend disabled");
        return false;
    }

    // Check if pre-built library exists
    let intel_lib = PathBuf::from("libsycl_ffi_intel.so");
    if intel_lib.exists() {
        println!("cargo:warning=Using pre-built Intel oneAPI SYCL library");
        return true;
    }

    println!("cargo:warning=Building Intel oneAPI SYCL backend");

    // Build with icpx
    let icpx_cmd = if Command::new("icpx").arg("--version").output().is_ok() {
        "icpx"
    } else {
        "/opt/intel/oneapi/compiler/latest/bin/icpx"
    };

    // Build as shared library with Intel backend flag
    let output = Command::new(icpx_cmd)
        .args(&[
            "-fPIC",
            "-fsycl",
            "-O3",
            "-shared",
            "-DBACKEND_INTEL",  // Preprocessor flag for Intel-specific code
            "sycl_ffi.cpp",
            "-o", "libsycl_ffi_intel.so",
        ])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            println!("cargo:warning=Intel oneAPI SYCL backend compiled successfully");
            true
        },
        Ok(out) => {
            eprintln!("Intel oneAPI compilation failed:");
            eprintln!("stdout: {}", String::from_utf8_lossy(&out.stdout));
            eprintln!("stderr: {}", String::from_utf8_lossy(&out.stderr));
            println!("cargo:warning=Intel oneAPI backend build failed - continuing without it");
            false
        },
        Err(e) => {
            println!("cargo:warning=Failed to execute icpx: {} - continuing without Intel backend", e);
            false
        }
    }
}

/// Try to build AdaptiveCpp backend
/// Returns true if successful, false otherwise
fn try_build_adaptive_backend() -> bool {
    // Check if AdaptiveCpp (acpp) compiler is available
    let acpp_available = Command::new("acpp").arg("--version").output().is_ok()
        || Command::new("/opt/adaptivecpp/bin/acpp")
            .arg("--version")
            .output()
            .is_ok();

    if !acpp_available {
        println!("cargo:warning=AdaptiveCpp (acpp) not found - AdaptiveCpp backend disabled");
        return false;
    }

    // Check if pre-built library exists
    let adaptive_lib = PathBuf::from("libsycl_ffi_adaptive.so");
    if adaptive_lib.exists() {
        println!("cargo:warning=Using pre-built AdaptiveCpp SYCL library");
        // Still need to check runtime libraries
        let acpp_cmd = if Command::new("acpp").arg("--version").output().is_ok() {
            "acpp"
        } else {
            "/opt/adaptivecpp/bin/acpp"
        };
        let runtime_dirs = determine_runtime_dirs(acpp_cmd);
        return runtime_dirs.iter().any(|dir| runtime_lib_present(dir));
    }

    println!("cargo:warning=Building AdaptiveCpp SYCL backend");

    let acpp_cmd = if Command::new("acpp").arg("--version").output().is_ok() {
        "acpp"
    } else {
        "/opt/adaptivecpp/bin/acpp"
    };

    // Check runtime libraries
    let runtime_dirs = determine_runtime_dirs(acpp_cmd);
    let has_runtime = runtime_dirs.iter().any(|dir| runtime_lib_present(dir));
    if !has_runtime {
        println!("cargo:warning=AdaptiveCpp runtime libraries not found - AdaptiveCpp backend disabled");
        return false;
    }

    // Get output directory
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Compile with acpp as shared library
    let output = Command::new(acpp_cmd)
        .args(&[
            "-fPIC",
            "-O3",
            "-std=c++17",
            "-shared",
            "-DBACKEND_ADAPTIVE",  // Preprocessor flag for AdaptiveCpp-specific code
            "sycl_ffi.cpp",
            "-o",
        ])
        .arg("libsycl_ffi_adaptive.so")
        .args(&["-lacpp-rt", "-lacpp-common"])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            println!("cargo:warning=AdaptiveCpp SYCL backend compiled successfully");
            true
        },
        Ok(out) => {
            eprintln!("AdaptiveCpp compilation failed:");
            eprintln!("stdout: {}", String::from_utf8_lossy(&out.stdout));
            eprintln!("stderr: {}", String::from_utf8_lossy(&out.stderr));
            println!("cargo:warning=AdaptiveCpp backend build failed - continuing without it");
            false
        },
        Err(e) => {
            println!("cargo:warning=Failed to execute acpp: {} - continuing without AdaptiveCpp backend", e);
            false
        }
    }
}

// Link functions removed - we use dynamic loading via libloading instead

fn create_stub_library() {
    // Create a minimal stub library so Rust code compiles even without SYCL
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    std::fs::write(
        out_dir.join("sycl_stub.c"),
        r#"
// Stub implementations when AdaptiveCpp is not available
#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

typedef void* SyclDevice;
typedef void* SyclQueue;
typedef void* SyclBuffer;

int sycl_discover_devices(void* device_info, size_t* device_count) {
    *device_count = 0;
    return 1; // SYCL_ERROR_DEVICE_NOT_FOUND
}

int sycl_get_device(unsigned int device_index, SyclDevice* device) {
    return 1;
}

int sycl_create_queue(SyclDevice device, SyclQueue* queue) {
    return 1;
}

int sycl_create_buffer(SyclQueue queue, size_t size_bytes, SyclBuffer* buffer) {
    return 2; // SYCL_ERROR_OUT_OF_MEMORY
}

void sycl_release_device(SyclDevice device) {}
void sycl_release_queue(SyclQueue queue) {}
void sycl_release_buffer(SyclBuffer buffer) {}
"#,
    )
    .expect("Failed to create stub file");

    // Compile stub
    let cc_status = Command::new("cc")
        .arg("-c")
        .arg("-fPIC")
        .arg(out_dir.join("sycl_stub.c"))
        .arg("-o")
        .arg(out_dir.join("sycl_stub.o"))
        .status()
        .expect("Failed to compile stub");

    if !cc_status.success() {
        panic!("Failed to compile SYCL stub");
    }

    // Create static library
    let ar_status = Command::new("ar")
        .arg("rcs")
        .arg(out_dir.join("libsycl_ffi.a"))
        .arg(out_dir.join("sycl_stub.o"))
        .status()
        .expect("Failed to create stub library");

    if !ar_status.success() {
        panic!("Failed to create stub library");
    }

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=sycl_ffi");
}

fn locate_acpp_binary(acpp_cmd: &str) -> Option<PathBuf> {
    if acpp_cmd.contains('/') {
        let path = PathBuf::from(acpp_cmd);
        if path.exists() {
            Some(path)
        } else {
            None
        }
    } else {
        find_in_path(acpp_cmd)
    }
}

fn find_in_path(bin: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(bin);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn determine_runtime_dirs(acpp_cmd: &str) -> Vec<PathBuf> {
    let mut seen: HashSet<PathBuf> = HashSet::new();

    if let Some(dir) = env::var_os("ACPP_LIB_DIR") {
        seen.insert(PathBuf::from(dir));
    }
    if let Some(dir) = env::var_os("ADAPTIVECPP_LIB_DIR") {
        seen.insert(PathBuf::from(dir));
    }
    if let Some(home) = env::var_os("ADAPTIVECPP_HOME") {
        let root = PathBuf::from(home);
        seen.insert(root.join("lib"));
        seen.insert(root.join("lib64"));
    }

    if let Some(bin) = locate_acpp_binary(acpp_cmd) {
        if let Some(bin_dir) = bin.parent() {
            seen.insert(bin_dir.to_path_buf());
            if let Some(root) = bin_dir.parent() {
                seen.insert(root.join("lib"));
                seen.insert(root.join("lib64"));
            }
        }
    }

    let mut dirs: Vec<PathBuf> = Vec::new();
    for dir in seen {
        if dir.exists() {
            dirs.push(dir);
        }
    }

    if dirs.is_empty() {
        println!("cargo:warning=Could not locate AdaptiveCpp runtime directory via environment or binary location");
    }

    dirs
}

fn runtime_lib_present(dir: &Path) -> bool {
    const LIBS: &[&str] = &["acpp-rt", "acpp-common"];
    const EXTENSIONS: &[&str] = &["so", "dylib", "a"];

    LIBS.iter().all(|lib| {
        EXTENSIONS
            .iter()
            .map(|ext| dir.join(format!("lib{lib}.{ext}")))
            .any(|path| path.exists())
    })
}
