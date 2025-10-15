# 1 "sycl_wrapper/sycl_ffi.hpp"
# 1 "<built-in>" 1
# 1 "<built-in>" 3
# 453 "<built-in>" 3
# 1 "<command line>" 1
# 1 "<built-in>" 2
# 1 "sycl_wrapper/sycl_ffi.hpp" 2




# 1 "/usr/lib/llvm-18/lib/clang/18/include/stdint.h" 1 3
# 52 "/usr/lib/llvm-18/lib/clang/18/include/stdint.h" 3
# 1 "/usr/include/stdint.h" 1 3 4
# 26 "/usr/include/stdint.h" 3 4
# 1 "/usr/include/x86_64-linux-gnu/bits/libc-header-start.h" 1 3 4
# 33 "/usr/include/x86_64-linux-gnu/bits/libc-header-start.h" 3 4
# 1 "/usr/include/features.h" 1 3 4
# 392 "/usr/include/features.h" 3 4
# 1 "/usr/include/features-time64.h" 1 3 4
# 20 "/usr/include/features-time64.h" 3 4
# 1 "/usr/include/x86_64-linux-gnu/bits/wordsize.h" 1 3 4
# 21 "/usr/include/features-time64.h" 2 3 4
# 1 "/usr/include/x86_64-linux-gnu/bits/timesize.h" 1 3 4
# 19 "/usr/include/x86_64-linux-gnu/bits/timesize.h" 3 4
# 1 "/usr/include/x86_64-linux-gnu/bits/wordsize.h" 1 3 4
# 20 "/usr/include/x86_64-linux-gnu/bits/timesize.h" 2 3 4
# 22 "/usr/include/features-time64.h" 2 3 4
# 393 "/usr/include/features.h" 2 3 4
# 464 "/usr/include/features.h" 3 4
# 1 "/usr/include/stdc-predef.h" 1 3 4
# 465 "/usr/include/features.h" 2 3 4
# 486 "/usr/include/features.h" 3 4
# 1 "/usr/include/x86_64-linux-gnu/sys/cdefs.h" 1 3 4
# 559 "/usr/include/x86_64-linux-gnu/sys/cdefs.h" 3 4
# 1 "/usr/include/x86_64-linux-gnu/bits/wordsize.h" 1 3 4
# 560 "/usr/include/x86_64-linux-gnu/sys/cdefs.h" 2 3 4
# 1 "/usr/include/x86_64-linux-gnu/bits/long-double.h" 1 3 4
# 561 "/usr/include/x86_64-linux-gnu/sys/cdefs.h" 2 3 4
# 487 "/usr/include/features.h" 2 3 4
# 510 "/usr/include/features.h" 3 4
# 1 "/usr/include/x86_64-linux-gnu/gnu/stubs.h" 1 3 4
# 10 "/usr/include/x86_64-linux-gnu/gnu/stubs.h" 3 4
# 1 "/usr/include/x86_64-linux-gnu/gnu/stubs-64.h" 1 3 4
# 11 "/usr/include/x86_64-linux-gnu/gnu/stubs.h" 2 3 4
# 511 "/usr/include/features.h" 2 3 4
# 34 "/usr/include/x86_64-linux-gnu/bits/libc-header-start.h" 2 3 4
# 27 "/usr/include/stdint.h" 2 3 4
# 1 "/usr/include/x86_64-linux-gnu/bits/types.h" 1 3 4
# 27 "/usr/include/x86_64-linux-gnu/bits/types.h" 3 4
# 1 "/usr/include/x86_64-linux-gnu/bits/wordsize.h" 1 3 4
# 28 "/usr/include/x86_64-linux-gnu/bits/types.h" 2 3 4
# 1 "/usr/include/x86_64-linux-gnu/bits/timesize.h" 1 3 4
# 19 "/usr/include/x86_64-linux-gnu/bits/timesize.h" 3 4
# 1 "/usr/include/x86_64-linux-gnu/bits/wordsize.h" 1 3 4
# 20 "/usr/include/x86_64-linux-gnu/bits/timesize.h" 2 3 4
# 29 "/usr/include/x86_64-linux-gnu/bits/types.h" 2 3 4


