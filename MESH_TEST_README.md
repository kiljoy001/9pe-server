# Mesh Networking Test

## Quick Start

```bash
./test_mesh.sh
```

That's it! This will:
1. Build the server with full features
2. Start two local mesh nodes
3. Show live logs from both nodes
4. Display peer discovery events
5. Stop cleanly when you press Ctrl+C

## What You'll See

The test runs two nodes:
- **Node 1**: mesh port 9000, 9P port 5640
- **Node 2**: mesh port 9001, 9P port 5641

Both nodes will:
- ✅ Register themselves via mDNS as `_9pe-mesh._udp.local.`
- ✅ Discover each other automatically on the local network
- ✅ Attempt QUIC connection on discovery
- ✅ Maintain DHT routing tables
- ✅ Exchange node IDs and peer lists

## Expected Log Output

```
[Node 1] Registered mDNS service: 9pe-XXXXXXXX on port 9000
[Node 1] mDNS discovery active, browsing for _9pe-mesh._udp.local.
[Node 2] Registered mDNS service: 9pe-YYYYYYYY on port 9001
[Node 2] mDNS discovery active, browsing for _9pe-mesh._udp.local.
[Node 1] mDNS service resolved: 9pe-YYYYYYYY
[Node 1] Discovered peer via mDNS: 127.0.0.1:9001
[Node 2] mDNS service resolved: 9pe-XXXXXXXX
[Node 2] Discovered peer via mDNS: 127.0.0.1:9000
```

## Manual Testing

If you want more control:

```bash
# Terminal 1
export LD_LIBRARY_PATH=/home/scott/Repo/9pe-server:/opt/intel/oneapi/compiler/latest/lib:/opt/intel/oneapi/mkl/latest/lib
./target/debug/ninep-server serve --mesh --mesh-port 9000

# Terminal 2
export LD_LIBRARY_PATH=/home/scott/Repo/9pe-server:/opt/intel/oneapi/compiler/latest/lib:/opt/intel/oneapi/mkl/latest/lib
./target/debug/ninep-server serve --mesh --mesh-port 9001
```

## Troubleshooting

**"libsycl_ffi.so not found"**
- Run `./build_intel.sh` first to build the Intel SYCL library
- Or set `LD_LIBRARY_PATH` as shown above

**No peer discovery**
- Check firewall isn't blocking mDNS (UDP port 5353)
- Verify both nodes are on the same network
- Try running with `RUST_LOG=debug` for verbose output

**Nodes can't connect**
- Ensure mesh ports (9000, 9001) aren't blocked
- Check if ports are already in use: `netstat -tuln | grep 900`

## Implementation Status

✅ mDNS service registration
✅ mDNS peer browsing
✅ DHT (Kademlia) discovery
✅ QUIC mesh transport
✅ Automatic peer connection
✅ Node ID propagation

Ready for production testing!
