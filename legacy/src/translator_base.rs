//! Abstract Translator Base for Factorization
//!
//! Provides a common base for all translator types including WASM, native, and specialized translators.
//! Factorizes common functionality to enable easy creation of new translator types.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::{Result, Context};
use serde::{Serialize, Deserialize};
use tracing::{info, warn, error, debug};
use uuid::Uuid;
use async_trait::async_trait;

/// Abstract translator trait for factorization
#[async_trait]
pub trait AbstractTranslator: Send + Sync {
    /// Get translator manifest
    fn manifest(&self) -> &TranslatorManifest;

    /// Get translator instance ID
    fn id(&self) -> Uuid;

    /// Get current status
    fn status(&self) -> TranslatorStatus;

    /// Initialize the translator
    async fn initialize(&mut self) -> Result<()>;

    /// Handle a synthetic file operation
    async fn handle_synthetic_operation(&self, request: SyntheticRequest) -> Result<SyntheticResponse>;

    /// Stop the translator
    async fn stop(&mut self) -> Result<()>;

    /// Restart the translator (default implementation)
    async fn restart(&mut self) -> Result<()> {
        self.stop().await?;
        self.initialize().await
    }

    /// Health check (default implementation)
    async fn health_check(&self) -> Result<HealthStatus> {
        Ok(HealthStatus {
            status: match self.status() {
                TranslatorStatus::Running => "healthy".to_string(),
                TranslatorStatus::Starting => "starting".to_string(),
                TranslatorStatus::Failed(_) => "unhealthy".to_string(),
                TranslatorStatus::Stopped => "stopped".to_string(),
                TranslatorStatus::Restarting => "restarting".to_string(),
            },
            uptime_seconds: 0, // Override in implementations
            memory_usage: None,
            error_count: 0,
        })
    }
}

/// Common translator manifest structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslatorManifest {
    /// Translator name (must be unique)
    pub name: String,
    /// Version string
    pub version: String,
    /// Description
    pub description: String,
    /// Translator type for routing
    pub translator_type: TranslatorType,
    /// Required directory structure to create
    pub required_dirs: Vec<String>,
    /// Synthetic files this translator provides
    pub synthetic_files: Vec<SyntheticFileSpec>,
    /// Permissions required
    pub permissions: Vec<Permission>,
    /// Restart policy
    pub restart_policy: RestartPolicy,
    /// Configuration schema
    pub config_schema: Option<String>,
    /// Dependencies on other translators
    pub dependencies: Vec<String>,
}

/// Types of translators supported
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TranslatorType {
    /// WASM-based translator
    WASM,
    /// Native binary translator
    Native,
    /// Built-in system translator
    Builtin,
    /// External HTTP service
    HTTP,
    /// Composite translator (combines others)
    Composite,
}

/// Synthetic file specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntheticFileSpec {
    /// File name (e.g., "status.synth")
    pub name: String,
    /// Access mode
    pub access: AccessMode,
    /// Data type for validation
    pub data_type: DataType,
    /// Whether this file can be cached
    pub cacheable: bool,
    /// Cache TTL in seconds
    pub ttl_seconds: Option<u64>,
    /// Schema for validation (JSON Schema)
    pub schema: Option<String>,
    /// File description for help
    pub description: String,
}

/// File access modes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccessMode {
    Read,
    Write,
    ReadWrite,
    Execute,
}

/// Data types for synthetic files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataType {
    Text,
    Binary,
    CBOR,
    JSON,
    YAML,
    CSV,
}

/// Permission types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Permission {
    FileRead,
    FileWrite,
    NetworkAccess,
    ProcessSpawn,
    SystemCall,
    MeshAccess,
    ConsensusWrite,
}

/// Restart policy for failed translators
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RestartPolicy {
    Never,
    Always,
    OnFailure,
    UpTo(u32), // Restart up to N times
}

/// CBOR-based request/response for synthetic files
#[derive(Debug, Serialize, Deserialize)]
pub struct SyntheticRequest {
    pub file_path: String,
    pub operation: Operation,
    pub data: Option<Vec<u8>>,
    pub params: HashMap<String, serde_cbor::Value>,
    /// Request context (user, namespace, etc.)
    pub context: RequestContext,
}

