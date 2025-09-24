//! Authentication and Authorization for 9P.e Server
//!
//! Implements capability-based security with Ed25519 signatures

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use anyhow::{Result, Context};
use serde::{Serialize, Deserialize};
use ed25519_dalek::{SigningKey, VerifyingKey, Signature, Signer};
use sha2::Sha256;

/// Permission flags (can be OR'd together)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Permissions(u32);

impl Permissions {
    pub const NONE: Permissions = Permissions(0);
    pub const READ: Permissions = Permissions(1 << 0);
    pub const WRITE: Permissions = Permissions(1 << 1);
    pub const EXECUTE: Permissions = Permissions(1 << 2);
    pub const DELETE: Permissions = Permissions(1 << 3);
    pub const ADMIN: Permissions = Permissions(1 << 4);
    pub const TRAVERSE: Permissions = Permissions(1 << 5);
    pub const MOUNT: Permissions = Permissions(1 << 6);
    pub const ALL: Permissions = Permissions(0xFFFFFFFF);

    pub fn has(&self, perm: Permissions) -> bool {
        (self.0 & perm.0) == perm.0
    }

    pub fn with(self, other: Permissions) -> Permissions {
        Permissions(self.0 | other.0)
    }

    pub fn add(&mut self, perm: Permissions) {
        self.0 |= perm.0;
    }

    pub fn remove(&mut self, perm: Permissions) {
        self.0 &= !perm.0;
    }
}

/// Authentication method
#[derive(Debug, Clone)]
pub enum AuthMethod {
    None,                              // No auth (dangerous!)
    Password(String),                  // Simple password
    PublicKey(VerifyingKey),          // Ed25519 public key
    Certificate(Vec<u8>),             // X.509 certificate
    Kerberos(String),                 // Kerberos principal
    OAuth2(String),                   // OAuth2 token
    Capability(SignedCapability),     // Capability token
}

/// User identity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub uid: u32,
    pub username: String,
    pub groups: Vec<String>,
    pub home_dir: String,
    pub shell: String,
    pub public_key: Option<Vec<u8>>,
}

/// Signed capability token (like Kerberos tickets)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedCapability {
    pub capability: Capability,
    pub signature: Vec<u8>,  // Ed25519 signature
}

/// Capability granting access to resources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    pub id: String,                    // Unique capability ID
    pub issuer: String,                // Who issued this
    pub subject: String,               // Who can use this
    pub resource: String,              // What resource (path pattern)
    pub permissions: u32,              // Permission bits
    pub issued_at: u64,                // Unix timestamp
    pub expires_at: u64,               // Unix timestamp
    pub max_uses: Option<u32>,         // Usage limit
    pub delegation_allowed: bool,      // Can delegate to others
    pub conditions: Vec<Condition>,    // Additional conditions
}

/// Condition that must be met for capability to be valid
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Condition {
    IpRange(String, String),      // Must be from IP range
    TimeWindow(u64, u64),         // Only valid during time window
    MfaRequired,                  // Requires 2FA
    RateLimited(u32, u64),        // Max requests per time period
    GeographicRegion(String),     // Must be from region
}

/// Access Control List entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AclEntry {
    pub principal: String,         // User or group
    pub permissions: u32,  // Permission bits directly
    pub inheritable: bool,
}

/// Security context for a connection
#[derive(Debug, Clone)]
pub struct SecurityContext {
    pub user: Option<User>,
    pub auth_method: AuthMethod,
    pub capabilities: Vec<SignedCapability>,
    pub session_key: Option<Vec<u8>>,
    pub ip_address: std::net::IpAddr,
    pub authenticated_at: Option<u64>,
    pub mfa_verified: bool,
}

/// Authentication service
pub struct AuthService {
    users: Arc<RwLock<HashMap<String, User>>>,
    groups: Arc<RwLock<HashMap<String, HashSet<String>>>>,
    acls: Arc<RwLock<HashMap<String, Vec<AclEntry>>>>,
    capabilities: Arc<RwLock<HashMap<String, SignedCapability>>>,
    revoked: Arc<RwLock<HashSet<String>>>,  // Revoked capability IDs
    server_keypair: SigningKey,
    trusted_keys: Arc<RwLock<HashMap<String, VerifyingKey>>>,
}

