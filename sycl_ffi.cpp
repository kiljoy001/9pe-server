#include "sycl_ffi.hpp"
#include <sycl/sycl.hpp>
#include <map>
#include <vector>
#include <string>
#include <mutex>
#include <iostream>
#include <cstring>

// Handle implementations
struct SyclDeviceImpl {
    std::shared_ptr<sycl::device> device;
};

struct SyclQueueImpl {
    std::shared_ptr<sycl::queue> queue;
};

struct SyclBufferImpl {
    void* ptr;
    size_t size;
    std::shared_ptr<sycl::context> context; // Need context to free USM
};

struct SyclEventImpl {
    std::shared_ptr<sycl::event> event;
};

// Global maps with thread safety
static std::map<uint32_t, std::shared_ptr<sycl::device>> g_device_map;
static std::map<uint32_t, std::shared_ptr<sycl::queue>> g_queue_map;
static std::map<uint32_t, SyclBufferImpl*> g_buffer_map; // Use raw pointer map for buffers as they are manually managed
static std::map<uint32_t, std::shared_ptr<sycl::event>> g_event_map;

static std::mutex g_device_mutex;
static std::mutex g_queue_mutex;
static std::mutex g_buffer_mutex;
static std::mutex g_event_mutex;

static uint32_t g_device_counter = 0;
static uint32_t g_queue_counter = 0;
static uint32_t g_buffer_counter = 0;
static uint32_t g_event_counter = 0;

// Error message storage (thread-local for thread safety)
static thread_local std::string g_last_error;

static void set_error(const std::string& msg) {
    g_last_error = msg;
}

static void clear_error() {
    g_last_error.clear();
}

// Backend detection
typedef enum {
    FFI_BACKEND_OPENCL = 0,
    FFI_BACKEND_CUDA = 1,
    FFI_BACKEND_HIP = 2,
    FFI_BACKEND_LEVEL_ZERO = 3,
    FFI_BACKEND_CPU = 4
} SyclBackend;

static SyclBackend get_backend_from_device(const sycl::device& dev) {
    try {
        auto be = dev.get_backend();

        // Try to detect backend from name and platform
        std::string name = dev.get_info<sycl::info::device::name>();
        std::string platform_name = dev.get_platform().get_info<sycl::info::platform::name>();

        // Intel Level-Zero
        if (platform_name.find("Level-Zero") != std::string::npos ||
            name.find("Intel") != std::string::npos) {
            return FFI_BACKEND_LEVEL_ZERO;
        }

        // NVIDIA CUDA
        if (platform_name.find("CUDA") != std::string::npos ||
            name.find("NVIDIA") != std::string::npos) {
            return FFI_BACKEND_CUDA;
        }

        // AMD HIP
        if (platform_name.find("HIP") != std::string::npos ||
            name.find("AMD") != std::string::npos ||
            name.find("Radeon") != std::string::npos) {
            return FFI_BACKEND_HIP;
        }

        // CPU
        if (name.find("CPU") != std::string::npos ||
            platform_name.find("host") != std::string::npos) {
            return FFI_BACKEND_CPU;
        }

        // Default to OpenCL
        return FFI_BACKEND_OPENCL;

    } catch (...) {
        return FFI_BACKEND_OPENCL;
    }
}

