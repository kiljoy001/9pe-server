# 9P.e Protocol Specification

## Protocol Overview

The 9P.e (9P Extended) protocol is a backward-compatible extension of the Plan 9 filesystem protocol that adds modern features while maintaining compatibility with existing 9P2000 clients.

## Transport

### QUIC Transport Layer

9P.e uses QUIC (RFC 9000) as its primary transport protocol, providing:

- **Mandatory TLS 1.3 encryption**
- **Multiplexed streams** (multiple 9P sessions per connection)
- **0-RTT connection establishment** for returning clients
- **Connection migration** (IP address changes)
- **Built-in flow control and congestion control**
- **No head-of-line blocking** (UDP-based)

#### Connection Establishment

1. **QUIC Handshake**: TLS 1.3 + transport parameters
2. **9P.e Version Negotiation**: Protocol version and capabilities
3. **Authentication**: Challenge-response with Ed25519 signatures
4. **Session Establishment**: Ready for file operations

```
Client                           Server
  │                                │
  ├─────── QUIC Connect ─────────→│ (includes TLS 1.3 handshake)
  │←─────── QUIC Accept ──────────┤
  │                                │
  ├─────── Tversion ─────────────→│ msize=1048576, version="9P.e-1.0"
  │←─────── Rversion ─────────────┤ msize=1048576, version="9P.e-1.0"
  │                                │
  ├─────── Tauth ───────────────→│ afid=1, uname="alice", aname=""
  │←─────── Rauth ───────────────┤ aqid=...
  │                                │
  ├─────── Tattach ─────────────→│ fid=0, afid=1, uname="alice", aname=""
  │←─────── Rattach ─────────────┤ qid=...
```

### Legacy TCP Support

For backward compatibility, 9P.e can operate over TCP using the traditional 9P2000 wire format. Only core messages are supported over TCP.

## Message Format

### Basic Structure

All 9P.e messages follow the standard 9P format:

```
┌─────────────┬──────────┬─────────┬─────────────────┐
│ Length (4B) │ Type(1B) │ Tag(2B) │ Payload (varlen)│
└─────────────┴──────────┴─────────┴─────────────────┘
```

- **Length**: Total message size in bytes (little-endian)
- **Type**: Message type identifier
- **Tag**: Request/response correlation ID
- **Payload**: Message-specific data

### Extended Message Types

9P.e extends the original 9P2000 message types:

| Range | Category | Description |
|-------|----------|-------------|
| 100-119 | Core 9P2000 | Original Plan 9 messages (backward compatible) |
| 120-139 | Streaming | Async I/O and large file support |
| 140-159 | Multiplexing | Channel management and priorities |
| 160-179 | Capabilities | Access control and permissions |
| 180-199 | Synthetic | Dynamic file generation |
| 200-219 | Translators | Sandboxed filesystem extensions |
| 220-239 | Consensus | GHOSTDAG distributed consensus |

## Core Messages (9P2000 Compatible)

### Tversion/Rversion (100/101)
**Protocol version negotiation**

```rust
Tversion {
    msize: u32,        // Maximum message size
    version: String,   // Protocol version ("9P2000" or "9P.e-1.0")
}

Rversion {
    msize: u32,        // Agreed maximum message size
    version: String,   // Agreed protocol version
}
```

### Tauth/Rauth (102/103)
**Authentication initiation**

```rust
Tauth {
    afid: u32,         // Authentication file ID
    uname: String,     // User name
    aname: String,     // Access name (optional)
}

Rauth {
    aqid: Qid,         // Authentication file qid
}
```

### Tattach/Rattach (104/105)
**Filesystem attachment**

```rust
Tattach {
    fid: u32,          // File ID for root
    afid: u32,         // Authentication file ID (or NOFID)
    uname: String,     // User name
    aname: String,     // Access name
}

Rattach {
    qid: Qid,          // Root directory qid
}
```

### Twalk/Rwalk (110/111)
**Directory traversal**

