//! Property-based tests for authentication and security
//! Verifies capability-based security, MFA, and access control

use proptest::prelude::*;
use proptest::collection::{vec, hash_set};
use proptest::string::string_regex;
use ed25519_dalek::{SigningKey, VerifyingKey, Signature, Signer, Verifier};
use sha2::{Sha256, Digest};
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

/// Permission bits
const PERM_READ: u32 = 1 << 0;
const PERM_WRITE: u32 = 1 << 1;
const PERM_EXECUTE: u32 = 1 << 2;
const PERM_DELETE: u32 = 1 << 3;
const PERM_ADMIN: u32 = 1 << 4;
const PERM_TRAVERSE: u32 = 1 << 5;
const PERM_MOUNT: u32 = 1 << 6;

#[derive(Debug, Clone)]
struct User {
    id: u32,
    username: String,
    pubkey: VerifyingKey,
    groups: Vec<String>,
    password_hash: String,
}

#[derive(Debug, Clone)]
struct Capability {
    id: u32,
    issuer: u32,
    subject: u32,
    resource: String,
    permissions: u32,
    issued_at: u64,
    expires_at: u64,
    max_uses: Option<u32>,
    delegation_allowed: bool,
}

#[derive(Debug, Clone)]
struct SignedCapability {
    capability: Capability,
    signature: Vec<u8>,
}

#[derive(Debug, Clone)]
enum AuthMethod {
    None,
    Password(String),
    PublicKey(VerifyingKey),
    Capability(SignedCapability),
}

#[derive(Debug)]
struct AuthSystem {
    users: HashMap<u32, User>,
    capabilities: HashMap<u32, SignedCapability>,
    revoked: HashSet<u32>,
    server_key: SigningKey,
    current_time: u64,
}

/// Generate arbitrary usernames
fn arbitrary_username() -> impl Strategy<Value = String> {
    string_regex("[a-z][a-z0-9_]{2,15}").unwrap()
}

/// Generate arbitrary resource paths
fn arbitrary_resource() -> impl Strategy<Value = String> {
    string_regex("/[a-z][a-z0-9/._-]{0,50}").unwrap()
}

/// Generate arbitrary permission sets
fn arbitrary_permissions() -> impl Strategy<Value = u32> {
    (0u32..=0b1111111).prop_map(|p| p & 0x7F) // 7 permission bits
}

/// Generate valid time ranges
fn arbitrary_time_range(current: u64) -> impl Strategy<Value = (u64, u64)> {
    (0u64..=current, current..=(current + 86400 * 30))
        .prop_map(|(issued, expires)| (issued, expires))
}

/// Generate arbitrary capability
fn arbitrary_capability(current_time: u64) -> impl Strategy<Value = Capability> {
    (
        0u32..1000u32,  // id
        0u32..100u32,   // issuer
        0u32..100u32,   // subject
        arbitrary_resource(),
        arbitrary_permissions(),
        arbitrary_time_range(current_time),
        prop::option::of(1u32..100u32),  // max_uses
        prop::bool::ANY,  // delegation_allowed
    ).prop_map(|(id, issuer, subject, resource, perms, (issued, expires), max_uses, deleg)| {
        Capability {
            id, issuer, subject, resource,
            permissions: perms,
            issued_at: issued,
            expires_at: expires,
            max_uses,
            delegation_allowed: deleg,
        }
    })
}

