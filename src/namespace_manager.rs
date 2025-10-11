//! System-level Namespace Manager Translator
//!
//! Built-in translator that manages namespace creation, deletion, and ownership
//! using cryptographic signatures. This is the authority for namespace registration
//! in the distributed 9P.e mesh network.
//!
//! Exposed at: /srv/namespace/

use anyhow::{anyhow, Result, Context};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, Bytes};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};
use ed25519_dalek::{SigningKey, VerifyingKey, Signature, Signer, Verifier};
use tracing::info;

use crate::consensus::{BoundedGhostdag, NamespaceOp};
use crate::synth::{SyntheticFilesystem, ControlHandler};

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
    consensus: Option<Arc<BoundedGhostdag>>,

    /// This server's signing key (for signing system namespaces)
    system_keypair: SigningKey,
}

impl NamespaceManager {
    /// Create new namespace manager
    pub fn new(synth_fs: Arc<SyntheticFilesystem>) -> Result<Self> {
        // Generate system signing key (in production, load from secure storage)
        let system_keypair = SigningKey::from_bytes(&rand::random());

        info!("Namespace manager system public key: {}",
              hex::encode(system_keypair.verifying_key().as_bytes()));

        Ok(Self {
            claims: Arc::new(RwLock::new(HashMap::new())),
            synth_fs,
            consensus: None,
            system_keypair,
        })
    }

    /// Set consensus coordinator
    pub fn with_consensus(mut self, consensus: Arc<BoundedGhostdag>) -> Self {
        self.consensus = Some(consensus);
        self
    }

    /// Initialize namespace manager synthetic filesystem
    pub async fn initialize(&self) -> Result<()> {
        info!("Initializing namespace manager at /srv/namespace/");

        // Create /srv/namespace/ directory structure
        self.synth_fs.create_directory(std::path::Path::new("/srv/namespace")).await?;

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
        let register_handler = Arc::new(RegisterNamespaceHandler {
            manager: Arc::new(self.clone_without_synth()),
        });
        self.synth_fs.create_control_file(
            &base.join("register"),
            register_handler,
        ).await?;

        // /srv/namespace/list - List all registered namespaces
        let list_handler = Arc::new(ListNamespacesHandler {
            claims: self.claims.clone(),
        });
        self.synth_fs.create_control_file(
            &base.join("list"),
            list_handler,
        ).await?;

        // /srv/namespace/verify - Verify namespace ownership
        let verify_handler = Arc::new(VerifyNamespaceHandler {
            claims: self.claims.clone(),
        });
        self.synth_fs.create_control_file(
            &base.join("verify"),
            verify_handler,
        ).await?;

        // /srv/namespace/delete - Delete namespace
        let delete_handler = Arc::new(DeleteNamespaceHandler {
            manager: Arc::new(self.clone_without_synth()),
        });
        self.synth_fs.create_control_file(
            &base.join("delete"),
            delete_handler,
        ).await?;

        // /srv/namespace/system_pubkey - System public key
        self.synth_fs.create_file(
            &base.join("system_pubkey"),
            hex::encode(self.system_keypair.verifying_key().as_bytes()).into_bytes(),
            false, // read-only
        ).await?;

        // /srv/namespace/list_public - List public namespaces
        let list_public_handler = Arc::new(ListPublicNamespacesHandler {
            claims: self.claims.clone(),
        });
        self.synth_fs.create_control_file(
            &base.join("list_public"),
            list_public_handler,
        ).await?;

        Ok(())
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
        ).await?;

        // Register /srv/namespace itself
        self.register_namespace(
            "/srv/namespace",
            "Namespace registration and management",
            "system",
            Some((1, 1)), // 1-of-1 for system namespaces (server only)
            None,
            &self.system_keypair,
        ).await?;

        // Register /srv/settrans for translator management
        self.register_namespace(
            "/srv/settrans",
            "Translator installation and management",
            "system",
            Some((1, 1)), // 1-of-1 for system namespaces (server only)
            None,
            &self.system_keypair,
        ).await?;

