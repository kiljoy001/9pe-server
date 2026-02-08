//! Sovereign identity system for autonomous compute nodes
//!
//! Each node generates its own cryptographic identity using ECC with:
//! - Ed25519 for signatures and work validation
//! - P-256 for TLS certificates  
//! - X25519 for key exchange
//!
//! Identities are distributed via DHT for peer discovery and authentication.

use anyhow::Result;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use p256::{NistP256, PublicKey as P256PublicKey, SecretKey as P256SecretKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::{Duration, SystemTime};
use tracing::{debug, info};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

/// Unique node identifier derived from public key
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub String);

impl NodeId {
    pub fn new(hex_str: String) -> Self {
        Self(hex_str)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Sovereign node identity with all cryptographic keys
#[derive(Clone)]
pub struct SovereignIdentity {
    /// Node identifier (hash of Ed25519 public key)
    pub node_id: NodeId,

    /// Ed25519 key pair for signatures and HMAC validation
    pub ed25519_key: SigningKey,
    pub ed25519_public: VerifyingKey,

    /// P-256 key pair for TLS certificates
    pub p256_key: P256SecretKey,
    pub p256_public: P256PublicKey,

    /// X25519 key pair for key exchange
    pub x25519_key: StaticSecret,
    pub x25519_public: X25519PublicKey,

    /// Self-signed X.509 certificate for TLS
    pub certificate: Vec<u8>,
    pub private_key_der: Vec<u8>,

    /// Creation timestamp
    pub created_at: SystemTime,

    /// Node-controlled permissions and capabilities
    pub permissions: NodePermissions,
}

impl std::fmt::Debug for SovereignIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SovereignIdentity")
            .field("node_id", &self.node_id)
            .field("ed25519_key", &"REDACTED")
            .field("ed25519_public", &self.ed25519_public)
            .field("p256_key", &"REDACTED")
            .field("p256_public", &self.p256_public)
            .field("x25519_key", &"REDACTED")
            .field("x25519_public", &self.x25519_public)
            .field("certificate", &"REDACTED")
            .field("created_at", &self.created_at)
            .field("permissions", &self.permissions)
            .finish()
    }
}

impl SovereignIdentity {
    /// Generate a new sovereign identity for this node
    pub fn generate() -> Result<Self> {
        let permissions = NodePermissions::owner_defaults();
        Self::generate_with_permissions(permissions)
    }

    pub fn generate_with_permissions(permissions: NodePermissions) -> Result<Self> {
        info!("Generating new sovereign identity");

        // Generate Ed25519 key pair (for signatures/HMAC)
        let mut secret_bytes = [0u8; 32];
        use rand::RngCore;
        OsRng.fill_bytes(&mut secret_bytes);
        let secret = ed25519_dalek::SecretKey::from(secret_bytes);
        let ed25519_key = SigningKey::from_bytes(&secret);
        let ed25519_public = VerifyingKey::from(&ed25519_key);

        // Generate P-256 key pair (for TLS certificates)
        let p256_key = P256SecretKey::random(&mut OsRng);
        let p256_public = P256PublicKey::from_secret_scalar(&p256_key.to_nonzero_scalar());

        // Generate X25519 key pair (for key exchange)
        let x25519_key = StaticSecret::random_from_rng(&mut OsRng);
        let x25519_public = X25519PublicKey::from(&x25519_key);

        // Derive node ID from Ed25519 public key
        let node_id_bytes = ed25519_public.to_bytes();
        let node_id_hex = hex::encode(&node_id_bytes);
        let node_id = NodeId::new(node_id_hex);

        info!("Generated identity with NodeID: {}", node_id.as_str());

        // Generate self-signed certificate
        let (cert_der, key_der) = Self::generate_certificate(&node_id)?;

        Ok(Self {
            node_id,
            ed25519_key,
            ed25519_public,
            p256_key,
            p256_public,
            x25519_key,
            x25519_public,
            certificate: cert_der,
            private_key_der: key_der,
            created_at: SystemTime::now(),
            permissions,
        })
    }

    /// Generate self-signed X.509 certificate
    fn generate_certificate(node_id: &NodeId) -> Result<(Vec<u8>, Vec<u8>)> {
        use rcgen::{Certificate, CertificateParams, DistinguishedName, DnType};

        let mut params = CertificateParams::new(vec![node_id.as_str().to_string()]);
        params.distinguished_name = DistinguishedName::new();
        params
            .distinguished_name
            .push(DnType::CommonName, node_id.as_str());
        params.not_before = rcgen::date_time_ymd(2026, 1, 1);
        params.not_after = rcgen::date_time_ymd(2036, 1, 1); // 10-year validity

        let cert = Certificate::from_params(params)?;
        let cert_der = cert.serialize_der()?;
        let key_der = cert.serialize_private_key_der();

        debug!("Generated self-signed certificate for {}", node_id.as_str());
        Ok((cert_der, key_der))
    }

    /// Sign data with the Ed25519 key for work validation/HMAC
    pub fn sign(&self, data: &[u8]) -> Signature {
        self.ed25519_key.sign(data)
    }

    /// Verify signature using the Ed25519 public key
    pub fn verify(&self, data: &[u8], signature: &Signature) -> bool {
        self.ed25519_public.verify(data, signature).is_ok()
    }

    /// Get certificate chain for QUIC/TLS
    pub fn certificate_chain(&self) -> Vec<Vec<u8>> {
        vec![self.certificate.clone()]
    }

    /// Get private key for QUIC/TLS
    pub fn private_key(&self) -> &[u8] {
        &self.private_key_der
    }

    pub fn p256_public_key_bytes(&self) -> Vec<u8> {
        self.p256_public.to_sec1_bytes().to_vec()
    }
}

