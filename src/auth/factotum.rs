//! Plan 9 Factotum Authentication Support
//!
//! Factotum is Plan 9's authentication agent. It holds keys and responds to
//! authentication challenges on behalf of users, enabling single sign-on.
//!
//! This module implements the client side of the factotum protocol, allowing
//! the 9P.e server to delegate authentication to a factotum agent.
//!
//! ## Protocol Overview
//!
//! 1. Server generates a challenge (random nonce)
//! 2. Client sends challenge to factotum along with server identity
//! 3. Factotum returns a ticket (signed/encrypted response)
//! 4. Client sends ticket to server
//! 5. Server verifies ticket using shared secret
//!
//! ## Usage
//!
//! Configure factotum in the auth config:
//! ```toml
//! [auth]
//! auth_method = "Factotum"  # or "Both" for fallback
//!
//! [auth.factotum]
//! address = "/mnt/factotum/rpc"  # Unix socket path
//! auth_dom = "mynetwork"          # Authentication domain
//! ```

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::time::{timeout, Duration};
use tracing::{debug, warn};

use super::FactotumConfig;

/// Factotum RPC message types
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum FactotumOp {
    /// Start authentication
    AuthStart = 0,
    /// Read challenge
    AuthRead = 1,
    /// Write response
    AuthWrite = 2,
    /// Authentication complete
    AuthOk = 3,
    /// Authentication failed
    AuthErr = 4,
    /// Get attribute
    AttrRead = 5,
    /// Set attribute
    AttrWrite = 6,
}

/// Authentication challenge sent to client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactotumChallenge {
    /// Random challenge nonce (32 bytes)
    pub challenge: [u8; 32],
    /// Server's authentication domain
    pub auth_dom: String,
    /// Server's host ID (for the client to verify)
    pub host_id: String,
    /// Timestamp to prevent replay attacks
    pub timestamp: u64,
}

/// Authentication ticket returned by factotum
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactotumTicket {
    /// Username being authenticated
    pub username: String,
    /// Authentication domain
    pub auth_dom: String,
    /// Challenge response (HMAC of challenge with user's key)
    pub response: [u8; 32],
    /// Ticket timestamp
    pub timestamp: u64,
    /// Optional capabilities granted by factotum
    pub capabilities: Vec<String>,
    /// Ticket signature (HMAC of above fields with shared secret)
    pub signature: [u8; 32],
}

/// Factotum client for communicating with the auth agent
pub struct FactotumClient {
    config: FactotumConfig,
}

impl FactotumClient {
    /// Create a new factotum client
    pub fn new(config: FactotumConfig) -> Self {
        Self { config }
    }

    /// Check if factotum is available
    pub async fn is_available(&self) -> bool {
        if self.config.address.starts_with('/') {
            // Unix socket
            Path::new(&self.config.address).exists()
        } else {
            // TCP address - try to connect
            match tokio::net::TcpStream::connect(&self.config.address).await {
                Ok(_) => true,
                Err(_) => false,
            }
        }
    }

    /// Generate a challenge for authentication
    pub fn generate_challenge(&self, host_id: &str) -> FactotumChallenge {
        use rand::RngCore;
        let mut challenge = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut challenge);

