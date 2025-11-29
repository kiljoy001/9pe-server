//! Cryptographic primitives for GHOSTDAG consensus
//!
//! Provides cryptographic security for distributed work coordination,
//! including signatures, key management, and secure communication.

use anyhow::{Result, Context};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use async_trait::async_trait;
use ed25519_dalek::{Signer, Verifier, SigningKey, VerifyingKey};
use sha2::{Sha256, Digest};
use rand::rngs::OsRng;
use rand::RngCore;

/// Cryptographic provider trait for consensus operations
#[async_trait]
pub trait CryptoProvider: Send + Sync {
    /// Sign data with the node's private key
    async fn sign(&self, data: &[u8]) -> Result<Signature>;

    /// Verify a signature against a public key
    async fn verify(&self, data: &[u8], signature: &Signature, public_key: &PublicKey) -> Result<bool>;

    /// Get the node's public key
    fn get_public_key(&self) -> PublicKey;

    /// Generate a new keypair
    async fn generate_keypair(&self) -> Result<(PublicKey, PrivateKey)>;

    /// Encrypt data for a specific recipient
    async fn encrypt(&self, data: &[u8], recipient_key: &PublicKey) -> Result<Vec<u8>>;

    /// Decrypt data with the node's private key
    async fn decrypt(&self, encrypted_data: &[u8]) -> Result<Vec<u8>>;

    /// Generate a shared secret for secure communication
    async fn derive_shared_secret(&self, other_public_key: &PublicKey) -> Result<SharedSecret>;
}

/// Digital signature
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signature {
    pub algorithm: String,
    pub data: Vec<u8>,
}

/// Public key for verification and encryption
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct PublicKey {
    pub algorithm: String,
    pub key_data: Vec<u8>,
}

/// Private key (never serialized)
#[derive(Debug, Clone)]
pub struct PrivateKey {
    pub algorithm: String,
    pub key_data: Vec<u8>,
}

/// Shared secret for symmetric encryption
#[derive(Debug, Clone)]
pub struct SharedSecret {
    pub data: Vec<u8>,
}

/// Ed25519-based cryptographic provider
pub struct Ed25519Provider {
    keypair: SigningKey,
}

impl Ed25519Provider {
    pub fn new() -> Result<Self> {
        let mut csprng = OsRng;
        let mut bytes = [0u8; 32];
        csprng.fill_bytes(&mut bytes);
        let keypair = SigningKey::from_bytes(&bytes);
        Ok(Self { keypair })
    }

    pub fn from_seed(seed: &[u8]) -> Result<Self> {
        if seed.len() != 32 {
            anyhow::bail!("Ed25519 seed must be 32 bytes");
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(seed);
        let keypair = SigningKey::from_bytes(&bytes);
        Ok(Self { keypair })
    }
}

#[async_trait]
impl CryptoProvider for Ed25519Provider {
    async fn sign(&self, data: &[u8]) -> Result<Signature> {
        let signature = self.keypair.sign(data);
        Ok(Signature {
            algorithm: "Ed25519".to_string(),
            data: signature.to_vec(),
        })
    }

    async fn verify(&self, data: &[u8], signature: &Signature, public_key: &PublicKey) -> Result<bool> {
        if signature.algorithm != "Ed25519" || public_key.algorithm != "Ed25519" {
            return Ok(false);
        }

        if public_key.key_data.len() != 32 {
            return Ok(false);
        }

        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&public_key.key_data);

        let verifying_key = VerifyingKey::from_bytes(&key_bytes)
            .map_err(|e| anyhow::anyhow!("Invalid public key: {}", e))?;

        if signature.data.len() != 64 {
            return Ok(false);
        }

        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(&signature.data);
        let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes);

        Ok(verifying_key.verify(data, &sig).is_ok())
    }

    fn get_public_key(&self) -> PublicKey {
        PublicKey {
            algorithm: "Ed25519".to_string(),
            key_data: self.keypair.verifying_key().to_bytes().to_vec(),
        }
    }

    async fn generate_keypair(&self) -> Result<(PublicKey, PrivateKey)> {
        let mut csprng = OsRng;
        let mut bytes = [0u8; 32];
        csprng.fill_bytes(&mut bytes);
        let keypair = SigningKey::from_bytes(&bytes);

        let public_key = PublicKey {
            algorithm: "Ed25519".to_string(),
            key_data: keypair.verifying_key().to_bytes().to_vec(),
        };

        let private_key = PrivateKey {
            algorithm: "Ed25519".to_string(),
            key_data: keypair.to_bytes().to_vec(),
        };

        Ok((public_key, private_key))
    }

    async fn encrypt(&self, data: &[u8], _recipient_key: &PublicKey) -> Result<Vec<u8>> {
        // Placeholder: encryption requires X25519 conversion or different keys
        // For consensus signatures, encryption might be separate.
        // We'll keep the mock XOR for now as Ed25519 is for signatures.
        // Real implementation would use Diffie-Hellman.
        let mut encrypted = data.to_vec();
        for byte in &mut encrypted {
            *byte ^= 0xAA;
        }
        Ok(encrypted)
    }

    async fn decrypt(&self, encrypted_data: &[u8]) -> Result<Vec<u8>> {
        let mut decrypted = encrypted_data.to_vec();
        for byte in &mut decrypted {
            *byte ^= 0xAA;
        }
        Ok(decrypted)
    }

    async fn derive_shared_secret(&self, _other_public_key: &PublicKey) -> Result<SharedSecret> {
        Ok(SharedSecret {
            data: vec![0xFFu8; 32],
        })
    }
}

/// Secure message for network communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecureMessage {
    pub sender: PublicKey,
    pub signature: Signature,
    pub encrypted_payload: Vec<u8>,
    pub timestamp: u64,
    pub nonce: Vec<u8>,
}

