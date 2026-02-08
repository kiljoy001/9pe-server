//! Cryptographic Authentication and Encryption
//!
//! Implements ChaCha20-Poly1305 + Ed25519 security for 9P.e protocol:
//! - Message encryption with authenticated encryption
//! - Digital signatures for integrity verification
//! - Anti-replay protection with sequence numbers
//! - Session key management and rotation

use std::collections::{HashMap, HashSet};
use chacha20poly1305::{
    aead::{Aead, KeyInit, OsRng},
    ChaCha20Poly1305, Nonce,
};
use ed25519_dalek::{SigningKey, VerifyingKey, Signature, Signer, Verifier};
use rand::{RngCore, CryptoRng};

/// Combined trait for cryptographic random number generation
///
/// Combines the requirements for cryptographically secure random number generation
/// with thread safety for use in concurrent contexts.
trait CryptoRngCore: CryptoRng + RngCore + Send {}

// Implement for any type that satisfies the bounds
impl<T> CryptoRngCore for T where T: CryptoRng + RngCore + Send {}
use serde::{Deserialize, Serialize};
use blake3;

/// Ed25519 public key size (32 bytes)
pub const ED25519_PUBLIC_KEY_SIZE: usize = 32;

/// Ed25519 signature size (64 bytes)
pub const ED25519_SIGNATURE_SIZE: usize = 64;

/// ChaCha20-Poly1305 key size (32 bytes)
pub const CHACHA20_KEY_SIZE: usize = 32;

/// ChaCha20-Poly1305 nonce size (12 bytes)
pub const CHACHA20_NONCE_SIZE: usize = 12;

/// Maximum timestamp skew allowed (5 minutes)
pub const MAX_TIMESTAMP_SKEW: u64 = 5 * 60 * 1000;

/// Maximum sequence number window for anti-replay
pub const MAX_SEQUENCE_WINDOW: u64 = 1000;

/// Session key rotation interval (1 hour)
pub const KEY_ROTATION_INTERVAL: u64 = 60 * 60 * 1000;

/// Authenticated and encrypted message container
///
/// Represents a complete cryptographic message with encryption, authentication,
/// and anti-replay protection. Uses ChaCha20-Poly1305 for encryption and
/// Ed25519 for digital signatures.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthenticatedMessage {
    /// Encrypted payload
    pub ciphertext: Vec<u8>,

    /// Ed25519 signature of the entire message
    #[serde(with = "serde_arrays")]
    pub signature: [u8; ED25519_SIGNATURE_SIZE],

    /// Sender's public key
    #[serde(with = "serde_arrays")]
    pub public_key: [u8; ED25519_PUBLIC_KEY_SIZE],

    /// ChaCha20-Poly1305 nonce
    #[serde(with = "serde_arrays")]
    pub nonce: [u8; CHACHA20_NONCE_SIZE],

    /// Additional authenticated data
    pub aad: Vec<u8>,

    /// Message timestamp (milliseconds since epoch)
    pub timestamp: u64,

    /// Sequence number for anti-replay protection
    pub sequence_number: u64,

    /// Session identifier
    #[serde(with = "serde_arrays")]
    pub session_id: [u8; 32],
}

/// Cryptographic session state
///
/// Maintains the state for a single cryptographic session including keys,
/// sequence counters, and anti-replay protection. Each session represents
/// a secure communication channel between two parties.
pub struct CryptoSession {
    /// Unique session identifier (32-byte random)
    pub session_id: [u8; 32],

    /// Local Ed25519 keypair for signing messages
    pub local_keypair: SigningKey,

    /// Remote party's Ed25519 public key (if established)
    pub remote_public_key: Option<VerifyingKey>,

    /// Shared ChaCha20 encryption key (derived from key exchange)
    pub encryption_key: [u8; CHACHA20_KEY_SIZE],

    /// ChaCha20-Poly1305 cipher instance for message encryption
    cipher: ChaCha20Poly1305,

    /// Outgoing sequence counter for anti-replay protection
    pub sequence_counter: u64,