extern "C" {

SyclError sycl_discover_devices() {
    std::lock_guard<std::mutex> lock(g_device_mutex);
    
    try {
        g_device_map.clear();
        g_device_counter = 0;
        
        auto platforms = sycl::platform::get_platforms();
        for (const auto& platform : platforms) {
            auto devices = platform.get_devices();
            for (const auto& device : devices) {
                g_device_map[g_device_counter++] = std::make_shared<sycl::device>(device);
            }
        }
        
        return SYCL_SUCCESS;
    } catch (const std::exception& e) {
        set_error(std::string("Device discovery failed: ") + e.what());
        return SYCL_ERROR_EXECUTION_FAILED;
    }
}

SyclError sycl_get_device_count(uint32_t* count) {
    if (!count) return SYCL_ERROR_INVALID_DEVICE;
    
    std::lock_guard<std::mutex> lock(g_device_mutex);
    *count = static_cast<uint32_t>(g_device_map.size());
    return SYCL_SUCCESS;
}

SyclError sycl_get_device(uint32_t index, SyclDevice* device) {
    if (!device) return SYCL_ERROR_INVALID_DEVICE;
    
    std::lock_guard<std::mutex> lock(g_device_mutex);
    auto it = g_device_map.find(index);
    if (it == g_device_map.end()) {
        return SYCL_ERROR_INVALID_DEVICE;
    }
    
    *device = new SyclDeviceImpl{it->second};
    return SYCL_SUCCESS;
}

SyclError sycl_get_device_info(SyclDevice device, char* name, size_t name_size, int* backend) {
    if (!device || !device->device) return SYCL_ERROR_INVALID_DEVICE;
    
    try {
        if (name && name_size > 0) {
            std::string device_name = device->device->get_info<sycl::info::device::name>();
            size_t copy_size = std::min(device_name.length(), name_size - 1);
            std::memcpy(name, device_name.c_str(), copy_size);
            name[copy_size] = '\0';
        }
        
        if (backend) {
            *backend = static_cast<int>(get_backend_from_device(*device->device));
        }
        
        return SYCL_SUCCESS;
    } catch (const std::exception& e) {
        set_error(std::string("Get device info failed: ") + e.what());
        return SYCL_ERROR_EXECUTION_FAILED;
    }
}

SyclError sycl_release_device(SyclDevice device) {
    if (!device) return SYCL_ERROR_INVALID_DEVICE;
    delete device;
    return SYCL_SUCCESS;
}

SyclError sycl_create_queue(SyclDevice device, SyclQueue* queue) {
    if (!device || !device->device || !queue) return SYCL_ERROR_INVALID_DEVICE;
    
    try {
        std::lock_guard<std::mutex> lock(g_queue_mutex);
        
        auto sycl_queue = std::make_shared<sycl::queue>(
            *device->device,
            sycl::property_list{sycl::property::queue::enable_profiling{}, sycl::property::queue::in_order{}}
        );
        
        uint32_t queue_id = g_queue_counter++;
        g_queue_map[queue_id] = sycl_queue;
        
        *queue = new SyclQueueImpl{sycl_queue};
        return SYCL_SUCCESS;
    } catch (const std::exception& e) {
        set_error(std::string("Create queue failed: ") + e.what());
        return SYCL_ERROR_EXECUTION_FAILED;
    }
}

SyclError sycl_queue_wait(SyclQueue queue) {
    if (!queue || !queue->queue) return SYCL_ERROR_INVALID_QUEUE;

    try {
        queue->queue->wait();
        return SYCL_SUCCESS;
    } catch (const std::exception& e) {
        set_error(std::string("Queue wait failed: ") + e.what());
        return SYCL_ERROR_EXECUTION_FAILED;
    }
}

SyclError sycl_release_queue(SyclQueue queue) {
    if (!queue) return SYCL_ERROR_INVALID_QUEUE;

    delete queue;
    return SYCL_SUCCESS;
}

SyclError sycl_create_buffer(SyclQueue queue, size_t size, SyclBuffer* buffer) {
    if (!queue || !queue->queue || !buffer) return SYCL_ERROR_INVALID_QUEUE;
    
    try {
        std::lock_guard<std::mutex> lock(g_buffer_mutex);
        
        // Use Unified Shared Memory (USM) - Device Allocation
        // This allocates memory directly on the device associated with the queue
        void* ptr = sycl::malloc_device(size, *queue->queue);
        
        if (!ptr) {
            return SYCL_ERROR_OUT_OF_MEMORY;
        }
        
        // Capture context for freeing later
        auto context = std::make_shared<sycl::context>(queue->queue->get_context());

        uint32_t buffer_id = g_buffer_counter++;
        
        *buffer = new SyclBufferImpl{ptr, size, context};
        g_buffer_map[buffer_id] = *buffer;
        
        return SYCL_SUCCESS;
    } catch (const std::exception& e) {
        set_error(std::string("Create buffer failed: ") + e.what());
        return SYCL_ERROR_OUT_OF_MEMORY;
    }
}

SyclError sycl_release_buffer(SyclBuffer buffer) {
    if (!buffer || !buffer->ptr) return SYCL_ERROR_INVALID_BUFFER;
    
    std::lock_guard<std::mutex> lock(g_buffer_mutex);
    
    try {
        // Free USM memory using the captured context
        sycl::free(buffer->ptr, *buffer->context);
        delete buffer;
        return SYCL_SUCCESS;
    } catch (const std::exception& e) {
        set_error(std::string("Release buffer failed: ") + e.what());
        return SYCL_ERROR_EXECUTION_FAILED;
    }
}

SyclError sycl_buffer_write(SyclQueue queue, SyclBuffer buffer, const void* data, size_t offset, size_t size) {
    if (!queue || !queue->queue) {
        set_error("Invalid queue handle");
        return SYCL_ERROR_INVALID_QUEUE;
    }
    if (!buffer || !buffer->ptr || !data) {
        set_error("Invalid buffer or data pointer");
        return SYCL_ERROR_INVALID_BUFFER;
    }
    if (offset + size > buffer->size) {
        set_error("Write would exceed buffer bounds");
        return SYCL_ERROR_INVALID_BUFFER;
    }

    try {
        // FIXED: Use the provided queue directly instead of reconstructing
        void* dest = static_cast<char*>(buffer->ptr) + offset;
        queue->queue->memcpy(dest, data, size).wait();

        clear_error();
        return SYCL_SUCCESS;
    } catch (const std::exception& e) {
        set_error(std::string("Buffer write failed: ") + e.what());
        return SYCL_ERROR_EXECUTION_FAILED;
    }
}

SyclError sycl_buffer_read(SyclQueue queue, SyclBuffer buffer, void* data, size_t offset, size_t size) {
    if (!queue || !queue->queue) {
        set_error("Invalid queue handle");
        return SYCL_ERROR_INVALID_QUEUE;
    }
    if (!buffer || !buffer->ptr || !data) {
        set_error("Invalid buffer or data pointer");
        return SYCL_ERROR_INVALID_BUFFER;
    }
    if (offset + size > buffer->size) {
        set_error("Read would exceed buffer bounds");
        return SYCL_ERROR_INVALID_BUFFER;
    }

    try {
        // FIXED: Use the provided queue directly
        void* src = static_cast<char*>(buffer->ptr) + offset;
        queue->queue->memcpy(data, src, size).wait();

        clear_error();
        return SYCL_SUCCESS;
    } catch (const std::exception& e) {
        set_error(std::string("Buffer read failed: ") + e.what());
        return SYCL_ERROR_EXECUTION_FAILED;
    }
}

SyclError sycl_release_event(SyclEvent event) {
    if (!event) return SYCL_ERROR_INVALID_BUFFER;
    
    std::lock_guard<std::mutex> lock(g_event_mutex);
    delete event;
    return SYCL_SUCCESS;
}

SyclError sycl_get_kernel_time(SyclEvent event, uint64_t* start_ns, uint64_t* end_ns) {
    if (!event || !event->event || !start_ns || !end_ns) return SYCL_ERROR_INVALID_BUFFER;
    
    try {
        auto sycl_event = event->event;
        // Wait for event to complete before querying profiling info
        sycl_event->wait();
        *start_ns = sycl_event->get_profiling_info<sycl::info::event_profiling::command_start>();
        *end_ns = sycl_event->get_profiling_info<sycl::info::event_profiling::command_end>();
        return SYCL_SUCCESS;
    } catch (const std::exception& e) {
        set_error(std::string("Get kernel time failed: ") + e.what());
        return SYCL_ERROR_EXECUTION_FAILED;
    }
}

SyclError sycl_matmul_f32_async(SyclQueue q, SyclBuffer A, SyclBuffer B, SyclBuffer C,
                                uint32_t M, uint32_t N, uint32_t K, SyclEvent* out_ev) {
    if (!q || !q->queue || !A || !B || !C || !out_ev) return SYCL_ERROR_INVALID_QUEUE;

    try {
        std::lock_guard<std::mutex> lock(g_event_mutex);

        float* a_ptr = static_cast<float*>(A->ptr);
        float* b_ptr = static_cast<float*>(B->ptr);
        float* c_ptr = static_cast<float*>(C->ptr);

        // OPTIMIZED: Tiled matmul with local memory for better cache utilization
        // Tile size tuned for Intel Arc (16x16 works well with XMX)
        constexpr int TILE_SIZE = 16;

        // Submit tiled matmul kernel
        sycl::event ev = q->queue->submit([&](sycl::handler& h) {
            // Allocate local memory tiles
            sycl::local_accessor<float, 2> tile_A(sycl::range<2>(TILE_SIZE, TILE_SIZE), h);
            sycl::local_accessor<float, 2> tile_B(sycl::range<2>(TILE_SIZE, TILE_SIZE), h);

            // Calculate global size with padding
            auto global_range = sycl::range<2>(
                ((M + TILE_SIZE - 1) / TILE_SIZE) * TILE_SIZE,
                ((N + TILE_SIZE - 1) / TILE_SIZE) * TILE_SIZE
            );
            auto local_range = sycl::range<2>(TILE_SIZE, TILE_SIZE);

            h.parallel_for(sycl::nd_range<2>(global_range, local_range),
                          [=](sycl::nd_item<2> item) {
                // Global indices
                int row = item.get_global_id(0);
                int col = item.get_global_id(1);

                // Local indices within work group
                int local_row = item.get_local_id(0);
                int local_col = item.get_local_id(1);

                float sum = 0.0f;

                // Tile across K dimension
                int num_tiles = (K + TILE_SIZE - 1) / TILE_SIZE;

                for (int t = 0; t < num_tiles; ++t) {
                    // Load tile of A into local memory
                    int a_col = t * TILE_SIZE + local_col;
                    if (row < M && a_col < K) {
                        tile_A[local_row][local_col] = a_ptr[row * K + a_col];
                    } else {
                        tile_A[local_row][local_col] = 0.0f;
                    }

                    // Load tile of B into local memory
                    int b_row = t * TILE_SIZE + local_row;
                    if (b_row < K && col < N) {
                        tile_B[local_row][local_col] = b_ptr[b_row * N + col];
                    } else {
                        tile_B[local_row][local_col] = 0.0f;
                    }

                    // Synchronize to ensure tiles are loaded
                    item.barrier(sycl::access::fence_space::local_space);

                    // Compute partial dot product using local memory
                    for (int k = 0; k < TILE_SIZE; ++k) {
                        sum += tile_A[local_row][k] * tile_B[k][local_col];
                    }

                    // Synchronize before loading next tile
                    item.barrier(sycl::access::fence_space::local_space);
                }

                // Write result
                if (row < M && col < N) {
                    c_ptr[row * N + col] = sum;
                }
            });
        });

        uint32_t event_id = g_event_counter++;
        auto event_ptr = std::make_shared<sycl::event>(ev);
        g_event_map[event_id] = event_ptr;

        *out_ev = new SyclEventImpl{event_ptr};
        clear_error();
        return SYCL_SUCCESS;
    } catch (const std::exception& e) {
        set_error(std::string("Matmul failed: ") + e.what());
        return SYCL_ERROR_EXECUTION_FAILED;
    }
}

SyclError sycl_ternary_matmul_async(SyclQueue q, SyclBuffer A, SyclBuffer B, SyclBuffer C,
                                    uint32_t M, uint32_t N, uint32_t K, SyclEvent* out_ev) {
    if (!q || !q->queue || !A || !B || !C || !out_ev) return SYCL_ERROR_INVALID_QUEUE;

    try {
        std::lock_guard<std::mutex> lock(g_event_mutex);

        // USM pointers - int8_t for ternary values (-1, 0, 1)
        int8_t* a_ptr = static_cast<int8_t*>(A->ptr);
        int8_t* b_ptr = static_cast<int8_t*>(B->ptr);
        float* c_ptr = static_cast<float*>(C->ptr);

        // OPTIMIZED: Tiled ternary matmul with vectorization
        // Ternary operations benefit from integer ALUs, not FPUs
        constexpr int TILE_SIZE = 16;

        sycl::event ev = q->queue->submit([&](sycl::handler& h) {
            sycl::local_accessor<int8_t, 2> tile_A(sycl::range<2>(TILE_SIZE, TILE_SIZE), h);
            sycl::local_accessor<int8_t, 2> tile_B(sycl::range<2>(TILE_SIZE, TILE_SIZE), h);

            auto global_range = sycl::range<2>(
                ((M + TILE_SIZE - 1) / TILE_SIZE) * TILE_SIZE,
                ((N + TILE_SIZE - 1) / TILE_SIZE) * TILE_SIZE
            );
            auto local_range = sycl::range<2>(TILE_SIZE, TILE_SIZE);

            h.parallel_for(sycl::nd_range<2>(global_range, local_range),
                          [=](sycl::nd_item<2> item) {
                int row = item.get_global_id(0);
                int col = item.get_global_id(1);
                int local_row = item.get_local_id(0);
                int local_col = item.get_local_id(1);

                // Use int32 accumulator for better performance with ternary
                int32_t sum = 0;

                int num_tiles = (K + TILE_SIZE - 1) / TILE_SIZE;

                for (int t = 0; t < num_tiles; ++t) {
                    // Load tiles
                    int a_col = t * TILE_SIZE + local_col;
                    if (row < M && a_col < K) {
                        tile_A[local_row][local_col] = a_ptr[row * K + a_col];
                    } else {
                        tile_A[local_row][local_col] = 0;
                    }

                    int b_row = t * TILE_SIZE + local_row;
                    if (b_row < K && col < N) {
                        tile_B[local_row][local_col] = b_ptr[b_row * N + col];
                    } else {
                        tile_B[local_row][local_col] = 0;
                    }

                    item.barrier(sycl::access::fence_space::local_space);

                    // Ternary multiply-add (values are -1, 0, 1)
                    // This uses integer ALUs which are much faster than FP
                    #pragma unroll
                    for (int k = 0; k < TILE_SIZE; ++k) {
                        sum += static_cast<int32_t>(tile_A[local_row][k]) *
                               static_cast<int32_t>(tile_B[k][local_col]);
                    }

                    item.barrier(sycl::access::fence_space::local_space);
                }

                // Convert to float for output
                if (row < M && col < N) {
                    c_ptr[row * N + col] = static_cast<float>(sum);
                }
            });
        });

        uint32_t event_id = g_event_counter++;
        auto event_ptr = std::make_shared<sycl::event>(ev);
        g_event_map[event_id] = event_ptr;

        *out_ev = new SyclEventImpl{event_ptr};
        clear_error();
        return SYCL_SUCCESS;
    } catch (const std::exception& e) {
        set_error(std::string("Ternary matmul failed: ") + e.what());
        return SYCL_ERROR_EXECUTION_FAILED;
    }
}

// Error handling functions
const char* sycl_get_last_error() {
    return g_last_error.empty() ? nullptr : g_last_error.c_str();
}

void sycl_clear_error() {
    clear_error();
}

// Handle management and cleanup
SyclError sycl_cleanup_unused_handles() {
    // For now, this is a placeholder - in a full implementation,
    // we would track handle reference counts and clean up unused ones
    // Currently all handles are explicitly managed by the user
    return SYCL_SUCCESS;
}

SyclError sycl_get_active_handle_count(uint32_t* devices, uint32_t* queues,
                                       uint32_t* buffers, uint32_t* events) {
    if (devices) {
        std::lock_guard<std::mutex> lock(g_device_mutex);
        *devices = static_cast<uint32_t>(g_device_map.size());
    }
    if (queues) {
        std::lock_guard<std::mutex> lock(g_queue_mutex);
        *queues = static_cast<uint32_t>(g_queue_map.size());
    }
    if (buffers) {
        std::lock_guard<std::mutex> lock(g_buffer_mutex);
        *buffers = static_cast<uint32_t>(g_buffer_map.size());
    }
    if (events) {
        std::lock_guard<std::mutex> lock(g_event_mutex);
        *events = static_cast<uint32_t>(g_event_map.size());
    }
    return SYCL_SUCCESS;
}

// Multi-device functions are in sycl_recursive_discovery.cpp

} // extern "C"