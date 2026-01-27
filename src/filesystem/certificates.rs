//! 9P certificate filesystem for cluster authentication
//!
//! Provides certificate management as 9P file operations

use anyhow::Result;
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use std::time::SystemTime;
use tracing::info;

use crate::identity::NodePermissions;

/// 9P certificate filesystem implementation
pub struct CertificateFilesystem {
    node_permissions: HashMap<String, NodePermissions>,
    certificate_cache: HashMap<String, SovereignCertificate>,
    audit_log: Vec<AuditEntry>,
    revoked_certificates: HashMap<Vec<u8>, RevocationStatus>,
}

impl CertificateFilesystem {
    pub fn new() -> Self {
        Self {
            node_permissions: HashMap::new(),
            certificate_cache: HashMap::new(),
            audit_log: Vec::new(),
            revoked_certificates: HashMap::new(),
        }
    }

    /// Handle certificate file operations
    pub async fn handle_file_operation(
        &mut self,
        path: &Path,
        operation: FileOperation,
        data: &[u8],
    ) -> Result<Vec<u8>, FilesystemError> {
        let path_str = path.to_string_lossy();
        
        match path_str.as_ref() {
            "node-certificates/index" => self.handle_node_index_read(),
            path if path_str.starts_with("node-certificates/") => {
                self.handle_node_certificate_operation(path_str, operation, data)
            }
            path if path_str.starts_with("enrollment/") => {
                self.handle_enrollment_operation(path_str, operation, data)
            }
            path if path_str.starts_with("permissions/") => {
                self.handle_permission_operation(path_str, operation, data)
            }
            path if path_str.starts_with("revocation/") => {
                self.handle_revocation_operation(path_str, operation, data)
            }
            path if path_str.starts_with("audit/") => {
                self.handle_audit_operation(path_str, operation, data)
            }
            _ => Err(FilesystemError::FileNotFound),
        }
    }

    fn handle_node_index_read(&self) -> Result<Vec<u8>, FilesystemError> {
        let mut node_ids: Vec<String> = self.certificate_cache.keys().cloned().collect();
        node_ids.sort();
        let json = serde_json::to_string(&node_ids)
            .map_err(|_| FilesystemError::SerializationFailed)?;
        Ok(json.into_bytes())
    }

    /// Handle node certificate operations
    fn handle_node_certificate_operation(
        &mut self,
        path: String,
        operation: FileOperation,
        data: &[u8],
    ) -> Result<Vec<u8>, FilesystemError> {
        // Extract node ID from path
        let node_id = extract_node_id_from_path(&path)?;
        
        match operation {
            FileOperation::Read => {
                let cert = self.get_node_certificate(&node_id)?;
                let cert_data = self.node_certificate_to_pem(&cert)?;
                self.log_audit(&format!("node-certificates/{}", node_id), "read", "");
                Ok(cert_data)
            }
            FileOperation::Write => {
                let mut request = deserialize_enrollment_request(data)?;
                if request.node_id.is_empty() {
                    request.node_id = node_id;
                }
                let cert = self.enroll_node_certificate(request).await?;
                Ok(self.node_certificate_to_pem(&cert)?)
            }
            FileOperation::Stat => {
                let cert = self.get_node_certificate(&node_id)?;
                let stat = node_certificate_stat(&cert)?;
                Ok(serialize_stat(&stat))
            }
            FileOperation::Remove => {
                // Only allow revocation, not direct removal
                Err(FilesystemError::PermissionDenied)
            }
        }
    }

    /// Handle enrollment operations
    fn handle_enrollment_operation(
        &mut self,
        path: String,
        operation: FileOperation,
        data: &[u8],
    ) -> Result<Vec<u8>, FilesystemError> {
        if path.contains("request") {
            // Self-signed certificate enrollment
            self.handle_enrollment_request(data).await
        } else {
            Err(FilesystemError::FileNotFound)
        }
    }

    /// Handle permission operations
    fn handle_permission_operation(
        &mut self,
        path: String,
        operation: FileOperation,
        data: &[u8],
    ) -> Result<Vec<u8>, FilesystemError> {
        let node_id = extract_node_id_from_path(&path)?;
        
        match operation {
            FileOperation::Read => {
                let permissions = self.get_node_permissions(&node_id)?;
                let perm_data = serialize_permissions(permissions)?;
                self.log_audit(&format!("permissions/{}", node_id), "read", "");
                Ok(perm_data)
            }
            FileOperation::Write => {
                let permissions = deserialize_permissions(data)?;
                self.update_node_permissions(&node_id, permissions)?;
                self.log_audit(&format!("permissions/{}", node_id), "write", &format!("{} bytes", data.len()));
                Ok(vec![])
            }
            _ => Err(FilesystemError::PermissionDenied),
        }
    }