```rust
Twalk {
    fid: u32,          // Current directory fid
    newfid: u32,       // New fid for result
    wnames: Vec<String>, // Path components to walk
}

Rwalk {
    wqids: Vec<Qid>,   // Qids for each successfully walked component
}
```

### Topen/Ropen (112/113)
**File opening**

```rust
Topen {
    fid: u32,          // File ID
    mode: u8,          // Open mode (OREAD, OWRITE, ORDWR, etc.)
}

Ropen {
    qid: Qid,          // File qid
    iounit: u32,       // Maximum I/O unit size (0 = no limit)
}
```

### Tcreate/Rcreate (114/115)
**File creation**

```rust
Tcreate {
    fid: u32,          // Directory fid
    name: String,      // New file name
    perm: u32,         // Permissions
    mode: u8,          // Open mode
}

Rcreate {
    qid: Qid,          // New file qid
    iounit: u32,       // Maximum I/O unit size
}
```

### Tread/Rread (116/117)
**Data reading**

```rust
Tread {
    fid: u32,          // File ID
    offset: u64,       // Read offset
    count: u32,        // Bytes to read
}

Rread {
    data: Vec<u8>,     // File data
}
```

### Twrite/Rwrite (118/119)
**Data writing**

```rust
Twrite {
    fid: u32,          // File ID
    offset: u64,       // Write offset
    data: Vec<u8>,     // Data to write
}

Rwrite {
    count: u32,        // Bytes actually written
}
```

## Streaming Messages (120-139)

For large file transfers and asynchronous I/O:

### TstreamInit/RstreamInit (120/121)
**Initialize streaming transfer**

```rust
TstreamInit {
    stream_id: u32,    // Unique stream identifier
    fid: u32,          // File ID
    mode: u8,          // Stream mode (read/write)
}

RstreamInit {
    stream_id: u32,    // Confirmed stream ID
    chunk_size: u32,   // Recommended chunk size
}
```

### TstreamData/RstreamData (122/123)
**Stream data chunk**

```rust
TstreamData {
    stream_id: u32,    // Stream identifier
    chunk_id: u32,     // Chunk sequence number
    data: Vec<u8>,     // Chunk data
}

RstreamData {
    stream_id: u32,    // Stream identifier
    chunk_id: u32,     // Acknowledged chunk
    status: u8,        // Chunk status (OK, retransmit, etc.)
}
```

### TstreamEnd/RstreamEnd (124/125)
**Finalize streaming transfer**

```rust
TstreamEnd {
    stream_id: u32,    // Stream identifier
    final_chunk: u32,  // Last chunk number
}

RstreamEnd {
    stream_id: u32,    // Stream identifier
    total_bytes: u64,  // Total bytes transferred
}
```

## Multiplexing Messages (140-159)

For managing multiple channels with different priorities:

### TmultiplexChannel/RmultiplexChannel (140/141)
**Channel management**

```rust
TmultiplexChannel {
    channel_id: u32,   // Channel identifier
    priority: u8,      // Channel priority (0=highest, 255=lowest)
}

RmultiplexChannel {
    channel_id: u32,   // Confirmed channel ID
    max_concurrent: u32, // Maximum concurrent operations
}
```

## Capability Messages (160-179)

Fine-grained access control system:

### TcapabilityGrant/RcapabilityGrant (160/161)
**Grant capabilities**

```rust
TcapabilityGrant {
    cap_id: u64,       // Capability identifier
    fid: u32,          // File/directory this applies to
    permissions: u32,  // Permission bits
}

RcapabilityGrant {
    cap_id: u64,       // Granted capability ID
    expires: u64,      // Expiration timestamp
}
```

### TcapabilityRevoke/RcapabilityRevoke (162/163)
**Revoke capabilities**

```rust
TcapabilityRevoke {
    cap_id: u64,       // Capability to revoke
}

RcapabilityRevoke {
    cap_id: u64,       // Revoked capability ID
}
```

### TcapabilityCheck/RcapabilityCheck (164/165)
**Check permissions**

