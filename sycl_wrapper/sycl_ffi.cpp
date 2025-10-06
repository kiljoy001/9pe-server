#include "sycl_ffi.hpp"
#include <sycl/sycl.hpp>
#include <vector>
#include <map>
#include <memory>
#include <cstring>
#include <iostream>

// Internal state management
static std::vector<sycl::device> g_devices;
static std::map<SyclDevice, sycl::device*> g_device_map;
static std::map<SyclQueue, std::shared_ptr<sycl::queue>> g_queue_map;
static std::map<SyclBuffer, std::shared_ptr<void>> g_buffer_map;
static std::map<SyclKernel, std::shared_ptr<sycl::kernel>> g_kernel_map;

// Helper: Convert SYCL exception to error code
static SyclError handle_exception(const std::exception& e) {
    std::cerr << "SYCL Error: " << e.what() << std::endl;
    return SYCL_ERROR_RUNTIME_ERROR;
}

// ============================================================================
// Device Management
// ============================================================================

SyclError sycl_discover_devices(SyclDeviceInfo* device_info, size_t* device_count) {
    try {
        g_devices.clear();

        // Get all devices (GPU, CPU, accelerators)
        auto all_devices = sycl::device::get_devices();

        size_t count = 0;
        for (const auto& device : all_devices) {
            if (count < *device_count) {
                // Fill device info
                auto& info = device_info[count];

                std::string name = device.get_info<sycl::info::device::name>();
                std::string vendor = device.get_info<sycl::info::device::vendor>();

                strncpy(info.name, name.c_str(), sizeof(info.name) - 1);
                strncpy(info.vendor, vendor.c_str(), sizeof(info.vendor) - 1);

                info.compute_units = device.get_info<sycl::info::device::max_compute_units>();
                info.global_memory_size = device.get_info<sycl::info::device::global_mem_size>();
                info.local_memory_size = device.get_info<sycl::info::device::local_mem_size>();
                info.max_work_group_size = device.get_info<sycl::info::device::max_work_group_size>();

                info.is_gpu = device.is_gpu();
                info.is_cpu = device.is_cpu();
                info.supports_fp64 = device.has(sycl::aspect::fp64);
                info.supports_fp16 = device.has(sycl::aspect::fp16);

                g_devices.push_back(device);
            }
            count++;
        }

        *device_count = count;
        return SYCL_SUCCESS;

    } catch (const std::exception& e) {
        return handle_exception(e);
    }
}

SyclError sycl_get_device(uint32_t device_index, SyclDevice* device) {
    try {
        if (device_index >= g_devices.size()) {
            return SYCL_ERROR_DEVICE_NOT_FOUND;
        }

        auto* dev_ptr = new sycl::device(g_devices[device_index]);
        *device = static_cast<SyclDevice>(dev_ptr);
        g_device_map[*device] = dev_ptr;

        return SYCL_SUCCESS;

    } catch (const std::exception& e) {
        return handle_exception(e);
    }
}

SyclError sycl_get_device_backend(SyclDevice device, SyclBackend* backend) {
    try {
        auto it = g_device_map.find(device);
        if (it == g_device_map.end()) {
            return SYCL_ERROR_DEVICE_NOT_FOUND;
        }

        auto platform = it->second->get_platform();
        std::string platform_name = platform.get_info<sycl::info::platform::name>();

        // Detect backend from platform name
        if (platform_name.find("CUDA") != std::string::npos) {
            *backend = SYCL_BACKEND_CUDA;
        } else if (platform_name.find("HIP") != std::string::npos ||
                   platform_name.find("AMD") != std::string::npos) {
            *backend = SYCL_BACKEND_HIP;
        } else if (platform_name.find("Level-Zero") != std::string::npos) {
            *backend = SYCL_BACKEND_LEVEL_ZERO;
        } else if (it->second->is_cpu()) {
            *backend = SYCL_BACKEND_CPU;
        } else {
            *backend = SYCL_BACKEND_OPENCL; // Default
        }

        return SYCL_SUCCESS;

    } catch (const std::exception& e) {
        return handle_exception(e);
    }
}

