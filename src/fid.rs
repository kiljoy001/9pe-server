//! UUIDv8-based File Identifier System
//!
//! Extends the traditional 9P 32-bit fid with user and namespace context
//! while maintaining wire compatibility with 9P2000 clients.
//!
//! ## UUIDv8 Layout for 9P.e Fids
//!
//! ```text
//!  0                   1                   2                   3
//!  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |                      9P fid (32 bits)                        |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |      connection_id (16 bits) |  ver  | namespace_shard (12)  |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |var|                user_identity_hash (62 bits)              |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |                   user_identity_hash (continued)             |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! ```
//!
//! - **9P fid**: Traditional 32-bit file identifier (wire-compatible)
//! - **connection_id**: Identifies the TCP/QUIC connection
//! - **namespace_shard**: Hash of namespace path (for multi-tenant isolation)
//! - **user_identity_hash**: Truncated hash of user's public key

use std::fmt;
use std::hash::{Hash, Hasher};
use blake3;
use serde::{Deserialize, Serialize};

/// Extended file identifier using UUIDv8 format
///
/// Maintains wire compatibility with 9P2000 while adding
/// user and namespace context for multi-tenant scenarios.
#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct ExtendedFid {
    /// Raw UUID bytes
    bytes: [u8; 16],
}

impl ExtendedFid {
    /// UUIDv8 version nibble (0x8)
    const VERSION: u8 = 0x80;
    /// UUIDv8 variant bits (0b10xx_xxxx)
    const VARIANT: u8 = 0x80;

    /// Create a new extended fid from components
    ///
    /// # Arguments
    /// * `fid` - Traditional 9P 32-bit file identifier
    /// * `connection_id` - Connection/session identifier
    /// * `namespace` - Namespace path (hashed to 12 bits)
    /// * `user_pubkey` - User's public key (hashed to 62 bits)
    pub fn new(fid: u32, connection_id: u16, namespace: &str, user_pubkey: &[u8]) -> Self {
        let mut bytes = [0u8; 16];

        // Bytes 0-3: 9P fid (big-endian for UUID compatibility)
        bytes[0..4].copy_from_slice(&fid.to_be_bytes());

        // Bytes 4-5: connection_id
        bytes[4..6].copy_from_slice(&connection_id.to_be_bytes());

        // Byte 6: version (high nibble) + namespace_shard high 4 bits
        let namespace_hash = Self::hash_namespace(namespace);
        bytes[6] = Self::VERSION | ((namespace_hash >> 8) as u8 & 0x0F);

        // Byte 7: namespace_shard low 8 bits
        bytes[7] = namespace_hash as u8;

        // Bytes 8-15: variant (2 bits) + user_identity_hash (62 bits)
        let user_hash = Self::hash_user(user_pubkey);
        bytes[8] = Self::VARIANT | ((user_hash >> 56) as u8 & 0x3F);
        bytes[9] = (user_hash >> 48) as u8;
        bytes[10] = (user_hash >> 40) as u8;
        bytes[11] = (user_hash >> 32) as u8;
        bytes[12] = (user_hash >> 24) as u8;
        bytes[13] = (user_hash >> 16) as u8;
        bytes[14] = (user_hash >> 8) as u8;
        bytes[15] = user_hash as u8;

        Self { bytes }
    }

    /// Create from a simple 9P fid (for unauthenticated/legacy connections)
    pub fn from_simple_fid(fid: u32, connection_id: u16) -> Self {
        Self::new(fid, connection_id, "", &[])
    }

    /// Create from raw UUID bytes
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self { bytes }
    }

    /// Get the raw UUID bytes
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.bytes
    }

    /// Extract the traditional 9P fid (wire-compatible)
    pub fn fid(&self) -> u32 {
        u32::from_be_bytes([self.bytes[0], self.bytes[1], self.bytes[2], self.bytes[3]])
    }

    /// Extract the connection ID
    pub fn connection_id(&self) -> u16 {
        u16::from_be_bytes([self.bytes[4], self.bytes[5]])
    }

    /// Extract the namespace shard (12 bits)
    pub fn namespace_shard(&self) -> u16 {
        let high = (self.bytes[6] & 0x0F) as u16;
        let low = self.bytes[7] as u16;
        (high << 8) | low
    }

    /// Extract the user identity hash (62 bits)
    pub fn user_hash(&self) -> u64 {
        let b = &self.bytes;
        ((b[8] & 0x3F) as u64) << 56
            | (b[9] as u64) << 48
            | (b[10] as u64) << 40
            | (b[11] as u64) << 32
            | (b[12] as u64) << 24
            | (b[13] as u64) << 16
            | (b[14] as u64) << 8
            | (b[15] as u64)
    }

    /// Check if this fid belongs to the same user
    pub fn same_user(&self, other: &ExtendedFid) -> bool {
        self.user_hash() == other.user_hash()
    }

    /// Check if this fid is in the same namespace
    pub fn same_namespace(&self, other: &ExtendedFid) -> bool {
        self.namespace_shard() == other.namespace_shard()
    }

    /// Check if this fid is from the same connection
    pub fn same_connection(&self, other: &ExtendedFid) -> bool {
        self.connection_id() == other.connection_id()
    }

    /// Hash a namespace path to 12 bits
    fn hash_namespace(namespace: &str) -> u16 {
        if namespace.is_empty() {
            return 0;
        }
        let hash = blake3::hash(namespace.as_bytes());
        let bytes = hash.as_bytes();
        // Take first 2 bytes and mask to 12 bits
        let val = u16::from_le_bytes([bytes[0], bytes[1]]);
        val & 0x0FFF
    }

    /// Hash a user public key to 64 bits (we use 62)
    fn hash_user(pubkey: &[u8]) -> u64 {
        if pubkey.is_empty() {
            return 0;
        }
        let hash = blake3::hash(pubkey);
        let bytes = hash.as_bytes();
        u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5], bytes[6], bytes[7],
        ])
    }

    /// Format as standard UUID string
    pub fn to_uuid_string(&self) -> String {
        let b = &self.bytes;
        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            b[0], b[1], b[2], b[3],
            b[4], b[5],
            b[6], b[7],
            b[8], b[9],
            b[10], b[11], b[12], b[13], b[14], b[15]
        )
    }
}