typedef unsigned char __u_char;
typedef unsigned short int __u_short;
typedef unsigned int __u_int;
typedef unsigned long int __u_long;


typedef signed char __int8_t;
typedef unsigned char __uint8_t;
typedef signed short int __int16_t;
typedef unsigned short int __uint16_t;
typedef signed int __int32_t;
typedef unsigned int __uint32_t;

typedef signed long int __int64_t;
typedef unsigned long int __uint64_t;






typedef __int8_t __int_least8_t;
typedef __uint8_t __uint_least8_t;
typedef __int16_t __int_least16_t;
typedef __uint16_t __uint_least16_t;
typedef __int32_t __int_least32_t;
typedef __uint32_t __uint_least32_t;
typedef __int64_t __int_least64_t;
typedef __uint64_t __uint_least64_t;



typedef long int __quad_t;
typedef unsigned long int __u_quad_t;







typedef long int __intmax_t;
typedef unsigned long int __uintmax_t;
# 141 "/usr/include/x86_64-linux-gnu/bits/types.h" 3 4
# 1 "/usr/include/x86_64-linux-gnu/bits/typesizes.h" 1 3 4
# 142 "/usr/include/x86_64-linux-gnu/bits/types.h" 2 3 4
# 1 "/usr/include/x86_64-linux-gnu/bits/time64.h" 1 3 4
# 143 "/usr/include/x86_64-linux-gnu/bits/types.h" 2 3 4


typedef unsigned long int __dev_t;
typedef unsigned int __uid_t;
typedef unsigned int __gid_t;
typedef unsigned long int __ino_t;
typedef unsigned long int __ino64_t;
typedef unsigned int __mode_t;
typedef unsigned long int __nlink_t;
typedef long int __off_t;
typedef long int __off64_t;
typedef int __pid_t;
typedef struct { int __val[2]; } __fsid_t;
typedef long int __clock_t;
typedef unsigned long int __rlim_t;
typedef unsigned long int __rlim64_t;
typedef unsigned int __id_t;
typedef long int __time_t;
typedef unsigned int __useconds_t;
typedef long int __suseconds_t;
typedef long int __suseconds64_t;

typedef int __daddr_t;
typedef int __key_t;


typedef int __clockid_t;


typedef void * __timer_t;


typedef long int __blksize_t;




typedef long int __blkcnt_t;
typedef long int __blkcnt64_t;


typedef unsigned long int __fsblkcnt_t;
typedef unsigned long int __fsblkcnt64_t;


typedef unsigned long int __fsfilcnt_t;
typedef unsigned long int __fsfilcnt64_t;


typedef long int __fsword_t;

typedef long int __ssize_t;


typedef long int __syscall_slong_t;

typedef unsigned long int __syscall_ulong_t;



typedef __off64_t __loff_t;
typedef char *__caddr_t;


typedef long int __intptr_t;


typedef unsigned int __socklen_t;




typedef int __sig_atomic_t;
# 28 "/usr/include/stdint.h" 2 3 4
# 1 "/usr/include/x86_64-linux-gnu/bits/wchar.h" 1 3 4
# 29 "/usr/include/stdint.h" 2 3 4
# 1 "/usr/include/x86_64-linux-gnu/bits/wordsize.h" 1 3 4
# 30 "/usr/include/stdint.h" 2 3 4




# 1 "/usr/include/x86_64-linux-gnu/bits/stdint-intn.h" 1 3 4
# 24 "/usr/include/x86_64-linux-gnu/bits/stdint-intn.h" 3 4
typedef __int8_t int8_t;
typedef __int16_t int16_t;
typedef __int32_t int32_t;
typedef __int64_t int64_t;
# 35 "/usr/include/stdint.h" 2 3 4


