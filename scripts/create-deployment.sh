#!/bin/bash
# Deployment script for 9P.e GPU server
# Bundles AdaptiveCpp runtime libraries for self-contained deployment

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}9P.e GPU Server Deployment Script${NC}"
echo "==================================="

# Check if we're in the right directory
if [ ! -f "Cargo.toml" ]; then
    echo -e "${RED}Error: Must run from project root directory${NC}"
    exit 1
fi

# Build the release version
echo -e "${YELLOW}Building release version...${NC}"
cargo build --release --bin gpu_synthetic_demo
echo -e "${GREEN}Build completed successfully${NC}"

# Create deployment directory
DEPLOY_DIR="dist"
echo -e "${YELLOW}Creating deployment directory: $DEPLOY_DIR${NC}"
rm -rf $DEPLOY_DIR
mkdir -p $DEPLOY_DIR

# Copy the binary
echo -e "${YELLOW}Copying application binary...${NC}"
cp target/release/gpu_synthetic_demo $DEPLOY_DIR/
strip $DEPLOY_DIR/gpu_synthetic_demo 2>/dev/null || echo "Could not strip binary"

# Copy required libraries
echo -e "${YELLOW}Copying AdaptiveCpp runtime libraries...${NC}"
cp /opt/adaptivecpp/lib/libacpp-rt.so $DEPLOY_DIR/
cp /opt/adaptivecpp/lib/libacpp-common.so $DEPLOY_DIR/

# Create launcher script
echo -e "${YELLOW}Creating launcher script...${NC}"
cat > $DEPLOY_DIR/ninep-gpu << 'EOF'
#!/bin/bash
# Launcher script for 9P.e GPU server
# Automatically sets up library path for bundled libraries

# Get the directory where this script is located
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY="$SCRIPT_DIR/gpu_synthetic_demo"
LIB_DIR="$SCRIPT_DIR"

# Check if binary exists
if [ ! -f "$BINARY" ]; then
    echo "Error: Application binary not found at $BINARY"
    exit 1
fi

# Set library path to include bundled libraries
export LD_LIBRARY_PATH="$LIB_DIR:$LD_LIBRARY_PATH"

# Run the application
exec "$BINARY" "$@"
EOF

# Make launcher executable
chmod +x $DEPLOY_DIR/ninep-gpu

# Create a simple README
cat > $DEPLOY_DIR/README.md << 'EOF'
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
EOF

# Show what we've created
echo -e "${GREEN}Deployment package created successfully!${NC}"
echo
echo "Contents of $DEPLOY_DIR/:"
ls -lh $DEPLOY_DIR/
echo
echo -e "${GREEN}To run the application:${NC}"
echo "  cd $DEPLOY_DIR"
echo "  ./ninep-gpu"
echo
echo -e "${YELLOW}Total deployment size: $(du -sh $DEPLOY_DIR | cut -f1)${NC}"