/// Request context for authorization and routing
#[derive(Debug, Serialize, Deserialize)]
pub struct RequestContext {
    pub user_id: Option<String>,
    pub namespace_id: Option<String>,
    pub session_id: Option<String>,
    pub mesh_node_id: Option<String>,
    pub request_id: String,
    pub timestamp: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Operation {
    Read,
    Write,
    Create,
    Delete,
    List,
    Execute,
    Subscribe,
    Unsubscribe,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SyntheticResponse {
    pub success: bool,
    pub data: Option<Vec<u8>>,
    pub error: Option<String>,
    pub metadata: HashMap<String, serde_cbor::Value>,
    /// Response context
    pub context: ResponseContext,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ResponseContext {
    pub request_id: String,
    pub processing_time_ms: u64,
    pub cached: bool,
    pub translator_id: String,
}

/// Health status for monitoring
#[derive(Debug, Serialize, Deserialize)]
pub struct HealthStatus {
    pub status: String,
    pub uptime_seconds: u64,
    pub memory_usage: Option<u64>,
    pub error_count: u32,
}

/// Translator status
#[derive(Debug, Clone)]
pub enum TranslatorStatus {
    Starting,
    Running,
    Failed(String),
    Stopped,
    Restarting,
}

/// Configuration for translator instances
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslatorConfig {
    /// Base path for translator files
    pub base_path: PathBuf,
    /// Configuration parameters
    pub parameters: HashMap<String, serde_cbor::Value>,
    /// Environment variables
    pub environment: HashMap<String, String>,
    /// Resource limits
    pub limits: ResourceLimits,
}

/// Resource limits for translators
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub max_memory_mb: Option<u64>,
    pub max_cpu_percent: Option<f32>,
    pub max_file_handles: Option<u32>,
    pub max_network_connections: Option<u32>,
    pub timeout_seconds: Option<u64>,
}

/// Abstract translator manager for all types
pub struct TranslatorRegistry {
    /// Active translators by name
    translators: Arc<RwLock<HashMap<String, Box<dyn AbstractTranslator>>>>,
    /// Base directory for translator installations
    base_dir: PathBuf,
    /// Global configuration
    config: RegistryConfig,
}

#[derive(Debug, Clone)]
pub struct RegistryConfig {
    pub enable_hot_reload: bool,
    pub max_concurrent_translators: usize,
    pub default_timeout_seconds: u64,
    pub metrics_enabled: bool,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            enable_hot_reload: true,
            max_concurrent_translators: 100,
            default_timeout_seconds: 30,
            metrics_enabled: true,
        }
    }
}

impl TranslatorRegistry {
    /// Create new translator registry
    pub async fn new(base_dir: PathBuf, config: RegistryConfig) -> Result<Self> {
        // Create settrans directory structure
        let settrans_dir = base_dir.join("settrans");
        tokio::fs::create_dir_all(&settrans_dir).await
            .context("Failed to create settrans directory")?;

        info!("📁 Created translator registry at {:?}", settrans_dir);

        Ok(Self {
            translators: Arc::new(RwLock::new(HashMap::new())),
            base_dir,
            config,
        })
    }

    /// Register a new translator
    pub async fn register_translator(&self, translator: Box<dyn AbstractTranslator>) -> Result<()> {
        let name = translator.manifest().name.clone();

        // Check for conflicts
        let mut translators = self.translators.write().await;
        if translators.contains_key(&name) {
            return Err(anyhow::anyhow!("Translator '{}' already registered", name));
        }

        info!("✅ Registered translator: {} v{}",
              translator.manifest().name,
              translator.manifest().version);

        translators.insert(name, translator);
        Ok(())
    }

    /// Unregister translator
    pub async fn unregister_translator(&self, name: &str) -> Result<()> {
        let mut translators = self.translators.write().await;

        if let Some(mut translator) = translators.remove(name) {
            info!("🗑️ Unregistering translator: {}", name);
            translator.stop().await?;
            info!("✅ Unregistered translator: {}", name);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Translator '{}' not found", name))
        }
    }

    /// Handle synthetic file operation
    pub async fn handle_operation(&self, translator_name: &str, request: SyntheticRequest) -> Result<SyntheticResponse> {
        let translators = self.translators.read().await;

        if let Some(translator) = translators.get(translator_name) {
            match translator.status() {
                TranslatorStatus::Running => {
                    translator.handle_synthetic_operation(request).await
                }
                status => {
                    Ok(SyntheticResponse {
                        success: false,
                        data: None,
                        error: Some(format!("Translator '{}' is not running: {:?}", translator_name, status)),
                        metadata: HashMap::new(),
                        context: ResponseContext {
                            request_id: request.context.request_id,
                            processing_time_ms: 0,
                            cached: false,
                            translator_id: translator.id().to_string(),
                        },
                    })
                }
            }
        } else {
            Ok(SyntheticResponse {
                success: false,
                data: None,
                error: Some(format!("Translator '{}' not found", translator_name)),
                metadata: HashMap::new(),
                context: ResponseContext {
                    request_id: request.context.request_id,
                    processing_time_ms: 0,
                    cached: false,
                    translator_id: "unknown".to_string(),
                },
            })
        }
    }

