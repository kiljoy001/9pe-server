#!/bin/bash
# Test GPU Canvas Rendering via 9P Protocol

set -e

SERVER="localhost:5640"
CLIENT="./target/release/ninep-server client connect"

echo "=== GPU Canvas Test Script ==="
echo "Server: $SERVER"
echo

# Note: The client command doesn't support write operations yet
# This script demonstrates the intended workflow

echo "Step 1: Initialize V8 Context"
echo "  (Would write to /n/v8/session/context)"
echo

echo "Step 2: Trigger Test Pattern Rendering"
echo "  echo '{\"action\":\"render_test\"}' > /n/v8/session/events"
echo

echo "Step 3: Read Canvas as PNG"
echo "  cat /n/v8/session/canvas.png > output.png"
echo

echo "==== Current Status ===="
echo "The 9P server is running, but the built-in client doesn't support"
echo "write operations or file reading yet."
echo
echo "To properly test the canvas, you need either:"
echo "1. HTTP Gateway (not enabled in current build)"
echo "2. Full 9P client with read/write support"
echo "3. FUSE mount to access /n/v8/ as a regular filesystem"
echo

echo "Checking if server is reachable..."
if nc -z -w 2 localhost 5640 2>/dev/null; then
    echo "✅ Server is listening on port 5640"
else
    echo "❌ Server is not reachable on port 5640"
    exit 1
fi

echo
echo "Server log (last 10 lines):"
tail -10 server.log || echo "No server.log found"
