use anyhow::{Context, Result};
use ed25519_dalek::{SigningKey, VerifyingKey, Signer, Verifier, Signature};
use rand::{rngs::OsRng, RngCore};
use std::sync::Arc;

/// Provider for Ed25519 cryptographic operations
#[derive(Clone)]
pub struct Ed25519Provider {
    keypair: Arc<SigningKey>,
}

impl Ed25519Provider {
    /// Create a new Ed25519 provider with a generated keypair
    pub fn new() -> Result<Self> {
        let mut csprng = OsRng;
        let mut key_bytes = [0u8; 32];
        csprng.fill_bytes(&mut key_bytes);
        let secret = ed25519_dalek::SecretKey::from(key_bytes);
        let keypair = SigningKey::from_bytes(&secret);
        Ok(Self {
            keypair: Arc::new(keypair),
        })
    }

    /// Sign data using the stored private key
    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        self.keypair.sign(message).to_bytes().to_vec()
    }

    /// Verify a signature against a public key
    pub fn verify(&self, public_key: &[u8], message: &[u8], signature: &[u8]) -> Result<bool> {
        if public_key.len() != 32 || signature.len() != 64 {
            return Ok(false);
        }

        let verifying_key = VerifyingKey::from_bytes(public_key.try_into().unwrap())
            .map_err(|_| anyhow::anyhow!("Invalid public key"))?;
        
        let signature_obj = Signature::from_bytes(signature.try_into().unwrap());
        
        match verifying_key.verify(message, &signature_obj) {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// Get the public key bytes
    pub fn public_key(&self) -> Vec<u8> {
        self.keypair.verifying_key().to_bytes().to_vec()
    }
}
