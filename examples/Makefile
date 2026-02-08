# Makefile for Intel multi-device examples
# NO TENSOR LEFT BEHIND!

ACPP := /opt/adaptivecpp/bin/acpp
CXX := g++

CXXFLAGS := -std=c++17 -O3 -fPIC
ACPP_FLAGS := -std=c++17 -O3

# Include paths
INCLUDES := -I..

# Libraries
ACPP_LIBS := -L/opt/adaptivecpp/lib -lacpp-rt -lacpp-common
LIBS := -lstdc++ -lpthread $(ACPP_LIBS)

# Targets
.PHONY: all clean test

all: test_simple test_basic

# Build the multi-device SYCL FFI with recursive discovery
../libsycl_ffi.so: ../sycl_ffi.cpp ../sycl_recursive_discovery.cpp ../sycl_ffi.hpp
	@echo "Building SYCL FFI with recursive lazy discovery..."
	$(ACPP) $(ACPP_FLAGS) -fPIC -c ../sycl_ffi.cpp -o /tmp/sycl_ffi.o
	$(ACPP) $(ACPP_FLAGS) -fPIC -c ../sycl_recursive_discovery.cpp -o /tmp/sycl_recursive.o
	$(CXX) -shared -fPIC -o ../libsycl_ffi.so /tmp/sycl_ffi.o /tmp/sycl_recursive.o $(LIBS)
	@echo "✓ SYCL FFI built with recursive discovery!"

# Build test program
test_all_intel_devices: test_all_intel_devices.cpp ../libsycl_ffi.so
	@echo "Building multi-device test program..."
	$(CXX) $(CXXFLAGS) $(INCLUDES) test_all_intel_devices.cpp -o test_all_intel_devices \
		-L.. -lsycl_ffi -Wl,-rpath,.. -Wl,-rpath,/opt/adaptivecpp/lib $(LIBS)
	@echo "✓ Test program built successfully"

# Build simple test
test_simple: test_simple.cpp ../libsycl_ffi.so
	@echo "Building simple multi-device test..."
	$(CXX) $(CXXFLAGS) $(INCLUDES) test_simple.cpp -o test_simple \
		-L.. -lsycl_ffi -Wl,-rpath,.. -Wl,-rpath,/opt/adaptivecpp/lib $(LIBS)
	@echo "✓ Simple test built successfully"

# Build basic test (without Intel-specific functions)
test_basic: test_basic.cpp ../libsycl_ffi.so
	@echo "Building basic SYCL test..."
	$(CXX) $(CXXFLAGS) $(INCLUDES) test_basic.cpp -o test_basic \
		-L.. -lsycl_ffi -Wl,-rpath,.. -Wl,-rpath,/opt/adaptivecpp/lib $(LIBS)
	@echo "✓ Basic test built successfully"

# Run the test
test: test_simple
	@echo ""
	@echo "========================================="
	@echo "  NO TENSOR LEFT BEHIND! 🚀"
	@echo "  Testing All Intel Devices"
	@echo "========================================="
	@echo ""
	LD_LIBRARY_PATH=/opt/adaptivecpp/lib:..:$$LD_LIBRARY_PATH ./test_simple

clean:
	rm -f test_all_intel_devices test_simple test_basic
	rm -f /tmp/sycl_*.o
	rm -f ../libsycl_ffi.so
	@echo "Cleaned"

help:
	@echo "NO TENSOR LEFT BEHIND - Build System"
	@echo ""
	@echo "Targets:"
	@echo "  make all   - Build everything"
	@echo "  make test  - Build and run multi-device test"
	@echo "  make clean - Remove build artifacts"
	@echo ""
	@echo "This will detect and utilize:"
	@echo "  ✓ Intel CPU (12 cores + oneMKL)"
	@echo "  ✓ Intel iGPU (UHD 770, zero-copy)"
	@echo "  ✓ Intel dGPU (Arc Pro B50, XMX)"
	@echo "  ✓ Intel GNA (10mW neural accelerator)"
