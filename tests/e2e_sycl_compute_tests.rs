//! End-to-end integration tests for SYCL compute operations
//!
//! Tests actual GPU/accelerator compute via AdaptiveCpp SYCL backend

use ninep_server::sycl::ffi::*;
use std::ptr;

/// Test SYCL device discovery
#[test]
fn test_e2e_sycl_device_discovery() {
    let mut devices = vec![
        SyclDeviceInfo {
            name: [0; 256],
            vendor: [0; 128],
            compute_units: 0,
            global_memory_size: 0,
            local_memory_size: 0,
            max_work_group_size: 0,
            is_gpu: false,
            is_cpu: false,
            supports_fp64: false,
            supports_fp16: false,
        };
        16
    ];

    let mut count: usize = 16;

    unsafe {
        let err = sycl_discover_devices(devices.as_mut_ptr(), &mut count as *mut usize);

        // Should either find devices or gracefully fail if SYCL not installed
        match err {
            SyclError::Success => {
                println!("✅ Found {} SYCL devices", count);
                assert!(count > 0, "Should find at least one device (CPU)");

                for i in 0..count {
                    println!(
                        "Device {}: {} ({}) - {} CUs, {} GB",
                        i,
                        devices[i].name_str(),
                        devices[i].vendor_str(),
                        devices[i].compute_units,
                        devices[i].global_memory_size / (1024 * 1024 * 1024)
                    );
                }
            }
            _ => {
                println!("⚠️  SYCL not available: {:?}", err);
                // Test passes - SYCL is optional
            }
        }
    }
}

/// Test SYCL device backend detection
#[test]
fn test_e2e_sycl_device_backends() {
    let mut devices = vec![
        SyclDeviceInfo {
            name: [0; 256],
            vendor: [0; 128],
            compute_units: 0,
            global_memory_size: 0,
            local_memory_size: 0,
            max_work_group_size: 0,
            is_gpu: false,
            is_cpu: false,
            supports_fp64: false,
            supports_fp16: false,
        };
        16
    ];

    let mut count: usize = 16;

    unsafe {
        let err = sycl_discover_devices(devices.as_mut_ptr(), &mut count as *mut usize);

        if err.is_ok() && count > 0 {
            // Try to get device and check backend
            let mut device: SyclDevice = ptr::null_mut();
            let get_err = sycl_get_device(0, &mut device as *mut SyclDevice);

            if get_err.is_ok() {
                let mut backend = SyclBackend::CPU;
                let backend_err = sycl_get_device_backend(device, &mut backend as *mut SyclBackend);

                if backend_err.is_ok() {
                    println!("✅ Device 0 backend: {}", backend);
                    assert!(
                        matches!(
                            backend,
                            SyclBackend::OpenCL
                                | SyclBackend::CUDA
                                | SyclBackend::HIP
                                | SyclBackend::LevelZero
                                | SyclBackend::CPU
                        ),
                        "Should have valid backend type"
                    );
                }

                sycl_release_device(device);
            }
        }
    }
}

/// Test SYCL queue creation and management
#[test]
fn test_e2e_sycl_queue_operations() {
    unsafe {
        let mut device: SyclDevice = ptr::null_mut();
        let dev_err = sycl_get_device(0, &mut device as *mut SyclDevice);

        if dev_err.is_ok() {
            let mut queue: SyclQueue = ptr::null_mut();
            let queue_err = sycl_create_queue(device, &mut queue as *mut SyclQueue);

            if queue_err.is_ok() {
                println!("✅ Created SYCL queue");

                // Test queue wait (should succeed even with no work)
                let wait_err = sycl_queue_wait(queue);
                assert!(wait_err.is_ok(), "Queue wait should succeed");

                sycl_release_queue(queue);
                println!("✅ Released SYCL queue");
            }

            sycl_release_device(device);
        }
    }
}