/// Node permissions for sovereign authorization
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodePermissions {
    pub can_submit_jobs: bool,
    pub can_monitor_resources: bool,
    pub can_view_logs: bool,
    pub max_concurrent_jobs: u32,
    pub max_gpu_hours_per_month: u64,
    pub allowed_compute_types: Vec<String>,
    pub network_scope: NetworkScope,
    pub expires_at: Option<SystemTime>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkScope {
    pub cluster_name: String,
    pub allowed_networks: Vec<String>,
    pub blocked_networks: Vec<String>,
}

impl NodePermissions {
    pub fn owner_defaults() -> Self {
        Self {
            can_submit_jobs: true,
            can_monitor_resources: true,
            can_view_logs: true,
            max_concurrent_jobs: 16,
            max_gpu_hours_per_month: 720,
            allowed_compute_types: vec!["sycl".to_string()],
            network_scope: NetworkScope {
                cluster_name: "default".to_string(),
                allowed_networks: Vec::new(),
                blocked_networks: Vec::new(),
            },
            expires_at: None,
        }
    }
}

/// HMAC-based work receipt for job validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkReceipt {
    /// Hash of the work data (SHA-256)
    pub job_hash: [u8; 32],

    /// Node's signature over the job hash
    pub signature: Vec<u8>,

    /// Node ID that produced this work  
    pub node_id: NodeId,

    /// Timestamp when work was completed
    pub timestamp: u64,

    /// Optional results data (compressed, encrypted)
    pub results: Vec<u8>,
}

impl WorkReceipt {
    /// Create a new work receipt for completed job
    pub fn new(job_data: &[u8], results: Vec<u8>, identity: &SovereignIdentity) -> Result<Self> {
        use sha2::{Digest, Sha256};

        // Hash the job data
        let mut hasher = Sha256::new();
        hasher.update(job_data);
        let job_hash: [u8; 32] = hasher.finalize().into();

        // Sign the hash
        let signature = identity.sign(&job_hash);

        Ok(Self {
            job_hash,
            signature: signature.to_bytes().to_vec(),
            node_id: identity.node_id.clone(),
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_secs(),
            results,
        })
    }

    /// Verify work receipt signature against a public key
    pub fn verify_signature(&self, public_key: &VerifyingKey) -> bool {
        if let Ok(signature) = Signature::try_from(&self.signature[..]) {
            public_key.verify(&self.job_hash, &signature).is_ok()
        } else {
            false
        }
    }
}

/// DHT record for node discovery and service advertisement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DhtRecord {
    /// Node identifier
    pub node_id: NodeId,

    /// Ed25519 public key for identity verification
    pub public_key: Vec<u8>,

    /// P-256 public key for TLS certificate verification
    pub p256_public_key: Vec<u8>,

    /// Self-signed certificate DER for TLS pinning
    pub certificate_der: Vec<u8>,

    /// Node-managed permissions
    pub permissions: NodePermissions,

    /// Optional human-friendly name for this node
    pub node_name: Option<String>,

    /// Hash of address@node_name for fast lookup
    pub node_name_hash: Vec<u8>,

    /// Network address for direct connection
    pub network_addr: SocketAddr,

    /// Services this node provides
    pub services: std::collections::HashMap<String, ServiceInfo>,

    /// Node capabilities (CPU, GPU, memory, etc.)
    pub capabilities: NodeCapabilities,

    /// Timestamp of last update
    pub timestamp: u64,
}

/// Information about a service provided by a node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceInfo {
    /// Mount point or service endpoint
    pub mount_point: String,

    /// Service capabilities (compute, storage, etc.)
    pub service_type: String,

    /// Resource requirements and capabilities
    pub capabilities: ServiceCapabilities,
}

/// Node hardware/software capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeCapabilities {
    pub has_gpu: bool,
    pub gpu_type: Option<String>,
    pub cpu_cores: Option<u32>,
    pub memory_gb: Option<u32>,
    pub storage_gb: Option<u32>,
    pub supported_operations: Vec<String>,
}

/// Service-specific capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServiceCapabilities {
    pub max_parallel_jobs: Option<u32>,
    pub memory_requirements_mb: Option<u32>,
    pub gpu_required: bool,
    pub specialized_hardware: Vec<String>,
    pub capability_flags: u16,
}

/// Capability flags for compute jobs (matches UUID v8 encoding)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u16)]
pub enum Capability {
    None = 0,
    BasicCompute = 1 << 0,
    HighPrecision = 1 << 1,
    TensorOperations = 1 << 2,
    SharedMemory = 1 << 3,
    PrivilegedAccess = 1 << 15,
}

impl Capability {
    pub fn from_u16(val: u16) -> Self {
        match val {
            0 => Capability::None,
            1 => Capability::BasicCompute,
            _ => Capability::BasicCompute, // Default to basic
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sovereign_identity_generation() {
        let identity = SovereignIdentity::generate().expect("Failed to generate identity");

        // Verify all keys are present
        assert!(!identity.node_id.as_str().is_empty());
        assert!(!identity.certificate.is_empty());
        assert!(!identity.private_key_der.is_empty());

        // Test signing/verification
        let test_data = b"test work data";
        let signature = identity.sign(test_data);
        assert!(identity.verify(test_data, &signature));
    }

    #[test]
    fn test_work_receipt() {
        let identity = SovereignIdentity::generate().expect("Failed to generate identity");
        let job_data = b"compute job payload";
        let results = vec![1, 2, 3, 4, 5];

        let receipt = WorkReceipt::new(job_data, results.clone(), &identity)
            .expect("Failed to create work receipt");

        // Verify receipt
        assert_eq!(receipt.results, results);
        assert!(receipt.verify_signature(&identity.ed25519_public));
    }
}