    /// Handle revocation operations
    fn handle_revocation_operation(
        &mut self,
        path: String,
        operation: FileOperation,
        data: &[u8],
    ) -> Result<Vec<u8>, FilesystemError> {
        match operation {
            FileOperation::Write => {
                // Write to revocation list
                self.revoke_certificate(data).await
            }
            FileOperation::Read => {
                // Read revocation status
                let status = self.get_revocation_status(data)?;
                let status_data = serialize_revocation_status(&status)?;
                Ok(status_data)
            }
            _ => Err(FilesystemError::PermissionDenied),
        }
    }

    /// Handle audit operations
    fn handle_audit_operation(
        &self,
        path: String,
        operation: FileOperation,
        data: &[u8],
    ) -> Result<Vec<u8>, FilesystemError> {
        match operation {
            FileOperation::Read => {
                if path.contains("recent") {
                    // Recent audit log
                    let recent_logs = self.get_recent_audit_logs()?;
                    let logs_data = serialize_audit_logs(&recent_logs)?;
                    Ok(logs_data)
                } else {
                    Err(FilesystemError::PermissionDenied)
                }
            }
            _ => Err(FilesystemError::PermissionDenied),
        }
    }

    /// Process certificate enrollment request
    async fn handle_enrollment_request(
        &mut self,
        data: &[u8],
    ) -> Result<Vec<u8>, FilesystemError> {
        // Parse enrollment request
        let enrollment_request = deserialize_enrollment_request(data)?;
        let node_id = enrollment_request.node_id.clone();
        
        info!("Processing enrollment request for node: {}", node_id);
        
        // Validate request
        if node_id.is_empty() {
            return Err(FilesystemError::InvalidRequest);
        }
        
        // Store self-signed certificate
        let cert = self.enroll_node_certificate(enrollment_request).await?;
        
        // Log enrollment
        self.log_audit("enrollment/request", "processed", &node_id);
        
        // Return certificate in PEM format
        Ok(self.node_certificate_to_pem(&cert)?)
    }

    /// Update node permissions
    fn update_node_permissions(
        &mut self,
        node_id: &str,
        permissions: NodePermissions,
    ) -> Result<(), FilesystemError> {
        // Validate permissions
        if permissions.max_concurrent_jobs == 0 {
            return Err(FilesystemError::InvalidPermissions);
        }
        
        self.node_permissions.insert(node_id.to_string(), permissions);
        
        Ok(())
    }

    /// Get node certificate
    fn get_node_certificate(&self, node_id: &str) -> Result<SovereignCertificate, FilesystemError> {
        self.certificate_cache
            .get(node_id)
            .cloned()
            .ok_or(FilesystemError::CertificateNotFound)
    }

    /// Get node permissions
    fn get_node_permissions(&self, node_id: &str) -> Result<NodePermissions, FilesystemError> {
        self.node_permissions
            .get(node_id)
            .cloned()
            .ok_or(FilesystemError::PermissionsNotFound)
    }

    /// Node certificate to PEM
    fn node_certificate_to_pem(&self, cert: &SovereignCertificate) -> Result<Vec<u8>, FilesystemError> {
        Ok(pem_wrap("9PE CERTIFICATE", &cert.certificate_der))
    }

    /// Revoke certificate
    async fn revoke_certificate(
        &mut self,
        data: &[u8],
    ) -> Result<Vec<u8>, FilesystemError> {
        let revocation_request = deserialize_revocation_request(data)?;
        
        info!("Revoking certificate: {}", revocation_request.serial_number);
        
        let status = RevocationStatus {
            is_revoked: true,
            revocation_reason: Some(revocation_request.reason),
            revocation_time: Some(SystemTime::now()),
            revocation_location: None,
        };

        self.revoked_certificates
            .insert(revocation_request.serial_number.clone(), status);

        if let Some(cert) = self
            .certificate_cache
            .values_mut()
            .find(|c| c.serial_number == revocation_request.serial_number)
        {
            cert.is_revoked = true;
        }
        
        self.log_audit(
            "revocation/list",
            "updated",
            &format!("Serial: {:?}", revocation_request.serial_number),
        );
        
        Ok("Certificate revoked".to_string().into_bytes())
    }