```rust
TcapabilityCheck {
    cap_id: u64,       // Capability to check
}

RcapabilityCheck {
    cap_id: u64,       // Capability ID
    valid: bool,       // Whether capability is valid
    permissions: u32,  // Current permissions
}
```

## Synthetic File Messages (180-199)

For dynamic content generation:

### TsyntheticCreate/RsyntheticCreate (180/181)
**Create synthetic file**

```rust
TsyntheticCreate {
    fid: u32,          // File ID for synthetic file
    generator: String, // Generator type ("system_stats", "log_viewer", etc.)
    params: Vec<u8>,   // Generator-specific parameters
}

RsyntheticCreate {
    fid: u32,          // Created file ID
    qid: Qid,          // File qid
}
```

### TsyntheticUpdate/RsyntheticUpdate (182/183)
**Update synthetic file parameters**

```rust
TsyntheticUpdate {
    fid: u32,          // Synthetic file ID
    new_params: Vec<u8>, // New generator parameters
}

RsyntheticUpdate {
    fid: u32,          // Updated file ID
}
```

### TsyntheticRefresh/RsyntheticRefresh (184/185)
**Force content regeneration**

```rust
TsyntheticRefresh {
    fid: u32,          // Synthetic file ID
    force: bool,       // Force regeneration even if cached
}

RsyntheticRefresh {
    fid: u32,          // Refreshed file ID
    generation: u64,   // New generation number
}
```

## Translator Messages (200-219)

For sandboxed filesystem extensions:

### TtranslatorSpawn/RtranslatorSpawn (200/201)
**Spawn translator process**

```rust
TtranslatorSpawn {
    translator_id: u32, // Translator identifier
    code: Vec<u8>,     // Translator code (WebAssembly or native)
    config: Vec<u8>,   // Configuration data
}

RtranslatorSpawn {
    translator_id: u32, // Spawned translator ID
    pid: u32,          // Process ID (if applicable)
}
```

### TtranslatorMessage/RtranslatorMessage (202/203)
**Message passing with translator**

```rust
TtranslatorMessage {
    translator_id: u32, // Target translator
    data: Vec<u8>,     // Message data
}

RtranslatorMessage {
    translator_id: u32, // Source translator
    data: Vec<u8>,     // Response data
}
```

### TtranslatorKill/RtranslatorKill (204/205)
**Terminate translator**

```rust
TtranslatorKill {
    translator_id: u32, // Translator to terminate
}

RtranslatorKill {
    translator_id: u32, // Terminated translator ID
}
```

## Consensus Messages (220-239)

For GHOSTDAG distributed consensus:

### TconsensusPropose/RconsensusPropose (220/221)
**Propose new block**

```rust
TconsensusPropose {
    block_hash: [u8; 32],      // Block hash
    parent_hashes: Vec<[u8; 32]>, // Parent block hashes
}

RconsensusPropose {
    block_hash: [u8; 32],      // Proposed block hash
    status: u8,                // Proposal status (accepted/rejected)
}
```

### TconsensusVote/RconsensusVote (222/223)
**Vote on block**

```rust
TconsensusVote {
    block_hash: [u8; 32],      // Block to vote on
    vote: bool,                // True=accept, False=reject
}

RconsensusVote {
    block_hash: [u8; 32],      // Voted block hash
    vote_count: u32,           // Current vote count
}
```

### TconsensusCommit/RconsensusCommit (224/225)
**Commit block to consensus**

```rust
TconsensusCommit {
    block_hash: [u8; 32],      // Block to commit
    blue_score: u64,           // Blue score in GHOSTDAG
}

RconsensusCommit {
    block_hash: [u8; 32],      // Committed block hash
    final_blue_score: u64,     // Final blue score
}
```

## Error Handling

### Terror/Rerror (126/127)
**Error reporting**

```rust
Terror {
    original_tag: u16,  // Tag of message that caused error
    ename: String,      // Error description
}

Rerror {
    ename: String,      // Error description
    errno: u32,         // Error code (optional)
}
```