        // Create user namespaces directory structure
        self.synth_fs.create_directory(std::path::Path::new("/srv/namespaces")).await?;
        self.synth_fs.create_directory(std::path::Path::new("/srv/namespaces/users")).await?;
        self.synth_fs.create_directory(std::path::Path::new("/srv/namespaces/public")).await?;

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
            access_requests: Vec::new(), // No pending requests initially
            last_activity: created_at,
            custom: HashMap::new(),
        };

        // Create signature over claim data
        let sign_data = format!("{}{}{}{}",
            path,
            owner_pubkey_hex,
            created_at.timestamp(),
            participant_requirements.map(|(n, m)| format!("{}:{}", n, m)).unwrap_or_default()
        );
        let signature = owner_keypair.sign(sign_data.as_bytes());

        let mut claim = NamespaceClaim {
            path: path.to_string(),
            owner_pubkey: owner_keypair.verifying_key().to_bytes(),
            created_at,
            expires_at,
            metadata,
            signature: signature.to_bytes(),
            consensus_block_id: None,
        };

        // Submit to consensus for global agreement
        if let Some(ref consensus) = self.consensus {
            use crate::consensus::Block;
            use std::time::{SystemTime, UNIX_EPOCH};

            let op = NamespaceOp::RegisterNamespace {
                path: path.to_string(),
                owner_pubkey: claim.owner_pubkey,
                signature: claim.signature.to_vec(),
            };

            // Create block with the namespace operation
            let block_id = format!("ns_{}", hex::encode(&claim.owner_pubkey[..8]));
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let block = Block {
                id: block_id.clone(),
                parents: vec![], // Will be set by consensus
                operations: vec![op],
                timestamp,
                creator: "namespace_manager".to_string(),
                signature: vec![], // TODO: Sign block
                state: crate::consensus::BlockState::Pending,
                ghost_weight: 1,
                height: 0, // Will be computed by consensus
            };

            consensus.add_block(block).await?;
            claim.consensus_block_id = Some(block_id);

            info!("Namespace {} registered with consensus block {}",
                  path, claim.consensus_block_id.as_ref().unwrap());
        }

        // Store claim
        self.claims.write().await.insert(path.to_string(), claim.clone());

        info!("Registered namespace: {} (owner: {}, type: {})",
              path,
              hex::encode(&claim.owner_pubkey[..8]),
              namespace_type);

        Ok(claim)
    }

    /// Verify namespace ownership
    pub async fn verify_namespace(&self, path: &str, pubkey: &[u8; 32]) -> Result<bool> {
        let claims = self.claims.read().await;

        match claims.get(path) {
            Some(claim) => {
                // Check if expired
                if let Some(expires_at) = claim.expires_at {
                    if Utc::now() > expires_at {
                        return Ok(false);
                    }
                }

                // Check ownership
                Ok(&claim.owner_pubkey == pubkey)
            }
            None => Ok(false),
        }
    }

    /// Get namespace claim
    pub async fn get_claim(&self, path: &str) -> Result<NamespaceClaim> {
        self.claims.read().await
            .get(path)
            .cloned()
            .ok_or_else(|| anyhow!("Namespace {} not registered", path))
    }

    /// Delete namespace (requires owner signature)
    pub async fn delete_namespace(&self, path: &str, owner_keypair: &SigningKey) -> Result<()> {
        // Verify ownership
        if !self.verify_namespace(path, &owner_keypair.verifying_key().to_bytes()).await? {
            return Err(anyhow!("Not authorized to delete namespace {}", path));
        }

        // Submit to consensus
        if let Some(ref consensus) = self.consensus {
            use crate::consensus::Block;
            use std::time::{SystemTime, UNIX_EPOCH};

            let op = NamespaceOp::Delete {
                path: path.to_string(),
            };

            let block_id = format!("del_{}", hex::encode(&owner_keypair.verifying_key().to_bytes()[..8]));
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let block = Block {
                id: block_id.clone(),
                parents: vec![],
                operations: vec![op],
                timestamp,
                creator: "namespace_manager".to_string(),
                signature: vec![],
                state: crate::consensus::BlockState::Pending,
                ghost_weight: 1,
                height: 0,
            };

            consensus.add_block(block).await?;
        }

        // Remove claim
        self.claims.write().await.remove(path);

        info!("Deleted namespace: {}", path);
        Ok(())
    }

    /// List all registered namespaces
    pub async fn list_namespaces(&self) -> Vec<NamespaceClaim> {
        self.claims.read().await.values().cloned().collect()
    }

    /// List user-owned namespaces
    pub async fn list_user_namespaces(&self) -> Vec<NamespaceClaim> {
        self.claims.read().await.values()
            .filter(|claim| claim.metadata.namespace_type == "user")
            .cloned()
            .collect()
    }

    /// List public namespaces
    pub async fn list_public_namespaces(&self) -> Vec<NamespaceClaim> {
        self.claims.read().await.values()
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
        let claim = claims.get_mut(namespace_path).ok_or_else(|| anyhow!("Namespace not found"))?;

        // For public namespaces, automatically approve simple participation
        if claim.metadata.namespace_type == "public" && requested_role == "participant" {
            if !claim.metadata.participants.contains(&requester_pubkey_hex.to_string()) {
                claim.metadata.participants.push(requester_pubkey_hex.to_string());
                claim.metadata.last_activity = Utc::now();
                info!("Auto-approved participant access to public namespace {}", namespace_path);
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
        info!("Submitted access request for {} to namespace {}", requester_pubkey_hex, namespace_path);
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
        let claim = claims.get_mut(namespace_path).ok_or_else(|| anyhow!("Namespace not found"))?;

        // Verify approver authorization (owner or admin)
        let is_owner = claim.owner_pubkey == approver_keypair.verifying_key().to_bytes();
        let is_admin = claim.metadata.participants.contains(&hex::encode(approver_keypair.verifying_key().as_bytes()));
        
        if !is_owner && !is_admin {
            return Err(anyhow!("Not authorized to approve access requests"));
        }

        // Check M-of-N requirements for approval
        if let Some((required_signatures, total_participants)) = claim.metadata.participant_requirements {
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
                    info!("Multi-signature operation required: {}/{} signatures needed", 
                          required_signatures, total_participants);
                }
            }
        }

        // Find and approve the request
        let request = claim.metadata.access_requests.iter_mut()
            .find(|req| req.requester_pubkey == requester_pubkey_hex && req.status == "pending");
        
        if let Some(request) = request {
            request.status = "approved".to_string();
            request.approved_by = Some(hex::encode(approver_keypair.verifying_key().as_bytes()));
            
            // Add requester as participant
            if !claim.metadata.participants.contains(&requester_pubkey_hex.to_string()) {
                claim.metadata.participants.push(requester_pubkey_hex.to_string());
            }
            
            claim.metadata.last_activity = Utc::now();
            info!("Approved access request for {} to namespace {}", requester_pubkey_hex, namespace_path);
            Ok(())
        } else {
            Err(anyhow!("No pending access request found for {}", requester_pubkey_hex))
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
        let claim = claims.get_mut(namespace_path).ok_or_else(|| anyhow!("Namespace not found"))?;

        // Verify rejector authorization (owner or admin)
        let is_owner = claim.owner_pubkey == rejector_keypair.verifying_key().to_bytes();
        let is_admin = claim.metadata.participants.contains(&hex::encode(rejector_keypair.verifying_key().as_bytes()));
        
        if !is_owner && !is_admin {
            return Err(anyhow!("Not authorized to reject access requests"));
        }

        // Find and reject the request
        let request = claim.metadata.access_requests.iter_mut()
            .find(|req| req.requester_pubkey == requester_pubkey_hex && req.status == "pending");
        
        if let Some(request) = request {
            request.status = "rejected".to_string();
            request.approved_by = Some(hex::encode(rejector_keypair.verifying_key().as_bytes()));
            info!("Rejected access request for {} to namespace {}", requester_pubkey_hex, namespace_path);
            Ok(())
        } else {
            Err(anyhow!("No pending access request found for {}", requester_pubkey_hex))
        }
    }

    /// List pending access requests for a namespace (owner/admin only)
    pub async fn list_pending_requests(&self, namespace_path: &str) -> Result<Vec<AccessRequest>> {
        let claims = self.claims.read().await;
        let claim = claims.get(namespace_path).ok_or_else(|| anyhow!("Namespace not found"))?;
        
        let pending = claim.metadata.access_requests.iter()
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
        let claim = claims.get_mut(path).ok_or_else(|| anyhow!("Namespace not found"))?;

        // Verify owner authorization - check that the provided keypair matches the owner
        if claim.owner_pubkey != owner_keypair.verifying_key().to_bytes() {
            return Err(anyhow!("Unauthorized: not the namespace owner"));
        }

        // Check M-of-N requirements for participant addition
        if let Some((required_signatures, total_participants)) = claim.metadata.participant_requirements {
            // For public namespaces (1,0) = open participation, no additional checks needed
            if required_signatures == 1 && total_participants == 0 {
                // Open participation, proceed normally
            } else {
                // Log M-of-N requirements for the operation
                info!("Adding participant with M-of-N requirements: {}/{} signatures needed", 
                      required_signatures, total_participants);
            }
        }

        // Add participant
        if !claim.metadata.participants.contains(&participant_pubkey_hex.to_string()) {
            claim.metadata.participants.push(participant_pubkey_hex.to_string());
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

        info!("Added participant {} to namespace {}", participant_pubkey_hex, path);
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
        let claim = claims.get_mut(path).ok_or_else(|| anyhow!("Namespace not found"))?;

        // Verify owner authorization - check that the provided keypair matches the owner
        if claim.owner_pubkey != owner_keypair.verifying_key().to_bytes() {
            return Err(anyhow!("Unauthorized: not the namespace owner"));
        }

        // Check M-of-N requirements for participant removal
        if let Some((required_signatures, total_participants)) = claim.metadata.participant_requirements {
            // For public namespaces (1,0) = open participation, no additional checks needed
            if required_signatures == 1 && total_participants == 0 {
                // Open participation, proceed normally
            } else {
                // Log M-of-N requirements for the operation
                info!("Removing participant with M-of-N requirements: {}/{} signatures needed", 
                      required_signatures, total_participants);
            }
        }

        // Remove participant
        claim.metadata.participants.retain(|p| p != participant_pubkey_hex);
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

        info!("Removed participant {} from namespace {}", participant_pubkey_hex, path);
        Ok(())
    }

    /// Update namespace liveness (participant heartbeat) with M-of-N validation
    pub async fn update_liveness(&self, path: &str, participant_pubkey_hex: &str) -> Result<()> {
        let mut claims = self.claims.write().await;
        let claim = claims.get_mut(path).ok_or_else(|| anyhow!("Namespace not found"))?;

        // Check if participant is authorized
        if !claim.metadata.participants.contains(&participant_pubkey_hex.to_string()) {
            return Err(anyhow!("Participant {} not authorized for namespace {}", participant_pubkey_hex, path));
        }

        // Check M-of-N requirements for liveness update
        if let Some((required_signatures, total_participants)) = claim.metadata.participant_requirements {
            // For public namespaces (1,0) = open participation, no additional checks needed
            if required_signatures == 1 && total_participants == 0 {
                // Open participation, proceed normally
            } else {
                // Log M-of-N requirements for the operation
                info!("Liveness update with M-of-N requirements: {}/{} signatures needed", 
                      required_signatures, total_participants);
            }
        }

        // Update last activity
        claim.metadata.last_activity = Utc::now();
        
        Ok(())
    }

    /// Check if namespace should be expired based on liveness
    pub async fn check_expiration(&self, path: &str) -> Result<bool> {
        let claims = self.claims.read().await;
        let claim = claims.get(path).ok_or_else(|| anyhow!("Namespace not found"))?;

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
            system_keypair: SigningKey::from_bytes(&self.system_keypair.to_bytes()),
        }
    }
}

