#!/bin/bash
# Simple mesh networking test script

export LD_LIBRARY_PATH=/home/scott/Repo/9pe-server:/opt/intel/oneapi/compiler/latest/lib:/opt/intel/oneapi/mkl/latest/lib

echo "Building 9pe-server with full features..."
cargo build --features full --bin ninep-server

echo ""
echo "Starting two mesh nodes for local testing..."
echo "Press Ctrl+C to stop both nodes"
echo ""

# Start first node in background
./target/debug/ninep-server serve --mesh --mesh-port 9000 --port 5640 2>&1 | sed 's/^/[Node 1] /' &
NODE1_PID=$!

sleep 2

# Start second node in background
./target/debug/ninep-server serve --mesh --mesh-port 9001 --port 5641 2>&1 | sed 's/^/[Node 2] /' &
NODE2_PID=$!

# Wait and show discovery logs
sleep 3
echo ""
echo "=== Mesh nodes running ==="
echo "Node 1 PID: $NODE1_PID (mesh: 9000, 9p: 5640)"
echo "Node 2 PID: $NODE2_PID (mesh: 9001, 9p: 5641)"
echo ""
echo "Watching for peer discovery (Ctrl+C to exit)..."
echo ""

# Wait for user interrupt
trap "echo ''; echo 'Stopping nodes...'; kill $NODE1_PID $NODE2_PID 2>/dev/null; exit 0" SIGINT SIGTERM

# Keep script running
wait
