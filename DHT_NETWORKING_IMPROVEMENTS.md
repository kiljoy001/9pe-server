# DHT Networking Improvements

This document summarizes the improvements made to DHT and mesh networking integration.

## 1. Bootstrap Peers from Config/CLI → start_networking

**Problem**: DHT `start_networking` was hardcoded to use an empty bootstrap peer list, preventing proper cluster formation.

**Solution**: Wired bootstrap peers from configuration through to DHT initialization.

### Changes Made:

#### `src/server/mod.rs` (ServerConfig)
- Added `dht_bootstrap_peers: Vec<String>` field to ServerConfig (line 76)
- Added `service_discovery: Vec<String>` field to ServerConfig (line 77)
- Updated DHT networking initialization to parse and use bootstrap peers (lines 121-148)
  - Converts bootstrap peer strings to libp2p Multiaddr format
  - Supports both native Multiaddr format and SocketAddr format
  - Logs bootstrap peer count when non-empty

#### `src/server/builder.rs`
- Bootstrap peers already extracted from config (lines 149-150)
- Already passed to ServerConfig (lines 197-198)
- Now properly utilized by DHT

### How It Works:

```toml
# config.toml
[server]
dht_bootstrap_peers = [
    "/ip4/192.168.1.100/tcp/9651/p2p/12D3KooWABC...",
    "192.168.1.101:9651"  # Also accepts SocketAddr format
]
```

When the server starts:
1. Config loader reads `dht_bootstrap_peers` from TOML
2. ServerBuilder passes them to ServerConfig
3. Server::new() converts them to libp2p Multiaddr format
4. DHT `start_networking()` receives the bootstrap peers
5. DHT connects to bootstrap peers for cluster discovery

## 2. Mesh Discovery Tied to DHT Service Index

**Problem**: Mesh networking didn't automatically connect to nodes advertising specific services.

**Solution**: Added service-based discovery that queries DHT service index and auto-connects.

### Changes Made:

#### `src/mesh.rs` (MeshNetwork struct)
- Added `service_discovery: Vec<String>` field (line 42)
- Updated constructor to accept service_discovery parameter (line 54)
- Enhanced `run_dht_discovery()` to query DHT for service providers (lines 545-580)
  - Loops through requested services
  - Calls `dht.find_nodes_with_service()` for each
  - Auto-connects to discovered service providers
  - Logs service discovery connections at info level

#### `src/server/mod.rs`
- Updated MeshNetwork::new() call to pass service_discovery (line 279)

#### `src/mesh_control.rs`
- Updated test to pass empty service_discovery vector (line 237)

### How It Works:

```toml
# config.toml
[server]
service_discovery = ["compute", "storage", "ollama"]
```

Every 30 seconds in the DHT discovery loop:
1. Mesh queries DHT for nodes advertising each service
2. For each service provider found:
   - Checks if already connected
   - Attempts QUIC mesh connection if not
   - Logs connection attempt at info level
3. Service providers are discovered via DHT service index

**Service Advertisement** (already existed):
- Nodes call `dht.advertise_service(name, mount_point, capabilities)`
- Service record stored in DHT with key `service:{name}:{node_id}`
- Service index updated at `service-index:{name}` containing list of node IDs
- Periodic maintenance refreshes service advertisements

## 3. DHT Networking Integration Test

**File**: `tests/dht_networking.rs`

### Test Coverage:

1. **test_two_node_dht_discovery()**
   - Spawns two in-process DHT nodes
   - Verifies self-registration works
   - Tests local lookup functionality
   - Validates DHT record contents

2. **test_dht_service_advertisement()**
   - Tests service advertisement API
   - Verifies service discovery via `find_nodes_with_service()`
   - Checks service metadata in DHT records

3. **test_dht_maintenance_refresh()**
   - Tests periodic maintenance cycles
   - Verifies service records persist through refresh
   - Uses short interval (200ms) for testing

4. **test_dht_peer_address_update()**
   - Tests dynamic address updates
   - Verifies `update_peer_address()` functionality
   - Validates updated records are queryable

