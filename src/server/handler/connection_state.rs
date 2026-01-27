//! Connection state management

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use rand::rngs::OsRng;
use rand::RngCore;

use crate::dht::SovereignDht;
use crate::identity::{NodeId, NodePermissions};

use super::auth::{AuthChallenge, AuthResponse, verify_auth_response};

/// File handle information
#[derive(Debug, Clone)]
pub struct FileHandle {
    pub fid: u32,
    pub path: String,
    pub mode: u8,
    pub offset: u64,
    pub synthetic: bool,
    pub translator_id: Option<String>,
}

/// Connection state manager
#[derive(Clone)]
pub struct ConnectionState {
    /// Active file handles
    fids: Arc<RwLock<HashMap<u32, FileHandle>>>,

    /// Next available fid
    next_fid: Arc<RwLock<u32>>,

    /// Active auth sessions keyed by auth fid
    auth_sessions: Arc<RwLock<HashMap<u32, AuthSession>>>,

    /// Latest authenticated permissions
    auth_permissions: Arc<RwLock<Option<NodePermissions>>>,

    /// Optional DHT reference for auth verification
    dht: Arc<RwLock<Option<Arc<SovereignDht>>>>,
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionState {
    /// Create a new connection state manager
    pub fn new() -> Self {
        Self {
            fids: Arc::new(RwLock::new(HashMap::new())),
            next_fid: Arc::new(RwLock::new(1)),
            auth_sessions: Arc::new(RwLock::new(HashMap::new())),
            auth_permissions: Arc::new(RwLock::new(None)),
            dht: Arc::new(RwLock::new(None)),
        }
    }

    /// Add a new file handle
    pub async fn add_fid(&self, fid: u32, handle: FileHandle) {
        let mut fids = self.fids.write().await;
        fids.insert(fid, handle);
    }

    /// Get a file handle by fid
    pub async fn get_fid(&self, fid: u32) -> Option<FileHandle> {
        let fids = self.fids.read().await;
        fids.get(&fid).cloned()
    }

    /// Remove a file handle
    pub async fn remove_fid(&self, fid: u32) -> Option<FileHandle> {
        let mut fids = self.fids.write().await;
        fids.remove(&fid)
    }

    /// Update file offset
    pub async fn update_offset(&self, fid: u32, offset: u64) {
        let mut fids = self.fids.write().await;
        if let Some(handle) = fids.get_mut(&fid) {
            handle.offset = offset;
        }
    }

    /// Get next available fid
    pub async fn next_fid(&self) -> u32 {
        let mut next = self.next_fid.write().await;
        let fid = *next;
        *next += 1;
        fid
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
        let mut sessions = self.auth_sessions.write().await;
        let session = sessions
            .get_mut(&afid)
            .ok_or_else(|| anyhow::anyhow!("Unknown auth fid"))?;

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
        session.verified = true;

        let mut auth_permissions = self.auth_permissions.write().await;
        *auth_permissions = Some(permissions.clone());

        Ok(permissions)
    }

    pub async fn auth_permissions(&self) -> Option<NodePermissions> {
        let auth_permissions = self.auth_permissions.read().await;
        auth_permissions.clone()
    }

    pub async fn set_dht(&self, dht: Arc<SovereignDht>) {
        let mut slot = self.dht.write().await;
        *slot = Some(dht);
    }
}

#[derive(Debug, Clone)]
struct AuthSession {
    challenge: AuthChallenge,
    verified: bool,
}
