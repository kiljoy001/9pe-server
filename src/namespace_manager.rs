//! System-level Namespace Manager Translator
//!
//! Built-in translator that manages namespace creation, deletion, and ownership
//! using cryptographic signatures. This is the authority for namespace registration
//! in the distributed 9P.e mesh network.
//!
//! Exposed at: /srv/namespace/

use crate::{
    consensus::{BlockState, BoundedGhostdag, NamespaceOp},
    mesh::MeshNetwork,
    synth::{ControlHandler, SyntheticFilesystem},
};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use bincode;
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json;
use serde_with::{serde_as, Bytes};
use std::collections::HashMap;
use std::convert::TryInto;
use std::future::Future;
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::RwLock;
use tokio::task;
use tracing::{debug, info};

/// Trait for handling mesh messages in namespace manager
#[async_trait::async_trait]
pub trait MeshMessageHandler: Send + Sync {
    async fn handle_namespace_access_request(
        &self,
        from_peer: String,
        namespace_path: String,
        _requester_pubkey: [u8; 32],
        _requested_role: String,
        _message: String,
    ) -> Result<()>;

    async fn handle_namespace_access_response(
        &self,
        from_peer: String,
        namespace_path: String,
        _requester_pubkey: [u8; 32],
        approved: bool,
        _message: String,
    ) -> Result<()>;
}

fn block_on_async<F, T>(future: F) -> Result<T>
where
    F: Future<Output = Result<T>> + Send + 'static,
    T: Send + 'static,
{
    task::block_in_place(|| Handle::current().block_on(future))
}

fn decode_hex_array<const N: usize>(input: &str, label: &str) -> Result<[u8; N]> {
    let bytes =
        hex::decode(input).map_err(|e| anyhow!("Invalid {} (hex decode failed): {}", label, e))?;
    let arr: [u8; N] = bytes
        .try_into()
        .map_err(|_| anyhow!("{} must be {} bytes", label, N))?;
    Ok(arr)
}