impl AuthService {
    pub fn new() -> Self {
        let server_keypair = SigningKey::from_bytes(&rand::random());

        Self {
            users: Arc::new(RwLock::new(HashMap::new())),
            groups: Arc::new(RwLock::new(HashMap::new())),
            acls: Arc::new(RwLock::new(HashMap::new())),
            capabilities: Arc::new(RwLock::new(HashMap::new())),
            revoked: Arc::new(RwLock::new(HashSet::new())),
            server_keypair,
            trusted_keys: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Authenticate a user
    pub async fn authenticate(&self, method: &AuthMethod) -> Result<User> {
        match method {
            AuthMethod::None => {
                Err(anyhow::anyhow!("Authentication required"))
            }

            AuthMethod::Password(password) => {
                // Check against stored hashes (would use argon2 in production)
                let users = self.users.read().await;
                for (_, user) in users.iter() {
                    // In production, compare with hashed password
                    if password == &user.username {  // INSECURE - just for demo
                        return Ok(user.clone());
                    }
                }
                Err(anyhow::anyhow!("Invalid password"))
            }

            AuthMethod::PublicKey(pubkey) => {
                // Find user with this public key
                let users = self.users.read().await;
                for (_, user) in users.iter() {
                    if let Some(key) = &user.public_key {
                        if key == &pubkey.to_bytes() {
                            return Ok(user.clone());
                        }
                    }
                }
                Err(anyhow::anyhow!("Unknown public key"))
            }

            AuthMethod::Capability(signed_cap) => {
                // Verify capability signature
                self.verify_capability(signed_cap).await?;

                // Get user from capability subject
                let users = self.users.read().await;
                users.get(&signed_cap.capability.subject)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("User not found"))
            }

            _ => Err(anyhow::anyhow!("Auth method not implemented"))
        }
    }

    /// Verify a signed capability
    pub async fn verify_capability(&self, signed_cap: &SignedCapability) -> Result<()> {
        // Check if revoked
        if self.revoked.read().await.contains(&signed_cap.capability.id) {
            return Err(anyhow::anyhow!("Capability revoked"));
        }

        // Check expiration
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if now > signed_cap.capability.expires_at {
            return Err(anyhow::anyhow!("Capability expired"));
        }

        // Verify signature
        let trusted_keys = self.trusted_keys.read().await;
        let pubkey = trusted_keys.get(&signed_cap.capability.issuer)
            .ok_or_else(|| anyhow::anyhow!("Unknown issuer"))?;

        let message = serde_json::to_vec(&signed_cap.capability)?;
        let sig_bytes: [u8; 64] = signed_cap.signature.as_slice().try_into()
            .map_err(|_| anyhow::anyhow!("Invalid signature length"))?;
        let signature = Signature::from_bytes(&sig_bytes);

        use ed25519_dalek::Verifier;
        pubkey.verify(&message, &signature)
            .map_err(|e| anyhow::anyhow!("Signature verification failed: {}", e))?;

        Ok(())
    }

    /// Check if user has permission for resource
    pub async fn authorize(
        &self,
        context: &SecurityContext,
        resource: &str,
        permission: Permissions,
    ) -> Result<bool> {
        // Check if authenticated
        let user = context.user.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not authenticated"))?;

        // Check capabilities first (most specific)
        for signed_cap in &context.capabilities {
            if self.capability_allows(&signed_cap.capability, resource, permission).await? {
                return Ok(true);
            }
        }

        // Check ACLs
        if self.check_acl(user, resource, permission).await? {
            return Ok(true);
        }

        // Check group permissions
        for group in &user.groups {
            if self.check_group_permission(group, resource, permission).await? {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Check if capability allows access
    async fn capability_allows(
        &self,
        cap: &Capability,
        resource: &str,
        permission: Permissions,
    ) -> Result<bool> {
        // Check resource pattern match
        if !self.matches_pattern(&cap.resource, resource) {
            return Ok(false);
        }

        // Check permissions
        if !Permissions(cap.permissions).has(permission) {
            return Ok(false);
        }

        // Check conditions
        for condition in &cap.conditions {
            if !self.check_condition(condition).await? {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Check ACL for user
    async fn check_acl(
        &self,
        user: &User,
        resource: &str,
        permission: Permissions,
    ) -> Result<bool> {
        let acls = self.acls.read().await;

        // Find most specific ACL
        let mut best_match = None;
        let mut best_len = 0;

        for (path, entries) in acls.iter() {
            if resource.starts_with(path) && path.len() > best_len {
                for entry in entries {
                    if entry.principal == user.username || entry.principal == "*" {
                        best_match = Some(entry);
                        best_len = path.len();
                    }
                }
            }
        }

        if let Some(entry) = best_match {
            return Ok(Permissions(entry.permissions).has(permission));
        }

        Ok(false)
    }

    /// Check group permission
    async fn check_group_permission(
        &self,
        group: &str,
        resource: &str,
        permission: Permissions,
    ) -> Result<bool> {
        // Would check group ACLs
        Ok(false)
    }

    /// Pattern matching for resources (glob-style)
    fn matches_pattern(&self, pattern: &str, resource: &str) -> bool {
        // Simple pattern matching (* and **)
        if pattern == "*" {
            return true;
        }
        if pattern == resource {
            return true;
        }
        if pattern.ends_with("/**") {
            let prefix = &pattern[..pattern.len() - 3];
            return resource.starts_with(prefix);
        }
        if pattern.ends_with("/*") {
            let prefix = &pattern[..pattern.len() - 2];
            if resource.starts_with(prefix) {
                let rest = &resource[prefix.len()..];
                return !rest.contains('/');
            }
        }
        false
    }

    /// Check condition
    async fn check_condition(&self, condition: &Condition) -> Result<bool> {
        match condition {
            Condition::TimeWindow(start, end) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                Ok(now >= *start && now <= *end)
            }
            Condition::MfaRequired => {
                // Would check MFA status in context
                Ok(true)
            }
            _ => Ok(true) // Other conditions not implemented yet
        }
    }

    /// Issue a new capability
    pub async fn issue_capability(
        &self,
        subject: String,
        resource: String,
        permissions: Permissions,
        duration_secs: u64,
    ) -> Result<SignedCapability> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let capability = Capability {
            id: uuid::Uuid::new_v4().to_string(),
            issuer: "server".to_string(),
            subject,
            resource,
            permissions: permissions.0,
            issued_at: now,
            expires_at: now + duration_secs,
            max_uses: None,
            delegation_allowed: false,
            conditions: vec![],
        };

        // Sign capability
        let message = serde_json::to_vec(&capability)?;
        let signature = self.server_keypair.sign(&message);

        let signed = SignedCapability {
            capability,
            signature: signature.to_bytes().to_vec(),
        };

        // Store capability
        self.capabilities.write().await
            .insert(signed.capability.id.clone(), signed.clone());

        Ok(signed)
    }

    /// Revoke a capability
    pub async fn revoke_capability(&self, cap_id: String) -> Result<()> {
        self.revoked.write().await.insert(cap_id);
        Ok(())
    }

    /// Add user
    pub async fn add_user(&self, user: User) -> Result<()> {
        self.users.write().await.insert(user.username.clone(), user);
        Ok(())
    }

    /// Add ACL entry
    pub async fn add_acl(&self, path: String, entry: AclEntry) -> Result<()> {
        self.acls.write().await
            .entry(path)
            .or_insert_with(Vec::new)
            .push(entry);
        Ok(())
    }

    /// Trust a public key
    pub async fn trust_key(&self, name: String, pubkey: VerifyingKey) -> Result<()> {
        self.trusted_keys.write().await.insert(name, pubkey);
        Ok(())
    }
}

/// Rate limiting for DoS protection
pub struct RateLimiter {
    limits: Arc<RwLock<HashMap<std::net::IpAddr, (u32, u64)>>>,  // (count, reset_time)
    max_requests: u32,
    window_secs: u64,
}

impl RateLimiter {
    pub fn new(max_requests: u32, window_secs: u64) -> Self {
        Self {
            limits: Arc::new(RwLock::new(HashMap::new())),
            max_requests,
            window_secs,
        }
    }

    pub async fn check(&self, ip: std::net::IpAddr) -> Result<bool> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut limits = self.limits.write().await;
        let (count, reset_time) = limits.entry(ip).or_insert((0, now + self.window_secs));

        if now > *reset_time {
            // Reset window
            *count = 1;
            *reset_time = now + self.window_secs;
            Ok(true)
        } else if *count >= self.max_requests {
            Ok(false)  // Rate limited
        } else {
            *count += 1;
            Ok(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_permissions() {
        let mut perms = Permissions::READ;
        assert!(perms.has(Permissions::READ));
        assert!(!perms.has(Permissions::WRITE));

        perms.add(Permissions::WRITE);
        assert!(perms.has(Permissions::WRITE));
    }

    #[tokio::test]
    async fn test_pattern_matching() {
        let auth = AuthService::new();

        assert!(auth.matches_pattern("*", "/any/path"));
        assert!(auth.matches_pattern("/home/**", "/home/user/file"));
        assert!(auth.matches_pattern("/tmp/*", "/tmp/file"));
        assert!(!auth.matches_pattern("/tmp/*", "/tmp/dir/file"));
    }

    #[tokio::test]
    async fn test_capability_issuance() {
        let auth = AuthService::new();

        let cap = auth.issue_capability(
            "user".to_string(),
            "/tmp/**".to_string(),
            Permissions::READ,
            3600,
        ).await.unwrap();

        assert_eq!(cap.capability.subject, "user");
        assert!(auth.verify_capability(&cap).await.is_ok());
    }
}