        FactotumChallenge {
            challenge,
            auth_dom: self.config.auth_dom.clone(),
            host_id: host_id.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    /// Verify a ticket from factotum
    pub fn verify_ticket(
        &self,
        challenge: &FactotumChallenge,
        ticket: &FactotumTicket,
    ) -> Result<()> {
        // Check timestamp (allow 5 minute window)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if ticket.timestamp < challenge.timestamp.saturating_sub(300) {
            bail!("Ticket timestamp too old");
        }
        if ticket.timestamp > now + 300 {
            bail!("Ticket timestamp in the future");
        }

        // Check auth domain matches
        if ticket.auth_dom != challenge.auth_dom {
            bail!("Auth domain mismatch");
        }

        // Verify signature if we have a shared secret
        if let Some(ref secret_b64) = self.config.auth_secret {
            let secret = base64_decode(secret_b64)
                .context("Invalid auth_secret base64")?;

            let expected_sig = self.compute_ticket_signature(ticket, &secret);
            if !constant_time_eq(&ticket.signature, &expected_sig) {
                bail!("Invalid ticket signature");
            }

            // Verify challenge response
            let expected_response = self.compute_challenge_response(
                &challenge.challenge,
                &ticket.username,
                &secret,
            );
            if !constant_time_eq(&ticket.response, &expected_response) {
                bail!("Invalid challenge response");
            }
        } else {
            // No shared secret - we trust the factotum connection itself
            // This is less secure but allows for key-per-user setups
            warn!("No auth_secret configured - trusting factotum connection");
        }

        Ok(())
    }

    /// Request authentication from factotum (client-side)
    /// This is called by the client to get a ticket
    pub async fn request_auth(
        &self,
        challenge: &FactotumChallenge,
        username: &str,
    ) -> Result<FactotumTicket> {
        let timeout_duration = Duration::from_millis(self.config.timeout_ms);

        if self.config.address.starts_with('/') {
            // Unix socket connection
            self.request_auth_unix(challenge, username, timeout_duration).await
        } else {
            // TCP connection
            self.request_auth_tcp(challenge, username, timeout_duration).await
        }
    }

    async fn request_auth_unix(
        &self,
        challenge: &FactotumChallenge,
        username: &str,
        timeout_duration: Duration,
    ) -> Result<FactotumTicket> {
        let stream = timeout(
            timeout_duration,
            UnixStream::connect(&self.config.address),
        )
        .await
        .context("Factotum connection timeout")?
        .context("Failed to connect to factotum")?;

        self.do_auth_protocol(stream, challenge, username, timeout_duration).await
    }

    async fn request_auth_tcp(
        &self,
        challenge: &FactotumChallenge,
        username: &str,
        timeout_duration: Duration,
    ) -> Result<FactotumTicket> {
        let stream = timeout(
            timeout_duration,
            tokio::net::TcpStream::connect(&self.config.address),
        )
        .await
        .context("Factotum connection timeout")?
        .context("Failed to connect to factotum")?;

        self.do_auth_protocol(stream, challenge, username, timeout_duration).await
    }

    async fn do_auth_protocol<S>(
        &self,
        mut stream: S,
        challenge: &FactotumChallenge,
        username: &str,
        timeout_duration: Duration,
    ) -> Result<FactotumTicket>
    where
        S: AsyncReadExt + AsyncWriteExt + Unpin,
    {
        // Send start message
        // Format: "start proto=p9any role=client"
        let start_msg = format!(
            "start proto=p9any role=client dom={} user={}\n",
            challenge.auth_dom, username
        );

        timeout(timeout_duration, stream.write_all(start_msg.as_bytes()))
            .await
            .context("Write timeout")?
            .context("Failed to send start")?;

        // Read response
        let mut response_buf = vec![0u8; 4096];
        let n = timeout(timeout_duration, stream.read(&mut response_buf))
            .await
            .context("Read timeout")?
            .context("Failed to read response")?;

        let response = String::from_utf8_lossy(&response_buf[..n]);
        debug!("Factotum response: {}", response);

        if response.starts_with("error") || response.starts_with("!") {
            bail!("Factotum error: {}", response);
        }

        // Send challenge
        let challenge_msg = format!(
            "write {}\n",
            hex::encode(&challenge.challenge)
        );
        timeout(timeout_duration, stream.write_all(challenge_msg.as_bytes()))
            .await
            .context("Write timeout")?
            .context("Failed to send challenge")?;

        // Read ticket
        let n = timeout(timeout_duration, stream.read(&mut response_buf))
            .await
            .context("Read timeout")?
            .context("Failed to read ticket")?;

        // Parse ticket from response
        self.parse_ticket_response(&response_buf[..n], username, challenge)
    }