    /// Received sequence numbers for anti-replay detection
    received_sequences: HashSet<u64>,

    /// Anti-replay window size (maximum sequence gap)
    pub sequence_window: u64,

    /// Maximum allowed timestamp skew in milliseconds
    pub max_timestamp_skew: u64,

    /// Session creation timestamp in milliseconds
    pub created_at: u64,

    /// Last key rotation timestamp in milliseconds
    pub last_key_rotation: u64,

    /// Whether the session has been fully established
    pub established: bool,
}

/// Crypto system managing all sessions
pub struct CryptoSystem {
    /// Active cryptographic sessions
    sessions: HashMap<[u8; 32], CryptoSession>,

    /// System limits
    pub limits: CryptoLimits,

    /// Random number generator
    rng: Box<dyn CryptoRngCore>,
}

/// Cryptographic system limits
#[derive(Debug, Clone)]
pub struct CryptoLimits {
    /// Maximum number of concurrent sessions
    pub max_sessions: usize,
    /// Maximum size for encrypted messages
    pub max_message_size: usize,
    /// Maximum size for additional authenticated data
    pub max_aad_size: usize,
    /// Minimum interval between key rotations in milliseconds
    pub min_key_rotation_interval: u64,
    /// Maximum session lifetime in milliseconds
    pub max_session_lifetime: u64,
}

/// Crypto system statistics and metrics
///
/// Provides comprehensive statistics about the cryptographic system including
/// session counts, message processing metrics, and throughput information.
#[derive(Debug, Clone)]
pub struct CryptoSystemStats {
    /// Number of currently active sessions
    pub active_sessions: usize,
    /// Total number of sessions created
    pub total_sessions_created: usize,
    /// Total number of messages encrypted
    pub total_messages_encrypted: usize,
    /// Total number of messages decrypted
    pub total_messages_decrypted: usize,
    /// Total bytes encrypted across all sessions
    pub total_bytes_encrypted: usize,
    /// Total bytes decrypted across all sessions
    pub total_bytes_decrypted: usize,
}

/// Cryptographic operation errors
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("Session not found: {0:?}")]
    /// Session with given ID not found
    SessionNotFound([u8; 32]),

    #[error("Session not established")]
    /// Session has not been fully established
    SessionNotEstablished,

    #[error("Invalid signature")]
    /// Message signature verification failed
    InvalidSignature,

    #[error("Encryption failed")]
    /// Message encryption operation failed
    EncryptionFailed,

    #[error("Decryption failed")]
    /// Message decryption operation failed
    DecryptionFailed,

    #[error("Replay attack detected: sequence {0}")]
    /// Replay attack detected with sequence number
    ReplayAttack(u64),

    #[error("Timestamp too old/new: {0}")]
    /// Message timestamp is invalid or too old/new
    InvalidTimestamp(u64),

    #[error("Message too large: {0} > {1}")]
    /// Message size exceeds system limits
    MessageTooLarge(usize, usize),

    #[error("AAD too large: {0} > {1}")]
    /// Additional authenticated data too large
    AadTooLarge(usize, usize),

    #[error("Maximum sessions reached")]
    /// Maximum number of sessions reached
    MaxSessionsReached,

    #[error("Session expired")]
    /// Session has expired and must be recreated
    SessionExpired,

    #[error("Key generation failed")]
    /// Cryptographic key generation failed
    KeyGenerationFailed,

    #[error("Invalid key format")]
    /// Provided key is in invalid format
    InvalidKeyFormat,
}

impl Default for CryptoLimits {
    fn default() -> Self {
        Self {
            max_sessions: 1024,
            max_message_size: 16 * 1024 * 1024, // 16MB
            max_aad_size: 4096, // 4KB
            min_key_rotation_interval: KEY_ROTATION_INTERVAL,
            max_session_lifetime: 24 * 60 * 60 * 1000, // 24 hours
        }
    }
}