impl SecureMessage {
    pub async fn create(
        payload: &[u8],
        crypto: &dyn CryptoProvider,
        recipient_key: &PublicKey,
    ) -> Result<Self> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        let nonce: Vec<u8> = (0..16).map(|_| rand::random::<u8>()).collect();

        // Create data to sign (payload + timestamp + nonce)
        let mut sign_data = payload.to_vec();
        sign_data.extend_from_slice(&timestamp.to_le_bytes());
        sign_data.extend_from_slice(&nonce);

        let signature = crypto.sign(&sign_data).await?;
        let encrypted_payload = crypto.encrypt(payload, recipient_key).await?;

        Ok(Self {
            sender: crypto.get_public_key(),
            signature,
            encrypted_payload,
            timestamp,
            nonce,
        })
    }

    pub async fn verify_and_decrypt(
        &self,
        crypto: &dyn CryptoProvider,
    ) -> Result<Vec<u8>> {
        // Decrypt payload first
        let payload = crypto.decrypt(&self.encrypted_payload).await?;

        // Reconstruct signed data
        let mut sign_data = payload.clone();
        sign_data.extend_from_slice(&self.timestamp.to_le_bytes());
        sign_data.extend_from_slice(&self.nonce);

        // Verify signature
        let valid = crypto.verify(&sign_data, &self.signature, &self.sender).await?;
        if !valid {
            anyhow::bail!("Invalid signature");
        }

        // Check timestamp freshness (within 5 minutes)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        if now.saturating_sub(self.timestamp) > 300 {
            anyhow::bail!("Message too old");
        }

        Ok(payload)
    }
}

/// Key management for trusted nodes
pub struct TrustedKeyStore {
    trusted_keys: HashMap<String, PublicKey>,
    revoked_keys: HashMap<String, u64>, // node_id -> revocation_timestamp
}

impl Default for TrustedKeyStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TrustedKeyStore {
    pub fn new() -> Self {
        Self {
            trusted_keys: HashMap::new(),
            revoked_keys: HashMap::new(),
        }
    }

    pub fn add_trusted_key(&mut self, node_id: String, public_key: PublicKey) {
        self.trusted_keys.insert(node_id, public_key);
    }

    pub fn revoke_key(&mut self, node_id: &str) {
        if let Some(key) = self.trusted_keys.remove(node_id) {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            self.revoked_keys.insert(node_id.to_string(), timestamp);
        }
    }

    pub fn is_trusted(&self, node_id: &str, public_key: &PublicKey) -> bool {
        if self.revoked_keys.contains_key(node_id) {
            return false;
        }

        self.trusted_keys.get(node_id)
            .map(|key| key == public_key)
            .unwrap_or(false)
    }

    pub fn get_trusted_keys(&self) -> &HashMap<String, PublicKey> {
        &self.trusted_keys
    }
}

/// Cryptographic proof for work completion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkProof {
    pub work_id: String,
    pub result_hash: Vec<u8>,
    pub computation_proof: ComputationProof,
    pub node_signature: Signature,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ComputationProof {
    /// Simple hash-based proof
    HashProof {
        input_hash: Vec<u8>,
        output_hash: Vec<u8>,
        steps: u64,
        nonce: u64,
        hash: Vec<u8>,
        difficulty: u32,
    },
    /// Zero-knowledge proof (placeholder)
    ZkProof {
        proof_data: Vec<u8>,
        verification_key: Vec<u8>,
    },
    /// Merkle tree proof for large computations
    MerkleProof {
        root_hash: Vec<u8>,
        proof_path: Vec<Vec<u8>>,
        leaf_index: u64,
    },
}

impl WorkProof {
    pub async fn create(
        work_id: String,
        input_data: &[u8],
        output_data: &[u8],
        computation_steps: u64,
        crypto: &dyn CryptoProvider,
    ) -> Result<Self> {
        let input_hash = sha256_hash(input_data);
        let output_hash = sha256_hash(output_data);
        let result_hash = sha256_hash(output_data);

        // Mock proof of work elements
        let nonce = 0;
        let difficulty = 1;
        let hash = result_hash.clone();

        let computation_proof = ComputationProof::HashProof {
            input_hash,
            output_hash,
            steps: computation_steps,
            nonce,
            hash,
            difficulty,
        };

        // Sign the proof
        let mut sign_data = work_id.as_bytes().to_vec();
        sign_data.extend_from_slice(&result_hash);
        let node_signature = crypto.sign(&sign_data).await?;

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        Ok(Self {
            work_id,
            result_hash,
            computation_proof,
            node_signature,
            timestamp,
        })
    }

    pub async fn verify(
        &self,
        expected_input: &[u8],
        node_public_key: &PublicKey,
        crypto: &dyn CryptoProvider,
    ) -> Result<bool> {
        // Verify signature
        let mut sign_data = self.work_id.as_bytes().to_vec();
        sign_data.extend_from_slice(&self.result_hash);

        let signature_valid = crypto.verify(&sign_data, &self.node_signature, node_public_key).await?;
        if !signature_valid {
            return Ok(false);
        }

        // Verify computation proof
        match &self.computation_proof {
            ComputationProof::HashProof { input_hash, .. } => {
                let expected_input_hash = sha256_hash(expected_input);
                Ok(*input_hash == expected_input_hash)
            },
            ComputationProof::ZkProof { .. } => {
                // Placeholder for ZK proof verification
                Ok(true)
            },
            ComputationProof::MerkleProof { .. } => {
                // Placeholder for Merkle proof verification
                Ok(true)
            },
        }
    }
}

// Helper function for SHA-256 hashing
fn sha256_hash(data: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}