# 1 "/usr/include/x86_64-linux-gnu/bits/stdint-uintn.h" 1 3 4
# 24 "/usr/include/x86_64-linux-gnu/bits/stdint-uintn.h" 3 4
typedef __uint8_t uint8_t;
typedef __uint16_t uint16_t;
typedef __uint32_t uint32_t;
typedef __uint64_t uint64_t;
# 38 "/usr/include/stdint.h" 2 3 4





typedef __int_least8_t int_least8_t;
typedef __int_least16_t int_least16_t;
typedef __int_least32_t int_least32_t;
typedef __int_least64_t int_least64_t;


typedef __uint_least8_t uint_least8_t;
typedef __uint_least16_t uint_least16_t;
typedef __uint_least32_t uint_least32_t;
typedef __uint_least64_t uint_least64_t;





typedef signed char int_fast8_t;

typedef long int int_fast16_t;
typedef long int int_fast32_t;
typedef long int int_fast64_t;
# 71 "/usr/include/stdint.h" 3 4
typedef unsigned char uint_fast8_t;

typedef unsigned long int uint_fast16_t;
typedef unsigned long int uint_fast32_t;
typedef unsigned long int uint_fast64_t;
# 87 "/usr/include/stdint.h" 3 4
typedef long int intptr_t;


typedef unsigned long int uintptr_t;
# 101 "/usr/include/stdint.h" 3 4
typedef __intmax_t intmax_t;
typedef __uintmax_t uintmax_t;
# 53 "/usr/lib/llvm-18/lib/clang/18/include/stdint.h" 2 3
# 6 "sycl_wrapper/sycl_ffi.hpp" 2
# 1 "/usr/lib/llvm-18/lib/clang/18/include/stdbool.h" 1 3
# 7 "sycl_wrapper/sycl_ffi.hpp" 2
# 1 "/usr/lib/llvm-18/lib/clang/18/include/stddef.h" 1 3
# 72 "/usr/lib/llvm-18/lib/clang/18/include/stddef.h" 3
# 1 "/usr/lib/llvm-18/lib/clang/18/include/__stddef_ptrdiff_t.h" 1 3
# 18 "/usr/lib/llvm-18/lib/clang/18/include/__stddef_ptrdiff_t.h" 3
typedef long int ptrdiff_t;
# 73 "/usr/lib/llvm-18/lib/clang/18/include/stddef.h" 2 3




# 1 "/usr/lib/llvm-18/lib/clang/18/include/__stddef_size_t.h" 1 3
# 18 "/usr/lib/llvm-18/lib/clang/18/include/__stddef_size_t.h" 3
typedef long unsigned int size_t;
# 78 "/usr/lib/llvm-18/lib/clang/18/include/stddef.h" 2 3
# 87 "/usr/lib/llvm-18/lib/clang/18/include/stddef.h" 3
# 1 "/usr/lib/llvm-18/lib/clang/18/include/__stddef_wchar_t.h" 1 3
# 88 "/usr/lib/llvm-18/lib/clang/18/include/stddef.h" 2 3




# 1 "/usr/lib/llvm-18/lib/clang/18/include/__stddef_null.h" 1 3
# 93 "/usr/lib/llvm-18/lib/clang/18/include/stddef.h" 2 3




# 1 "/usr/lib/llvm-18/lib/clang/18/include/__stddef_nullptr_t.h" 1 3
# 98 "/usr/lib/llvm-18/lib/clang/18/include/stddef.h" 2 3
# 107 "/usr/lib/llvm-18/lib/clang/18/include/stddef.h" 3
# 1 "/usr/lib/llvm-18/lib/clang/18/include/__stddef_max_align_t.h" 1 3
# 19 "/usr/lib/llvm-18/lib/clang/18/include/__stddef_max_align_t.h" 3
typedef struct {
  long long __clang_max_align_nonce1
      __attribute__((__aligned__(__alignof__(long long))));
  long double __clang_max_align_nonce2
      __attribute__((__aligned__(__alignof__(long double))));
} max_align_t;
# 108 "/usr/lib/llvm-18/lib/clang/18/include/stddef.h" 2 3