5. **test_dht_with_timeout()**
   - Tests lookup timeout behavior
   - Verifies operations don't hang on non-existent nodes
   - Uses tokio timeout wrapper

6. **test_dht_benefits()** (documentation)
   - Documents DHT design benefits
   - Always passes, serves as inline documentation

### Test Architecture:

- Uses `tokio::test` for async testing
- Creates in-process DHT nodes (no external dependencies)
- Uses OS-assigned ports (`:0`) to avoid conflicts
- Tests local DHT functionality (bootstrap not required)
- Short delays (100-500ms) for test speed

### Future Enhancements:

To test full two-node DHT communication:
1. Need to extract actual listen addresses after binding
2. Pass node 1's Multiaddr to node 2 as bootstrap
3. Test cross-node record propagation
4. Verify DHT routing table updates

Currently tests cover:
- ✅ DHT initialization and networking
- ✅ Self-registration
- ✅ Local record storage and lookup
- ✅ Service advertisement and discovery
- ✅ Address updates
- ✅ Timeout handling
- ⏳ Cross-node DHT propagation (needs bootstrap setup)

## Configuration Example

Complete working example:

```toml
[server]
listen_addr = "0.0.0.0:5640"
node_id = "node-cluster-01"
node_name = "compute-node-east"
dht_port = 9651

# Bootstrap peers for DHT cluster formation
dht_bootstrap_peers = [
    "/ip4/10.0.1.100/tcp/9651/p2p/12D3KooWABC123...",
    "10.0.1.101:9651",
    "[::1]:9651"  # IPv6 also supported
]

# Services to discover and connect to
service_discovery = [
    "compute",   # Connect to GPU compute providers
    "storage",   # Connect to storage nodes
    "ollama"     # Connect to LLM inference nodes
]

[consensus]
enabled = true
peers = [
    "10.0.1.100:9650",  # Mesh bootstrap (separate from DHT)
    "10.0.1.101:9650"
]
```

## Benefits

### 1. Automatic Cluster Discovery
- Nodes automatically find each other via DHT
- No manual peer management required
- Resilient to network topology changes

### 2. Service-Oriented Architecture
- Clients discover services by capability, not address
- Services can migrate between nodes
- Load balancing through service discovery

### 3. Zero-Configuration Clustering
- Bootstrap from just one known peer
- DHT propagates full cluster topology
- New nodes integrate automatically

### 4. Decentralized and Scalable
- No central discovery service
- O(log N) lookup complexity
- Handles node churn gracefully

## Implementation Notes

### libp2p Integration
- Uses libp2p Kademlia DHT for peer routing
- Ed25519 keys from SovereignIdentity for libp2p PeerId
- Multiaddr format supports various transports (TCP, QUIC, etc.)

### Bootstrap Peer Formats
Supports multiple address formats:
- Native libp2p: `/ip4/127.0.0.1/tcp/9651/p2p/12D3K...`
- SocketAddr: `127.0.0.1:9651`
- IPv6: `[::1]:9651`

### Service Discovery Timing
- DHT discovery runs every 30 seconds
- Service queries are rate-limited by this interval
- Connections are attempted once per cycle
- Already-connected peers are skipped

### Error Handling
- Bootstrap peer parsing failures are logged but don't stop startup
- Failed service connections are logged at debug level
- DHT networking failure logs warning but doesn't prevent server start
- Graceful degradation: services work without DHT

## Testing

Run the DHT networking integration tests:

```bash
cargo test --test dht_networking
```

All tests are async and use tokio runtime.
Tests are self-contained and don't require external services.

## Files Modified

- `src/server/mod.rs` - Added bootstrap peer wiring and ServerConfig fields
- `src/server/builder.rs` - Already had bootstrap peer extraction (no changes needed)
- `src/mesh.rs` - Added service_discovery field and logic
- `src/mesh_control.rs` - Updated test constructor call
- `tests/dht_networking.rs` - Created comprehensive test suite

## Compatibility

- Backward compatible - empty vectors disable features
- Config fields are optional (default to empty)
- Works with existing mesh and DHT code
- No breaking API changes
