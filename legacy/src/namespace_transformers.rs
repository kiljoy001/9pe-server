//! Namespace Transformers - /n/ and /srv virtual directories
//!
//! Implements Plan 9's namespace mounting and service registry as programmable transformers

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};

use crate::synthetic::SyntheticGenerator;
use crate::namespace_consensus::{NamespaceConsensus, EventType, FileOperation};

/// Namespace mount point - represents a mounted namespace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceMount {
    pub namespace_id: String,
    pub mount_point: String,  // e.g., "/n/workspace"
    pub permissions: u32,     // Read/write/execute permissions
    pub mounted_at: u64,      // Timestamp when mounted
    pub remote_address: Option<String>, // If it's a remote namespace
}

/// Service in the /srv registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceEntry {
    pub service_name: String,
    pub namespace_id: String,
    pub service_type: ServiceType,
    pub endpoint: String,
    pub capabilities: Vec<String>,
    pub registered_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServiceType {
    /// File service (standard 9P.e)
    FileService,
    /// Compute service (GPU/CPU pool)
    ComputeService,
    /// Authentication service
    AuthService,
    /// Custom service
    Custom(String),
}

/// /n/ namespace directory transformer
/// Provides virtual mounting of namespaces as directories
pub struct NamespaceMountGenerator {
    /// Currently mounted namespaces
    mounts: Arc<RwLock<HashMap<String, NamespaceMount>>>,
    /// Reference to namespace consensus
    consensus: Option<Arc<NamespaceConsensus>>,
}

impl NamespaceMountGenerator {
    pub fn new(consensus: Option<Arc<NamespaceConsensus>>) -> Self {
        Self {
            mounts: Arc::new(RwLock::new(HashMap::new())),
            consensus,
        }
    }

    /// Mount a namespace at /n/<name>
    pub async fn mount_namespace(
        &self,
        namespace_id: String,
        mount_name: String,
        permissions: u32,
    ) -> Result<()> {
        let mount_point = format!("/n/{}", mount_name);

        // If we have consensus, join the namespace
        if let Some(consensus) = &self.consensus {
            consensus.join_namespace(namespace_id.clone()).await?;
        }

        let mount = NamespaceMount {
            namespace_id: namespace_id.clone(),
            mount_point: mount_point.clone(),
            permissions,
            mounted_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
            remote_address: None,
        };

        self.mounts.write().await.insert(mount_name, mount);

        // Track the mount event
        if let Some(consensus) = &self.consensus {
            let event = EventType::FileOp {
                path: mount_point,
                op: FileOperation::Execute, // Mount is like executing
            };
            consensus.submit_event(&namespace_id, event).await?;
        }

        Ok(())
    }

    /// Unmount a namespace
    pub async fn unmount_namespace(&self, mount_name: &str) -> Result<()> {
        if let Some(mount) = self.mounts.write().await.remove(mount_name) {
            // Track the unmount event
            if let Some(consensus) = &self.consensus {
                let event = EventType::FileOp {
                    path: mount.mount_point,
                    op: FileOperation::Delete, // Unmount is like deletion
                };
                consensus.submit_event(&mount.namespace_id, event).await?;
            }
        }
        Ok(())
    }

