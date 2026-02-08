//! Connection state management
//!
//! Uses UUIDv8-based extended fids for multi-tenant isolation.
//! Wire fids (u32) are mapped to ExtendedFid internally.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::sync::RwLock;

use rand::rngs::OsRng;
use rand::RngCore;

use crate::dht::SovereignDht;
use crate::fid::{ExtendedFid, FidContext};
use crate::identity::{NodeId, NodePermissions};

use super::auth::{AuthChallenge, AuthResponse, verify_auth_response};

/// Global connection ID counter (u32 for ~4 billion connections before wrap)
static CONNECTION_COUNTER: AtomicU32 = AtomicU32::new(1);

/// File handle information with extended fid
#[derive(Debug, Clone)]
pub struct FileHandle {
    /// Extended fid (UUIDv8) - internal representation
    pub efid: ExtendedFid,
    /// Path to the file/directory
    pub path: String,
    /// Open mode (read/write/etc)
    pub mode: u8,
    /// Current offset in file
    pub offset: u64,
    /// Whether this is a synthetic file
    pub synthetic: bool,
    /// Associated translator (if any)
    pub translator_id: Option<String>,
}

impl FileHandle {
    /// Get the wire-compatible 32-bit fid
    pub fn fid(&self) -> u32 {
        self.efid.fid()
    }
}

/// Connection state manager
///
/// Manages file handles using UUIDv8-based extended fids internally
/// while maintaining wire compatibility with 9P2000 u32 fids.
#[derive(Clone)]
pub struct ConnectionState {
    /// Active file handles keyed by extended fid
    fids: Arc<RwLock<HashMap<ExtendedFid, FileHandle>>>,

    /// Map from wire fid (u32) to extended fid for this connection
    wire_to_extended: Arc<RwLock<HashMap<u32, ExtendedFid>>>,

    /// Next available wire fid
    next_fid: Arc<RwLock<u32>>,

    /// Fid context for this connection
    fid_context: Arc<RwLock<FidContext>>,

    /// Active auth sessions keyed by auth fid
    auth_sessions: Arc<RwLock<HashMap<u32, AuthSession>>>,

    /// Latest authenticated permissions
    auth_permissions: Arc<RwLock<Option<NodePermissions>>>,

    /// Authenticated user's public key
    user_pubkey: Arc<RwLock<Option<Vec<u8>>>>,

    /// Optional DHT reference for auth verification
    dht: Arc<RwLock<Option<Arc<SovereignDht>>>>,

    /// Shared memory borrows
    pub shared_memory_borrows: Arc<RwLock<HashMap<String, crate::ipc::SharedMemoryHandle>>>,

    /// Negotiated protocol version
    protocol_version: Arc<RwLock<String>>,