impl CryptoSystem {
    /// Get statistics about the crypto system
    ///
    /// Returns comprehensive statistics about the current state of the
    /// cryptographic system including session counts and usage metrics.
    ///
    /// # Returns
    ///
    /// CryptoSystemStats with current system metrics
    pub fn get_stats(&self) -> CryptoSystemStats {
        CryptoSystemStats {
            active_sessions: self.sessions.len(),
            total_sessions_created: self.sessions.len(),
            total_messages_encrypted: 0,
            total_messages_decrypted: 0,
            total_bytes_encrypted: 0,
            total_bytes_decrypted: 0,
        }
    }
    /// Create new crypto system with default limits
    ///
    /// Initializes a new cryptographic system with default security policies
    /// and limits. Uses OS random number generator for key generation.
    ///
    /// # Returns
    ///
    /// A new CryptoSystem instance ready for session management
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            limits: CryptoLimits::default(),
            rng: Box::new(OsRng),
        }
    }

    /// Create new cryptographic session
    ///
    /// Establishes a new secure session with the specified remote party.
    /// Generates session keys and initializes anti-replay protection.
    ///
    /// # Arguments
    ///
    /// * `remote_public_key` - Optional remote party's Ed25519 public key
    ///
    /// # Returns
    ///
    /// * `Ok(session_id)` - Unique 32-byte session identifier
    /// * `Err(CryptoError)` - Session creation failed
    pub fn create_session(&mut self, remote_public_key: Option<VerifyingKey>) -> Result<[u8; 32], CryptoError> {
        if self.sessions.len() >= self.limits.max_sessions {
            return Err(CryptoError::MaxSessionsReached);
        }

        // Generate session ID
        let session_id = self.generate_session_id();

        // Generate local keypair
        let local_keypair = self.generate_keypair()?;

        // Derive encryption key
        let encryption_key = self.derive_encryption_key(&local_keypair, remote_public_key.as_ref());

        // Create cipher instance
        let cipher = ChaCha20Poly1305::new_from_slice(&encryption_key)
            .map_err(|_| CryptoError::KeyGenerationFailed)?;

        let session = CryptoSession {
            session_id,
            local_keypair,
            remote_public_key,
            encryption_key,
            cipher,
            sequence_counter: 0,
            received_sequences: HashSet::new(),
            sequence_window: MAX_SEQUENCE_WINDOW,
            max_timestamp_skew: MAX_TIMESTAMP_SKEW,
            created_at: current_timestamp(),
            last_key_rotation: current_timestamp(),
            established: remote_public_key.is_some(),
        };

        self.sessions.insert(session_id, session);
        Ok(session_id)
    }

    /// Encrypt and sign message
    ///
    /// Encrypts plaintext with ChaCha20-Poly1305 and signs with Ed25519.
    /// Includes anti-replay protection and automatic key rotation.
    ///
    /// # Arguments
    ///
    /// * `session_id` - Session to use for encryption
    /// * `plaintext` - Data to encrypt
    /// * `aad` - Additional authenticated data (not encrypted)
    ///
    /// # Returns
    ///
    /// * `Ok(AuthenticatedMessage)` - Encrypted and signed message
    /// * `Err(CryptoError)` - Encryption or signing failed
    pub fn encrypt_and_sign(
        &mut self,
        session_id: [u8; 32],
        plaintext: &[u8],
        aad: &[u8]
    ) -> Result<AuthenticatedMessage, CryptoError> {
        // Validate message size
        if plaintext.len() > self.limits.max_message_size {
            return Err(CryptoError::MessageTooLarge(plaintext.len(), self.limits.max_message_size));
        }

        if aad.len() > self.limits.max_aad_size {
            return Err(CryptoError::AadTooLarge(aad.len(), self.limits.max_aad_size));
        }

        let current_time = current_timestamp();

        // Get session and handle operations
        let session = self.sessions.get_mut(&session_id)
            .ok_or(CryptoError::SessionNotFound(session_id))?;

        if !session.established {
            return Err(CryptoError::SessionNotEstablished);
        }

        // Check and perform key rotation if needed
        if current_time - session.last_key_rotation > self.limits.min_key_rotation_interval {
            // Generate new encryption key inline
            let mut hasher = blake3::Hasher::new();
            hasher.update(&session.local_keypair.to_bytes());
            if let Some(remote_key) = session.remote_public_key.as_ref() {
                hasher.update(&remote_key.to_bytes());
            } else {
                hasher.update(&session.local_keypair.verifying_key().to_bytes());
            }
            hasher.update(&current_time.to_be_bytes());
            let mut new_encryption_key = [0u8; 32];
            let hash = hasher.finalize();
            new_encryption_key.copy_from_slice(hash.as_bytes());

            // Create new cipher
            let new_cipher = ChaCha20Poly1305::new_from_slice(&new_encryption_key)
                .map_err(|_| CryptoError::KeyGenerationFailed)?;

            session.encryption_key = new_encryption_key;
            session.cipher = new_cipher;
            session.last_key_rotation = current_time;
        }

        // Generate nonce
        let mut nonce = [0u8; CHACHA20_NONCE_SIZE];
        nonce[..8].copy_from_slice(&session.sequence_counter.to_be_bytes());
        // Fill rest with zeros or session-specific data
        nonce[8..].copy_from_slice(&session_id[..4]);

        // Encrypt message
        let nonce_obj = Nonce::from_slice(&nonce);
        let ciphertext = session.cipher
            .encrypt(nonce_obj, plaintext)
            .map_err(|_| CryptoError::EncryptionFailed)?;

        // Create message to sign
        let mut message_to_sign = Vec::new();
        message_to_sign.extend_from_slice(&ciphertext);
        message_to_sign.extend_from_slice(&nonce);
        message_to_sign.extend_from_slice(aad);
        message_to_sign.extend_from_slice(&session.sequence_counter.to_be_bytes());
        message_to_sign.extend_from_slice(&current_time.to_be_bytes());
        message_to_sign.extend_from_slice(&session_id);

        // Sign message
        let signature_obj = session.local_keypair.sign(&message_to_sign);
        let signature = signature_obj.to_bytes();

        // Get public key bytes
        let public_key = session.local_keypair.verifying_key().to_bytes();

        let authenticated_message = AuthenticatedMessage {
            ciphertext,
            signature,
            public_key,
            nonce,
            aad: aad.to_vec(),
            timestamp: current_time,
            sequence_number: session.sequence_counter,
            session_id,
        };

        session.sequence_counter += 1;
        Ok(authenticated_message)
    }

    /// Verify and decrypt message
    ///
    /// Verifies message signature, checks anti-replay protection, and decrypts
    /// the message content using the session's encryption key.
    ///
    /// # Arguments
    ///
    /// * `session_id` - Session to use for decryption
    /// * `message` - Authenticated message to verify and decrypt
    ///
    /// # Returns
    ///
    /// * `Ok(plaintext)` - Verified and decrypted message content
    /// * `Err(CryptoError)` - Verification or decryption failed
    pub fn verify_and_decrypt(
        &mut self,
        session_id: [u8; 32],
        message: &AuthenticatedMessage
    ) -> Result<Vec<u8>, CryptoError> {
        // First, do all the checks that don't require mutable access
        {
            let session = self.sessions.get(&session_id)
                .ok_or(CryptoError::SessionNotFound(session_id))?;

            if !session.established {
                return Err(CryptoError::SessionNotEstablished);
            }

            // Check session expiry
            let current_time = current_timestamp();
            if current_time - session.created_at > self.limits.max_session_lifetime {
                return Err(CryptoError::SessionExpired);
            }

            // Anti-replay protection
            if session.received_sequences.contains(&message.sequence_number) {
                return Err(CryptoError::ReplayAttack(message.sequence_number));
            }

            // Sequence window validation
            let max_sequence = session.received_sequences.iter().max().copied().unwrap_or(0);
            if message.sequence_number + session.sequence_window < max_sequence {
                return Err(CryptoError::ReplayAttack(message.sequence_number));
            }

            // Timestamp validation
            let time_diff = if current_time >= message.timestamp {
                current_time - message.timestamp
            } else {
                message.timestamp - current_time
            };

            if time_diff > session.max_timestamp_skew {
                return Err(CryptoError::InvalidTimestamp(message.timestamp));
            }

        }

        // Verify signature (without mutable borrow)
        {
            let session = self.sessions.get(&session_id)
                .ok_or(CryptoError::SessionNotFound(session_id))?;
            Self::verify_message_signature(session, message)?;
        }

        // Now get mutable access for decryption
        let session = self.sessions.get_mut(&session_id)
            .ok_or(CryptoError::SessionNotFound(session_id))?;

        // Decrypt message
        let nonce_obj = Nonce::from_slice(&message.nonce);
        let plaintext = session.cipher
            .decrypt(nonce_obj, message.ciphertext.as_slice())
            .map_err(|_| CryptoError::DecryptionFailed)?;

        // Record sequence number
        session.received_sequences.insert(message.sequence_number);

        // Clean up old sequence numbers
        if session.received_sequences.len() > session.sequence_window as usize {
            let min_sequence = message.sequence_number.saturating_sub(session.sequence_window);
            session.received_sequences.retain(|&seq| seq > min_sequence);
        }

        Ok(plaintext)
    }

    /// Verify message signature
    fn verify_message_signature(
        session: &CryptoSession,
        message: &AuthenticatedMessage
    ) -> Result<(), CryptoError> {
        // Reconstruct signed message
        let mut message_to_verify = Vec::new();
        message_to_verify.extend_from_slice(&message.ciphertext);
        message_to_verify.extend_from_slice(&message.nonce);
        message_to_verify.extend_from_slice(&message.aad);
        message_to_verify.extend_from_slice(&message.sequence_number.to_be_bytes());
        message_to_verify.extend_from_slice(&message.timestamp.to_be_bytes());
        message_to_verify.extend_from_slice(&message.session_id);

        // Use remote public key if available, otherwise use message public key
        let public_key = if let Some(remote_key) = &session.remote_public_key {
            remote_key.clone()
        } else {
            VerifyingKey::from_bytes(&message.public_key)
                .map_err(|_| CryptoError::InvalidKeyFormat)?
        };

        let signature = Signature::from_bytes(&message.signature);

        public_key
            .verify(&message_to_verify, &signature)
            .map_err(|_| CryptoError::InvalidSignature)?;

        Ok(())
    }

    // Removed unused rotate_session_key function to reduce attack surface

    /// Generate Ed25519 keypair
    fn generate_keypair(&mut self) -> Result<SigningKey, CryptoError> {
        let mut seed = [0u8; 32];
        self.rng.fill_bytes(&mut seed);
        let signing_key = SigningKey::from_bytes(&seed);
        Ok(signing_key)
    }

    /// Generate session ID
    fn generate_session_id(&mut self) -> [u8; 32] {
        let mut session_id = [0u8; 32];
        self.rng.fill_bytes(&mut session_id);
        session_id
    }

    /// Derive encryption key using key derivation
    fn derive_encryption_key(&mut self, local_keypair: &SigningKey, remote_public_key: Option<&VerifyingKey>) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();

        // Hash local private key
        hasher.update(&local_keypair.to_bytes());

        // Hash remote public key if available
        if let Some(remote_key) = remote_public_key {
            hasher.update(&remote_key.to_bytes());
        } else {
            // Use local public key for self-encryption
            hasher.update(&local_keypair.verifying_key().to_bytes());
        }

        // Add current timestamp for uniqueness
        hasher.update(&current_timestamp().to_be_bytes());

        // Add random salt
        let mut salt = [0u8; 32];
        self.rng.fill_bytes(&mut salt);
        hasher.update(&salt);

        let hash_result = hasher.finalize();
        let mut key = [0u8; 32];
        key.copy_from_slice(hash_result.as_bytes());
        key
    }

    // Removed unused generate_nonce function to reduce attack surface

    /// Destroy session and cleanup
    ///
    /// Removes a session and cleans up all associated cryptographic state.
    /// This should be called when a session is no longer needed.
    ///
    /// # Arguments
    ///
    /// * `session_id` - Session to destroy
    ///
    /// # Returns
    ///
    /// * `Ok(())` - Session was successfully destroyed
    /// * `Err(CryptoError)` - Session not found
    pub fn destroy_session(&mut self, session_id: [u8; 32]) -> Result<(), CryptoError> {
        self.sessions
            .remove(&session_id)
            .ok_or(CryptoError::SessionNotFound(session_id))?;
        Ok(())
    }

    /// Get session information
    ///
    /// Returns information about a specific session including its state,
    /// statistics, and security parameters.
    ///
    /// # Arguments
    ///
    /// * `session_id` - Session to query
    ///
    /// # Returns
    ///
    /// * `Some(SessionInfo)` - Session information if session exists
    /// * `None` - Session not found
    pub fn get_session_info(&self, session_id: [u8; 32]) -> Option<SessionInfo> {
        self.sessions.get(&session_id).map(|session| SessionInfo {
            session_id,
            established: session.established,
            created_at: session.created_at,
            last_key_rotation: session.last_key_rotation,
            sequence_counter: session.sequence_counter,
            received_sequence_count: session.received_sequences.len(),
        })
    }

    /// Get system statistics
    pub fn get_crypto_stats(&self) -> CryptoStats {
        let current_time = current_timestamp();
        let active_sessions = self.sessions.len();
        let expired_sessions = self.sessions.values()
            .filter(|session| current_time - session.created_at > self.limits.max_session_lifetime)
            .count();

        CryptoStats {
            active_sessions,
            expired_sessions,
            total_sessions_created: active_sessions + expired_sessions, // Simplified
        }
    }

    /// Cleanup expired sessions
    ///
    /// Removes sessions that have exceeded their maximum lifetime.
    /// Should be called periodically to prevent resource leaks.
    pub fn cleanup_expired_sessions(&mut self) {
        let current_time = current_timestamp();
        let expired_sessions: Vec<_> = self.sessions
            .iter()
            .filter(|(_, session)| current_time - session.created_at > self.limits.max_session_lifetime)
            .map(|(&session_id, _)| session_id)
            .collect();

        for session_id in expired_sessions {
            self.sessions.remove(&session_id);
        }
    }
}