    /// List all mounted namespaces
    pub async fn list_mounts(&self) -> Vec<(String, NamespaceMount)> {
        self.mounts.read().await
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Check if a namespace is mounted
    pub async fn is_mounted(&self, mount_name: &str) -> bool {
        self.mounts.read().await.contains_key(mount_name)
    }
}

#[async_trait]
impl SyntheticGenerator for NamespaceMountGenerator {
    async fn generate(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        let mounts = self.mounts.read().await;

        let mut content = String::new();
        content.push_str("# Namespace Mounts (/n/)\n\n");

        if mounts.is_empty() {
            content.push_str("No namespaces mounted.\n\n");
            content.push_str("To mount a namespace:\n");
            content.push_str("  echo 'workspace' > /n/ctl  # Mount namespace 'workspace' at /n/workspace\n");
            content.push_str("  echo 'gpu_pool' > /n/ctl   # Mount namespace 'gpu_pool' at /n/gpu_pool\n\n");
        } else {
            for (mount_name, mount) in mounts.iter() {
                content.push_str(&format!("## /n/{}\n", mount_name));
                content.push_str(&format!("- Namespace: {}\n", mount.namespace_id));
                content.push_str(&format!("- Permissions: {:o}\n", mount.permissions));
                content.push_str(&format!("- Mounted: {} seconds ago\n",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs() - mount.mounted_at));

                if let Some(addr) = &mount.remote_address {
                    content.push_str(&format!("- Remote: {}\n", addr));
                }
                content.push_str("\n");
            }
        }

        // Show available operations
        content.push_str("## Operations\n");
        content.push_str("- `cat /n/` - List mounted namespaces\n");
        content.push_str("- `echo 'nsname' > /n/ctl` - Mount namespace\n");
        content.push_str("- `echo 'unmount nsname' > /n/ctl` - Unmount namespace\n");
        content.push_str("- `ls /n/nsname/` - Browse namespace files\n");

        let bytes = content.as_bytes();
        let start = offset.min(bytes.len() as u64) as usize;
        let end = (start + count as usize).min(bytes.len());
        Ok(bytes[start..end].to_vec())
    }

    async fn size(&self) -> u64 {
        4096 // Dynamic size for namespace listing
    }

    fn refresh_rate_ms(&self) -> u64 {
        5000 // Refresh every 5 seconds
    }
}

/// /srv/ service registry transformer
/// Enhanced version of the existing ServiceDiscoveryGenerator with namespace integration
pub struct ServiceRegistryGenerator {
    /// Registered services by name
    services: Arc<RwLock<HashMap<String, ServiceEntry>>>,
    /// Reference to namespace consensus
    consensus: Option<Arc<NamespaceConsensus>>,
}

impl ServiceRegistryGenerator {
    pub fn new(consensus: Option<Arc<NamespaceConsensus>>) -> Self {
        Self {
            services: Arc::new(RwLock::new(HashMap::new())),
            consensus,
        }
    }

    /// Register a service in the registry
    pub async fn register_service(
        &self,
        service_name: String,
        namespace_id: String,
        service_type: ServiceType,
        endpoint: String,
        capabilities: Vec<String>,
    ) -> Result<()> {
        let service = ServiceEntry {
            service_name: service_name.clone(),
            namespace_id: namespace_id.clone(),
            service_type,
            endpoint,
            capabilities,
            registered_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
        };

        self.services.write().await.insert(service_name.clone(), service);

        // Track service registration in consensus
        if let Some(consensus) = &self.consensus {
            let event = EventType::FileOp {
                path: format!("/srv/{}", service_name),
                op: FileOperation::Write, // Registration is like writing
            };
            consensus.submit_event(&namespace_id, event).await?;
        }

        Ok(())
    }

    /// Unregister a service
    pub async fn unregister_service(&self, service_name: &str) -> Result<()> {
        if let Some(service) = self.services.write().await.remove(service_name) {
            // Track service unregistration
            if let Some(consensus) = &self.consensus {
                let event = EventType::FileOp {
                    path: format!("/srv/{}", service_name),
                    op: FileOperation::Delete,
                };
                consensus.submit_event(&service.namespace_id, event).await?;
            }
        }
        Ok(())
    }