    fn parse_ticket_response(
        &self,
        data: &[u8],
        username: &str,
        challenge: &FactotumChallenge,
    ) -> Result<FactotumTicket> {
        // Try to parse as our ticket format first
        if let Ok(ticket) = serde_cbor::from_slice::<FactotumTicket>(data) {
            return Ok(ticket);
        }

        // Try to parse as Plan 9 native format
        // Format: "ok" followed by hex-encoded response
        let response_str = String::from_utf8_lossy(data);
        if response_str.starts_with("ok ") {
            let hex_response = response_str[3..].trim();
            if hex_response.len() >= 64 {
                let response_bytes = hex::decode(&hex_response[..64])
                    .context("Invalid hex in ticket")?;
                let mut response = [0u8; 32];
                response.copy_from_slice(&response_bytes);

                // Generate signature locally since Plan 9 factotum doesn't provide one
                let mut signature = [0u8; 32];
                if let Some(ref secret_b64) = self.config.auth_secret {
                    if let Ok(secret) = base64_decode(secret_b64) {
                        signature = self.compute_ticket_signature_parts(
                            username,
                            &challenge.auth_dom,
                            &response,
                            challenge.timestamp,
                            &secret,
                        );
                    }
                }

                return Ok(FactotumTicket {
                    username: username.to_string(),
                    auth_dom: challenge.auth_dom.clone(),
                    response,
                    timestamp: challenge.timestamp,
                    capabilities: Vec::new(),
                    signature,
                });
            }
        }

        bail!("Failed to parse factotum ticket response")
    }

    fn compute_ticket_signature(&self, ticket: &FactotumTicket, secret: &[u8]) -> [u8; 32] {
        self.compute_ticket_signature_parts(
            &ticket.username,
            &ticket.auth_dom,
            &ticket.response,
            ticket.timestamp,
            secret,
        )
    }

    fn compute_ticket_signature_parts(
        &self,
        username: &str,
        auth_dom: &str,
        response: &[u8; 32],
        timestamp: u64,
        secret: &[u8],
    ) -> [u8; 32] {
        use sha2::{Sha256, Digest};

        let mut hasher = Sha256::new();
        hasher.update(secret);
        hasher.update(username.as_bytes());
        hasher.update(auth_dom.as_bytes());
        hasher.update(response);
        hasher.update(&timestamp.to_le_bytes());

        let result = hasher.finalize();
        let mut sig = [0u8; 32];
        sig.copy_from_slice(&result);
        sig
    }

    fn compute_challenge_response(
        &self,
        challenge: &[u8; 32],
        username: &str,
        secret: &[u8],
    ) -> [u8; 32] {
        use sha2::{Sha256, Digest};

        let mut hasher = Sha256::new();
        hasher.update(secret);
        hasher.update(challenge);
        hasher.update(username.as_bytes());

        let result = hasher.finalize();
        let mut response = [0u8; 32];
        response.copy_from_slice(&result);
        response
    }
}

/// Constant-time comparison to prevent timing attacks
fn constant_time_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut result = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        result |= x ^ y;
    }
    result == 0
}