impl fmt::Debug for ExtendedFid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExtendedFid")
            .field("fid", &self.fid())
            .field("connection", &self.connection_id())
            .field("namespace_shard", &format!("{:03x}", self.namespace_shard()))
            .field("user_hash", &format!("{:016x}", self.user_hash()))
            .finish()
    }
}

impl fmt::Display for ExtendedFid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_uuid_string())
    }
}

impl PartialEq for ExtendedFid {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}

impl Eq for ExtendedFid {}

impl Hash for ExtendedFid {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.bytes.hash(state);
    }
}

/// Context for creating extended fids within a connection
#[derive(Clone)]
pub struct FidContext {
    /// Connection identifier
    connection_id: u16,
    /// Current namespace path
    namespace: String,
    /// User's public key (if authenticated)
    user_pubkey: Vec<u8>,
}

impl FidContext {
    /// Create a new fid context for a connection
    pub fn new(connection_id: u16) -> Self {
        Self {
            connection_id,
            namespace: String::new(),
            user_pubkey: Vec::new(),
        }
    }

    /// Set the authenticated user's public key
    pub fn set_user(&mut self, pubkey: Vec<u8>) {
        self.user_pubkey = pubkey;
    }

    /// Set the current namespace
    pub fn set_namespace(&mut self, namespace: String) {
        self.namespace = namespace;
    }

    /// Get the connection ID
    pub fn connection_id(&self) -> u16 {
        self.connection_id
    }

    /// Create an extended fid from a wire fid
    pub fn extend_fid(&self, wire_fid: u32) -> ExtendedFid {
        ExtendedFid::new(
            wire_fid,
            self.connection_id,
            &self.namespace,
            &self.user_pubkey,
        )
    }

    /// Check if a given extended fid belongs to this context
    pub fn owns_fid(&self, efid: &ExtendedFid) -> bool {
        efid.connection_id() == self.connection_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extended_fid_roundtrip() {
        let fid = 42u32;
        let conn_id = 1234u16;
        let namespace = "/srv/compute";
        let pubkey = b"test_public_key_32_bytes_here!!";

        let efid = ExtendedFid::new(fid, conn_id, namespace, pubkey);

        assert_eq!(efid.fid(), fid);
        assert_eq!(efid.connection_id(), conn_id);
        assert!(efid.namespace_shard() > 0); // Hashed, non-zero
        assert!(efid.user_hash() > 0); // Hashed, non-zero
    }

    #[test]
    fn test_simple_fid() {
        let efid = ExtendedFid::from_simple_fid(100, 5);

        assert_eq!(efid.fid(), 100);
        assert_eq!(efid.connection_id(), 5);
        assert_eq!(efid.namespace_shard(), 0);
        assert_eq!(efid.user_hash(), 0);
    }

    #[test]
    fn test_same_user_detection() {
        let pubkey = b"user_pubkey_here";

        let fid1 = ExtendedFid::new(1, 100, "/ns1", pubkey);
        let fid2 = ExtendedFid::new(2, 200, "/ns2", pubkey);
        let fid3 = ExtendedFid::new(3, 100, "/ns1", b"different_key");

        assert!(fid1.same_user(&fid2));
        assert!(!fid1.same_user(&fid3));
    }

    #[test]
    fn test_same_namespace_detection() {
        let fid1 = ExtendedFid::new(1, 100, "/srv/compute", b"user1");
        let fid2 = ExtendedFid::new(2, 200, "/srv/compute", b"user2");
        let fid3 = ExtendedFid::new(3, 100, "/srv/storage", b"user1");

        assert!(fid1.same_namespace(&fid2));
        assert!(!fid1.same_namespace(&fid3));
    }

    #[test]
    fn test_uuid_format() {
        let efid = ExtendedFid::new(0x12345678, 0xABCD, "/test", b"key");
        let uuid_str = efid.to_uuid_string();

        // Should be valid UUID format: 8-4-4-4-12
        assert_eq!(uuid_str.len(), 36);
        assert_eq!(&uuid_str[8..9], "-");
        assert_eq!(&uuid_str[13..14], "-");
        assert_eq!(&uuid_str[18..19], "-");
        assert_eq!(&uuid_str[23..24], "-");

        // Check version nibble (should be 8)
        let version_char = uuid_str.chars().nth(14).unwrap();
        assert_eq!(version_char, '8');
    }

    #[test]
    fn test_fid_context() {
        let mut ctx = FidContext::new(42);
        ctx.set_namespace("/myns".to_string());
        ctx.set_user(b"my_pubkey".to_vec());

        let efid = ctx.extend_fid(100);

        assert_eq!(efid.fid(), 100);
        assert_eq!(efid.connection_id(), 42);
        assert!(ctx.owns_fid(&efid));

        let other_efid = ExtendedFid::from_simple_fid(100, 99);
        assert!(!ctx.owns_fid(&other_efid));
    }
}