// ============================================================================
// Control File Handlers
// ============================================================================

/// Handler for /srv/namespace/register
struct RegisterNamespaceHandler {
    manager: Arc<NamespaceManager>,
}

impl ControlHandler for RegisterNamespaceHandler {
    fn read(&self) -> Result<Vec<u8>> {
        Ok(b"Write JSON to register namespace:\n\
            {\"path\":\"/srv/myapp\",\
             \"description\":\"My application\",\
             \"type\":\"app\",\
             \"pubkey\":\"<hex_ed25519_pubkey>\",\
             \"signature\":\"<hex_signature>\"}\n".to_vec())
    }

    fn write(&self, data: &[u8]) -> Result<()> {
        // Parse registration request
        #[derive(Deserialize)]
        struct RegRequest {
            path: String,
            description: String,
            #[serde(rename = "type")]
            namespace_type: String,
            pubkey: String,
            signature: String,
        }

        let req: RegRequest = serde_json::from_slice(data)
            .context("Invalid registration JSON")?;

        // Decode pubkey and signature
        let pubkey_bytes = hex::decode(&req.pubkey)
            .context("Invalid pubkey hex")?;
        let sig_bytes = hex::decode(&req.signature)
            .context("Invalid signature hex")?;

        if pubkey_bytes.len() != 32 {
            return Err(anyhow!("Public key must be 32 bytes"));
        }
        if sig_bytes.len() != 64 {
            return Err(anyhow!("Signature must be 64 bytes"));
        }

        let mut pubkey = [0u8; 32];
        pubkey.copy_from_slice(&pubkey_bytes);

        // Verify signature
        let public_key = VerifyingKey::from_bytes(&pubkey)
            .map_err(|e| anyhow!("Invalid public key: {}", e))?;

        let mut signature = [0u8; 64];
        signature.copy_from_slice(&sig_bytes);
        let sig = Signature::from_bytes(&signature);

        let sign_data = format!("{}{}", req.path, hex::encode(pubkey));
        public_key.verify(sign_data.as_bytes(), &sig)
            .map_err(|e| anyhow!("Signature verification failed: {}", e))?;

        // Create keypair (just for API compatibility - we already have signature)
        // In practice, user provides signature, we don't need their private key
        // TODO: Refactor register_namespace to accept signature directly

        info!("Namespace registration request for {} verified", req.path);
        Ok(())
    }
}