    /// List all services
    pub async fn list_services(&self) -> Vec<(String, ServiceEntry)> {
        self.services.read().await
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Get services by namespace
    pub async fn get_namespace_services(&self, namespace_id: &str) -> Vec<(String, ServiceEntry)> {
        self.services.read().await
            .iter()
            .filter(|(_, service)| service.namespace_id == namespace_id)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Get compute services (for GPU/CPU pooling)
    pub async fn get_compute_services(&self) -> Vec<(String, ServiceEntry)> {
        self.services.read().await
            .iter()
            .filter(|(_, service)| matches!(service.service_type, ServiceType::ComputeService))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

#[async_trait]
impl SyntheticGenerator for ServiceRegistryGenerator {
    async fn generate(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        let services = self.services.read().await;

        let mut content = String::new();
        content.push_str("# Service Registry (/srv/)\n\n");

        if services.is_empty() {
            content.push_str("No services registered.\n\n");
            content.push_str("Services will appear here as they are discovered and registered.\n");
            content.push_str("Each service becomes a file in /srv/ that you can connect to.\n\n");
        } else {
            // Group by service type
            let mut file_services = Vec::new();
            let mut compute_services = Vec::new();
            let mut auth_services = Vec::new();
            let mut custom_services = Vec::new();

            for (name, service) in services.iter() {
                match service.service_type {
                    ServiceType::FileService => file_services.push((name, service)),
                    ServiceType::ComputeService => compute_services.push((name, service)),
                    ServiceType::AuthService => auth_services.push((name, service)),
                    ServiceType::Custom(_) => custom_services.push((name, service)),
                }
            }

            if !file_services.is_empty() {
                content.push_str("## 📁 File Services\n");
                for (name, service) in file_services {
                    content.push_str(&format!("- `/srv/{}` → {}\n", name, service.endpoint));
                    content.push_str(&format!("  - Namespace: {}\n", service.namespace_id));
                    content.push_str(&format!("  - Capabilities: {}\n", service.capabilities.join(", ")));
                }
                content.push_str("\n");
            }

            if !compute_services.is_empty() {
                content.push_str("## 🖥️ Compute Services\n");
                for (name, service) in compute_services {
                    content.push_str(&format!("- `/srv/{}` → {}\n", name, service.endpoint));
                    content.push_str(&format!("  - Namespace: {}\n", service.namespace_id));
                    content.push_str(&format!("  - GPU/CPU Pool: {}\n", service.capabilities.join(", ")));
                }
                content.push_str("\n");
            }

            if !auth_services.is_empty() {
                content.push_str("## 🔐 Auth Services\n");
                for (name, service) in auth_services {
                    content.push_str(&format!("- `/srv/{}` → {}\n", name, service.endpoint));
                }
                content.push_str("\n");
            }

            if !custom_services.is_empty() {
                content.push_str("## 🔧 Custom Services\n");
                for (name, service) in custom_services {
                    if let ServiceType::Custom(ref type_name) = service.service_type {
                        content.push_str(&format!("- `/srv/{}` ({}) → {}\n", name, type_name, service.endpoint));
                    }
                }
                content.push_str("\n");
            }
        }

        // Usage instructions
        content.push_str("## Usage\n");
        content.push_str("- `ls /srv/` - List all services\n");
        content.push_str("- `cat /srv/service_name` - Get service info\n");
        content.push_str("- `cp /srv/service_name /n/local/connection` - Connect to service\n");
        content.push_str("- Services automatically appear as they join namespaces\n");

        let bytes = content.as_bytes();
        let start = offset.min(bytes.len() as u64) as usize;
        let end = (start + count as usize).min(bytes.len());
        Ok(bytes[start..end].to_vec())
    }

    async fn size(&self) -> u64 {
        8192 // Larger dynamic size for service listing
    }

    fn refresh_rate_ms(&self) -> u64 {
        3000 // Refresh every 3 seconds for real-time service discovery
    }
}

/// Control file generator for namespace operations
/// Allows writing commands to /n/ctl and /srv/ctl
pub struct NamespaceControlGenerator {
    mount_generator: Arc<NamespaceMountGenerator>,
    service_generator: Arc<ServiceRegistryGenerator>,
}

impl NamespaceControlGenerator {
    pub fn new(
        mount_generator: Arc<NamespaceMountGenerator>,
        service_generator: Arc<ServiceRegistryGenerator>,
    ) -> Self {
        Self {
            mount_generator,
            service_generator,
        }
    }

    /// Process a control command
    pub async fn process_command(&self, command: &str, target: &str) -> Result<String> {
        let parts: Vec<&str> = command.trim().split_whitespace().collect();

        match target {
            "/n/ctl" => {
                match parts.as_slice() {
                    [namespace_id] => {
                        // Mount namespace with default permissions
                        self.mount_generator.mount_namespace(
                            namespace_id.to_string(),
                            namespace_id.to_string(),
                            0o755,
                        ).await?;
                        Ok(format!("Mounted namespace '{}' at /n/{}", namespace_id, namespace_id))
                    },
                    ["mount", namespace_id, mount_name] => {
                        self.mount_generator.mount_namespace(
                            namespace_id.to_string(),
                            mount_name.to_string(),
                            0o755,
                        ).await?;
                        Ok(format!("Mounted namespace '{}' at /n/{}", namespace_id, mount_name))
                    },
                    ["unmount", mount_name] => {
                        self.mount_generator.unmount_namespace(mount_name).await?;
                        Ok(format!("Unmounted /n/{}", mount_name))
                    },
                    _ => Ok("Usage: echo 'namespace_id' > /n/ctl OR echo 'mount ns_id mount_name' > /n/ctl OR echo 'unmount mount_name' > /n/ctl".to_string()),
                }
            },
            "/srv/ctl" => {
                match parts.as_slice() {
                    ["register", service_name, namespace_id, service_type, endpoint] => {
                        let stype = match *service_type {
                            "file" => ServiceType::FileService,
                            "compute" => ServiceType::ComputeService,
                            "auth" => ServiceType::AuthService,
                            custom => ServiceType::Custom(custom.to_string()),
                        };

                        self.service_generator.register_service(
                            service_name.to_string(),
                            namespace_id.to_string(),
                            stype,
                            endpoint.to_string(),
                            vec![], // Empty capabilities for now
                        ).await?;

                        Ok(format!("Registered service '{}' at /srv/{}", service_name, service_name))
                    },
                    ["unregister", service_name] => {
                        self.service_generator.unregister_service(service_name).await?;
                        Ok(format!("Unregistered service '{}'", service_name))
                    },
                    _ => Ok("Usage: echo 'register service_name namespace_id service_type endpoint' > /srv/ctl OR echo 'unregister service_name' > /srv/ctl".to_string()),
                }
            },
            _ => Ok("Unknown control target".to_string()),
        }
    }
}

#[async_trait]
impl SyntheticGenerator for NamespaceControlGenerator {
    async fn generate(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        let content = "# Namespace Control Interface\n\n\
            Write commands to this file to control namespaces and services:\n\
            - /n/ctl: Mount/unmount namespaces\n\
            - /srv/ctl: Register/unregister services\n\n\
            This file is write-only - commands are processed immediately.\n";

        let bytes = content.as_bytes();
        let start = offset.min(bytes.len() as u64) as usize;
        let end = (start + count as usize).min(bytes.len());
        Ok(bytes[start..end].to_vec())
    }

    async fn size(&self) -> u64 {
        256 // Small control file
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_namespace_mount() {
        let generator = NamespaceMountGenerator::new(None);

        // Mount a namespace
        generator.mount_namespace(
            "test_ns".to_string(),
            "workspace".to_string(),
            0o755,
        ).await.unwrap();

        // Check it's mounted
        assert!(generator.is_mounted("workspace").await);

        // List mounts
        let mounts = generator.list_mounts().await;
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].0, "workspace");
    }

    #[tokio::test]
    async fn test_service_registry() {
        let generator = ServiceRegistryGenerator::new(None);

        // Register a service
        generator.register_service(
            "test_service".to_string(),
            "test_ns".to_string(),
            ServiceType::FileService,
            "192.168.1.100:5641".to_string(),
            vec!["read".to_string(), "write".to_string()],
        ).await.unwrap();

        // List services
        let services = generator.list_services().await;
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].0, "test_service");
    }
}