    /// Get revocation status
    fn get_revocation_status(
        &self,
        data: &[u8],
    ) -> Result<RevocationStatus, FilesystemError> {
        let serial_number = extract_serial_number_from_data(data)?;
        
        // Check revocation status
        Ok(self
            .revoked_certificates
            .get(&serial_number)
            .cloned()
            .unwrap_or(RevocationStatus {
                is_revoked: false,
                revocation_reason: None,
                revocation_time: None,
                revocation_location: None,
            }))
    }

    /// Get recent audit logs
    fn get_recent_audit_logs(&self) -> Result<Vec<AuditEntry>, FilesystemError> {
        // Return last 100 audit entries
        let recent = self.audit_log
            .iter()
            .rev()
            .take(100)
            .cloned()
            .collect();
        Ok(recent)
    }

    /// Log audit entry
    fn log_audit(&mut self, target: &str, operation: &str, details: &str) {
        let entry = AuditEntry {
            timestamp: SystemTime::now(),
            target: target.to_string(),
            operation: operation.to_string(),
            details: details.to_string(),
            source: "filesystem".to_string(),
        };
        self.audit_log.push(entry);
        
        // Keep only last 1000 entries
        if self.audit_log.len() > 1000 {
            self.audit_log.remove(0);
        }
    }

    async fn enroll_node_certificate(
        &mut self,
        request: EnrollmentRequest,
    ) -> Result<SovereignCertificate, FilesystemError> {
        if request.node_id.is_empty() {
            return Err(FilesystemError::InvalidRequest);
        }

        let cert = SovereignCertificate::from_request(&request)?;

        self.certificate_cache
            .insert(request.node_id.clone(), cert.clone());
        self.node_permissions
            .insert(request.node_id.clone(), request.permissions.clone());

        self.log_audit(
            "enrollment/request",
            "approved",
            &request.node_id,
        );

        Ok(cert)
    }
}

// Helper functions for data serialization/deserialization
fn extract_node_id_from_path(path: &str) -> Result<String, FilesystemError> {
    // Extract node ID from path like "node-certificates/node-001.crt"
    if let Some(stripped) = path.strip_prefix("node-certificates/") {
        if let Some((node_id, _)) = stripped.split_once('.') {
            return Ok(node_id.to_string());
        }
    }
    Err(FilesystemError::InvalidPath)
}

fn deserialize_enrollment_request(data: &[u8]) -> Result<EnrollmentRequest, FilesystemError> {
    // Parse enrollment request from JSON
    let request: EnrollmentRequest = serde_json::from_slice(data)
        .map_err(|_| FilesystemError::InvalidRequest)?;
    Ok(request)
}

fn serialize_permissions(permissions: &NodePermissions) -> Result<Vec<u8>, FilesystemError> {
    let json = serde_json::to_string(permissions)
        .map_err(|_| FilesystemError::SerializationFailed)?;
    Ok(json.into_bytes())
}

fn deserialize_permissions(data: &[u8]) -> Result<NodePermissions, FilesystemError> {
    let permissions: NodePermissions = serde_json::from_slice(data)
        .map_err(|_| FilesystemError::InvalidPermissions)?;
    Ok(permissions)
}

fn serialize_stat(stat: &NodeCertificateStat) -> Result<Vec<u8>, FilesystemError> {
    let json = serde_json::to_string(stat)
        .map_err(|_| FilesystemError::SerializationFailed)?;
    Ok(json.into_bytes())
}

fn node_certificate_stat(cert: &SovereignCertificate) -> Result<NodeCertificateStat, FilesystemError> {
    Ok(NodeCertificateStat {
        node_id: cert.node_id.clone(),
        issued_at: cert.issued_at,
        expires_at: cert.expires_at,
        serial_number: cert.serial_number.clone(),
        status: if cert.is_revoked { "revoked" } else { "valid" }.to_string(),
    })
}

fn deserialize_revocation_request(data: &[u8]) -> Result<RevocationRequest, FilesystemError> {
    let request: RevocationRequest = serde_json::from_slice(data)
        .map_err(|_| FilesystemError::InvalidRequest)?;
    Ok(request)
}

fn extract_serial_number_from_data(data: &[u8]) -> Result<Vec<u8>, FilesystemError> {
    if data.is_empty() {
        return Err(FilesystemError::InvalidRequest);
    }

    if let Ok(request) = serde_json::from_slice::<RevocationRequest>(data) {
        return Ok(request.serial_number);
    }

    Ok(data.to_vec())
}

