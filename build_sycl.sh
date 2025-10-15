#!/bin/bash

# Build script for SYCL ternary spikformer library

set -e

BUILD_DIR="/home/scott/Repo/9pe-server"
INSTALL_DIR="/home/scott/Repo/9pe-server"

echo "Building SYCL ternary spikformer library..."

# Check if we're using oneAPI or hipSYCL
if command -v icpx &> /dev/null; then
    # oneAPI
    echo "Using oneAPI (icpx)"
    CXX=icpx
    SYCL_FLAGS="-fsycl"
elif command -v dpcpp &> /dev/null; then
    # oneAPI DPC++
    echo "Using oneAPI (dpcpp)"
    CXX=dpcpp
    SYCL_FLAGS="-fsycl"
elif command -v clang++ &> /dev/null && command -v hipsycl-config &> /dev/null; then
    # hipSYCL
    echo "Using hipSYCL"
    CXX=clang++
    SYCL_FLAGS="$(hipsycl-config --cxxflags)"
    LINK_FLAGS="$(hipsycl-config --ldflags)"
else
    echo "No SYCL compiler found. Please install oneAPI or hipSYCL."
    exit 1
fi

# Compile FFI library
echo "Compiling FFI library..."
$CXX -std=c++17 -O2 -fPIC $SYCL_FLAGS \
    -shared $BUILD_DIR/sycl_ffi.cpp \
    -o $INSTALL_DIR/libsycl_ffi.so $LINK_FLAGS

# Compile ternary spikformer library
echo "Compiling ternary spikformer library..."
$CXX -std=c++17 -O2 -fPIC $SYCL_FLAGS \
    -shared $BUILD_DIR/sycl_ternary_spikformer.cpp \
    -o $INSTALL_DIR/libternary_spikformer.so $LINK_FLAGS

echo "Build complete!"
echo "Libraries created:"
echo "  - $INSTALL_DIR/libsycl_ffi.so"
echo "  - $INSTALL_DIR/libternary_spikformer.so"

# Test device discovery
echo "Testing device discovery..."
$CXX -std=c++17 -O2 $SYCL_FLAGS \
    -I$BUILD_DIR $BUILD_DIR/test_devices.cpp \
    -o $INSTALL_DIR/test_devices \
    -L$INSTALL_DIR -lsycl_ffi $LINK_FLAGS

echo "Run $INSTALL_DIR/test_devices to test device discovery"