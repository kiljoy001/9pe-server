#ifndef SYCL_FFI_HPP
#define SYCL_FFI_HPP

#include <cstdint>
#include <cstdlib>

#ifdef __cplusplus
extern "C" {
#endif

// Error codes
typedef enum {
    SYCL_SUCCESS = 0,
    SYCL_ERROR_INVALID_DEVICE = 1,
    SYCL_ERROR_INVALID_QUEUE = 2,
    SYCL_ERROR_INVALID_BUFFER = 3,
    SYCL_ERROR_EXECUTION_FAILED = 4,
    SYCL_ERROR_OUT_OF_MEMORY = 5
} SyclError;

// Handle types
typedef struct SyclDeviceImpl* SyclDevice;
typedef struct SyclQueueImpl* SyclQueue;
typedef struct SyclBufferImpl* SyclBuffer;
typedef struct SyclEventImpl* SyclEvent;

// Device discovery and management
SyclError sycl_discover_devices();
SyclError sycl_get_device_count(uint32_t* count);
SyclError sycl_get_device(uint32_t index, SyclDevice* device);
SyclError sycl_get_device_info(SyclDevice device, char* name, size_t name_size, int* backend);

// Queue management
SyclError sycl_create_queue(SyclDevice device, SyclQueue* queue);
SyclError sycl_release_queue(SyclQueue queue);

// Buffer management
SyclError sycl_create_buffer(SyclQueue queue, size_t size, SyclBuffer* buffer);
SyclError sycl_release_buffer(SyclBuffer buffer);
SyclError sycl_buffer_write(SyclBuffer buffer, const void* data, size_t offset, size_t size);
SyclError sycl_buffer_read(SyclBuffer buffer, void* data, size_t offset, size_t size);

// Event management
SyclError sycl_release_event(SyclEvent event);
SyclError sycl_get_kernel_time(SyclEvent event, uint64_t* start_ns, uint64_t* end_ns);

// Float32 operations
SyclError sycl_matmul_f32_async(SyclQueue q, SyclBuffer A, SyclBuffer B, SyclBuffer C,
                                uint32_t M, uint32_t N, uint32_t K, SyclEvent* out_ev);

// Ternary operations
SyclError sycl_ternary_matmul_async(SyclQueue q, SyclBuffer A, SyclBuffer B, SyclBuffer C,
                                    uint32_t M, uint32_t N, uint32_t K, SyclEvent* out_ev);

#ifdef __cplusplus
}
#endif

#endif // SYCL_FFI_HPP