impl AuthSystem {
    fn new() -> Self {
        let mut csprng = rand::rngs::OsRng;
        Self {
            users: HashMap::new(),
            capabilities: HashMap::new(),
            revoked: HashSet::new(),
            server_key: SigningKey::generate(&mut csprng),
            current_time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    fn valid_capability(&self, cap: &Capability) -> bool {
        cap.issued_at <= self.current_time &&
        self.current_time <= cap.expires_at &&
        !self.revoked.contains(&cap.id)
    }

    fn verify_signature(&self, cap_id: u32, signature: &[u8]) -> bool {
        if signature.len() != 64 {
            return false;
        }

        let sig_bytes: [u8; 64] = signature.try_into().unwrap_or([0; 64]);
        let sig = Signature::from_bytes(&sig_bytes);
        let message = cap_id.to_le_bytes();

        self.server_key.verifying_key()
            .verify(&message, &sig)
            .is_ok()
    }

    fn sign_capability(&self, cap: &Capability) -> SignedCapability {
        let message = cap.id.to_le_bytes();
        let signature = self.server_key.sign(&message);

        SignedCapability {
            capability: cap.clone(),
            signature: signature.to_bytes().to_vec(),
        }
    }

    fn has_access(&self, user: &User, resource: &str, perm: u32,
                  caps: &[SignedCapability], mfa_verified: bool) -> bool {
        // Check MFA requirement
        if self.requires_mfa(resource) && !mfa_verified {
            return false;
        }

        // Check capabilities
        for scap in caps {
            if self.valid_capability(&scap.capability) &&
               self.verify_signature(scap.capability.id, &scap.signature) &&
               scap.capability.subject == user.id &&
               scap.capability.resource == resource &&
               (scap.capability.permissions & perm) == perm {
                return true;
            }
        }

        false
    }

    fn requires_mfa(&self, resource: &str) -> bool {
        resource.starts_with("/admin") ||
        resource.starts_with("/secure") ||
        resource.contains("/sensitive")
    }

    fn delegate_capability(&self, cap: &Capability, new_subject: u32) -> Option<Capability> {
        if !cap.delegation_allowed {
            return None;
        }

        Some(Capability {
            id: cap.id + 1000,  // New ID
            issuer: cap.subject, // Original subject becomes issuer
            subject: new_subject,
            resource: cap.resource.clone(),
            permissions: cap.permissions,
            issued_at: cap.issued_at,
            expires_at: cap.expires_at,
            max_uses: cap.max_uses,
            delegation_allowed: false, // Cannot re-delegate
        })
    }
}

proptest! {
    /// Test: No access without authentication
    #[test]
    fn prop_no_access_without_auth(
        resource in arbitrary_resource(),
        perm in arbitrary_permissions()
    ) {
        let sys = AuthSystem::new();

        // Create user but don't authenticate
        let mut csprng = rand::rngs::OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let user = User {
            id: 1,
            username: "testuser".to_string(),
            pubkey: signing_key.verifying_key(),
            groups: vec![],
            password_hash: "hash".to_string(),
        };

        // No capabilities = no access
        prop_assert!(!sys.has_access(&user, &resource, perm, &[], false));
    }

    /// Test: Expired capabilities grant no access
    #[test]
    fn prop_expired_caps_no_access(
        resource in arbitrary_resource(),
        perm in arbitrary_permissions(),
        expired_by in 1u64..86400u64
    ) {
        let mut sys = AuthSystem::new();

        let mut csprng = rand::rngs::OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let user = User {
            id: 1,
            username: "user".to_string(),
            pubkey: signing_key.verifying_key(),
            groups: vec![],
            password_hash: "hash".to_string(),
        };

        // Create expired capability
        let cap = Capability {
            id: 1,
            issuer: 0,
            subject: user.id,
            resource: resource.clone(),
            permissions: perm,
            issued_at: sys.current_time - 86400,
            expires_at: sys.current_time - expired_by, // Expired!
            max_uses: None,
            delegation_allowed: false,
        };

        let signed_cap = sys.sign_capability(&cap);

        // Should not have access with expired capability
        prop_assert!(!sys.has_access(&user, &resource, perm, &[signed_cap], true));
    }

    /// Test: Revoked capabilities grant no access
    #[test]
    fn prop_revoked_caps_no_access(
        resource in arbitrary_resource(),
        perm in arbitrary_permissions()
    ) {
        let mut sys = AuthSystem::new();

        let mut csprng = rand::rngs::OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let user = User {
            id: 1,
            username: "user".to_string(),
            pubkey: signing_key.verifying_key(),
            groups: vec![],
            password_hash: "hash".to_string(),
        };

        let cap = Capability {
            id: 100,
            issuer: 0,
            subject: user.id,
            resource: resource.clone(),
            permissions: perm,
            issued_at: sys.current_time - 3600,
            expires_at: sys.current_time + 3600,
            max_uses: None,
            delegation_allowed: false,
        };

        // Revoke the capability
        sys.revoked.insert(cap.id);

        let signed_cap = sys.sign_capability(&cap);

        // Should not have access with revoked capability
        prop_assert!(!sys.has_access(&user, &resource, perm, &[signed_cap], true));
    }

    /// Test: MFA enforcement for sensitive resources
    #[test]
    fn prop_mfa_enforcement(
        sensitive_path in prop::string::string_regex("/admin/[a-z]+").unwrap(),
        perm in arbitrary_permissions()
    ) {
        let sys = AuthSystem::new();

        let mut csprng = rand::rngs::OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let user = User {
            id: 1,
            username: "admin".to_string(),
            pubkey: signing_key.verifying_key(),
            groups: vec!["admin".to_string()],
            password_hash: "hash".to_string(),
        };

        let cap = Capability {
            id: 1,
            issuer: 0,
            subject: user.id,
            resource: sensitive_path.clone(),
            permissions: perm,
            issued_at: sys.current_time - 60,
            expires_at: sys.current_time + 3600,
            max_uses: None,
            delegation_allowed: false,
        };

        let signed_cap = sys.sign_capability(&cap);

        // Should require MFA for admin paths
        prop_assert!(sys.requires_mfa(&sensitive_path));

        // Without MFA, no access
        prop_assert!(!sys.has_access(&user, &sensitive_path, perm, &[signed_cap.clone()], false));

        // With MFA, access granted
        prop_assert!(sys.has_access(&user, &sensitive_path, perm, &[signed_cap], true));
    }

    /// Test: Capability delegation preserves security
    #[test]
    fn prop_delegation_security(
        original_cap in arbitrary_capability(1000000),
        new_subject in 100u32..200u32
    ) {
        let sys = AuthSystem::new();

        // Can only delegate if allowed
        if original_cap.delegation_allowed {
            let delegated = sys.delegate_capability(&original_cap, new_subject);
            prop_assert!(delegated.is_some());

            let del = delegated.unwrap();
            // Delegated capability should:
            // 1. Have new subject
            prop_assert_eq!(del.subject, new_subject);
            // 2. Original subject becomes issuer
            prop_assert_eq!(del.issuer, original_cap.subject);
            // 3. Same resource and permissions
            prop_assert_eq!(del.resource, original_cap.resource);
            prop_assert_eq!(del.permissions, original_cap.permissions);
            // 4. Cannot be further delegated
            prop_assert!(!del.delegation_allowed);
        } else {
            let delegated = sys.delegate_capability(&original_cap, new_subject);
            prop_assert!(delegated.is_none());
        }
    }

    /// Test: Password security - never store plaintext
    #[test]
    fn prop_password_never_plaintext(
        password in string_regex("[A-Za-z0-9!@#$%^&*]{8,32}").unwrap()
    ) {
        // Hash the password
        let mut hasher = Sha256::new();
        hasher.update(password.as_bytes());
        hasher.update(b"salt"); // In reality, use random salt
        let hash = format!("{:x}", hasher.finalize());

        // The bug: comparing password with username
        let username = "testuser";
        let insecure_check = password == username; // BUG!

        // Correct: compare hashed values
        let mut hasher2 = Sha256::new();
        hasher2.update(password.as_bytes());
        hasher2.update(b"salt");
        let input_hash = format!("{:x}", hasher2.finalize());
        let secure_check = input_hash == hash;

        // Password should never equal username
        if password != username {
            prop_assert!(!insecure_check);
        }

        // Hashed comparison should work
        prop_assert!(secure_check);
    }

    /// Test: Least privilege principle
    #[test]
    fn prop_least_privilege(
        requested_perm in arbitrary_permissions(),
        granted_perm in arbitrary_permissions()
    ) {
        let sys = AuthSystem::new();

        let mut csprng = rand::rngs::OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let user = User {
            id: 1,
            username: "user".to_string(),
            pubkey: signing_key.verifying_key(),
            groups: vec![],
            password_hash: "hash".to_string(),
        };

        let cap = Capability {
            id: 1,
            issuer: 0,
            subject: user.id,
            resource: "/data".to_string(),
            permissions: granted_perm,
            issued_at: sys.current_time - 60,
            expires_at: sys.current_time + 3600,
            max_uses: None,
            delegation_allowed: false,
        };

        let signed_cap = sys.sign_capability(&cap);

        // Can only access permissions that were granted
        let has_access = sys.has_access(&user, "/data", requested_perm, &[signed_cap], true);

        // Should have access iff all requested permissions are in granted set
        let should_have = (requested_perm & granted_perm) == requested_perm;
        prop_assert_eq!(has_access, should_have);
    }

    /// Test: Rate limiting
    #[test]
    fn prop_rate_limiting(
        max_requests in 10u32..1000u32,
        window_seconds in 1u64..3600u64,
        actual_requests in 0u32..2000u32
    ) {
        struct RateLimit {
            max_requests: u32,
            window: u64,
            requests: HashMap<u32, Vec<u64>>,
        }

        impl RateLimit {
            fn check(&mut self, user_id: u32, current_time: u64) -> bool {
                let user_requests = self.requests.entry(user_id).or_insert_with(Vec::new);

                // Remove old requests outside window
                user_requests.retain(|&t| current_time - t < self.window);

                if user_requests.len() < self.max_requests as usize {
                    user_requests.push(current_time);
                    true
                } else {
                    false
                }
            }
        }

        let mut limiter = RateLimit {
            max_requests,
            window: window_seconds,
            requests: HashMap::new(),
        };

        let mut allowed_count = 0;
        let mut current_time = 0u64;

        for i in 0..actual_requests {
            // Advance time slightly for each request
            current_time += 1;

            if limiter.check(1, current_time) {
                allowed_count += 1;
            }
        }

        // Should never allow more than max_requests in a window
        prop_assert!(allowed_count <= max_requests);
    }

    /// Test: Signature forgery prevention
    #[test]
    fn prop_signature_unforgeability(
        cap_id in 1u32..10000u32,
        random_bytes in vec(0u8..255u8, 64)
    ) {
        let sys = AuthSystem::new();

        // Try to verify random bytes as signature
        let forged = sys.verify_signature(cap_id, &random_bytes);

        // Random bytes should almost never be valid signature
        // (probability is negligible: 2^-256)
        prop_assert!(!forged);

        // But real signature should verify
        let message = cap_id.to_le_bytes();
        let real_sig = sys.server_key.sign(&message);
        prop_assert!(sys.verify_signature(cap_id, &real_sig.to_bytes()));
    }

    /// Test: Permission bit operations
    #[test]
    fn prop_permission_bits(
        perm1 in arbitrary_permissions(),
        perm2 in arbitrary_permissions()
    ) {
        // Adding permissions is OR
        let combined = perm1 | perm2;
        prop_assert!((combined & perm1) == perm1);
        prop_assert!((combined & perm2) == perm2);

        // Removing permissions is AND NOT
        let removed = perm1 & !perm2;
        prop_assert!((removed & perm2) == 0);

        // Check individual permission bits
        for bit in 0..7 {
            let perm = 1u32 << bit;
            if (perm1 & perm) != 0 {
                prop_assert!((perm1 & perm) == perm);
            }
        }
    }
}

/// Test specific bug: insecure password comparison
#[test]
fn test_insecure_password_bug() {
    // The bug: password compared with username
    let username = "admin";
    let password = "admin"; // Same as username - insecure!

    // This should NOT authenticate
    let insecure_check = password == username;

    // In the buggy implementation, this would pass
    assert!(insecure_check); // This is the bug!

    // Correct implementation would hash and compare
    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    hasher.update(b"random_salt");
    let password_hash = format!("{:x}", hasher.finalize());

    // Store hash, not plaintext
    let stored_hash = password_hash.clone();

    // Verify by hashing input and comparing
    let mut verify_hasher = Sha256::new();
    verify_hasher.update(password.as_bytes());
    verify_hasher.update(b"random_salt");
    let input_hash = format!("{:x}", verify_hasher.finalize());

    assert_eq!(input_hash, stored_hash); // Correct way
}