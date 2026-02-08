# Feature Re-integration Plan

## ✅ What Works NOW (Proven!)

**Test:** `cargo run --example simple_file_server` + `cargo run --example simple_client`

```
✅ QUIC connection established
✅ TLS 1.3 encryption working
✅ 9P.e Version message exchange
✅ Message serialization/deserialization
✅ Server message loop handling requests
```

**Exit code: 0 - Clean success!**

## Core Implementation Status

| Component | Status | File | Lines |
|-----------|--------|------|-------|
| QUIC Server | ✅ Working | `examples/simple_file_server.rs` | ~150 |
| QUIC Client | ✅ Working | `examples/simple_client.rs` | ~170 |
| Message Loop | ✅ Working | Server handles conn→stream→message |  |
| Version Exchange | ✅ Working | Negotiates 9P.e-1.0 protocol | |
| DoS Protection | ✅ Working | Size limits enforced | |
| Rate Limiting | ✅ Implemented | In `src/rate_limiter.rs` | |
| Encryption | ✅ Working | TLS 1.3 via QUIC/rustls | |

## Next Steps (In Order)

### 1. Basic File Operations (Week 1)
**Goal:** Serve actual files

**Add to server loop:**
- Attach → return root directory qid
- Walk → traverse filesystem paths
- Open → open files for reading
- Read → return file contents
- Stat → return file metadata
- Clunk → close file descriptors

**Test:**
```bash
cargo run --example simple_client -- ls /
cargo run --example simple_client -- cat /tmp/test.txt
```

**Estimated:** 20-30 hours

---

### 2. Write Operations (Week 2)
**Goal:** Create and modify files

**Add:**
- Create → make new files
- Write → write file data
- Wstat → update file metadata
- Remove → delete files

**Test:**
```bash
cargo run --example simple_client -- echo "hello" > /tmp/new.txt
cargo run --example simple_client -- rm /tmp/old.txt
```

**Estimated:** 10-15 hours

---

### 3. FUSE Mount (Week 3)
**Goal:** Mount as real filesystem

**Use existing code:**
- `src/fuse_mount.rs` already exists
- Adapt for simple_file_server
- Test with standard tools

**Test:**
```bash
./mount_9pe.sh /mnt
ls /mnt
cat /mnt/README
echo "test" > /mnt/test.txt
```

**Estimated:** 8-12 hours

---

### 4. Re-enable Features (One at a Time)

#### 4a. Synthetic Files (Optional)
**When:** After basic file ops work
**Benefit:** Dynamic content generation
**Code:** `src/synth.rs` (already exists)
**Test:** `/proc`-like synthetic files
**Add time:** 1-2 days

#### 4b. Translators (Optional)
**When:** After synthetic files
**Benefit:** Sandboxed filesystem extensions
**Code:** `src/wasm/` + `src/settrans.rs`
**Test:** Compression translator
**Add time:** 2-3 days

#### 4c. Mesh Networking (Optional)
**When:** After core is stable
**Benefit:** Peer discovery, distributed namespace
**Code:** `src/mesh.rs` + `src/namespace_manager.rs`
**Test:** Multi-node setup
**Add time:** 3-4 days

#### 4d. Consensus (Optional)
**When:** After mesh works
**Benefit:** Distributed agreement
**Code:** `src/consensus.rs`
**Test:** Multi-node consensus
**Add time:** 4-5 days

#### 4e. GPU (Optional - Last)
**When:** Everything else works perfectly
**Benefit:** Compute offload
**Code:** `src/gpu/` + `src/sycl/` + `src/compute_control.rs`
**Test:** GPU synthetic files
**Add time:** 5-7 days
**Note:** Keep optional, causes test issues

---

## Decision Tree

```
Are basic file operations (read/write) working?
├─ NO → Focus on Week 1-2 tasks above
└─ YES → Can you mount via FUSE and use standard tools?
    ├─ NO → Focus on Week 3
    └─ YES → Choose ONE advanced feature to add
        ├─ Want dynamic content? → Synthetic files
        ├─ Want extensibility? → Translators
        ├─ Want distributed? → Mesh + Consensus
        └─ Want compute? → GPU (after everything else)
```

## Re-enabling a Feature

### Template Process

1. **Enable in Cargo.toml:**
   ```toml
   [features]
   default = ["synthetic"]  # Add feature here
   ```

2. **Check lib.rs:**
   ```rust
   #[cfg(feature = "synthetic")]
   pub mod synth;
   ```

3. **Update server/mod.rs:**
   - Uncomment feature-gated imports
   - Uncomment struct fields
   - Uncomment initialization code

4. **Test in isolation:**
   ```bash
   cargo test --features synthetic
   ```

5. **Update examples:**
   - Add feature usage to simple_file_server
   - Document in example comments

6. **Integration test:**
   - Write end-to-end test
   - Ensure doesn't break core functionality

## Success Criteria

**Before re-adding ANY advanced feature:**
- [ ] Can run simple_file_server
- [ ] Can connect with simple_client
- [ ] Can exchange Version messages
- [ ] Can Attach to root
- [ ] Can Walk paths
- [ ] Can Read files
- [ ] Can Write files
- [ ] Can mount via FUSE
- [ ] Standard tools work (`ls`, `cat`, `echo >`)

**Only then** add advanced features.

## Current State Summary

**Working:** QUIC + 9P.e core protocol + message exchange
**Missing:** File operations (Walk, Read, Write)
**Timeline:** 1-3 weeks to full file serving
**After that:** Add features incrementally

**The foundation is solid. Build the house before the pool.**
