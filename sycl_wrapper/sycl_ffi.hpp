#ifndef SYCL_FFI_HPP
#define SYCL_FFI_HPP

#include <stdint.h>
#include <stddef.h>

// C-compatible interface for SYCL operations
// This allows Rust to call SYCL via FFI without C++ name mangling

#ifdef __cplusplus
extern "C" {
#endif

// Opaque handle types (pointer-sized integers in Rust)
typedef void* SyclDevice;
typedef void* SyclQueue;
typedef void* SyclBuffer;
typedef void* SyclKernel;
typedef void* SyclEvent;

// Device information structure
typedef struct {
    char name[256];
    char vendor[128];
    uint32_t compute_units;
    uint64_t global_memory_size;
    uint64_t local_memory_size;
    uint32_t max_work_group_size;
    bool is_gpu;
    bool is_cpu;
    bool supports_fp64;
    bool supports_fp16;
} SyclDeviceInfo;

// Error codes
typedef enum {
    SYCL_SUCCESS = 0,
    SYCL_ERROR_DEVICE_NOT_FOUND = 1,
    SYCL_ERROR_OUT_OF_MEMORY = 2,
    SYCL_ERROR_INVALID_KERNEL = 3,
    SYCL_ERROR_INVALID_BUFFER = 4,
    SYCL_ERROR_RUNTIME_ERROR = 5,
} SyclError;

// Backend type (for diagnostics)
typedef enum {
    SYCL_BACKEND_OPENCL = 0,
    SYCL_BACKEND_CUDA = 1,
    SYCL_BACKEND_HIP = 2,
    SYCL_BACKEND_LEVEL_ZERO = 3,
    SYCL_BACKEND_CPU = 4,
} SyclBackend;

// ============================================================================
// Device Management
// ============================================================================

// Discover all available SYCL devices
// Returns number of devices found, fills device_info array
SyclError sycl_discover_devices(SyclDeviceInfo* device_info, size_t* device_count);

// Get device handle by index
SyclError sycl_get_device(uint32_t device_index, SyclDevice* device);

// Get device backend type
SyclError sycl_get_device_backend(SyclDevice device, SyclBackend* backend);

// Release device handle
void sycl_release_device(SyclDevice device);

// ============================================================================
// Queue Management
// ============================================================================

// Create command queue for device
SyclError sycl_create_queue(SyclDevice device, SyclQueue* queue);

// Wait for all operations in queue to complete
SyclError sycl_queue_wait(SyclQueue queue);

// Release queue handle
void sycl_release_queue(SyclQueue queue);

// ============================================================================
// Buffer Management
// ============================================================================

// Allocate buffer on device (USM or buffer abstraction)
SyclError sycl_create_buffer(SyclQueue queue, size_t size_bytes, SyclBuffer* buffer);

// Write data to buffer
SyclError sycl_write_buffer(SyclQueue queue, SyclBuffer buffer,
                             const void* data, size_t size_bytes, size_t offset);

// Read data from buffer
SyclError sycl_read_buffer(SyclQueue queue, SyclBuffer buffer,
                            void* data, size_t size_bytes, size_t offset);

// Release buffer
void sycl_release_buffer(SyclBuffer buffer);

// ============================================================================
// Kernel Execution (Simple Interface - Users write WASM translators for complex)
// ============================================================================

// Execute standard matrix multiplication: C = A * B
// All buffers are float32, dimensions: A[M*K], B[K*N], C[M*N]
SyclError sycl_matmul_f32(SyclQueue queue,
                          SyclBuffer buffer_a, SyclBuffer buffer_b, SyclBuffer buffer_c,
                          uint32_t M, uint32_t N, uint32_t K);

// Execute element-wise vector addition: c = a + b
SyclError sycl_vector_add_f32(SyclQueue queue,
                              SyclBuffer buffer_a, SyclBuffer buffer_b, SyclBuffer buffer_c,
                              size_t length);

// Execute ReLU activation: out[i] = max(0, in[i])
SyclError sycl_relu_f32(SyclQueue queue,
                        SyclBuffer buffer_in, SyclBuffer buffer_out,
                        size_t length);

// Execute 2D convolution (simplified - full version in translators)
// input: [batch, in_channels, height, width]
// kernel: [out_channels, in_channels, kernel_h, kernel_w]
// output: [batch, out_channels, out_height, out_width]
SyclError sycl_conv2d_f32(SyclQueue queue,
                          SyclBuffer input, SyclBuffer kernel, SyclBuffer output,
                          uint32_t batch, uint32_t in_channels, uint32_t out_channels,
                          uint32_t height, uint32_t width,
                          uint32_t kernel_h, uint32_t kernel_w,
                          uint32_t stride, uint32_t padding);

// ============================================================================
// Custom Kernel Compilation (for advanced users via translators)
// ============================================================================

// Compile SYCL kernel from source string
SyclError sycl_compile_kernel(SyclDevice device, const char* source,
                               const char* kernel_name, SyclKernel* kernel);

// Set kernel argument (by index)
SyclError sycl_set_kernel_arg_buffer(SyclKernel kernel, uint32_t arg_index, SyclBuffer buffer);
SyclError sycl_set_kernel_arg_scalar(SyclKernel kernel, uint32_t arg_index,
                                      const void* value, size_t size);

// Execute kernel with work dimensions
SyclError sycl_execute_kernel(SyclQueue queue, SyclKernel kernel,
                               const size_t* global_work_size,
                               const size_t* local_work_size,
                               uint32_t work_dim);

// Release kernel
void sycl_release_kernel(SyclKernel kernel);

// ============================================================================
// Profiling and Diagnostics
// ============================================================================

// Get kernel execution time in nanoseconds
SyclError sycl_get_kernel_time(SyclEvent event, uint64_t* nanoseconds);

// Get device utilization (0.0 - 1.0)
SyclError sycl_get_device_utilization(SyclDevice device, float* utilization);

#ifdef __cplusplus
}
#endif

#endif // SYCL_FFI_HPP