# 1 "/usr/lib/llvm-18/lib/clang/18/include/__stddef_offsetof.h" 1 3
# 113 "/usr/lib/llvm-18/lib/clang/18/include/stddef.h" 2 3
# 8 "sycl_wrapper/sycl_ffi.hpp" 2



typedef unsigned int uint32_t;



typedef unsigned long long uint64_t;



typedef signed int int32_t;



typedef signed long long int64_t;
# 43 "sycl_wrapper/sycl_ffi.hpp"
extern "C" {



typedef void* SyclDevice;
typedef void* SyclQueue;
typedef void* SyclBuffer;
typedef void* SyclKernel;
typedef void* SyclEvent;


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


typedef enum {
    SYCL_SUCCESS = 0,
    SYCL_ERROR_DEVICE_NOT_FOUND = 1,
    SYCL_ERROR_OUT_OF_MEMORY = 2,
    SYCL_ERROR_INVALID_KERNEL = 3,
    SYCL_ERROR_INVALID_BUFFER = 4,
    SYCL_ERROR_RUNTIME_ERROR = 5,
} SyclError;


typedef enum {
    SYCL_BACKEND_OPENCL = 0,
    SYCL_BACKEND_CUDA = 1,
    SYCL_BACKEND_HIP = 2,
    SYCL_BACKEND_LEVEL_ZERO = 3,
    SYCL_BACKEND_CPU = 4,
} SyclBackend;







SyclError sycl_discover_devices(SyclDeviceInfo* device_info, size_t* device_count);


SyclError sycl_get_device(uint32_t device_index, SyclDevice* device);


SyclError sycl_get_device_backend(SyclDevice device, SyclBackend* backend);


void sycl_release_device(SyclDevice device);






SyclError sycl_create_queue(SyclDevice device, SyclQueue* queue);


SyclError sycl_queue_wait(SyclQueue queue);


void sycl_release_queue(SyclQueue queue);






SyclError sycl_create_buffer(SyclQueue queue, size_t size_bytes, SyclBuffer* buffer);


SyclError sycl_write_buffer(SyclQueue queue, SyclBuffer buffer,
                             const void* data, size_t size_bytes, size_t offset);


SyclError sycl_read_buffer(SyclQueue queue, SyclBuffer buffer,
                            void* data, size_t size_bytes, size_t offset);


void sycl_release_buffer(SyclBuffer buffer);







SyclError sycl_matmul_f32(SyclQueue queue,
                          SyclBuffer buffer_a, SyclBuffer buffer_b, SyclBuffer buffer_c,
                          uint32_t M, uint32_t N, uint32_t K);


SyclError sycl_vector_add_f32(SyclQueue queue,
                              SyclBuffer buffer_a, SyclBuffer buffer_b, SyclBuffer buffer_c,
                              size_t length);


SyclError sycl_relu_f32(SyclQueue queue,
                        SyclBuffer buffer_in, SyclBuffer buffer_out,
                        size_t length);





SyclError sycl_conv2d_f32(SyclQueue queue,
                          SyclBuffer input, SyclBuffer kernel, SyclBuffer output,
                          uint32_t batch, uint32_t in_channels, uint32_t out_channels,
                          uint32_t height, uint32_t width,
                          uint32_t kernel_h, uint32_t kernel_w,
                          uint32_t stride, uint32_t padding);






SyclError sycl_compile_kernel(SyclDevice device, const char* source,
                               const char* kernel_name, SyclKernel* kernel);


SyclError sycl_set_kernel_arg_buffer(SyclKernel kernel, uint32_t arg_index, SyclBuffer buffer);
SyclError sycl_set_kernel_arg_scalar(SyclKernel kernel, uint32_t arg_index,
                                      const void* value, size_t size);


SyclError sycl_execute_kernel(SyclQueue queue, SyclKernel kernel,
                               const size_t* global_work_size,
                               const size_t* local_work_size,
                               uint32_t work_dim);


void sycl_release_kernel(SyclKernel kernel);






SyclError sycl_get_kernel_time(SyclEvent event, uint64_t* nanoseconds);


SyclError sycl_get_device_utilization(SyclDevice device, float* utilization);


}