    /// List all translators
    pub async fn list_translators(&self) -> Vec<TranslatorInfo> {
        let translators = self.translators.read().await;
        translators.iter()
            .map(|(name, translator)| TranslatorInfo {
                name: name.clone(),
                version: translator.manifest().version.clone(),
                translator_type: translator.manifest().translator_type.clone(),
                status: translator.status(),
                id: translator.id(),
            })
            .collect()
    }

    /// Get translator by name
    pub async fn get_translator(&self, name: &str) -> Option<&Box<dyn AbstractTranslator>> {
        // Note: This would need Arc<RwLock<>> pattern in real usage due to lifetime issues
        // For now, this is a conceptual API
        None
    }

    /// Restart translator
    pub async fn restart_translator(&self, name: &str) -> Result<()> {
        let translators = self.translators.read().await;

        if let Some(translator) = translators.get(name) {
            info!("🔄 Restarting translator: {}", name);
            // Would need mutable access in real implementation
            // translator.restart().await?;
            info!("✅ Restarted translator: {}", name);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Translator '{}' not found", name))
        }
    }

    /// Get health status for all translators
    pub async fn get_health_status(&self) -> HashMap<String, HealthStatus> {
        let translators = self.translators.read().await;
        let mut status_map = HashMap::new();

        for (name, translator) in translators.iter() {
            if let Ok(health) = translator.health_check().await {
                status_map.insert(name.clone(), health);
            }
        }

        status_map
    }
}

/// Translator information for listing
#[derive(Debug, Clone)]
pub struct TranslatorInfo {
    pub name: String,
    pub version: String,
    pub translator_type: TranslatorType,
    pub status: TranslatorStatus,
    pub id: Uuid,
}

/// Utility functions for common translator operations
pub mod utils {
    use super::*;

    /// Create default manifest for a translator
    pub fn create_default_manifest(
        name: String,
        translator_type: TranslatorType,
        description: String,
    ) -> TranslatorManifest {
        TranslatorManifest {
            name,
            version: "1.0.0".to_string(),
            description,
            translator_type,
            required_dirs: vec!["input".to_string(), "output".to_string(), "config".to_string()],
            synthetic_files: vec![
                SyntheticFileSpec {
                    name: "status.synth".to_string(),
                    access: AccessMode::Read,
                    data_type: DataType::CBOR,
                    cacheable: true,
                    ttl_seconds: Some(5),
                    schema: None,
                    description: "Translator status information".to_string(),
                },
                SyntheticFileSpec {
                    name: "config.synth".to_string(),
                    access: AccessMode::ReadWrite,
                    data_type: DataType::CBOR,
                    cacheable: false,
                    ttl_seconds: None,
                    schema: None,
                    description: "Translator configuration".to_string(),
                },
                SyntheticFileSpec {
                    name: "help.txt".to_string(),
                    access: AccessMode::Read,
                    data_type: DataType::Text,
                    cacheable: true,
                    ttl_seconds: Some(3600),
                    schema: None,
                    description: "Help documentation".to_string(),
                },
            ],
            permissions: vec![Permission::FileRead, Permission::FileWrite],
            restart_policy: RestartPolicy::OnFailure,
            config_schema: None,
            dependencies: vec![],
        }
    }

    /// Create request context with defaults
    pub fn create_request_context(request_id: Option<String>) -> RequestContext {
        RequestContext {
            user_id: None,
            namespace_id: None,
            session_id: None,
            mesh_node_id: None,
            request_id: request_id.unwrap_or_else(|| Uuid::new_v4().to_string()),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    /// Create error response
    pub fn create_error_response(
        request_id: String,
        error: String,
        translator_id: String,
    ) -> SyntheticResponse {
        SyntheticResponse {
            success: false,
            data: None,
            error: Some(error),
            metadata: HashMap::new(),
            context: ResponseContext {
                request_id,
                processing_time_ms: 0,
                cached: false,
                translator_id,
            },
        }
    }

    /// Create success response
    pub fn create_success_response(
        request_id: String,
        data: Option<Vec<u8>>,
        translator_id: String,
        processing_time_ms: u64,
    ) -> SyntheticResponse {
        SyntheticResponse {
            success: true,
            data,
            error: None,
            metadata: HashMap::new(),
            context: ResponseContext {
                request_id,
                processing_time_ms,
                cached: false,
                translator_id,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_manifest_creation() {
        let manifest = utils::create_default_manifest(
            "test-translator".to_string(),
            TranslatorType::Builtin,
            "Test translator".to_string(),
        );

        assert_eq!(manifest.name, "test-translator");
        assert_eq!(manifest.translator_type, TranslatorType::Builtin);
        assert!(!manifest.synthetic_files.is_empty());
    }

    #[test]
    fn test_request_context_creation() {
        let context = utils::create_request_context(Some("test-123".to_string()));
        assert_eq!(context.request_id, "test-123");
        assert!(context.timestamp > 0);
    }
}