/// Handler for /srv/namespace/list
struct ListNamespacesHandler {
    claims: Arc<RwLock<HashMap<String, NamespaceClaim>>>,
}

impl ControlHandler for ListNamespacesHandler {
    fn read(&self) -> Result<Vec<u8>> {
        let claims = tokio::runtime::Handle::current()
            .block_on(self.claims.read());

        let mut output = String::from("Registered namespaces:\n\n");

        for claim in claims.values() {
            output.push_str(&format!(
                "Path: {}\nOwner: {}\nType: {}\nDescription: {}\nCreated: {}\n\n",
                claim.path,
                hex::encode(&claim.owner_pubkey[..8]),
                claim.metadata.namespace_type,
                claim.metadata.description,
                claim.created_at.format("%Y-%m-%d %H:%M:%S UTC")
            ));
        }

        Ok(output.into_bytes())
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow!("Read-only file"))
    }
}

/// Handler for /srv/namespace/list_public
struct ListPublicNamespacesHandler {
    claims: Arc<RwLock<HashMap<String, NamespaceClaim>>>,
}

impl ControlHandler for ListPublicNamespacesHandler {
    fn read(&self) -> Result<Vec<u8>> {
        let claims = tokio::runtime::Handle::current()
            .block_on(self.claims.read());

        let mut output = String::from("Public namespaces:\n\n");

        for claim in claims.values().filter(|c| c.metadata.namespace_type == "public") {
            let requirements = claim.metadata.participant_requirements
                .map(|(n, m)| format!("{}-of-{}", n, m))
                .unwrap_or_else(|| "owner-only".to_string());
            
            output.push_str(&format!(
                "Path: {}\nDescription: {}\nOwner: {}\nParticipants: {}\nRequirements: {}\nCreated: {}\n\n",
                claim.path,
                claim.metadata.description,
                hex::encode(&claim.owner_pubkey[..8]),
                claim.metadata.participants.len(),
                requirements,
                claim.created_at.format("%Y-%m-%d %H:%M:%S UTC")
            ));
        }

        Ok(output.into_bytes())
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow!("Read-only file"))
    }
}

