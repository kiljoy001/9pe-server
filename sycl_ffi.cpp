#include "sycl_ffi.hpp"
#include <sycl/sycl.hpp>
#include <map>
#include <vector>
#include <string>
#include <mutex>
#include <iostream>

// Handle implementations
struct SyclDeviceImpl {
    std::shared_ptr<sycl::device> device;
};

struct SyclQueueImpl {
    std::shared_ptr<sycl::queue> queue;
};

struct SyclBufferImpl {
    std::shared_ptr<sycl::buffer<uint8_t, 1>> buffer;
    size_t size;
};

struct SyclEventImpl {
    std::shared_ptr<sycl::event> event;
};

// Global maps with thread safety
static std::map<uint32_t, std::shared_ptr<sycl::device>> g_device_map;
static std::map<uint32_t, std::shared_ptr<sycl::queue>> g_queue_map;
static std::map<uint32_t, std::shared_ptr<sycl::buffer<uint8_t, 1>>> g_buffer_map;
static std::map<uint32_t, std::shared_ptr<sycl::event>> g_event_map;

static std::mutex g_device_mutex;
static std::mutex g_queue_mutex;
static std::mutex g_buffer_mutex;
static std::mutex g_event_mutex;

static uint32_t g_device_counter = 0;
static uint32_t g_queue_counter = 0;
static uint32_t g_buffer_counter = 0;
static uint32_t g_event_counter = 0;

// Backend detection
typedef enum {
    SYCL_BACKEND_OPENCL = 0,
    SYCL_BACKEND_CUDA = 1,
    SYCL_BACKEND_HIP = 2,
    SYCL_BACKEND_LEVEL_ZERO = 3,
    SYCL_BACKEND_CPU = 4
} SyclBackend;

static SyclBackend get_backend_from_device(const sycl::device& dev) {
    auto be = dev.get_backend();
    switch (be) {
        case sycl::backend::ext_oneapi_cuda:    return SYCL_BACKEND_CUDA;
        case sycl::backend::ext_oneapi_hip:     return SYCL_BACKEND_HIP;
        case sycl::backend::ext_oneapi_level_zero: return SYCL_BACKEND_LEVEL_ZERO;
        case sycl::backend::opencl:             return SYCL_BACKEND_OPENCL;
        case sycl::backend::host:               return SYCL_BACKEND_CPU;
        default:                                return SYCL_BACKEND_OPENCL;
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
        std::cerr << "Device discovery failed: " << e.what() << std::endl;
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
        std::cerr << "Get device info failed: " << e.what() << std::endl;
        return SYCL_ERROR_EXECUTION_FAILED;
    }
}

SyclError sycl_create_queue(SyclDevice device, SyclQueue* queue) {
    if (!device || !device->device || !queue) return SYCL_ERROR_INVALID_DEVICE;
    
    try {
        std::lock_guard<std::mutex> lock(g_queue_mutex);
        
        auto sycl_queue = std::make_shared<sycl::queue>(
            *device->device,
            sycl::property_list{sycl::property::queue::enable_profiling{}}
        );
        
        uint32_t queue_id = g_queue_counter++;
        g_queue_map[queue_id] = sycl_queue;
        
        *queue = new SyclQueueImpl{sycl_queue};
        return SYCL_SUCCESS;
    } catch (const std::exception& e) {
        std::cerr << "Create queue failed: " << e.what() << std::endl;
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
        
        auto sycl_buffer = std::make_shared<sycl::buffer<uint8_t, 1>>(
            sycl::range<1>(size),
            sycl::property_list{sycl::property::buffer::use_host_ptr{}}
        );
        
        uint32_t buffer_id = g_buffer_counter++;
        g_buffer_map[buffer_id] = sycl_buffer;
        
        *buffer = new SyclBufferImpl{sycl_buffer, size};
        return SYCL_SUCCESS;
    } catch (const std::exception& e) {
        std::cerr << "Create buffer failed: " << e.what() << std::endl;
        return SYCL_ERROR_OUT_OF_MEMORY;
    }
}

SyclError sycl_release_buffer(SyclBuffer buffer) {
    if (!buffer) return SYCL_ERROR_INVALID_BUFFER;
    
    std::lock_guard<std::mutex> lock(g_buffer_mutex);
    // Remove from map would go here in a full implementation
    delete buffer;
    return SYCL_SUCCESS;
}

