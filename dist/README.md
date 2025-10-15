# 9P.e GPU Server

A self-contained GPU compute server with bundled runtime libraries.

## Usage

```bash
# Run the GPU demo
./ninep-gpu

# The application will automatically detect available GPUs
# and expose them as virtual files under /srv/compute/
```

## What's Included

- `gpu_synthetic_demo` - Main application binary
- `libacpp-rt.so` - AdaptiveCpp runtime library
- `libacpp-common.so` - AdaptiveCpp common utilities
- `ninep-gpu` - Launcher script that sets up library paths

## Requirements

- Linux x86_64 system
- GPU drivers already installed for your hardware
- No additional dependencies needed

## Supported Hardware

The application automatically detects and supports:
- Intel GPUs
- AMD GPUs  
- NVIDIA GPUs
- CPU OpenMP acceleration

Just run `./ninep-gpu` and it will work with whatever GPU hardware you have!