void sycl_release_device(SyclDevice device) {
    auto it = g_device_map.find(device);
    if (it != g_device_map.end()) {
        delete it->second;
        g_device_map.erase(it);
    }
}

// ============================================================================
// Queue Management
// ============================================================================

SyclError sycl_create_queue(SyclDevice device, SyclQueue* queue) {
    try {
        auto it = g_device_map.find(device);
        if (it == g_device_map.end()) {
            return SYCL_ERROR_DEVICE_NOT_FOUND;
        }

        // Create queue with profiling enabled
        auto q = std::make_shared<sycl::queue>(
            *(it->second),
            sycl::property::queue::in_order{}
        );

        *queue = static_cast<SyclQueue>(q.get());
        g_queue_map[*queue] = q;

        return SYCL_SUCCESS;

    } catch (const std::exception& e) {
        return handle_exception(e);
    }
}

SyclError sycl_queue_wait(SyclQueue queue) {
    try {
        auto it = g_queue_map.find(queue);
        if (it == g_queue_map.end()) {
            return SYCL_ERROR_RUNTIME_ERROR;
        }

        it->second->wait();
        return SYCL_SUCCESS;

    } catch (const std::exception& e) {
        return handle_exception(e);
    }
}

void sycl_release_queue(SyclQueue queue) {
    g_queue_map.erase(queue);
}

// ============================================================================
// Buffer Management
// ============================================================================

SyclError sycl_create_buffer(SyclQueue queue, size_t size_bytes, SyclBuffer* buffer) {
    try {
        auto it = g_queue_map.find(queue);
        if (it == g_queue_map.end()) {
            return SYCL_ERROR_RUNTIME_ERROR;
        }

        // Allocate USM shared memory (accessible from host and device)
        void* usm_ptr = sycl::malloc_shared(size_bytes, *(it->second));
        if (!usm_ptr) {
            return SYCL_ERROR_OUT_OF_MEMORY;
        }

        auto deleter = [q = it->second](void* ptr) {
            sycl::free(ptr, *q);
        };

        auto buffer_ptr = std::shared_ptr<void>(usm_ptr, deleter);
        *buffer = static_cast<SyclBuffer>(usm_ptr);
        g_buffer_map[*buffer] = buffer_ptr;

        return SYCL_SUCCESS;

    } catch (const std::exception& e) {
        return handle_exception(e);
    }
}

SyclError sycl_write_buffer(SyclQueue queue, SyclBuffer buffer,
                             const void* data, size_t size_bytes, size_t offset) {
    try {
        auto buf_it = g_buffer_map.find(buffer);
        if (buf_it == g_buffer_map.end()) {
            return SYCL_ERROR_INVALID_BUFFER;
        }

        auto q_it = g_queue_map.find(queue);
        if (q_it == g_queue_map.end()) {
            return SYCL_ERROR_RUNTIME_ERROR;
        }

        // USM shared memory - direct memcpy works
        void* dest = static_cast<char*>(buf_it->second.get()) + offset;
        std::memcpy(dest, data, size_bytes);

        // Ensure visibility
        q_it->second->wait();

        return SYCL_SUCCESS;

    } catch (const std::exception& e) {
        return handle_exception(e);
    }
}

SyclError sycl_read_buffer(SyclQueue queue, SyclBuffer buffer,
                            void* data, size_t size_bytes, size_t offset) {
    try {
        auto buf_it = g_buffer_map.find(buffer);
        if (buf_it == g_buffer_map.end()) {
            return SYCL_ERROR_INVALID_BUFFER;
        }

        auto q_it = g_queue_map.find(queue);
        if (q_it == g_queue_map.end()) {
            return SYCL_ERROR_RUNTIME_ERROR;
        }

        // Wait for pending operations
        q_it->second->wait();

        // USM shared memory - direct memcpy works
        const void* src = static_cast<const char*>(buf_it->second.get()) + offset;
        std::memcpy(data, src, size_bytes);

        return SYCL_SUCCESS;

    } catch (const std::exception& e) {
        return handle_exception(e);
    }
}

void sycl_release_buffer(SyclBuffer buffer) {
    g_buffer_map.erase(buffer);
}