SyclError sycl_buffer_write(SyclBuffer buffer, const void* data, size_t offset, size_t size) {
    if (!buffer || !buffer->buffer || !data) return SYCL_ERROR_INVALID_BUFFER;
    if (offset + size > buffer->size) return SYCL_ERROR_INVALID_BUFFER;
    
    try {
        auto host_acc = buffer->buffer->get_host_access(sycl::write_only);
        std::memcpy(host_acc.get_pointer() + offset, data, size);
        return SYCL_SUCCESS;
    } catch (const std::exception& e) {
        std::cerr << "Buffer write failed: " << e.what() << std::endl;
        return SYCL_ERROR_EXECUTION_FAILED;
    }
}

SyclError sycl_buffer_read(SyclBuffer buffer, void* data, size_t offset, size_t size) {
    if (!buffer || !buffer->buffer || !data) return SYCL_ERROR_INVALID_BUFFER;
    if (offset + size > buffer->size) return SYCL_ERROR_INVALID_BUFFER;
    
    try {
        auto host_acc = buffer->buffer->get_host_access(sycl::read_only);
        std::memcpy(data, host_acc.get_pointer() + offset, size);
        return SYCL_SUCCESS;
    } catch (const std::exception& e) {
        std::cerr << "Buffer read failed: " << e.what() << std::endl;
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
        *start_ns = sycl_event->get_profiling_info<sycl::info::event_profiling::command_start>();
        *end_ns = sycl_event->get_profiling_info<sycl::info::event_profiling::command_end>();
        return SYCL_SUCCESS;
    } catch (const std::exception& e) {
        std::cerr << "Get kernel time failed: " << e.what() << std::endl;
        return SYCL_ERROR_EXECUTION_FAILED;
    }
}

SyclError sycl_matmul_f32_async(SyclQueue q, SyclBuffer A, SyclBuffer B, SyclBuffer C,
                                uint32_t M, uint32_t N, uint32_t K, SyclEvent* out_ev) {
    if (!q || !q->queue || !A || !B || !C || !out_ev) return SYCL_ERROR_INVALID_QUEUE;
    
    try {
        std::lock_guard<std::mutex> lock(g_event_mutex);
        
        auto buf_a = reinterpret_cast<sycl::buffer<float, 1>*>(A->buffer.get());
        auto buf_b = reinterpret_cast<sycl::buffer<float, 1>*>(B->buffer.get());
        auto buf_c = reinterpret_cast<sycl::buffer<float, 1>*>(C->buffer.get());
        
        sycl::event ev = q->queue->submit([&](sycl::handler& h) {
            auto a_acc = buf_a->get_access<sycl::access::mode::read>(h);
            auto b_acc = buf_b->get_access<sycl::access::mode::read>(h);
            auto c_acc = buf_c->get_access<sycl::access::mode::write>(h);
            
            h.parallel_for(sycl::range<2>(M, N), [=](sycl::id<2> idx) {
                int i = idx[0], j = idx[1];
                float sum = 0.0f;
                for (uint32_t k = 0; k < K; ++k) {
                    sum += a_acc[i * K + k] * b_acc[k * N + j];
                }
                c_acc[i * N + j] = sum;
            });
        });
        
        uint32_t event_id = g_event_counter++;
        auto event_ptr = std::make_shared<sycl::event>(ev);
        g_event_map[event_id] = event_ptr;
        
        *out_ev = new SyclEventImpl{event_ptr};
        return SYCL_SUCCESS;
    } catch (const std::exception& e) {
        std::cerr << "Matmul failed: " << e.what() << std::endl;
        return SYCL_ERROR_EXECUTION_FAILED;
    }
}

SyclError sycl_ternary_matmul_async(SyclQueue q, SyclBuffer A, SyclBuffer B, SyclBuffer C,
                                    uint32_t M, uint32_t N, uint32_t K, SyclEvent* out_ev) {
    if (!q || !q->queue || !A || !B || !C || !out_ev) return SYCL_ERROR_INVALID_QUEUE;
    
    try {
        std::lock_guard<std::mutex> lock(g_event_mutex);
        
        // This would call the ternary matmul implementation
        // For now, we'll just create a placeholder event
        sycl::event ev = q->queue->submit([&](sycl::handler& h) {
            h.single_task([=]() {});
        });
        
        uint32_t event_id = g_event_counter++;
        auto event_ptr = std::make_shared<sycl::event>(ev);
        g_event_map[event_id] = event_ptr;
        
        *out_ev = new SyclEventImpl{event_ptr};
        return SYCL_SUCCESS;
    } catch (const std::exception& e) {
        std::cerr << "Ternary matmul failed: " << e.what() << std::endl;
        return SYCL_ERROR_EXECUTION_FAILED;
    }
}

} // extern "C"