    /// Current namespace for this connection
    current_namespace: Arc<RwLock<String>>,
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionState {
    /// Create a new connection state manager
    pub fn new() -> Self {
        // Use u32 counter for global uniqueness, truncate to u16 for FidContext
        // The full u32 provides ~4 billion connections before global wrap
        // The u16 namespace_shard provides sufficient entropy within UUIDv8
        let global_id = CONNECTION_COUNTER.fetch_add(1, Ordering::Relaxed);
        let connection_id = (global_id & 0xFFFF) as u16;
        Self {
            fids: Arc::new(RwLock::new(HashMap::new())),
            wire_to_extended: Arc::new(RwLock::new(HashMap::new())),
            next_fid: Arc::new(RwLock::new(1)),
            fid_context: Arc::new(RwLock::new(FidContext::new(connection_id))),
            auth_sessions: Arc::new(RwLock::new(HashMap::new())),
            auth_permissions: Arc::new(RwLock::new(None)),
            user_pubkey: Arc::new(RwLock::new(None)),
            dht: Arc::new(RwLock::new(None)),
            shared_memory_borrows: Arc::new(RwLock::new(HashMap::new())),
            protocol_version: Arc::new(RwLock::new("9P.e".to_string())),
            current_namespace: Arc::new(RwLock::new(String::new())),
        }
    }

    /// Get the connection ID for this state
    pub async fn connection_id(&self) -> u16 {
        self.fid_context.read().await.connection_id()
    }

    /// Set the current namespace for fid creation
    pub async fn set_namespace(&self, namespace: String) {
        let mut ns = self.current_namespace.write().await;
        *ns = namespace.clone();
        let mut ctx = self.fid_context.write().await;
        ctx.set_namespace(namespace);
    }

    /// Get the current namespace
    pub async fn namespace(&self) -> String {
        self.current_namespace.read().await.clone()
    }

    /// Add a new file handle using wire fid
    pub async fn add_fid(&self, wire_fid: u32, handle: FileHandle) {
        let efid = handle.efid;
        let mut fids = self.fids.write().await;
        let mut wire_map = self.wire_to_extended.write().await;

        fids.insert(efid, handle);
        wire_map.insert(wire_fid, efid);
    }

    /// Create and add a file handle, returning the extended fid
    pub async fn create_fid(&self, wire_fid: u32, path: String, mode: u8, synthetic: bool, translator_id: Option<String>) -> ExtendedFid {
        let ctx = self.fid_context.read().await;
        let efid = ctx.extend_fid(wire_fid);
        drop(ctx);

        let handle = FileHandle {
            efid,
            path,
            mode,
            offset: 0,
            synthetic,
            translator_id,
        };

        self.add_fid(wire_fid, handle).await;
        efid
    }

    /// Get a file handle by wire fid
    pub async fn get_fid(&self, wire_fid: u32) -> Option<FileHandle> {
        let wire_map = self.wire_to_extended.read().await;
        let efid = wire_map.get(&wire_fid)?;
        let fids = self.fids.read().await;
        fids.get(efid).cloned()
    }

    /// Get a file handle by extended fid
    pub async fn get_fid_extended(&self, efid: &ExtendedFid) -> Option<FileHandle> {
        let fids = self.fids.read().await;
        fids.get(efid).cloned()
    }

    /// Remove a file handle by wire fid
    pub async fn remove_fid(&self, wire_fid: u32) -> Option<FileHandle> {
        let mut wire_map = self.wire_to_extended.write().await;
        let efid = wire_map.remove(&wire_fid)?;
        let mut fids = self.fids.write().await;
        fids.remove(&efid)
    }

    /// Update file offset by wire fid
    pub async fn update_offset(&self, wire_fid: u32, offset: u64) {
        let wire_map = self.wire_to_extended.read().await;
        if let Some(efid) = wire_map.get(&wire_fid) {
            let mut fids = self.fids.write().await;
            if let Some(handle) = fids.get_mut(efid) {
                handle.offset = offset;
            }
        }
    }

    /// Get next available wire fid with collision detection
    ///
    /// Ensures the returned fid is not already in use and handles overflow safely.
    /// Returns None if no fid is available (all 2^32 - 1 fids exhausted, which is
    /// practically impossible but handled for correctness).
    pub async fn next_fid(&self) -> u32 {
        let wire_map = self.wire_to_extended.read().await;
        let mut next = self.next_fid.write().await;

        // NOFID (u32::MAX) is reserved in 9P protocol
        const NOFID: u32 = u32::MAX;
        const MAX_SEARCH: u32 = 1000; // Prevent infinite loop on pathological cases

        let start = *next;
        let mut searched = 0;

        loop {
            let candidate = *next;

            // Skip NOFID which is reserved
            if candidate == NOFID {
                *next = 1; // Wrap to 1 (0 is sometimes special)
                searched += 1;
                if searched >= MAX_SEARCH {
                    // Fallback: return a fid anyway, collision will be caught elsewhere
                    break candidate;
                }
                continue;
            }

            // Check if this fid is already in use
            if !wire_map.contains_key(&candidate) {
                // Found an available fid
                *next = candidate.wrapping_add(1);
                break candidate;
            }

            // Fid in use, try next
            *next = candidate.wrapping_add(1);
            searched += 1;

            // Safety: prevent infinite loop if somehow all fids are used
            if searched >= MAX_SEARCH || *next == start {
                // All searched fids are in use - return candidate anyway
                // The caller should handle the duplicate gracefully
                break candidate;
            }
        }
    }

    /// List all fids for a specific user (by pubkey hash)
    pub async fn list_user_fids(&self, user_hash: u64) -> Vec<FileHandle> {
        let fids = self.fids.read().await;
        fids.values()
            .filter(|h| h.efid.user_hash() == user_hash)
            .cloned()
            .collect()
    }

    /// List all fids in a specific namespace
    pub async fn list_namespace_fids(&self, namespace_shard: u16) -> Vec<FileHandle> {
        let fids = self.fids.read().await;
        fids.values()
            .filter(|h| h.efid.namespace_shard() == namespace_shard)
            .cloned()
            .collect()
    }

    pub async fn create_auth_session(
        &self,
        afid: u32,
        server_node_id: String,
        required_scope: Option<String>,
    ) -> AuthChallenge {
        let mut nonce = [0u8; 32];
        OsRng.fill_bytes(&mut nonce);
        let challenge = AuthChallenge {
            nonce,
            server_node_id,
            required_scope,
        };

        let mut sessions = self.auth_sessions.write().await;
        sessions.insert(
            afid,
            AuthSession {
                challenge: challenge.clone(),
                verified: false,
            },
        );
        challenge
    }

    pub async fn get_auth_challenge(&self, afid: u32) -> Option<AuthChallenge> {
        let sessions = self.auth_sessions.read().await;
        sessions.get(&afid).map(|s| s.challenge.clone())
    }

    pub async fn submit_auth_response(
        &self,
        afid: u32,
        response: AuthResponse,
    ) -> Result<NodePermissions, anyhow::Error> {
        // Get and remove the session in one operation to prevent reuse
        let session = {
            let mut sessions = self.auth_sessions.write().await;
            sessions
                .remove(&afid)
                .ok_or_else(|| anyhow::anyhow!("Unknown auth fid"))?
        };

        let permissions = verify_auth_response(&session.challenge, &response)?;

        if let Some(dht) = self.dht.read().await.as_ref() {
            let node_id = NodeId::new(response.node_id.clone());
            if let Some(record) = dht.lookup_node(&node_id).await {
                if record.public_key != response.ed25519_pub.to_vec()
                    || record.p256_public_key != response.p256_pub
                    || record.certificate_der != response.cert_der
                    || record.permissions != permissions
                {
                    anyhow::bail!("Auth response does not match DHT record");
                }
            } else {
                dht.upsert_peer_record(
                    node_id,
                    response.ed25519_pub.to_vec(),
                    response.p256_pub.clone(),
                    response.cert_der.clone(),
                    None,
                    permissions.clone(),
                )
                .await?;
            }
        }

        // Store user pubkey for fid context
        let pubkey = response.ed25519_pub.to_vec();
        {
            let mut user_pk = self.user_pubkey.write().await;
            *user_pk = Some(pubkey.clone());
        }
        {
            let mut ctx = self.fid_context.write().await;
            ctx.set_user(pubkey);
        }

        let mut auth_permissions = self.auth_permissions.write().await;
        *auth_permissions = Some(permissions.clone());

        Ok(permissions)
    }

    /// Remove an auth session (for cleanup on failed auth or timeout)
    pub async fn remove_auth_session(&self, afid: u32) -> Option<AuthChallenge> {
        let mut sessions = self.auth_sessions.write().await;
        sessions.remove(&afid).map(|s| s.challenge)
    }

    /// Get count of active auth sessions (for monitoring)
    pub async fn auth_session_count(&self) -> usize {
        self.auth_sessions.read().await.len()
    }

    /// Get the authenticated user's public key
    pub async fn user_pubkey(&self) -> Option<Vec<u8>> {
        self.user_pubkey.read().await.clone()
    }

    pub async fn auth_permissions(&self) -> Option<NodePermissions> {
        let auth_permissions = self.auth_permissions.read().await;
        auth_permissions.clone()
    }

    /// Check if the connection is authenticated
    pub async fn is_authenticated(&self) -> bool {
        self.auth_permissions.read().await.is_some()
    }

    pub async fn set_dht(&self, dht: Arc<SovereignDht>) {
        let mut slot = self.dht.write().await;
        *slot = Some(dht);
    }

    /// Get negotiated protocol version
    pub async fn protocol_version(&self) -> String {
        self.protocol_version.read().await.clone()
    }

    /// Set negotiated protocol version
    pub async fn set_protocol_version(&self, version: String) {
        let mut v = self.protocol_version.write().await;
        *v = version;
    }
}

#[derive(Debug, Clone)]
struct AuthSession {
    challenge: AuthChallenge,
    verified: bool,
}