/// Decode base64 (simple implementation)
fn base64_decode(s: &str) -> Result<Vec<u8>> {
    // Simple base64 decode without external dependency
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let s = s.trim_end_matches('=');
    let mut result = Vec::with_capacity(s.len() * 3 / 4);

    let mut buffer = 0u32;
    let mut bits = 0;

    for c in s.bytes() {
        let val = ALPHABET.iter().position(|&x| x == c)
            .ok_or_else(|| anyhow::anyhow!("Invalid base64 character"))? as u32;

        buffer = (buffer << 6) | val;
        bits += 6;

        if bits >= 8 {
            bits -= 8;
            result.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_challenge_generation() {
        let config = FactotumConfig::default();
        let client = FactotumClient::new(config);

        let challenge = client.generate_challenge("testhost");

        assert_eq!(challenge.auth_dom, "9pe");
        assert_eq!(challenge.host_id, "testhost");
        assert!(challenge.timestamp > 0);
        // Challenge should be random (not all zeros)
        assert!(challenge.challenge.iter().any(|&b| b != 0));
    }

    #[test]
    fn test_ticket_verification_no_secret() {
        let config = FactotumConfig {
            auth_secret: None,
            ..Default::default()
        };
        let client = FactotumClient::new(config);

        let challenge = client.generate_challenge("testhost");
        let ticket = FactotumTicket {
            username: "testuser".to_string(),
            auth_dom: "9pe".to_string(),
            response: [0u8; 32],
            timestamp: challenge.timestamp,
            capabilities: Vec::new(),
            signature: [0u8; 32],
        };

        // Without secret, verification should pass (trusts connection)
        assert!(client.verify_ticket(&challenge, &ticket).is_ok());
    }

    #[test]
    fn test_ticket_verification_with_secret() {
        let secret = "dGVzdHNlY3JldA=="; // "testsecret" in base64
        let config = FactotumConfig {
            auth_secret: Some(secret.to_string()),
            ..Default::default()
        };
        let client = FactotumClient::new(config);

        let challenge = client.generate_challenge("testhost");

        // Compute valid response and signature
        let secret_bytes = base64_decode(secret).unwrap();
        let response = client.compute_challenge_response(
            &challenge.challenge,
            "testuser",
            &secret_bytes,
        );
        let signature = client.compute_ticket_signature_parts(
            "testuser",
            "9pe",
            &response,
            challenge.timestamp,
            &secret_bytes,
        );

        let ticket = FactotumTicket {
            username: "testuser".to_string(),
            auth_dom: "9pe".to_string(),
            response,
            timestamp: challenge.timestamp,
            capabilities: Vec::new(),
            signature,
        };

        assert!(client.verify_ticket(&challenge, &ticket).is_ok());
    }

    #[test]
    fn test_ticket_verification_bad_signature() {
        let secret = "dGVzdHNlY3JldA==";
        let config = FactotumConfig {
            auth_secret: Some(secret.to_string()),
            ..Default::default()
        };
        let client = FactotumClient::new(config);

        let challenge = client.generate_challenge("testhost");
        let ticket = FactotumTicket {
            username: "testuser".to_string(),
            auth_dom: "9pe".to_string(),
            response: [0u8; 32],
            timestamp: challenge.timestamp,
            capabilities: Vec::new(),
            signature: [1u8; 32], // Wrong signature
        };

        assert!(client.verify_ticket(&challenge, &ticket).is_err());
    }

    #[test]
    fn test_ticket_verification_wrong_domain() {
        let config = FactotumConfig::default();
        let client = FactotumClient::new(config);

        let challenge = client.generate_challenge("testhost");
        let ticket = FactotumTicket {
            username: "testuser".to_string(),
            auth_dom: "wrongdomain".to_string(), // Wrong domain
            response: [0u8; 32],
            timestamp: challenge.timestamp,
            capabilities: Vec::new(),
            signature: [0u8; 32],
        };

        assert!(client.verify_ticket(&challenge, &ticket).is_err());
    }

    #[test]
    fn test_constant_time_eq() {
        let a = [1u8; 32];
        let b = [1u8; 32];
        let c = [2u8; 32];

        assert!(constant_time_eq(&a, &b));
        assert!(!constant_time_eq(&a, &c));
    }

    #[test]
    fn test_base64_decode() {
        assert_eq!(base64_decode("dGVzdA==").unwrap(), b"test");
        assert_eq!(base64_decode("aGVsbG8gd29ybGQ=").unwrap(), b"hello world");
        assert!(base64_decode("!!!").is_err());
    }
}
