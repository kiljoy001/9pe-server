#include "sycl_ffi.hpp"
#include <iostream>
#include <cstring>

int main() {
    std::cout << "Testing SYCL device discovery..." << std::endl;
    
    // Discover devices
    SyclError err = sycl_discover_devices();
    if (err != SYCL_SUCCESS) {
        std::cerr << "Failed to discover devices: " << err << std::endl;
        return 1;
    }
    
    // Get device count
    uint32_t device_count = 0;
    err = sycl_get_device_count(&device_count);
    if (err != SYCL_SUCCESS) {
        std::cerr << "Failed to get device count: " << err << std::endl;
        return 1;
    }
    
    std::cout << "Found " << device_count << " devices" << std::endl;
    
    // List devices
    for (uint32_t i = 0; i < device_count; ++i) {
        SyclDevice device;
        err = sycl_get_device(i, &device);
        if (err != SYCL_SUCCESS) {
            std::cerr << "Failed to get device " << i << std::endl;
            continue;
        }
        
        char name[256];
        int backend;
        err = sycl_get_device_info(device, name, sizeof(name), &backend);
        if (err == SYCL_SUCCESS) {
            const char* backend_names[] = {"OpenCL", "CUDA", "HIP", "Level Zero", "CPU"};
            std::cout << "Device " << i << ": " << name 
                      << " (Backend: " << backend_names[backend] << ")" << std::endl;
        }
        
        // Clean up
        delete device;
    }
    
    std::cout << "Device discovery test completed successfully!" << std::endl;
    return 0;
}