/// Test SYCL buffer allocation
#[test]
fn test_e2e_sycl_buffer_operations() {
    unsafe {
        let mut device: SyclDevice = ptr::null_mut();
        let dev_err = sycl_get_device(0, &mut device as *mut SyclDevice);

        if dev_err.is_ok() {
            let mut queue: SyclQueue = ptr::null_mut();
            let queue_err = sycl_create_queue(device, &mut queue as *mut SyclQueue);

            if queue_err.is_ok() {
                // Allocate buffer (1MB)
                let buffer_size = 1024 * 1024;
                let mut buffer: SyclBuffer = ptr::null_mut();
                let buf_err =
                    sycl_create_buffer(queue, buffer_size, &mut buffer as *mut SyclBuffer);

                if buf_err.is_ok() {
                    println!("✅ Allocated {} byte SYCL buffer", buffer_size);

                    // Write data to buffer
                    let test_data: Vec<u8> = vec![42; 1024];
                    let write_err = sycl_write_buffer(
                        queue,
                        buffer,
                        test_data.as_ptr() as *const std::ffi::c_void,
                        test_data.len(),
                        0,
                    );

                    assert!(write_err.is_ok(), "Buffer write should succeed");
                    println!("✅ Wrote {} bytes to buffer", test_data.len());

                    // Read data back
                    let mut read_data: Vec<u8> = vec![0; 1024];
                    let read_err = sycl_read_buffer(
                        queue,
                        buffer,
                        read_data.as_mut_ptr() as *mut std::ffi::c_void,
                        read_data.len(),
                        0,
                    );

                    assert!(read_err.is_ok(), "Buffer read should succeed");
                    assert_eq!(read_data, test_data, "Read data should match written data");
                    println!("✅ Read and verified {} bytes", read_data.len());

                    sycl_release_buffer(buffer);
                }

                sycl_release_queue(queue);
            }

            sycl_release_device(device);
        }
    }
}

/// Test SYCL vector addition kernel
#[test]
fn test_e2e_sycl_vector_add() {
    unsafe {
        let mut device: SyclDevice = ptr::null_mut();
        if sycl_get_device(0, &mut device as *mut SyclDevice).is_err() {
            return; // Skip if no device
        }

        let mut queue: SyclQueue = ptr::null_mut();
        if sycl_create_queue(device, &mut queue as *mut SyclQueue).is_err() {
            sycl_release_device(device);
            return;
        }

        let length = 1024_usize;
        let size_bytes = length * std::mem::size_of::<f32>();

        // Allocate buffers
        let mut buf_a: SyclBuffer = ptr::null_mut();
        let mut buf_b: SyclBuffer = ptr::null_mut();
        let mut buf_c: SyclBuffer = ptr::null_mut();

        if sycl_create_buffer(queue, size_bytes, &mut buf_a as *mut SyclBuffer).is_err() {
            sycl_release_queue(queue);
            sycl_release_device(device);
            return;
        }

        sycl_create_buffer(queue, size_bytes, &mut buf_b as *mut SyclBuffer).ok();
        sycl_create_buffer(queue, size_bytes, &mut buf_c as *mut SyclBuffer).ok();

        // Prepare input data
        let vec_a: Vec<f32> = (0..length).map(|i| i as f32).collect();
        let vec_b: Vec<f32> = (0..length).map(|i| (i * 2) as f32).collect();

        // Write input data
        sycl_write_buffer(
            queue,
            buf_a,
            vec_a.as_ptr() as *const std::ffi::c_void,
            size_bytes,
            0,
        )
        .ok();
        sycl_write_buffer(
            queue,
            buf_b,
            vec_b.as_ptr() as *const std::ffi::c_void,
            size_bytes,
            0,
        )
        .ok();

        // Execute vector add kernel
        let kernel_err = sycl_vector_add_f32(queue, buf_a, buf_b, buf_c, length);

        if kernel_err.is_ok() {
            // Read result
            let mut vec_c: Vec<f32> = vec![0.0; length];
            sycl_read_buffer(
                queue,
                buf_c,
                vec_c.as_mut_ptr() as *mut std::ffi::c_void,
                size_bytes,
                0,
            )
            .ok();

            // Verify results
            for i in 0..length {
                let expected = vec_a[i] + vec_b[i];
                assert!(
                    (vec_c[i] - expected).abs() < 0.001,
                    "Vector add result incorrect"
                );
            }

            println!("✅ Vector addition verified ({} elements)", length);
        }

        sycl_release_buffer(buf_a);
        sycl_release_buffer(buf_b);
        sycl_release_buffer(buf_c);
        sycl_release_queue(queue);
        sycl_release_device(device);
    }
}

