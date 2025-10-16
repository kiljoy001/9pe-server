//! Cryptographic primitives for GHOSTDAG consensus
//!
//! Provides cryptographic security for distributed work coordination,
//! including signatures, key management, and secure communication.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Cryptographic provider trait for consensus operations
#[async_trait]
pub trait CryptoProvider: Send + Sync {
    /// Sign data with the node's private key
    async fn sign(&self, data: &[u8]) -> Result<Signature>;

    /// Verify a signature against a public key
    async fn verify(
        &self,
        data: &[u8],
        signature: &Signature,
        public_key: &PublicKey,
    ) -> Result<bool>;

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
    #[allow(dead_code)]
    private_key: PrivateKey,
    public_key: PublicKey,
}

impl Ed25519Provider {
    pub fn new() -> Result<Self> {
        // In a real implementation, we'd use ed25519-dalek or similar
        // For now, mock implementation
        let private_key = PrivateKey {
            algorithm: "Ed25519".to_string(),
            key_data: vec![0u8; 32], // Mock private key
        };

        let public_key = PublicKey {
            algorithm: "Ed25519".to_string(),
            key_data: vec![1u8; 32], // Mock public key
        };

        Ok(Self {
            private_key,
            public_key,
        })
    }

    pub fn from_seed(seed: &[u8]) -> Result<Self> {
        if seed.len() != 32 {
            anyhow::bail!("Ed25519 seed must be 32 bytes");
        }

        // In real implementation, derive keypair from seed
        let mut private_key_data = vec![0u8; 32];
        private_key_data.copy_from_slice(seed);

        let private_key = PrivateKey {
            algorithm: "Ed25519".to_string(),
            key_data: private_key_data,
        };

        // Derive public key from private key
        let mut public_key_data = vec![0u8; 32];
        public_key_data[0] = seed[0]; // Mock derivation

        let public_key = PublicKey {
            algorithm: "Ed25519".to_string(),
            key_data: public_key_data,
        };

        Ok(Self {
            private_key,
            public_key,
        })
    }
}

#[async_trait]
impl CryptoProvider for Ed25519Provider {
    async fn sign(&self, data: &[u8]) -> Result<Signature> {
        // Mock signature generation
        let mut signature_data = vec![0u8; 64];
        signature_data[0] = data.len() as u8; // Simple mock

        Ok(Signature {
            algorithm: "Ed25519".to_string(),
            data: signature_data,
        })
    }

    async fn verify(
        &self,
        data: &[u8],
        signature: &Signature,
        public_key: &PublicKey,
    ) -> Result<bool> {
        if signature.algorithm != "Ed25519" || public_key.algorithm != "Ed25519" {
            return Ok(false);
        }

        let _ = data;
        // Mock verification - in real implementation, use ed25519-dalek
        Ok(signature.data.len() == 64 && !signature.data.iter().all(|&b| b == 0))
    }

    fn get_public_key(&self) -> PublicKey {
        self.public_key.clone()
    }

    async fn generate_keypair(&self) -> Result<(PublicKey, PrivateKey)> {
        // Generate new random keypair
        let private_key = PrivateKey {
            algorithm: "Ed25519".to_string(),
            key_data: (0..32).map(|_| rand::random::<u8>()).collect(),
        };

        let public_key = PublicKey {
            algorithm: "Ed25519".to_string(),
            key_data: (0..32).map(|_| rand::random::<u8>()).collect(),
        };

        Ok((public_key, private_key))
    }

    async fn encrypt(&self, data: &[u8], _recipient_key: &PublicKey) -> Result<Vec<u8>> {
        // Mock encryption - in real implementation, use X25519 + ChaCha20Poly1305
        let mut encrypted = data.to_vec();
        for byte in &mut encrypted {
            *byte ^= 0xAA; // Simple XOR for demo
        }
        Ok(encrypted)
    }

    async fn decrypt(&self, encrypted_data: &[u8]) -> Result<Vec<u8>> {
        // Mock decryption
        let mut decrypted = encrypted_data.to_vec();
        for byte in &mut decrypted {
            *byte ^= 0xAA; // Reverse XOR
        }
        Ok(decrypted)
    }

    async fn derive_shared_secret(&self, _other_public_key: &PublicKey) -> Result<SharedSecret> {
        // Mock shared secret derivation
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

    pub async fn verify_and_decrypt(&self, crypto: &dyn CryptoProvider) -> Result<Vec<u8>> {
        // Decrypt payload first
        let payload = crypto.decrypt(&self.encrypted_payload).await?;

        // Reconstruct signed data
        let mut sign_data = payload.clone();
        sign_data.extend_from_slice(&self.timestamp.to_le_bytes());
        sign_data.extend_from_slice(&self.nonce);

        // Verify signature
        let valid = crypto
            .verify(&sign_data, &self.signature, &self.sender)
            .await?;
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
        if self.trusted_keys.remove(node_id).is_some() {
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

        self.trusted_keys
            .get(node_id)
            .map(|key| key == public_key)
            .unwrap_or(false)
    }

    pub fn get_trusted_keys(&self) -> &HashMap<String, PublicKey> {
        &self.trusted_keys
    }

    pub fn get_key(&self, node_id: &str) -> Option<PublicKey> {
        self.trusted_keys.get(node_id).cloned()
    }
}

impl PublicKey {
    pub fn from_hex<S: AsRef<str>>(algorithm: String, key_hex: S) -> Result<Self> {
        let bytes = hex::decode(key_hex.as_ref()).map_err(|e| {
            anyhow!(
                "Invalid {} public key (hex decode failed): {}",
                algorithm,
                e
            )
        })?;
        Ok(Self {
            algorithm,
            key_data: bytes,
        })
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

        let computation_proof = ComputationProof::HashProof {
            input_hash,
            output_hash,
            steps: computation_steps,
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

        let signature_valid = crypto
            .verify(&sign_data, &self.node_signature, node_public_key)
            .await?;
        if !signature_valid {
            return Ok(false);
        }

        // Verify computation proof
        match &self.computation_proof {
            ComputationProof::HashProof { input_hash, .. } => {
                let expected_input_hash = sha256_hash(expected_input);
                Ok(*input_hash == expected_input_hash)
            }
            ComputationProof::ZkProof { .. } => {
                // Placeholder for ZK proof verification
                Ok(true)
            }
            ComputationProof::MerkleProof { .. } => {
                // Placeholder for Merkle proof verification
                Ok(true)
            }
        }
    }

    pub fn matches_output(&self, output_data: &[u8]) -> bool {
        self.result_hash == sha256_hash(output_data)
    }
}

// Helper function for SHA-256 hashing
fn sha256_hash(data: &[u8]) -> Vec<u8> {
    // In real implementation, use sha2 crate
    // For now, simple mock hash
    let mut hash = vec![0u8; 32];
    hash[0] = data.len() as u8;
    if !data.is_empty() {
        hash[1] = data[0];
        hash[31] = data[data.len() - 1];
    }
    hash
}
