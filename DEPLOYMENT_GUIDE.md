# 9P.e Server - Multi-Machine Deployment Guide

## Quick Setup: 3-Machine Grid

### Architecture Overview

```
Machine 1 (Intel Arc)          Machine 2 (NVIDIA)           Machine 3 (ARM)
┌─────────────────┐           ┌──────────────────┐         ┌────────────────┐
│ 9pe-server      │           │ 9pe-server       │         │ 9pe-server     │
│ + llama-server  │◄─────────►│ + OpenCL compute │◄───────►│ (coordinator)  │
│ (LLM inference) │   QUIC    │ (GPU matmul)     │  QUIC   │ (CPU only)     │
└─────────────────┘           └──────────────────┘         └────────────────┘
     Arc B50 GPU                  NVIDIA GPU                   No GPU
```

### Step 1: Build Binaries

**On x86_64 machine:**
```bash
cd /home/scott/Repo/9pe-server
cargo build --release
# Binary at: target/release/ninep-server
```

**For ARM server (cross-compile):**
```bash
# Install ARM target
rustup target add aarch64-unknown-linux-gnu

# Install cross-compilation tools
sudo apt install gcc-aarch64-linux-gnu

# Build for ARM
cargo build --release --target aarch64-unknown-linux-gnu

# Binary at: target/aarch64-unknown-linux-gnu/release/ninep-server
```

### Step 2: Configuration Files

Create config for each machine:

**Machine 1 (Intel Arc + LLM):**
```toml
# config_machine1.toml
[server]
listen_addr = "0.0.0.0:9009"  # Listen on all interfaces
node_id = "intel-arc-node"

[llama]
enabled = true
server_url = "http://localhost:8080"  # Your llama-launcher

[gpu]
enabled = true
backend = "opencl"  # Intel Arc uses OpenCL
device_id = 0

[consensus]
enabled = true
peers = [
    "192.168.1.102:9009",  # Machine 2
    "192.168.1.103:9009"   # Machine 3 (ARM)
]
```

**Machine 2 (NVIDIA):**
```toml
# config_machine2.toml
[server]
listen_addr = "0.0.0.0:9009"
node_id = "nvidia-node"

[llama]
enabled = false  # No LLM here, just compute

[gpu]
enabled = true
backend = "opencl"  # Or "cuda" if you have CUDA installed
device_id = 0

[consensus]
enabled = true
peers = [
    "192.168.1.101:9009",  # Machine 1
    "192.168.1.103:9009"   # Machine 3
]
```

**Machine 3 (ARM):**
```toml
# config_machine3.toml
[server]
listen_addr = "0.0.0.0:9009"
node_id = "arm-coordinator"

[llama]
enabled = false

[gpu]
enabled = false  # No GPU on ARM

[consensus]
enabled = true
peers = [
    "192.168.1.101:9009",  # Machine 1
    "192.168.1.102:9009"   # Machine 2
]
```

### Step 3: Copy Binaries to Machines

**Machine 1 (already built):**
```bash
# Already have it
```

**Machine 2 (copy x86_64 binary):**
```bash
scp target/release/ninep-server user@192.168.1.102:~/
scp config_machine2.toml user@192.168.1.102:~/config.toml
```

**Machine 3 (copy ARM binary):**
```bash
scp target/aarch64-unknown-linux-gnu/release/ninep-server user@192.168.1.103:~/
scp config_machine3.toml user@192.168.1.103:~/config.toml
```

### Step 4: Start Servers

**On each machine:**
```bash
./ninep-server --config config.toml
```

**Or with systemd (recommended):**

Create `/etc/systemd/system/9pe-server.service`:
```ini
[Unit]
Description=9P.e Server
After=network.target

[Service]
Type=simple
User=YOUR_USER
ExecStart=/home/YOUR_USER/ninep-server --config /home/YOUR_USER/config.toml
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

Then:
```bash
sudo systemctl daemon-reload
sudo systemctl enable 9pe-server
sudo systemctl start 9pe-server
sudo systemctl status 9pe-server
```

### Step 5: Test the Grid

**Check node connectivity:**
```bash
# On any machine
curl http://localhost:9009/status
```

**Submit an LLM job (will route to Machine 1):**
```bash
curl -X POST http://192.168.1.101:9009/jobs/llm \
  -H "Content-Type: application/json" \
  -d '{
    "prompt": "Hello, distributed AI!",
    "max_tokens": 100
  }'
```

**Submit a GPU compute job (will route to Machine 1 or 2):**
```bash
curl -X POST http://192.168.1.102:9009/jobs/compute \
  -H "Content-Type: application/json" \
  -d '{
    "operation": "matmul",
    "matrix_size": 1024
  }'
```

### Step 6: Mount the Filesystem (Optional)

**On Linux:**
```bash
# Install 9P client
sudo apt install 9mount

# Mount
sudo mkdir -p /mnt/9pe
sudo 9mount 192.168.1.101:9009 /mnt/9pe

# Access
ls /mnt/9pe
```

**On Plan 9:**
```bash
srv tcp!192.168.1.101!9009 9pe
mount /srv/9pe /n/9pe
```

## Troubleshooting

### Firewall Issues
```bash
# Allow 9P port
sudo ufw allow 9009/tcp
```

### Check Logs
```bash
# If using systemd
sudo journalctl -u 9pe-server -f

# Otherwise
./ninep-server --config config.toml --log-level debug
```

### GPU Not Detected

**Intel Arc:**
```bash
# Check OpenCL
clinfo

# Check drivers
ls /dev/dri/
```

**NVIDIA:**
```bash
# Check CUDA
nvidia-smi

# Check OpenCL
clinfo | grep NVIDIA
```

### Network Connectivity
```bash
# Test QUIC connection
nc -zv 192.168.1.101 9009
```

## Performance Tuning

### Machine 1 (Intel Arc + LLM)
- Tune llama.cpp context size for available VRAM
- Use `--no-kv-offload` if running out of GPU memory
- Set appropriate batch size

### Machine 2 (NVIDIA)
- Use CUDA backend if available (faster than OpenCL)
- Tune workgroup sizes for your GPU

### Machine 3 (ARM)
- Use as coordinator/storage
- Can run CPU-only tasks
- Good for file serving

## Advanced: Add More Machines

Just add their IP:port to the `peers` array in config.toml and restart!

```toml
[consensus]
peers = [
    "192.168.1.101:9009",
    "192.168.1.102:9009",
    "192.168.1.103:9009",
    "192.168.1.104:9009",  # Machine 4!
]
```

The consensus protocol will automatically discover and coordinate work.
