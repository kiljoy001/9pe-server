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
    SYCL_ERROR_OUT_OF_MEMORY = 5,
    SYCL_ERROR_INVALID_HANDLE = 6
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
SyclError sycl_release_device(SyclDevice device);

// Queue management
SyclError sycl_create_queue(SyclDevice device, SyclQueue* queue);
SyclError sycl_queue_wait(SyclQueue queue);  // Wait for all operations on queue to complete
SyclError sycl_release_queue(SyclQueue queue);

// Buffer management
SyclError sycl_create_buffer(SyclQueue queue, size_t size, SyclBuffer* buffer);
SyclError sycl_release_buffer(SyclBuffer buffer);
// FIXED: Now takes queue parameter to avoid reconstructing queue on every operation
SyclError sycl_buffer_write(SyclQueue queue, SyclBuffer buffer, const void* data, size_t offset, size_t size);
SyclError sycl_buffer_read(SyclQueue queue, SyclBuffer buffer, void* data, size_t offset, size_t size);

// Event management
SyclError sycl_release_event(SyclEvent event);
SyclError sycl_get_kernel_time(SyclEvent event, uint64_t* start_ns, uint64_t* end_ns);

// Float32 operations
SyclError sycl_matmul_f32_async(SyclQueue q, SyclBuffer A, SyclBuffer B, SyclBuffer C,
                                uint32_t M, uint32_t N, uint32_t K, SyclEvent* out_ev);

// Ternary operations
SyclError sycl_ternary_matmul_async(SyclQueue q, SyclBuffer A, SyclBuffer B, SyclBuffer C,
                                    uint32_t M, uint32_t N, uint32_t K, SyclEvent* out_ev);

// Intel-optimized operations (will try Intel path, fallback to standard)
// These use oneMKL and XMX when available for 10-100x speedup on Intel Arc
SyclError sycl_matmul_f32_intel(SyclQueue q, SyclBuffer A, SyclBuffer B, SyclBuffer C,
                                uint32_t M, uint32_t N, uint32_t K, SyclEvent* out_ev);

SyclError sycl_ternary_matmul_xmx(SyclQueue q, SyclBuffer A, SyclBuffer B, SyclBuffer C,
                                   uint32_t M, uint32_t N, uint32_t K, SyclEvent* out_ev);

// Query Intel-specific capabilities (single device)
SyclError sycl_get_intel_capabilities(SyclDevice device,
                                       bool* has_xmx,
                                       bool* has_onemkl,
                                       uint32_t* sub_group_size);

// Multi-device Intel management - NO TENSOR LEFT BEHIND!
// Enumerate ALL Intel devices (CPU, iGPU, dGPU, GNA, FPGA)
SyclError sycl_enumerate_intel_devices();

// Get count of Intel devices by type ("CPU", "iGPU", "dGPU", "GNA", "FPGA")
SyclError sycl_get_intel_device_count_by_type(const char* type, uint32_t* count);

// Get detailed capabilities of an Intel device
SyclError sycl_get_intel_device_capabilities(
    uint32_t device_index,
    char* name, size_t name_size,
    char* type, size_t type_size,
    bool* zero_copy,
    bool* xmx,
    uint32_t* compute_units,
    uint64_t* memory_size,
    float* power_watts
);

// Smart device selection based on workload characteristics
// Routes to GNA for low-latency, iGPU for power-efficient, dGPU for throughput
SyclError sycl_select_best_intel_device(
    uint64_t data_size_bytes,
    uint32_t latency_ms,
    float power_budget_watts,
    uint32_t* selected_device_index
);

// Zero-copy shared buffer (CPU + iGPU can access without PCIe transfer)
SyclError sycl_create_shared_buffer(
    SyclQueue queue,
    size_t size,
    SyclBuffer* buffer
);

// Error handling
const char* sycl_get_last_error();
void sycl_clear_error();

// Handle management and cleanup
SyclError sycl_cleanup_unused_handles();
SyclError sycl_get_active_handle_count(uint32_t* devices, uint32_t* queues, uint32_t* buffers, uint32_t* events);

#ifdef __cplusplus
}
#endif

#endif // SYCL_FFI_HPP