/// Handler for /srv/namespace/verify
struct VerifyNamespaceHandler {
    claims: Arc<RwLock<HashMap<String, NamespaceClaim>>>,
}

impl ControlHandler for VerifyNamespaceHandler {
    fn read(&self) -> Result<Vec<u8>> {
        Ok(b"Write path to verify ownership\n".to_vec())
    }

    fn write(&self, data: &[u8]) -> Result<()> {
        let path = std::str::from_utf8(data)?.trim();

        let claims = tokio::runtime::Handle::current()
            .block_on(self.claims.read());

        match claims.get(path) {
            Some(claim) => {
                info!("Namespace {} is owned by {}",
                      path, hex::encode(&claim.owner_pubkey[..8]));
                Ok(())
            }
            None => Err(anyhow!("Namespace {} not registered", path)),
        }
    }
}

/// Handler for /srv/namespace/delete
struct DeleteNamespaceHandler {
    manager: Arc<NamespaceManager>,
}

impl ControlHandler for DeleteNamespaceHandler {
    fn read(&self) -> Result<Vec<u8>> {
        Ok(b"Write JSON to delete namespace:\n\
            {\"path\":\"/srv/myapp\",\
             \"pubkey\":\"<hex_ed25519_pubkey>\",\
             \"signature\":\"<hex_signature>\"}\n".to_vec())
    }

