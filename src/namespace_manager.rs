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
use sha2::{Sha256, Digest};
use ed25519_dalek::{SigningKey, VerifyingKey, Signature, Signer, Verifier};
use tracing::{info, warn, error};

use crate::consensus::{BoundedGhostdag, NamespaceOp};
use crate::synth::{SyntheticFilesystem, SynthNode, SynthNodeType, ControlHandler};

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
pub struct NamespaceMetadata {
    /// Human-readable description
    pub description: String,

    /// Namespace type (compute, storage, ai, etc.)
    pub namespace_type: String,

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

        Ok(())
    }

    /// Register built-in system namespaces
    async fn register_system_namespaces(&self) -> Result<()> {
        // Register /srv/compute namespace for compute pool
        self.register_namespace(
            "/srv/compute",
            "Distributed compute pool for GPU/CPU resources",
            "compute",
            None,
            &self.system_keypair,
        ).await?;

        // Register /srv/namespace itself
        self.register_namespace(
            "/srv/namespace",
            "Namespace registration and management",
            "system",
            None,
            &self.system_keypair,
        ).await?;

        // Register /srv/settrans for translator management
        self.register_namespace(
            "/srv/settrans",
            "Translator installation and management",
            "system",
            None,
            &self.system_keypair,
        ).await?;

        Ok(())
    }

    /// Register a new namespace with cryptographic ownership
    pub async fn register_namespace(
        &self,
        path: &str,
        description: &str,
        namespace_type: &str,
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

        // Create claim
        let created_at = Utc::now();
        let metadata = NamespaceMetadata {
            description: description.to_string(),
            namespace_type: namespace_type.to_string(),
            custom: HashMap::new(),
        };

        // Create signature over claim data
        let sign_data = format!("{}{}{}",
            path,
            hex::encode(owner_keypair.verifying_key().as_bytes()),
            created_at.timestamp()
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

#[async_trait::async_trait]
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

        let sign_data = format!("{}{}", req.path, hex::encode(&pubkey));
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

#[async_trait::async_trait]
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

/// Handler for /srv/namespace/verify
struct VerifyNamespaceHandler {
    claims: Arc<RwLock<HashMap<String, NamespaceClaim>>>,
}

#[async_trait::async_trait]
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

#[async_trait::async_trait]
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
            &keypair,
        ).await.unwrap();

        assert!(manager.verify_namespace("/srv/test", &keypair.verifying_key().to_bytes())
            .await.unwrap());

        let other_keypair = SigningKey::from_bytes(&rand::random());
        assert!(!manager.verify_namespace("/srv/test", &other_keypair.verifying_key().to_bytes())
            .await.unwrap());
    }
}