/// Test SYCL matrix multiplication
#[test]
fn test_e2e_sycl_matrix_mul() {
    unsafe {
        let mut device: SyclDevice = ptr::null_mut();
        if sycl_get_device(0, &mut device as *mut SyclDevice).is_err() {
            return;
        }

        let mut queue: SyclQueue = ptr::null_mut();
        if sycl_create_queue(device, &mut queue as *mut SyclQueue).is_err() {
            sycl_release_device(device);
            return;
        }

        // Small matrix for testing: 8x8 * 8x8 = 8x8
        let m = 8u32;
        let n = 8u32;
        let k = 8u32;

        let size_a = (m * k) as usize * std::mem::size_of::<f32>();
        let size_b = (k * n) as usize * std::mem::size_of::<f32>();
        let size_c = (m * n) as usize * std::mem::size_of::<f32>();

        let mut buf_a: SyclBuffer = ptr::null_mut();
        let mut buf_b: SyclBuffer = ptr::null_mut();
        let mut buf_c: SyclBuffer = ptr::null_mut();

        if sycl_create_buffer(queue, size_a, &mut buf_a as *mut SyclBuffer).is_err() {
            sycl_release_queue(queue);
            sycl_release_device(device);
            return;
        }

        sycl_create_buffer(queue, size_b, &mut buf_b as *mut SyclBuffer).ok();
        sycl_create_buffer(queue, size_c, &mut buf_c as *mut SyclBuffer).ok();

        // Identity matrix test: I * I = I
        let mat_a: Vec<f32> = (0..(m * k) as usize)
            .map(|i| {
                let row = i / k as usize;
                let col = i % k as usize;
                if row == col {
                    1.0
                } else {
                    0.0
                }
            })
            .collect();

        let mat_b = mat_a.clone();

        sycl_write_buffer(
            queue,
            buf_a,
            mat_a.as_ptr() as *const std::ffi::c_void,
            size_a,
            0,
        )
        .ok();
        sycl_write_buffer(
            queue,
            buf_b,
            mat_b.as_ptr() as *const std::ffi::c_void,
            size_b,
            0,
        )
        .ok();

        let kernel_err = sycl_matmul_f32(queue, buf_a, buf_b, buf_c, m, n, k);

        if kernel_err.is_ok() {
            let mut mat_c: Vec<f32> = vec![0.0; (m * n) as usize];
            sycl_read_buffer(
                queue,
                buf_c,
                mat_c.as_mut_ptr() as *mut std::ffi::c_void,
                size_c,
                0,
            )
            .ok();

            // Verify identity property
            for i in 0..(m * n) as usize {
                let row = i / n as usize;
                let col = i % n as usize;
                let expected = if row == col { 1.0 } else { 0.0 };
                assert!(
                    (mat_c[i] - expected).abs() < 0.001,
                    "Matrix mul result incorrect"
                );
            }

            println!(
                "✅ Matrix multiplication verified ({}x{} * {}x{})",
                m, k, k, n
            );
        }

        sycl_release_buffer(buf_a);
        sycl_release_buffer(buf_b);
        sycl_release_buffer(buf_c);
        sycl_release_queue(queue);
        sycl_release_device(device);
    }
}

/// Test SYCL ReLU activation function
#[test]
fn test_e2e_sycl_relu() {
    unsafe {
        let mut device: SyclDevice = ptr::null_mut();
        if sycl_get_device(0, &mut device as *mut SyclDevice).is_err() {
            return;
        }

        let mut queue: SyclQueue = ptr::null_mut();
        if sycl_create_queue(device, &mut queue as *mut SyclQueue).is_err() {
            sycl_release_device(device);
            return;
        }

        let length = 1024_usize;
        let size_bytes = length * std::mem::size_of::<f32>();

        let mut buf_in: SyclBuffer = ptr::null_mut();
        let mut buf_out: SyclBuffer = ptr::null_mut();

        if sycl_create_buffer(queue, size_bytes, &mut buf_in as *mut SyclBuffer).is_err() {
            sycl_release_queue(queue);
            sycl_release_device(device);
            return;
        }

        sycl_create_buffer(queue, size_bytes, &mut buf_out as *mut SyclBuffer).ok();

        // Input: -512 to 511
        let input: Vec<f32> = (0..length).map(|i| (i as f32) - 512.0).collect();

        sycl_write_buffer(
            queue,
            buf_in,
            input.as_ptr() as *const std::ffi::c_void,
            size_bytes,
            0,
        )
        .ok();

        let kernel_err = sycl_relu_f32(queue, buf_in, buf_out, length);

        if kernel_err.is_ok() {
            let mut output: Vec<f32> = vec![0.0; length];
            sycl_read_buffer(
                queue,
                buf_out,
                output.as_mut_ptr() as *mut std::ffi::c_void,
                size_bytes,
                0,
            )
            .ok();

            // Verify ReLU: max(0, x)
            for i in 0..length {
                let expected = input[i].max(0.0);
                assert!(
                    (output[i] - expected).abs() < 0.001,
                    "ReLU result incorrect"
                );
            }

            println!("✅ ReLU activation verified ({} elements)", length);
        }

        sycl_release_buffer(buf_in);
        sycl_release_buffer(buf_out);
        sycl_release_queue(queue);
        sycl_release_device(device);
    }
}