fn serialize_revocation_status(status: &RevocationStatus) -> Result<Vec<u8>, FilesystemError> {
    let json = serde_json::to_string(status)
        .map_err(|_| FilesystemError::SerializationFailed)?;
    Ok(json.into_bytes())
}

fn serialize_audit_logs(logs: &[AuditEntry]) -> Result<Vec<u8>, FilesystemError> {
    let json = serde_json::to_string(logs)
        .map_err(|_| FilesystemError::SerializationFailed)?;
    Ok(json.into_bytes())
}

fn pem_wrap(label: &str, data: &[u8]) -> Vec<u8> {
    let encoded = general_purpose::STANDARD.encode(data);
    let mut pem = String::new();
    pem.push_str("-----BEGIN ");
    pem.push_str(label);
    pem.push_str("-----\n");

    for chunk in encoded.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        pem.push('\n');
    }

    pem.push_str("-----END ");
    pem.push_str(label);
    pem.push_str("-----\n");
    pem.into_bytes()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SovereignCertificate {
    pub node_id: String,
    pub certificate_der: Vec<u8>,
    pub ed25519_public_key: Vec<u8>,
    pub p256_public_key: Vec<u8>,
    pub serial_number: Vec<u8>,
    pub issued_at: SystemTime,
    pub expires_at: Option<SystemTime>,
    pub permissions: NodePermissions,
    pub is_revoked: bool,
}

impl SovereignCertificate {
    fn from_request(request: &EnrollmentRequest) -> Result<Self, FilesystemError> {
        if request.node_id.is_empty()
            || request.ed25519_public_key.len() != 32
            || request.p256_public_key.is_empty()
            || request.certificate_der.is_empty()
        {
            return Err(FilesystemError::InvalidRequest);
        }

        let mut hasher = Sha256::new();
        hasher.update(&request.certificate_der);
        let serial_number = hasher.finalize().to_vec();

        Ok(Self {
            node_id: request.node_id.clone(),
            certificate_der: request.certificate_der.clone(),
            ed25519_public_key: request.ed25519_public_key.clone(),
            p256_public_key: request.p256_public_key.clone(),
            serial_number,
            issued_at: SystemTime::now(),
            expires_at: request.permissions.expires_at,
            permissions: request.permissions.clone(),
            is_revoked: false,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevocationStatus {
    pub is_revoked: bool,
    pub revocation_reason: Option<RevocationReason>,
    pub revocation_time: Option<SystemTime>,
    pub revocation_location: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RevocationReason {
    Unspecified,
    KeyCompromise,
    AffiliationChanged,
    Superseded,
    CessationOfOperation,
    PrivilegeWithdrawn,
}

// Data structures for certificate operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentRequest {
    pub node_id: String,
    pub ed25519_public_key: Vec<u8>,
    pub p256_public_key: Vec<u8>,
    pub certificate_der: Vec<u8>,
    pub permissions: NodePermissions,
    pub contact_info: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCertificateStat {
    pub node_id: String,
    pub issued_at: SystemTime,
    pub expires_at: Option<SystemTime>,
    pub serial_number: Vec<u8>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevocationRequest {
    pub serial_number: Vec<u8>,
    pub reason: RevocationReason,
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: SystemTime,
    pub target: String,
    pub operation: String,
    pub details: String,
    pub source: String,
}

/// 9P file operations
#[derive(Debug, Clone)]
pub enum FileOperation {
    Read,
    Write,
    Stat,
    Remove,
    Create,
}

/// Filesystem errors
#[derive(Debug, thiserror::Error)]
pub enum FilesystemError {
    #[error("File not found")]
    FileNotFound,
    
    #[error("Permission denied")]
    PermissionDenied,
    
    #[error("Invalid path")]
    InvalidPath,
    
    #[error("Invalid request")]
    InvalidRequest,
    
    #[error("Invalid permissions")]
    InvalidPermissions,
    
    #[error("Certificate not found")]
    CertificateNotFound,
    
    #[error("Permissions not found")]
    PermissionsNotFound,
    
    #[error("Serialization failed")]
    SerializationFailed,
    
    #[error("Internal error")]
    Internal(#[from] anyhow::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_node_id_from_path() {
        let path = "node-certificates/node-001.crt";
        let node_id = extract_node_id_from_path(path).unwrap();
        assert_eq!(node_id, "node-001");
    }
}