// ============================================================================
// Standard AI Kernels
// ============================================================================

SyclError sycl_matmul_f32(SyclQueue queue,
                          SyclBuffer buffer_a, SyclBuffer buffer_b, SyclBuffer buffer_c,
                          uint32_t M, uint32_t N, uint32_t K) {
    try {
        auto q_it = g_queue_map.find(queue);
        if (q_it == g_queue_map.end()) {
            return SYCL_ERROR_RUNTIME_ERROR;
        }

        auto a_it = g_buffer_map.find(buffer_a);
        auto b_it = g_buffer_map.find(buffer_b);
        auto c_it = g_buffer_map.find(buffer_c);

        if (a_it == g_buffer_map.end() || b_it == g_buffer_map.end() || c_it == g_buffer_map.end()) {
            return SYCL_ERROR_INVALID_BUFFER;
        }

        float* A = static_cast<float*>(a_it->second.get());
        float* B = static_cast<float*>(b_it->second.get());
        float* C = static_cast<float*>(c_it->second.get());

        q_it->second->parallel_for(sycl::range<2>(M, N), [=](sycl::id<2> idx) {
            size_t row = idx[0];
            size_t col = idx[1];

            float sum = 0.0f;
            for (size_t k = 0; k < K; k++) {
                sum += A[row * K + k] * B[k * N + col];
            }
            C[row * N + col] = sum;
        }).wait();

        return SYCL_SUCCESS;

    } catch (const std::exception& e) {
        return handle_exception(e);
    }
}

SyclError sycl_vector_add_f32(SyclQueue queue,
                              SyclBuffer buffer_a, SyclBuffer buffer_b, SyclBuffer buffer_c,
                              size_t length) {
    try {
        auto q_it = g_queue_map.find(queue);
        if (q_it == g_queue_map.end()) {
            return SYCL_ERROR_RUNTIME_ERROR;
        }

        auto a_it = g_buffer_map.find(buffer_a);
        auto b_it = g_buffer_map.find(buffer_b);
        auto c_it = g_buffer_map.find(buffer_c);

        if (a_it == g_buffer_map.end() || b_it == g_buffer_map.end() || c_it == g_buffer_map.end()) {
            return SYCL_ERROR_INVALID_BUFFER;
        }

        float* a = static_cast<float*>(a_it->second.get());
        float* b = static_cast<float*>(b_it->second.get());
        float* c = static_cast<float*>(c_it->second.get());

        q_it->second->parallel_for(sycl::range<1>(length), [=](sycl::id<1> idx) {
            c[idx] = a[idx] + b[idx];
        }).wait();

        return SYCL_SUCCESS;

    } catch (const std::exception& e) {
        return handle_exception(e);
    }
}

SyclError sycl_relu_f32(SyclQueue queue,
                        SyclBuffer buffer_in, SyclBuffer buffer_out,
                        size_t length) {
    try {
        auto q_it = g_queue_map.find(queue);
        if (q_it == g_queue_map.end()) {
            return SYCL_ERROR_RUNTIME_ERROR;
        }

        auto in_it = g_buffer_map.find(buffer_in);
        auto out_it = g_buffer_map.find(buffer_out);

        if (in_it == g_buffer_map.end() || out_it == g_buffer_map.end()) {
            return SYCL_ERROR_INVALID_BUFFER;
        }

        float* input = static_cast<float*>(in_it->second.get());
        float* output = static_cast<float*>(out_it->second.get());

        q_it->second->parallel_for(sycl::range<1>(length), [=](sycl::id<1> idx) {
            output[idx] = sycl::max(0.0f, input[idx]);
        }).wait();

        return SYCL_SUCCESS;

    } catch (const std::exception& e) {
        return handle_exception(e);
    }
}