### Common Error Codes

| Code | Name | Description |
|------|------|-------------|
| 0 | ESUCCESS | No error |
| 1 | EPERM | Operation not permitted |
| 2 | ENOENT | No such file or directory |
| 3 | EBADF | Bad file descriptor |
| 4 | ENOMEM | Out of memory |
| 5 | EACCES | Permission denied |
| 6 | EBUSY | Device or resource busy |
| 7 | EEXIST | File exists |
| 8 | ENOTDIR | Not a directory |
| 9 | EISDIR | Is a directory |
| 10 | EINVAL | Invalid argument |
| 11 | EFBIG | File too large |
| 12 | ENOSPC | No space left on device |
| 13 | EROFS | Read-only file system |
| 14 | EAUTH | Authentication required |
| 15 | ECAPABILITY | Capability required |
| 16 | EVERSION | Unsupported protocol version |
| 17 | ESTREAMING | Streaming error |
| 18 | ECONSENSUS | Consensus error |

## Data Types

### Qid (13 bytes)
**File identifier**

```rust
struct Qid {
    qtype: u8,         // File type (QTDIR, QTFILE, etc.)
    version: u32,      // File version number
    path: u64,         // Unique file identifier
}
```

### Stat Structure
**File metadata**

```rust
struct Stat {
    size: u16,         // Size of stat structure
    qtype: u16,        // File type
    dev: u32,          // Device number
    qid: Qid,          // File qid
    mode: u32,         // Permissions and flags
    atime: u32,        // Access time
    mtime: u32,        // Modification time
    length: u64,       // File length
    name: String,      // File name
    uid: String,       // User ID
    gid: String,       // Group ID
    muid: String,      // Modifier user ID
}
```

## Security Model

### Transport Security
- **Mandatory TLS 1.3**: All connections encrypted
- **Certificate verification**: X.509 certificate chain validation
- **Forward secrecy**: Perfect forward secrecy for all sessions

### Message Authentication
- **Ed25519 signatures**: Critical operations signed
- **Replay protection**: Sequence numbers + timestamps
- **Integrity verification**: ChaCha20-Poly1305 AEAD

### Access Control
- **Capability-based**: Fine-grained permissions
- **Least privilege**: Minimal required access
- **Revocation**: Capabilities can be revoked
- **Expiration**: Time-limited access grants

### DoS Protection
- **Message size limits**: Prevent memory exhaustion
- **Rate limiting**: Per-connection request limits
- **Resource tracking**: Monitor and limit resource usage
- **Early validation**: Reject invalid messages quickly

## Backward Compatibility

### 9P2000 Compatibility
- **Wire format**: Identical for core messages
- **Message types**: 100-119 reserved for 9P2000
- **Legacy clients**: Can connect and operate normally
- **Feature detection**: Version negotiation reveals capabilities

### Migration Path
1. **Phase 1**: Deploy 9P.e servers with 9P2000 compatibility
2. **Phase 2**: Upgrade clients to support 9P.e features
3. **Phase 3**: Enable advanced features (consensus, translators)
4. **Phase 4**: Deprecate legacy TCP transport (optional)

## Implementation Notes

### Performance Considerations
- **Message batching**: Multiple small messages can be batched
- **Zero-copy**: Minimize data copying in implementation
- **Stream processing**: Large files use streaming to avoid buffering
- **Connection pooling**: QUIC multiplexing eliminates need for pools

### Reliability Features
- **Automatic retry**: QUIC handles packet loss and reordering
- **Connection migration**: Survives IP address changes
- **Flow control**: Prevents receiver overflow
- **Congestion control**: Adapts to network conditions

### Security Implementation
- **Constant-time crypto**: Prevents timing attacks
- **Secure random**: Cryptographically secure randomness
- **Key rotation**: Regular session key updates
- **Audit logging**: Security events logged for analysis

This specification provides a complete reference for implementing 9P.e protocol clients and servers while maintaining backward compatibility with existing 9P2000 infrastructure.