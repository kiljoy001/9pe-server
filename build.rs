// Build script for compiling SYCL C++ wrapper with AdaptiveCpp
use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=sycl_wrapper/sycl_ffi.cpp");
    println!("cargo:rerun-if-changed=sycl_wrapper/sycl_ffi.hpp");

    // Check if AdaptiveCpp (acpp) compiler is available
    let acpp_available = Command::new("acpp").arg("--version").output().is_ok()
        || Command::new("/opt/adaptivecpp/bin/acpp")
            .arg("--version")
            .output()
            .is_ok();

    if !acpp_available {
        println!("cargo:warning=AdaptiveCpp (acpp) not found - SYCL support will be disabled");
        println!("cargo:warning=Install AdaptiveCpp: https://github.com/AdaptiveCpp/AdaptiveCpp");
        println!("cargo:warning=Or use package manager: apt install adaptivecpp (Ubuntu 24.04+)");

        // Create a stub library so compilation doesn't fail
        create_stub_library();
        return;
    }

    println!("cargo:warning=Building SYCL wrapper with AdaptiveCpp");

    // Get output directory
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let sycl_wrapper_dir = PathBuf::from("sycl_wrapper");

    // Compile SYCL C++ code with acpp
    let acpp_cmd = if Command::new("acpp").arg("--version").output().is_ok() {
        "acpp"
    } else {
        "/opt/adaptivecpp/bin/acpp"
    };

    let runtime_dirs = determine_runtime_dirs(acpp_cmd);
    let has_runtime = runtime_dirs.iter().any(|dir| runtime_lib_present(dir));
    if !has_runtime {
        println!("cargo:warning=AdaptiveCpp runtime libraries not found; falling back to stub SYCL implementation");
        create_stub_library();
        return;
    }

    let output = Command::new(acpp_cmd)
        .arg("-c")
        .arg("-fPIC") // Position independent code for shared library
        .arg("-O3") // Optimize for performance
        .arg("-std=c++17")
        .arg(sycl_wrapper_dir.join("sycl_ffi.cpp"))
        .arg("-o")
        .arg(out_dir.join("sycl_ffi.o"))
        .output()
        .expect("Failed to compile SYCL wrapper with acpp");

    if !output.status.success() {
        eprintln!("acpp compilation failed:");
        eprintln!("stdout: {}", String::from_utf8_lossy(&output.stdout));
        eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));
        panic!("Failed to compile SYCL wrapper");
    }

    // Create static library from object file
    let ar_status = Command::new("ar")
        .arg("rcs")
        .arg(out_dir.join("libsycl_ffi.a"))
        .arg(out_dir.join("sycl_ffi.o"))
        .status()
        .expect("Failed to create static library");

    if !ar_status.success() {
        panic!("Failed to create static library");
    }

    // Link the library
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=sycl_ffi");

    // Link C++ standard library
    println!("cargo:rustc-link-lib=stdc++");

    for dir in &runtime_dirs {
        println!("cargo:rustc-link-search=native={}", dir.display());
        println!("cargo:rustc-link-arg=-Wl,-rpath={}", dir.display());
    }

    // Link AdaptiveCpp runtime (depends on selected backends)
    // The runtime will automatically select the appropriate backend:
    // - CUDA for NVIDIA
    // - HIP for AMD
    // - Level-Zero for Intel
    // - OpenCL fallback for others
    println!("cargo:rustc-link-lib=dylib=acpp-rt");
    println!("cargo:rustc-link-lib=dylib=acpp-common");

    println!("cargo:warning=SYCL wrapper compiled successfully with AdaptiveCpp");
}

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