SyclError sycl_conv2d_f32(SyclQueue queue,
                          SyclBuffer input, SyclBuffer kernel, SyclBuffer output,
                          uint32_t batch, uint32_t in_channels, uint32_t out_channels,
                          uint32_t height, uint32_t width,
                          uint32_t kernel_h, uint32_t kernel_w,
                          uint32_t stride, uint32_t padding) {
    // Simplified 2D convolution - users should write optimized translators
    // This is a reference implementation
    try {
        auto q_it = g_queue_map.find(queue);
        if (q_it == g_queue_map.end()) {
            return SYCL_ERROR_RUNTIME_ERROR;
        }

        auto in_it = g_buffer_map.find(input);
        auto k_it = g_buffer_map.find(kernel);
        auto out_it = g_buffer_map.find(output);

        if (in_it == g_buffer_map.end() || k_it == g_buffer_map.end() || out_it == g_buffer_map.end()) {
            return SYCL_ERROR_INVALID_BUFFER;
        }

        float* in_data = static_cast<float*>(in_it->second.get());
        float* k_data = static_cast<float*>(k_it->second.get());
        float* out_data = static_cast<float*>(out_it->second.get());

        uint32_t out_h = (height + 2 * padding - kernel_h) / stride + 1;
        uint32_t out_w = (width + 2 * padding - kernel_w) / stride + 1;

        // Naive convolution - optimize via translators!
        q_it->second->parallel_for(sycl::range<4>(batch, out_channels, out_h, out_w),
            [=](sycl::id<4> idx) {
                uint32_t b = idx[0];
                uint32_t oc = idx[1];
                uint32_t oh = idx[2];
                uint32_t ow = idx[3];

                float sum = 0.0f;

                for (uint32_t ic = 0; ic < in_channels; ic++) {
                    for (uint32_t kh = 0; kh < kernel_h; kh++) {
                        for (uint32_t kw = 0; kw < kernel_w; kw++) {
                            int32_t ih = oh * stride + kh - padding;
                            int32_t iw = ow * stride + kw - padding;

                            if (ih >= 0 && ih < (int32_t)height && iw >= 0 && iw < (int32_t)width) {
                                size_t in_idx = b * in_channels * height * width +
                                               ic * height * width +
                                               ih * width + iw;
                                size_t k_idx = oc * in_channels * kernel_h * kernel_w +
                                              ic * kernel_h * kernel_w +
                                              kh * kernel_w + kw;
                                sum += in_data[in_idx] * k_data[k_idx];
                            }
                        }
                    }
                }

                size_t out_idx = b * out_channels * out_h * out_w +
                                oc * out_h * out_w +
                                oh * out_w + ow;
                out_data[out_idx] = sum;
            }).wait();

        return SYCL_SUCCESS;

    } catch (const std::exception& e) {
        return handle_exception(e);
    }
}

// ============================================================================
// Custom Kernel Compilation (Stub - AdaptiveCpp supports JIT)
// ============================================================================

SyclError sycl_compile_kernel(SyclDevice device, const char* source,
                               const char* kernel_name, SyclKernel* kernel) {
    // Custom kernel compilation not implemented yet
    // Users should write WASM translators for now
    return SYCL_ERROR_INVALID_KERNEL;
}

SyclError sycl_set_kernel_arg_buffer(SyclKernel kernel, uint32_t arg_index, SyclBuffer buffer) {
    return SYCL_ERROR_INVALID_KERNEL;
}

SyclError sycl_set_kernel_arg_scalar(SyclKernel kernel, uint32_t arg_index,
                                      const void* value, size_t size) {
    return SYCL_ERROR_INVALID_KERNEL;
}

SyclError sycl_execute_kernel(SyclQueue queue, SyclKernel kernel,
                               const size_t* global_work_size,
                               const size_t* local_work_size,
                               uint32_t work_dim) {
    return SYCL_ERROR_INVALID_KERNEL;
}

void sycl_release_kernel(SyclKernel kernel) {
    // No-op for now
}

// ============================================================================
// Profiling and Diagnostics
// ============================================================================

SyclError sycl_get_kernel_time(SyclEvent event, uint64_t* nanoseconds) {
    // Not implemented yet
    *nanoseconds = 0;
    return SYCL_SUCCESS;
}

SyclError sycl_get_device_utilization(SyclDevice device, float* utilization) {
    // Not available in SYCL standard - hardware-specific
    *utilization = 0.0f;
    return SYCL_SUCCESS;
}
