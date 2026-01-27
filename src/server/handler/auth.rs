//! CBOR-based authentication file helpers for Tauth/Rauth.

use anyhow::Result;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::identity::NodePermissions;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthChallenge {
    pub nonce: [u8; 32],
    pub server_node_id: String,
    pub required_scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    pub node_id: String,
    pub ed25519_pub: [u8; 32],
    pub p256_pub: Vec<u8>,
    pub cert_der: Vec<u8>,
    pub permissions: NodePermissions,
    pub signature: [u8; 64],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuthResponseUnsigned {
    node_id: String,
    ed25519_pub: [u8; 32],
    p256_pub: Vec<u8>,
    cert_der: Vec<u8>,
    permissions: NodePermissions,
}

pub fn encode_auth_challenge(challenge: &AuthChallenge) -> Result<Vec<u8>> {
    Ok(serde_cbor::to_vec(challenge)?)
}

pub fn decode_auth_response(data: &[u8]) -> Result<AuthResponse> {
    Ok(serde_cbor::from_slice(data)?)
}

pub fn verify_auth_response(
    challenge: &AuthChallenge,
    response: &AuthResponse,
) -> Result<NodePermissions> {
    let unsigned = AuthResponseUnsigned {
        node_id: response.node_id.clone(),
        ed25519_pub: response.ed25519_pub,
        p256_pub: response.p256_pub.clone(),
        cert_der: response.cert_der.clone(),
        permissions: response.permissions.clone(),
    };

    let challenge_bytes = serde_cbor::to_vec(challenge)?;
    let response_bytes = serde_cbor::to_vec(&unsigned)?;

    let mut hasher = Sha256::new();
    hasher.update(&challenge_bytes);
    hasher.update(&response_bytes);
    let digest = hasher.finalize();

    let verifying_key = VerifyingKey::from_bytes(&response.ed25519_pub)
        .map_err(|_| anyhow::anyhow!("Invalid Ed25519 public key"))?;
    let signature = Signature::from_bytes(&response.signature)?;

    if verifying_key.verify(&digest, &signature).is_err() {
        anyhow::bail!("Invalid auth response signature");
    }

    Ok(response.permissions.clone())
}