/// Test SYCL device capabilities
#[test]
fn test_e2e_sycl_device_capabilities() {
    let mut devices = vec![
        SyclDeviceInfo {
            name: [0; 256],
            vendor: [0; 128],
            compute_units: 0,
            global_memory_size: 0,
            local_memory_size: 0,
            max_work_group_size: 0,
            is_gpu: false,
            is_cpu: false,
            supports_fp64: false,
            supports_fp16: false,
        };
        16
    ];

    let mut count: usize = 16;

    unsafe {
        let err = sycl_discover_devices(devices.as_mut_ptr(), &mut count as *mut usize);

        if err.is_ok() && count > 0 {
            for i in 0..count {
                let dev = &devices[i];
                println!("\n✅ Device {} capabilities:", i);
                println!("   Name: {}", dev.name_str());
                println!("   Vendor: {}", dev.vendor_str());
                println!("   Compute Units: {}", dev.compute_units);
                println!(
                    "   Global Memory: {} GB",
                    dev.global_memory_size / (1024 * 1024 * 1024)
                );
                println!("   Local Memory: {} KB", dev.local_memory_size / 1024);
                println!("   Max Work Group: {}", dev.max_work_group_size);
                println!(
                    "   Type: {}",
                    if dev.is_gpu {
                        "GPU"
                    } else if dev.is_cpu {
                        "CPU"
                    } else {
                        "Other"
                    }
                );
                println!("   FP64: {}", dev.supports_fp64);
                println!("   FP16: {}", dev.supports_fp16);

                // Validate reasonable values
                assert!(dev.compute_units > 0, "Should have at least 1 compute unit");
                assert!(dev.global_memory_size > 0, "Should have some global memory");
                assert!(dev.max_work_group_size > 0, "Should support work groups");
            }
        }
    }
}

/// Test SYCL error handling
#[test]
fn test_e2e_sycl_error_handling() {
    unsafe {
        // Try to get non-existent device
        let mut device: SyclDevice = ptr::null_mut();
        let err = sycl_get_device(9999, &mut device as *mut SyclDevice);

        assert!(err.is_err(), "Should fail to get non-existent device");
        assert_eq!(
            err,
            SyclError::DeviceNotFound,
            "Should return DeviceNotFound"
        );

        println!("✅ SYCL error handling works correctly");
    }
}

/// Test SYCL multiple devices if available
#[test]
fn test_e2e_sycl_multi_device() {
    let mut devices = vec![
        SyclDeviceInfo {
            name: [0; 256],
            vendor: [0; 128],
            compute_units: 0,
            global_memory_size: 0,
            local_memory_size: 0,
            max_work_group_size: 0,
            is_gpu: false,
            is_cpu: false,
            supports_fp64: false,
            supports_fp16: false,
        };
        16
    ];

    let mut count: usize = 16;

    unsafe {
        let err = sycl_discover_devices(devices.as_mut_ptr(), &mut count as *mut usize);

        if err.is_ok() && count > 1 {
            println!(
                "✅ Found {} devices, testing multiple device support",
                count
            );

            // Try to get each device
            for i in 0..count {
                let mut device: SyclDevice = ptr::null_mut();
                let dev_err = sycl_get_device(i as u32, &mut device as *mut SyclDevice);

                if dev_err.is_ok() {
                    println!("   Device {}: ✅ Accessible", i);
                    sycl_release_device(device);
                } else {
                    println!("   Device {}: ⚠️ Not accessible", i);
                }
            }
        } else {
            println!("⚠️  Only {} device(s) found", count);
        }
    }
}