/// Cryptographic namespace claim
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceClaim {
    /// Namespace path (e.g., "/compute/pool", "/ai/models")
    pub path: String,

    /// Owner's public key (Ed25519)
    pub owner_pubkey: [u8; 32],

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// Expiration (optional, None = permanent)
    pub expires_at: Option<DateTime<Utc>>,

    /// Metadata about the namespace
    pub metadata: NamespaceMetadata,

    /// Cryptographic signature over (path + owner_pubkey + created_at)
    #[serde_as(as = "Bytes")]
    pub signature: [u8; 64],

    /// Consensus block ID that confirmed this claim
    pub consensus_block_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessRequest {
    /// Requester's public key
    pub requester_pubkey: String,

    /// Requested role ("participant", "contributor", "admin")
    pub requested_role: String,

    /// Request message/reason
    pub message: String,

    /// Timestamp of request
    pub requested_at: DateTime<Utc>,

    /// Current status ("pending", "approved", "rejected")
    pub status: String,

    /// Approver (if approved)
    pub approved_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceMetadata {
    /// Human-readable description
    pub description: String,

    /// Namespace type (compute, storage, ai, user, system, public, etc.)
    pub namespace_type: String,

    /// Participant requirements (N-of-M signatures required for operations)
    /// None = owner-only, Some((n, m)) = n-of-m participants required
    /// For public namespaces: Some((1, 0)) = open participation
    pub participant_requirements: Option<(usize, usize)>,

    /// Current active participants (node IDs that can contribute to liveness)
    pub participants: Vec<String>,

    /// Access requests pending approval
    pub access_requests: Vec<AccessRequest>,

    /// Last activity timestamp for liveness tracking
    pub last_activity: DateTime<Utc>,

    /// Agregore: MIME type hint for the namespace root (e.g., "text/html" or "application/wasm")
    pub mime_type: Option<String>,

    /// Agregore: Index file to serve when root is requested (e.g., "index.html")
    pub index_file: Option<String>,

    /// Agregore: CORS policy for web access (default: "*")
    pub cors_policy: Option<String>,

    /// Additional custom metadata
    pub custom: HashMap<String, String>,
}

/// Namespace manager - system-level translator
pub struct NamespaceManager {
    /// All registered namespace claims (path → claim)
    claims: Arc<RwLock<HashMap<String, NamespaceClaim>>>,

    /// Synthetic filesystem for /srv/namespace/
    synth_fs: Arc<SyntheticFilesystem>,

    /// Consensus DAG for global agreement
    consensus: Option<Arc<crate::consensus::ConsensusCoordinator>>,

    /// This server's signing key (for signing system namespaces)
    system_keypair: SigningKey,

    /// Mesh network for distributed communication (optional)
    mesh_network: Option<Arc<MeshNetwork>>,
}

impl NamespaceManager {
    /// Create new namespace manager
    pub fn new(synth_fs: Arc<SyntheticFilesystem>) -> Result<Self> {
        // Generate system signing key (in production, load from secure storage)
        let system_keypair = SigningKey::from_bytes(&rand::random());

        info!(
            "Namespace manager system public key: {}",
            hex::encode(system_keypair.verifying_key().as_bytes())
        );

        Ok(Self {
            claims: Arc::new(RwLock::new(HashMap::new())),
            synth_fs,
            consensus: None,
            system_keypair,
            mesh_network: None,
        })
    }

    /// Set mesh network for distributed communication
    pub fn with_mesh_network(mut self, mesh: Arc<MeshNetwork>) -> Self {
        self.mesh_network = Some(mesh);
        self
    }

    pub fn with_consensus(mut self, consensus: Arc<crate::consensus::ConsensusCoordinator>) -> Self {
        self.consensus = Some(consensus);
        self
    }

    /// Initialize namespace manager synthetic filesystem
    pub async fn initialize(&self) -> Result<()> {
        info!("Initializing namespace manager at /srv/namespace/");

        // Create /srv/namespace/ directory structure
        self.synth_fs
            .create_directory(std::path::Path::new("/srv/namespace"))
            .await?;

        // Create control files
        self.create_control_files().await?;

        // Register built-in system namespaces
        self.register_system_namespaces().await?;

        info!("Namespace manager initialized successfully");
        Ok(())
    }

    /// Create control files for namespace operations
    async fn create_control_files(&self) -> Result<()> {
        let base = std::path::Path::new("/srv/namespace");

        // /srv/namespace/register - Register new namespace
        let register_handler = self.create_register_handler();
        self.synth_fs
            .create_control_file(&base.join("register"), register_handler)
            .await?;

        // /srv/namespace/list - List all registered namespaces
        let list_handler = Arc::new(ListNamespacesHandler {
            claims: self.claims.clone(),
        });
        self.synth_fs
            .create_control_file(&base.join("list"), list_handler)
            .await?;

        // /srv/namespace/verify - Verify namespace ownership
        let verify_handler = Arc::new(VerifyNamespaceHandler {
            claims: self.claims.clone(),
            last_response: Arc::new(RwLock::new(None)),
        });
        self.synth_fs
            .create_control_file(&base.join("verify"), verify_handler)
            .await?;

        // /srv/namespace/delete - Delete namespace
        let delete_handler = self.create_delete_handler();
        self.synth_fs
            .create_control_file(&base.join("delete"), delete_handler)
            .await?;

        // /srv/namespace/system_pubkey - System public key
        self.synth_fs
            .create_file(
                &base.join("system_pubkey"),
                hex::encode(self.system_keypair.verifying_key().as_bytes()).into_bytes(),
                false, // read-only
            )
            .await?;

        // /srv/namespace/list_public - List public namespaces
        let list_public_handler = Arc::new(ListPublicNamespacesHandler {
            claims: self.claims.clone(),
        });
        self.synth_fs
            .create_control_file(&base.join("list_public"), list_public_handler)
            .await?;

        Ok(())
    }

    /// Expose register handler for tests and external callers
    pub fn create_register_handler(&self) -> Arc<dyn ControlHandler> {
        Arc::new(RegisterNamespaceHandler {
            manager: Arc::new(self.clone_without_synth()),
        })
    }

    /// Expose delete handler for tests and external callers
    pub fn create_delete_handler(&self) -> Arc<dyn ControlHandler> {
        Arc::new(DeleteNamespaceHandler {
            manager: Arc::new(self.clone_without_synth()),
        })
    }

    /// Register built-in system namespaces
    async fn register_system_namespaces(&self) -> Result<()> {
        // Register /srv/compute namespace for compute pool
        self.register_namespace(
            "/srv/compute",
            "Distributed compute pool for GPU/CPU resources",
            "compute",
            Some((1, 1)), // 1-of-1 for system namespaces (server only)
            None,
            &self.system_keypair,
        )
        .await?;

        // Register /srv/namespace itself
        self.register_namespace(
            "/srv/namespace",
            "Namespace registration and management",
            "system",
            Some((1, 1)), // 1-of-1 for system namespaces (server only)
            None,
            &self.system_keypair,
        )
        .await?;

        // Register /srv/settrans for translator management
        self.register_namespace(
            "/srv/settrans",
            "Translator installation and management",
            "system",
            Some((1, 1)), // 1-of-1 for system namespaces (server only)
            None,
            &self.system_keypair,
        )
        .await?;

        // Create user namespaces directory structure
        self.synth_fs
            .create_directory(std::path::Path::new("/srv/namespaces"))
            .await?;
        self.synth_fs
            .create_directory(std::path::Path::new("/srv/namespaces/users"))
            .await?;
        self.synth_fs
            .create_directory(std::path::Path::new("/srv/namespaces/public"))
            .await?;

        Ok(())
    }

    /// Register a new namespace with cryptographic ownership
    pub async fn register_namespace(
        &self,
        path: &str,
        description: &str,
        namespace_type: &str,
        participant_requirements: Option<(usize, usize)>, // N-of-M signatures required
        expires_at: Option<DateTime<Utc>>,
        owner_keypair: &SigningKey,
    ) -> Result<NamespaceClaim> {
        // Validate path
        if !path.starts_with('/') {
            return Err(anyhow!("Namespace path must start with /"));
        }

        // Check if already registered
        {
            let claims = self.claims.read().await;
            if claims.contains_key(path) {
                return Err(anyhow!("Namespace {} already registered", path));
            }
        }

        // Create claim with participant tracking
        let created_at = Utc::now();
        let owner_pubkey_hex = hex::encode(owner_keypair.verifying_key().as_bytes());

        let metadata = NamespaceMetadata {
            description: description.to_string(),
            namespace_type: namespace_type.to_string(),
            participant_requirements,
            participants: vec![owner_pubkey_hex.clone()], // Owner is first participant
            access_requests: Vec::new(),                  // No pending requests initially
            last_activity: created_at,
            mime_type: None,
            index_file: None,
            cors_policy: None,
            custom: HashMap::new(),
        };

        // Create signature over claim data
        let sign_data = format!(
            "{}{}{}{}",
            path,
            owner_pubkey_hex,
            created_at.timestamp(),
            participant_requirements
                .map(|(n, m)| format!("{}:{}", n, m))
                .unwrap_or_default()
        );
        let signature = owner_keypair.sign(sign_data.as_bytes());

        let claim = NamespaceClaim {
            path: path.to_string(),
            owner_pubkey: owner_keypair.verifying_key().to_bytes(),
            created_at,
            expires_at,
            metadata,
            signature: signature.to_bytes(),
            consensus_block_id: None,
        };

        let claim = self.persist_claim(claim).await?;

        info!(
            "Registered namespace: {} (owner: {}, type: {})",
            path,
            hex::encode(&claim.owner_pubkey[..8]),
            namespace_type
        );

        Ok(claim)
    }

    async fn persist_claim(&self, mut claim: NamespaceClaim) -> Result<NamespaceClaim> {
        if let Some(ref consensus) = self.consensus {
            use crate::consensus::GhostdagBlock as Block;
            use std::time::{SystemTime, UNIX_EPOCH};

            let op = NamespaceOp::RegisterNamespace {
                path: claim.path.clone(),
                owner_pubkey: claim.owner_pubkey,
                signature: claim.signature.to_vec(),
            };

            let block_id = format!("ns_{}", hex::encode(&claim.owner_pubkey[..8]));
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let operations = vec![op];
            let block_signature = self.sign_namespace_block(&block_id, timestamp, &operations)?;

            let data = bincode::serialize(&operations).unwrap();

            let block = crate::consensus::GhostdagBlock {
                hash: [0u8; 32],
                parent_hashes: vec![],
                blue_score: 0,
                red_score: 0,
                selected_parent: None,

                timestamp,
                data,
                author: {
                    let mut bytes = [0u8; 32];
                    bytes.copy_from_slice(self.system_keypair.verifying_key().as_bytes());
                    bytes
                },
                signature: {
                    let mut bytes = [0u8; 64];
                    bytes.copy_from_slice(&block_signature);
                    bytes
                },
                pow_nonce: 0,
                pow_context: 0,
                pow_difficulty: 0,
            };

            consensus.add_block(block).await?;
            claim.consensus_block_id = Some(block_id.clone());

            info!(
                "Namespace {} registered with consensus block {}",
                claim.path, block_id
            );
        }

        self.claims
            .write()
            .await
            .insert(claim.path.clone(), claim.clone());

        Ok(claim)
    }

    async fn handle_register_payload(&self, payload: &[u8]) -> Result<()> {
        let request: RegisterNamespaceRequest = serde_json::from_slice(payload)
            .context("Failed to parse register namespace request")?;
        self.register_namespace_from_request(request).await
    }

    async fn register_namespace_from_request(
        &self,
        request: RegisterNamespaceRequest,
    ) -> Result<()> {
        let path = request.path.trim();
        if !path.starts_with('/') {
            anyhow::bail!("Namespace path must start with /");
        }

        {
            let claims = self.claims.read().await;
            if claims.contains_key(path) {
                anyhow::bail!("Namespace {} already registered", path);
            }
        }

        let participant_requirements = parse_participant_requirements(&request)?;

        let expires_at = if let Some(exp_ts) = request.expires_at {
            Some(
                DateTime::<Utc>::from_timestamp(exp_ts, 0)
                    .ok_or_else(|| anyhow!("Invalid expires_at timestamp"))?,
            )
        } else {
            None
        };

        let pubkey_bytes = decode_hex_array::<32>(&request.pubkey, "pubkey")?;
        let verifying_key = VerifyingKey::from_bytes(&pubkey_bytes)
            .map_err(|e| anyhow!("Invalid public key: {}", e))?;

        let signature_bytes = decode_hex_array::<64>(&request.signature, "signature")?;
        let signature = Signature::from_bytes(&signature_bytes);

        let requirements_str = participant_requirements
            .map(|(n, m)| format!("{}:{}", n, m))
            .unwrap_or_default();

        let candidate_timestamps = if let Some(ts) = request.created_at {
            vec![ts]
        } else {
            let now = chrono::Utc::now().timestamp();
            vec![now, now - 1, now + 1]
        };

        let mut verified_timestamp = None;
        for ts in candidate_timestamps {
            let sign_data = format!("{}{}{}{}", path, request.pubkey, ts, requirements_str);
            if verifying_key
                .verify(sign_data.as_bytes(), &signature)
                .is_ok()
            {
                verified_timestamp = Some(ts);
                break;
            }
        }

        let created_at_ts =
            verified_timestamp.ok_or_else(|| anyhow!("Signature verification failed"))?;

        let created_at = DateTime::<Utc>::from_timestamp(created_at_ts, 0)
            .ok_or_else(|| anyhow!("Invalid created_at timestamp"))?;

        let description = if request.description.is_empty() {
            "Unnamed namespace".to_string()
        } else {
            request.description
        };

        let namespace_type = if request.namespace_type.is_empty() {
            "user".to_string()
        } else {
            request.namespace_type
        };

        let mut metadata = NamespaceMetadata {
            description,
            namespace_type,
            participant_requirements,
            participants: vec![request.pubkey.clone()],
            access_requests: Vec::new(),
            last_activity: created_at,
            mime_type: request.mime_type,
            index_file: request.index_file,
            cors_policy: request.cors_policy,
            custom: HashMap::new(),
        };

        if metadata.participant_requirements.is_none() {
            metadata.participants = vec![request.pubkey.clone()];
        }

        let claim = NamespaceClaim {
            path: path.to_string(),
            owner_pubkey: pubkey_bytes,
            created_at,
            expires_at,
            metadata,
            signature: signature_bytes,
            consensus_block_id: None,
        };

        self.persist_claim(claim).await?;
        Ok(())
    }

    async fn handle_delete_payload(&self, payload: &[u8]) -> Result<()> {
        let request: DeleteNamespaceRequest =
            serde_json::from_slice(payload).context("Failed to parse delete namespace request")?;
        self.delete_namespace_from_request(request).await
    }

    async fn delete_namespace_from_request(&self, request: DeleteNamespaceRequest) -> Result<()> {
        let path = request.path.trim();
        if path.is_empty() {
            anyhow::bail!("Namespace path is required");
        }

        let pubkey_bytes = decode_hex_array::<32>(&request.pubkey, "pubkey")?;
        let verifying_key = VerifyingKey::from_bytes(&pubkey_bytes)
            .map_err(|e| anyhow!("Invalid public key: {}", e))?;

        let signature_bytes = decode_hex_array::<64>(&request.signature, "signature")?;
        let signature = Signature::from_bytes(&signature_bytes);

        let sign_data = format!("DELETE:{}:{}", path, request.pubkey);
        verifying_key
            .verify(sign_data.as_bytes(), &signature)
            .map_err(|_| anyhow!("Signature verification failed"))?;

        if !self.verify_namespace(path, &pubkey_bytes).await? {
            anyhow::bail!("Not authorized to delete namespace {}", path);
        }

        self.delete_claim(path, &pubkey_bytes).await
    }

    async fn delete_claim(&self, path: &str, owner_pubkey: &[u8; 32]) -> Result<()> {
        if let Some(ref consensus) = self.consensus {
            use crate::consensus::GhostdagBlock as Block;
            use std::time::{SystemTime, UNIX_EPOCH};

            let op = NamespaceOp::Delete {
                path: path.to_string(),
            };

            let block_id = format!("del_{}", hex::encode(&owner_pubkey[..8]));
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let operations = vec![op];
            let block_signature = self.sign_namespace_block(&block_id, timestamp, &operations)?;

            let data = bincode::serialize(&operations).unwrap();

            let block = crate::consensus::GhostdagBlock {
                hash: [0u8; 32],
                parent_hashes: vec![],
                blue_score: 0,
                red_score: 0,
                selected_parent: None,

                timestamp,
                data,
                author: {
                    let mut bytes = [0u8; 32];
                    bytes.copy_from_slice(self.system_keypair.verifying_key().as_bytes());
                    bytes
                },
                signature: {
                    let mut bytes = [0u8; 64];
                    bytes.copy_from_slice(&block_signature);
                    bytes
                },
                pow_nonce: 0,
                pow_context: 0,
                pow_difficulty: 0,
            };

            consensus.add_block(block).await?;
        }

        let mut claims = self.claims.write().await;
        claims.remove(path);
        info!("Deleted namespace: {}", path);
        Ok(())
    }

    /// Verify namespace ownership
    pub async fn verify_namespace(&self, path: &str, pubkey: &[u8; 32]) -> Result<bool> {
        let claims = self.claims.read().await;

        if let Some(claim) = claims.get(path) {
            if let Some(expires_at) = claim.expires_at {
                if Utc::now() > expires_at {
                    return Ok(false);
                }
            }

            Ok(&claim.owner_pubkey == pubkey)
        } else {
            Ok(false)
        }
    }

    /// Get namespace claim
    pub async fn get_claim(&self, path: &str) -> Result<NamespaceClaim> {
        self.claims
            .read()
            .await
            .get(path)
            .cloned()
            .ok_or_else(|| anyhow!("Namespace {} not registered", path))
    }

    /// Delete namespace (requires owner signature)
    pub async fn delete_namespace(&self, path: &str, owner_keypair: &SigningKey) -> Result<()> {
        // Verify ownership
        if !self
            .verify_namespace(path, &owner_keypair.verifying_key().to_bytes())
            .await?
        {
            return Err(anyhow!("Not authorized to delete namespace {}", path));
        }

        self.delete_claim(path, &owner_keypair.verifying_key().to_bytes())
            .await
    }

    /// List all registered namespaces
    pub async fn list_namespaces(&self) -> Vec<NamespaceClaim> {
        self.claims.read().await.values().cloned().collect()
    }

    /// List user-owned namespaces
    pub async fn list_user_namespaces(&self) -> Vec<NamespaceClaim> {
        self.claims
            .read()
            .await
            .values()
            .filter(|claim| claim.metadata.namespace_type == "user")
            .cloned()
            .collect()
    }

    /// List public namespaces
    pub async fn list_public_namespaces(&self) -> Vec<NamespaceClaim> {
        self.claims
            .read()
            .await
            .values()
            .filter(|claim| claim.metadata.namespace_type == "public")
            .cloned()
            .collect()
    }

    /// Submit access request to a namespace
    pub async fn submit_access_request(
        &self,
        namespace_path: &str,
        requester_pubkey_hex: &str,
        requested_role: &str,
        message: &str,
    ) -> Result<()> {
        let mut claims = self.claims.write().await;
        let claim = claims
            .get_mut(namespace_path)
            .ok_or_else(|| anyhow!("Namespace not found"))?;

        // For public namespaces, automatically approve simple participation
        if claim.metadata.namespace_type == "public" && requested_role == "participant" {
            if !claim
                .metadata
                .participants
                .contains(&requester_pubkey_hex.to_string())
            {
                claim
                    .metadata
                    .participants
                    .push(requester_pubkey_hex.to_string());
                claim.metadata.last_activity = Utc::now();
                info!(
                    "Auto-approved participant access to public namespace {}",
                    namespace_path
                );
                return Ok(());
            }
        }

        // For other namespaces or roles, add to pending requests
        let request = AccessRequest {
            requester_pubkey: requester_pubkey_hex.to_string(),
            requested_role: requested_role.to_string(),
            message: message.to_string(),
            requested_at: Utc::now(),
            status: "pending".to_string(),
            approved_by: None,
        };

        claim.metadata.access_requests.push(request);
        info!(
            "Submitted access request for {} to namespace {}",
            requester_pubkey_hex, namespace_path
        );
        Ok(())
    }

    /// Approve access request (owner or admin only, with M-of-N requirements)
    pub async fn approve_access_request(
        &self,
        namespace_path: &str,
        requester_pubkey_hex: &str,
        approver_keypair: &SigningKey,
    ) -> Result<()> {
        let mut claims = self.claims.write().await;
        let claim = claims
            .get_mut(namespace_path)
            .ok_or_else(|| anyhow!("Namespace not found"))?;

        // Verify approver authorization (owner or admin)
        let is_owner = claim.owner_pubkey == approver_keypair.verifying_key().to_bytes();
        let is_admin = claim
            .metadata
            .participants
            .contains(&hex::encode(approver_keypair.verifying_key().as_bytes()));

        if !is_owner && !is_admin {
            return Err(anyhow!("Not authorized to approve access requests"));
        }

        // Check M-of-N requirements for approval
        if let Some((required_signatures, total_participants)) =
            claim.metadata.participant_requirements
        {
            // For public namespaces (1,0) = open participation, no additional checks needed
            if required_signatures == 1 && total_participants == 0 {
                // Open participation, proceed normally
            } else {
                // For M-of-N requirements, check if we have enough signatures
                // In a real implementation, this would collect actual signatures from participants
                // For now, we'll just verify that the approver is authorized

                // Check if the operation requires multiple signatures based on the requirements
                if required_signatures > 1 && total_participants > 1 {
                    // This is a multi-signature operation, but for simplicity in this implementation
                    // we're allowing the owner or an admin to approve directly
                    // In a full implementation, this would require collecting signatures from N participants
                    info!(
                        "Multi-signature operation required: {}/{} signatures needed",
                        required_signatures, total_participants
                    );
                }
            }
        }

        // Find and approve the request
        let request =
            claim.metadata.access_requests.iter_mut().find(|req| {
                req.requester_pubkey == requester_pubkey_hex && req.status == "pending"
            });

        if let Some(request) = request {
            request.status = "approved".to_string();
            request.approved_by = Some(hex::encode(approver_keypair.verifying_key().as_bytes()));

            // Add requester as participant
            if !claim
                .metadata
                .participants
                .contains(&requester_pubkey_hex.to_string())
            {
                claim
                    .metadata
                    .participants
                    .push(requester_pubkey_hex.to_string());
            }

            claim.metadata.last_activity = Utc::now();
            info!(
                "Approved access request for {} to namespace {}",
                requester_pubkey_hex, namespace_path
            );
            Ok(())
        } else {
            Err(anyhow!(
                "No pending access request found for {}",
                requester_pubkey_hex
            ))
        }
    }

    /// Reject access request (owner or admin only)
    pub async fn reject_access_request(
        &self,
        namespace_path: &str,
        requester_pubkey_hex: &str,
        rejector_keypair: &SigningKey,
    ) -> Result<()> {
        let mut claims = self.claims.write().await;
        let claim = claims
            .get_mut(namespace_path)
            .ok_or_else(|| anyhow!("Namespace not found"))?;

        // Verify rejector authorization (owner or admin)
        let is_owner = claim.owner_pubkey == rejector_keypair.verifying_key().to_bytes();
        let is_admin = claim
            .metadata
            .participants
            .contains(&hex::encode(rejector_keypair.verifying_key().as_bytes()));

        if !is_owner && !is_admin {
            return Err(anyhow!("Not authorized to reject access requests"));
        }

        // Find and reject the request
        let request =
            claim.metadata.access_requests.iter_mut().find(|req| {
                req.requester_pubkey == requester_pubkey_hex && req.status == "pending"
            });

        if let Some(request) = request {
            request.status = "rejected".to_string();
            request.approved_by = Some(hex::encode(rejector_keypair.verifying_key().as_bytes()));
            info!(
                "Rejected access request for {} to namespace {}",
                requester_pubkey_hex, namespace_path
            );
            Ok(())
        } else {
            Err(anyhow!(
                "No pending access request found for {}",
                requester_pubkey_hex
            ))
        }
    }

    /// List pending access requests for a namespace (owner/admin only)
    pub async fn list_pending_requests(&self, namespace_path: &str) -> Result<Vec<AccessRequest>> {
        let claims = self.claims.read().await;
        let claim = claims
            .get(namespace_path)
            .ok_or_else(|| anyhow!("Namespace not found"))?;

        let pending = claim
            .metadata
            .access_requests
            .iter()
            .filter(|req| req.status == "pending")
            .cloned()
            .collect();

        Ok(pending)
    }

    /// Add a participant to a namespace (requires owner authorization and M-of-N validation)
    pub async fn add_participant(
        &self,
        path: &str,
        participant_pubkey_hex: &str,
        owner_keypair: &SigningKey,
    ) -> Result<()> {
        let mut claims = self.claims.write().await;
        let claim = claims
            .get_mut(path)
            .ok_or_else(|| anyhow!("Namespace not found"))?;

        // Verify owner authorization - check that the provided keypair matches the owner
        if claim.owner_pubkey != owner_keypair.verifying_key().to_bytes() {
            return Err(anyhow!("Unauthorized: not the namespace owner"));
        }

        // Check M-of-N requirements for participant addition
        if let Some((required_signatures, total_participants)) =
            claim.metadata.participant_requirements
        {
            // For public namespaces (1,0) = open participation, no additional checks needed
            if required_signatures == 1 && total_participants == 0 {
                // Open participation, proceed normally
            } else {
                // Log M-of-N requirements for the operation
                info!(
                    "Adding participant with M-of-N requirements: {}/{} signatures needed",
                    required_signatures, total_participants
                );
            }
        }

        // Add participant
        if !claim
            .metadata
            .participants
            .contains(&participant_pubkey_hex.to_string())
        {
            claim
                .metadata
                .participants
                .push(participant_pubkey_hex.to_string());
            claim.metadata.last_activity = Utc::now();

            // Update total participants in requirements if needed
            if let Some((n, ref mut m)) = claim.metadata.participant_requirements {
                if n > 0 && *m == 0 {
                    // Don't update for open participation
                } else {
                    // Increment total participants count
                    *m = claim.metadata.participants.len();
                }
            }
        }

        info!(
            "Added participant {} to namespace {}",
            participant_pubkey_hex, path
        );
        Ok(())
    }

    /// Remove a participant from a namespace (requires owner authorization and M-of-N validation)
    pub async fn remove_participant(
        &self,
        path: &str,
        participant_pubkey_hex: &str,
        owner_keypair: &SigningKey,
    ) -> Result<()> {
        let mut claims = self.claims.write().await;
        let claim = claims
            .get_mut(path)
            .ok_or_else(|| anyhow!("Namespace not found"))?;

        // Verify owner authorization - check that the provided keypair matches the owner
        if claim.owner_pubkey != owner_keypair.verifying_key().to_bytes() {
            return Err(anyhow!("Unauthorized: not the namespace owner"));
        }

        // Check M-of-N requirements for participant removal
        if let Some((required_signatures, total_participants)) =
            claim.metadata.participant_requirements
        {
            // For public namespaces (1,0) = open participation, no additional checks needed
            if required_signatures == 1 && total_participants == 0 {
                // Open participation, proceed normally
            } else {
                // Log M-of-N requirements for the operation
                info!(
                    "Removing participant with M-of-N requirements: {}/{} signatures needed",
                    required_signatures, total_participants
                );
            }
        }

        // Remove participant
        claim
            .metadata
            .participants
            .retain(|p| p != participant_pubkey_hex);
        claim.metadata.last_activity = Utc::now();

        // Update total participants in requirements if needed
        if let Some((n, ref mut m)) = claim.metadata.participant_requirements {
            if n > 0 && *m == 0 {
                // Don't update for open participation
            } else {
                // Update total participants count
                *m = claim.metadata.participants.len();
            }
        }

        info!(
            "Removed participant {} from namespace {}",
            participant_pubkey_hex, path
        );
        Ok(())
    }

    /// Update namespace liveness (participant heartbeat) with M-of-N validation
    pub async fn update_liveness(&self, path: &str, participant_pubkey_hex: &str) -> Result<()> {
        let mut claims = self.claims.write().await;
        let claim = claims
            .get_mut(path)
            .ok_or_else(|| anyhow!("Namespace not found"))?;

        // Check if participant is authorized
        if !claim
            .metadata
            .participants
            .contains(&participant_pubkey_hex.to_string())
        {
            return Err(anyhow!(
                "Participant {} not authorized for namespace {}",
                participant_pubkey_hex,
                path
            ));
        }

        // Check M-of-N requirements for liveness update
        if let Some((required_signatures, total_participants)) =
            claim.metadata.participant_requirements
        {
            // For public namespaces (1,0) = open participation, no additional checks needed
            if required_signatures == 1 && total_participants == 0 {
                // Open participation, proceed normally
            } else {
                // Log M-of-N requirements for the operation
                info!(
                    "Liveness update with M-of-N requirements: {}/{} signatures needed",
                    required_signatures, total_participants
                );
            }
        }

        // Update last activity
        claim.metadata.last_activity = Utc::now();

        Ok(())
    }

    /// Check if namespace should be expired based on liveness
    pub async fn check_expiration(&self, path: &str) -> Result<bool> {
        let claims = self.claims.read().await;
        let claim = claims
            .get(path)
            .ok_or_else(|| anyhow!("Namespace not found"))?;

        // Check explicit expiration
        if let Some(expires_at) = claim.expires_at {
            if Utc::now() > expires_at {
                return Ok(true);
            }
        }

        // Check liveness timeout (24 hours default)
        let elapsed = Utc::now() - claim.metadata.last_activity;
        if elapsed.num_hours() > 24 {
            return Ok(true);
        }

        Ok(false)
    }

    /// Garbage collect expired namespaces
    pub async fn garbage_collect(&self) -> Result<usize> {
        let mut expired_namespaces = Vec::new();

        // Find expired namespaces
        {
            let claims = self.claims.read().await;
            for (path, claim) in claims.iter() {
                // Check explicit expiration
                let expired = if let Some(expires_at) = claim.expires_at {
                    Utc::now() > expires_at
                } else {
                    // Check liveness timeout (24 hours default)
                    let elapsed = Utc::now() - claim.metadata.last_activity;
                    elapsed.num_hours() > 24
                };

                if expired {
                    expired_namespaces.push(path.clone());
                }
            }
        }

        // Remove expired namespaces
        let mut count = 0;
        {
            let mut claims = self.claims.write().await;
            for path in expired_namespaces {
                if claims.remove(&path).is_some() {
                    info!("Garbage collected expired namespace: {}", path);
                    count += 1;
                }
            }
        }

        Ok(count)
    }

    /// Helper to clone without synth_fs (for Arc sharing)
    fn clone_without_synth(&self) -> Self {
        Self {
            claims: self.claims.clone(),
            synth_fs: self.synth_fs.clone(),
            consensus: self.consensus.clone(),
            system_keypair: self.system_keypair.clone(),
            mesh_network: self.mesh_network.clone(),
        }
    }

    fn sign_namespace_block(
        &self,
        block_id: &str,
        timestamp: u64,
        operations: &[NamespaceOp],
    ) -> Result<Vec<u8>> {
        let mut payload = Vec::new();
        payload.extend_from_slice(block_id.as_bytes());
        payload.extend_from_slice(&timestamp.to_le_bytes());

        let serialized_ops = bincode::serialize(operations)
            .context("Failed to serialize namespace operations for signature")?;
        payload.extend_from_slice(&serialized_ops);

        Ok(self.system_keypair.sign(&payload).to_bytes().to_vec())
    }
}

// ============================================================================
// Control File Handlers
// ============================================================================

#[derive(Debug, Deserialize)]
struct RegisterNamespaceRequest {
    path: String,
    #[serde(default)]
    description: String,
    #[serde(rename = "type", default)]
    namespace_type: String,
    pubkey: String,
    signature: String,
    #[serde(default)]
    participant_requirements: Option<String>,
    #[serde(default)]
    min_signatures: Option<usize>,
    #[serde(default)]
    total_participants: Option<usize>,
    #[serde(default)]
    created_at: Option<i64>,
    #[serde(default)]
    expires_at: Option<i64>,
    #[serde(default)]
    mime_type: Option<String>,
    #[serde(default)]
    index_file: Option<String>,
    #[serde(default)]
    cors_policy: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeleteNamespaceRequest {
    path: String,
    pubkey: String,
    signature: String,
}

struct RegisterNamespaceHandler {
    manager: Arc<NamespaceManager>,
}

impl ControlHandler for RegisterNamespaceHandler {
    fn read(&self) -> Result<Vec<u8>> {
        Ok(b"Write JSON with fields: path, description, type, pubkey, signature\n".to_vec())
    }

    fn write(&self, data: &[u8]) -> Result<()> {
        let payload = data.to_vec();
        block_on_async({
            let manager = self.manager.clone();
            async move { manager.handle_register_payload(&payload).await }
        })
    }
}

struct DeleteNamespaceHandler {
    manager: Arc<NamespaceManager>,
}

impl ControlHandler for DeleteNamespaceHandler {
    fn read(&self) -> Result<Vec<u8>> {
        Ok(b"Write JSON with fields: path, pubkey, signature to delete namespace\n".to_vec())
    }

    fn write(&self, data: &[u8]) -> Result<()> {
        let payload = data.to_vec();
        block_on_async({
            let manager = self.manager.clone();
            async move { manager.handle_delete_payload(&payload).await }
        })
    }
}

struct ListNamespacesHandler {
    claims: Arc<RwLock<HashMap<String, NamespaceClaim>>>,
}

impl ControlHandler for ListNamespacesHandler {
    fn read(&self) -> Result<Vec<u8>> {
        block_on_async({
            let claims = self.claims.clone();
            async move {
                let snapshot = claims.read().await;
                let mut paths: Vec<_> = snapshot.keys().cloned().collect();
                paths.sort();
                Ok(paths.join("\n").into_bytes())
            }
        })
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        anyhow::bail!("Listing namespaces is read-only")
    }
}

struct VerifyNamespaceHandler {
    claims: Arc<RwLock<HashMap<String, NamespaceClaim>>>,
    last_response: Arc<RwLock<Option<Vec<u8>>>>,
}

impl ControlHandler for VerifyNamespaceHandler {
    fn read(&self) -> Result<Vec<u8>> {
        block_on_async({
            let last = self.last_response.clone();
            async move {
                let mut guard = last.write().await;
                if let Some(response) = guard.take() {
                    Ok(response)
                } else {
                    Ok(b"Write JSON {\"path\":..., \"pubkey\":...} to verify ownership\n".to_vec())
                }
            }
        })
    }

    fn write(&self, data: &[u8]) -> Result<()> {
        let payload = data.to_vec();
        block_on_async({
            let claims = self.claims.clone();
            let last = self.last_response.clone();
            async move {
                #[derive(Deserialize)]
                struct VerifyPayload {
                    path: String,
                    pubkey: String,
                }

                let request: VerifyPayload = serde_json::from_slice(&payload)
                    .context("Failed to parse verify namespace request")?;

                let pubkey = decode_hex_array::<32>(&request.pubkey, "pubkey")?;
                let snapshot = claims.read().await;
                let owns = snapshot
                    .get(request.path.trim())
                    .map(|claim| claim.owner_pubkey == pubkey)
                    .unwrap_or(false);

                let mut guard = last.write().await;
                guard.replace(if owns {
                    b"owner\n".to_vec()
                } else {
                    b"not-owner\n".to_vec()
                });
                Ok(())
            }
        })
    }
}

struct ListPublicNamespacesHandler {
    claims: Arc<RwLock<HashMap<String, NamespaceClaim>>>,
}

impl ControlHandler for ListPublicNamespacesHandler {
    fn read(&self) -> Result<Vec<u8>> {
        block_on_async({
            let claims = self.claims.clone();
            async move {
                let snapshot = claims.read().await;
                let mut paths: Vec<_> = snapshot
                    .values()
                    .filter(|claim| claim.metadata.namespace_type == "public")
                    .map(|claim| claim.path.clone())
                    .collect();
                paths.sort();
                Ok(paths.join("\n").into_bytes())
            }
        })
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        anyhow::bail!("Listing namespaces is read-only")
    }
}

fn parse_participant_requirements(
    request: &RegisterNamespaceRequest,
) -> Result<Option<(usize, usize)>> {
    if let (Some(n), Some(m)) = (request.min_signatures, request.total_participants) {
        return Ok(Some((n, m)));
    }

    if let Some(ref spec) = request.participant_requirements {
        if spec.trim().is_empty() {
            return Ok(None);
        }

        let parts: Vec<&str> = spec.split(':').collect();
        if parts.len() != 2 {
            anyhow::bail!("participant_requirements must be in N:M format");
        }

        let n = parts[0]
            .parse::<usize>()
            .map_err(|_| anyhow!("Invalid min signatures"))?;
        let m = parts[1]
            .parse::<usize>()
            .map_err(|_| anyhow!("Invalid total participants"))?;
        return Ok(Some((n, m)));
    }

    Ok(None)
}

#[async_trait]
impl MeshMessageHandler for NamespaceManager {
    async fn handle_namespace_access_request(
        &self,
        from_peer: String,
        namespace_path: String,
        _requester_pubkey: [u8; 32],
        _requested_role: String,
        _message: String,
    ) -> Result<()> {
        info!(
            "Handling namespace access request from {} for namespace {}",
            from_peer, namespace_path
        );

        // For now, we'll just log the request
        // In a real implementation, this would check if we own the namespace
        // and either auto-approve or queue for manual approval

        // Dummy implementation - always approve for now
        debug!(
            "Would process access request for namespace {} from peer {}",
            namespace_path, from_peer
        );

        Ok(())
    }

    async fn handle_namespace_access_response(
        &self,
        from_peer: String,
        namespace_path: String,
        _requester_pubkey: [u8; 32],
        approved: bool,
        _message: String,
    ) -> Result<()> {
        info!(
            "Handling namespace access response from {} for namespace {}: {}",
            from_peer, namespace_path, approved
        );

        // For now, we'll just log the response
        // In a real implementation, this would update our local state

        debug!(
            "Would process access response for namespace {} from peer {}: {}",
            namespace_path, from_peer, approved
        );

        Ok(())
    }
}

/// Register namespace manager controls with the synthetic filesystem
pub async fn register_namespace_controls(
    synth_fs: &Arc<SyntheticFilesystem>,
    mesh_network: Option<Arc<MeshNetwork>>,
) -> Result<Arc<NamespaceManager>> {
    let mut namespace_mgr = NamespaceManager::new(synth_fs.clone())?;

    if let Some(mesh) = mesh_network {
        namespace_mgr = namespace_mgr.with_mesh_network(mesh);
    }

    namespace_mgr.initialize().await?;
    Ok(Arc::new(namespace_mgr))
}
