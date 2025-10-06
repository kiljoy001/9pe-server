// Build script for compiling SYCL C++ wrapper with AdaptiveCpp
use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=sycl_wrapper/sycl_ffi.cpp");
    println!("cargo:rerun-if-changed=sycl_wrapper/sycl_ffi.hpp");

    // Check if AdaptiveCpp (acpp) compiler is available
    let acpp_available = Command::new("acpp")
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
    let output = Command::new("acpp")
        .arg("-c")
        .arg("-fPIC") // Position independent code for shared library
        .arg("-O3")   // Optimize for performance
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

    // Link AdaptiveCpp runtime (depends on selected backends)
    // The runtime will automatically select the appropriate backend:
    // - CUDA for NVIDIA
    // - HIP for AMD
    // - Level-Zero for Intel
    // - OpenCL fallback for others
    println!("cargo:rustc-link-lib=dylib=acpp");

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