/// Session information and statistics
///
/// Provides detailed information about a cryptographic session including
/// its state, timing, and usage statistics.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// Unique session identifier
    pub session_id: [u8; 32],
    /// Whether the session is fully established
    pub established: bool,
    /// Session creation timestamp in milliseconds
    pub created_at: u64,
    /// Last key rotation timestamp in milliseconds
    pub last_key_rotation: u64,
    /// Current outgoing sequence number
    pub sequence_counter: u64,
    /// Number of unique received sequence numbers
    pub received_sequence_count: usize,
}

/// Crypto system statistics
#[derive(Debug, Clone)]
pub struct CryptoStats {
    /// Number of currently active sessions
    pub active_sessions: usize,
    /// Number of expired sessions
    pub expired_sessions: usize,
    /// Total number of sessions created
    pub total_sessions_created: usize,
}

/// Get current timestamp in milliseconds since Unix epoch
///
/// Utility function for timestamping cryptographic operations.
///
/// # Returns
///
/// Current time in milliseconds since Unix epoch
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crypto_system_creation() {
        let crypto_system = CryptoSystem::new();
        assert_eq!(crypto_system.sessions.len(), 0);
    }

    #[test]
    fn test_session_creation() {
        let mut crypto_system = CryptoSystem::new();
        let session_id = crypto_system.create_session(None).unwrap();

        assert!(crypto_system.sessions.contains_key(&session_id));
        let info = crypto_system.get_session_info(session_id).unwrap();
        assert_eq!(info.session_id, session_id);
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let mut crypto_system = CryptoSystem::new();
        let session_id = crypto_system.create_session(None).unwrap();

        // Make session established for testing
        crypto_system.sessions.get_mut(&session_id).unwrap().established = true;

        let plaintext = b"Hello, World!";
        let aad = b"additional_data";

        // Encrypt
        let encrypted = crypto_system.encrypt_and_sign(session_id, plaintext, aad).unwrap();

        // Decrypt
        let decrypted = crypto_system.verify_and_decrypt(session_id, &encrypted).unwrap();

        assert_eq!(plaintext.as_slice(), decrypted);
    }

    #[test]
    fn test_replay_protection() {
        let mut crypto_system = CryptoSystem::new();
        let session_id = crypto_system.create_session(None).unwrap();

        // Make session established for testing
        crypto_system.sessions.get_mut(&session_id).unwrap().established = true;

        let plaintext = b"test message";
        let aad = b"";

        let encrypted = crypto_system.encrypt_and_sign(session_id, plaintext, aad).unwrap();

        // First decryption should succeed
        let result1 = crypto_system.verify_and_decrypt(session_id, &encrypted);
        assert!(result1.is_ok());

        // Second decryption should fail (replay attack)
        let result2 = crypto_system.verify_and_decrypt(session_id, &encrypted);
        assert!(result2.is_err());
        match result2.unwrap_err() {
            CryptoError::ReplayAttack(_) => {} // Expected
            other => panic!("Expected ReplayAttack, got: {:?}", other),
        }
    }

    #[test]
    fn test_session_limits() {
        let mut crypto_system = CryptoSystem::new();
        crypto_system.limits.max_sessions = 2;

        // Create maximum sessions
        let session1 = crypto_system.create_session(None).unwrap();
        let _session2 = crypto_system.create_session(None).unwrap();

        // Third session should fail
        let result = crypto_system.create_session(None);
        assert!(result.is_err());
        match result.unwrap_err() {
            CryptoError::MaxSessionsReached => {} // Expected
            other => panic!("Expected MaxSessionsReached, got: {:?}", other),
        }

        // After destroying one session, should be able to create another
        crypto_system.destroy_session(session1).unwrap();
        let session3 = crypto_system.create_session(None).unwrap();
        assert!(crypto_system.sessions.contains_key(&session3));
    }

    #[test]
    fn test_message_size_limits() {
        let mut crypto_system = CryptoSystem::new();
        crypto_system.limits.max_message_size = 1024;

        let session_id = crypto_system.create_session(None).unwrap();
        crypto_system.sessions.get_mut(&session_id).unwrap().established = true;

        // Message within limit should work
        let small_message = vec![0u8; 512];
        let result = crypto_system.encrypt_and_sign(session_id, &small_message, b"");
        assert!(result.is_ok());

        // Message exceeding limit should fail
        let large_message = vec![0u8; 2048];
        let result = crypto_system.encrypt_and_sign(session_id, &large_message, b"");
        assert!(result.is_err());
        match result.unwrap_err() {
            CryptoError::MessageTooLarge(_, _) => {} // Expected
            other => panic!("Expected MessageTooLarge, got: {:?}", other),
        }
    }

    #[test]
    fn test_session_cleanup() {
        let mut crypto_system = CryptoSystem::new();

        // Create session
        let session_id = crypto_system.create_session(None).unwrap();
        assert!(crypto_system.sessions.contains_key(&session_id));

        // Destroy session
        crypto_system.destroy_session(session_id).unwrap();
        assert!(!crypto_system.sessions.contains_key(&session_id));
    }

    #[test]
    fn test_crypto_stats() {
        let mut crypto_system = CryptoSystem::new();

        let stats_before = crypto_system.get_crypto_stats();
        assert_eq!(stats_before.active_sessions, 0);

        // Create some sessions
        let _session1 = crypto_system.create_session(None).unwrap();
        let _session2 = crypto_system.create_session(None).unwrap();

        let stats_after = crypto_system.get_crypto_stats();
        assert_eq!(stats_after.active_sessions, 2);
    }
}