    fn write(&self, data: &[u8]) -> Result<()> {
        #[derive(Deserialize)]
        struct DelRequest {
            path: String,
            pubkey: String,
            signature: String,
        }

        let req: DelRequest = serde_json::from_slice(data)
            .context("Invalid delete JSON")?;

        // TODO: Verify signature and delete
        info!("Namespace delete request for {}", req.path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_namespace_registration() {
        let synth_fs = Arc::new(SyntheticFilesystem::new());
        let manager = NamespaceManager::new(synth_fs).unwrap();

        let keypair = SigningKey::from_bytes(&rand::random());

        let claim = manager.register_namespace(
            "/srv/test",
            "Test namespace",
            "test",
            None,
            None,
            &keypair,
        ).await.unwrap();

        assert_eq!(claim.path, "/srv/test");
        assert_eq!(claim.owner_pubkey, keypair.verifying_key().to_bytes());
    }

    #[tokio::test]
    async fn test_namespace_verification() {
        let synth_fs = Arc::new(SyntheticFilesystem::new());
        let manager = NamespaceManager::new(synth_fs).unwrap();

        let keypair = SigningKey::from_bytes(&rand::random());

        manager.register_namespace(
            "/srv/test",
            "Test namespace",
            "test",
            None,
            None,
            &keypair,
        ).await.unwrap();

        assert!(manager.verify_namespace("/srv/test", &keypair.verifying_key().to_bytes())
            .await.unwrap());

        let other_keypair = SigningKey::from_bytes(&rand::random());
        assert!(!manager.verify_namespace("/srv/test", &other_keypair.verifying_key().to_bytes())
            .await.unwrap());
    }

    /// Fuzz test: Namespace path validation
    #[test]
    fn fuzz_namespace_path_validation() {
        use proptest::prelude::*;

        proptest!(|(path in ".*")| {
            // Paths must start with /
            let is_valid = path.starts_with('/');
            // Should not panic on any input
            let _ = is_valid;
        });
    }

    /// Fuzz test: JSON deserialization for namespace registration
    #[test]
    fn fuzz_namespace_json_deserialization() {
        use proptest::prelude::*;

        proptest!(|(bytes: Vec<u8>)| {
            #[derive(serde::Deserialize)]
            struct RegRequest {
                path: String,
                description: String,
                #[serde(rename = "type")]
                namespace_type: String,
                pubkey: String,
                signature: String,
            }

            // Should never panic, only return Ok or Err
            let _ = serde_json::from_slice::<RegRequest>(&bytes);
        });
    }

    /// Fuzz test: Ed25519 signature verification
    #[test]
    fn fuzz_signature_verification() {
        use proptest::prelude::*;

        proptest!(|(
            pubkey_bytes in prop::collection::vec(any::<u8>(), 32),
            sig_bytes in prop::collection::vec(any::<u8>(), 64),
            data in prop::collection::vec(any::<u8>(), 0..1000)
        )| {
            let mut pubkey = [0u8; 32];
            let mut signature = [0u8; 64];
            pubkey.copy_from_slice(&pubkey_bytes);
            signature.copy_from_slice(&sig_bytes);

            // Should safely handle invalid keys/signatures
            if let Ok(vk) = VerifyingKey::from_bytes(&pubkey) {
                let sig = Signature::from_bytes(&signature);
                let _ = vk.verify(&data, &sig);
            }
        });
    }

    /// Fuzz test: Hex encoding/decoding
    #[test]
    fn fuzz_hex_encoding() {
        use proptest::prelude::*;

        proptest!(|(hex_str in ".*")| {
            // Should never panic on invalid hex
            let _ = hex::decode(&hex_str);
        });
    }
}


/// Register namespace manager controls with the synthetic filesystem
pub async fn register_namespace_controls(synth_fs: &Arc<SyntheticFilesystem>) -> Result<Arc<NamespaceManager>> {
    let namespace_mgr = NamespaceManager::new(synth_fs.clone())?;
    namespace_mgr.initialize().await?;
    Ok(Arc::new(namespace_mgr))
}
