#!/bin/bash

# Build script for OpenCL GPU WASM transformer

set -e

echo "Building OpenCL GPU transformer..."

# Build the WASM module
cargo build --target wasm32-unknown-unknown --release

# Copy the WASM module to the transformer directory
cp target/wasm32-unknown-unknown/release/opencl_gpu_transformer.wasm opencl-gpu.wasm

# Copy manifest
cp transformer.toml opencl-gpu.toml

echo "Build complete! Files:"
echo "  - opencl-gpu.wasm"
echo "  - opencl-gpu.toml"
echo ""
echo "To load in 9P.e server, copy these files to /srv/translators/"