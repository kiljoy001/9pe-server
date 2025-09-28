//! Namespace Management Translator
//!
//! Built-in translator for managing namespaces globally across the mesh.
//! Provides synthetic files for namespace operations, join requests, and member management.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use anyhow::{Result, Context};
use serde::{Serialize, Deserialize};
use tracing::{info, warn, error, debug};
use uuid::Uuid;
use async_trait::async_trait;
// Note: Ed25519 imports temporarily disabled for MVP
// use ed25519_dalek::{PublicKey, Signature, Keypair};

use crate::translator_base::{
    AbstractTranslator, TranslatorManifest, TranslatorType, SyntheticFileSpec,
    AccessMode, DataType, Permission, RestartPolicy, SyntheticRequest, SyntheticResponse,
    Operation, TranslatorStatus, HealthStatus, RequestContext, ResponseContext,
    utils,
};
use crate::namespaces::{NamespaceManager, NamespacePermissions, ThresholdConfig, NamespacePolicies};
use crate::global_event_chain::GlobalEventChain;

/// Namespace management translator
pub struct NamespaceTranslator {
    manifest: TranslatorManifest,
    id: Uuid,
    status: TranslatorStatus,
    namespace_manager: Arc<NamespaceManager>,
    // Temporarily disabled due to thread safety issues - will be addressed in mesh networking integration
    // event_chain: Option<Arc<GlobalEventChain>>,
    start_time: SystemTime,
    operation_count: Arc<RwLock<u64>>,
    error_count: Arc<RwLock<u32>>,
}

impl NamespaceTranslator {
    /// Create new namespace translator
    pub fn new(namespace_manager: Arc<NamespaceManager>) -> Self {
        let manifest = Self::create_manifest();

        Self {
            manifest,
            id: Uuid::new_v4(),
            status: TranslatorStatus::Starting,
            namespace_manager,
            // event_chain is temporarily disabled for thread safety
            start_time: SystemTime::now(),
            operation_count: Arc::new(RwLock::new(0)),
            error_count: Arc::new(RwLock::new(0)),
        }
    }

    /// Create the namespace translator manifest
    fn create_manifest() -> TranslatorManifest {
        TranslatorManifest {
            name: "namespace-manager".to_string(),
            version: "1.0.0".to_string(),
            description: "Global namespace management with threshold signatures".to_string(),
            translator_type: TranslatorType::Builtin,
            required_dirs: vec![
                "namespaces".to_string(),
                "requests".to_string(),
                "members".to_string(),
                "admin".to_string(),
                "discovery".to_string(),
                "docs".to_string()
            ],
            synthetic_files: vec![
                SyntheticFileSpec {
                    name: "namespaces/create.synth".to_string(),
                    access: AccessMode::Write,
                    data_type: DataType::CBOR,
                    cacheable: false,
                    ttl_seconds: None,
                    schema: Some(r#"{"type":"object","properties":{"name":{"type":"string"},"threshold":{"type":"object"},"policies":{"type":"object"}},"required":["name"]}"#.to_string()),
                    description: "Create new namespace - write namespace creation request".to_string(),
                },
                SyntheticFileSpec {
                    name: "requests/join.synth".to_string(),
                    access: AccessMode::Write,
                    data_type: DataType::CBOR,
                    cacheable: false,
                    ttl_seconds: None,
                    schema: Some(r#"{"type":"object","properties":{"namespace_id":{"type":"string"},"message":{"type":"string"},"permissions":{"type":"object"}},"required":["namespace_id"]}"#.to_string()),
                    description: "Request to join namespace - write join request".to_string(),
                },
                SyntheticFileSpec {
                    name: "requests/approve.synth".to_string(),
                    access: AccessMode::Write,
                    data_type: DataType::CBOR,
                    cacheable: false,
                    ttl_seconds: None,
                    schema: Some(r#"{"type":"object","properties":{"namespace_id":{"type":"string"},"requester":{"type":"string"},"signature":{"type":"string"}},"required":["namespace_id","requester"]}"#.to_string()),
                    description: "Approve join request - write approval signature".to_string(),
                },
                SyntheticFileSpec {
                    name: "namespaces/list.synth".to_string(),
                    access: AccessMode::Read,
                    data_type: DataType::CBOR,
                    cacheable: true,
                    ttl_seconds: Some(30),
                    schema: None,
                    description: "List user's namespaces - read namespace list".to_string(),
                },
                SyntheticFileSpec {
                    name: "requests/pending.synth".to_string(),
                    access: AccessMode::Read,
                    data_type: DataType::CBOR,
                    cacheable: true,
                    ttl_seconds: Some(10),
                    schema: None,
                    description: "List pending join requests for user's namespaces".to_string(),
                },
                SyntheticFileSpec {
                    name: "admin/status.synth".to_string(),
                    access: AccessMode::Read,
                    data_type: DataType::CBOR,
                    cacheable: true,
                    ttl_seconds: Some(5),
                    schema: None,
                    description: "Namespace translator status and statistics".to_string(),
                },
                SyntheticFileSpec {
                    name: "discovery/global.synth".to_string(),
                    access: AccessMode::Read,
                    data_type: DataType::CBOR,
                    cacheable: true,
                    ttl_seconds: Some(60),
                    schema: None,
                    description: "Global namespace registry (public namespaces)".to_string(),
                },
                SyntheticFileSpec {
                    name: "docs/help.txt".to_string(),
                    access: AccessMode::Read,
                    data_type: DataType::Text,
                    cacheable: true,
                    ttl_seconds: Some(3600),
                    schema: None,
                    description: "Namespace management help documentation".to_string(),
                },
            ],
            permissions: vec![
                Permission::FileRead,
                Permission::FileWrite,
                Permission::MeshAccess,
                Permission::ConsensusWrite,
            ],
            restart_policy: RestartPolicy::Always,
            config_schema: Some(r#"{"type":"object","properties":{"default_threshold":{"type":"integer","minimum":1},"max_namespace_size":{"type":"integer"}}}"#.to_string()),
            dependencies: vec!["global-event-chain".to_string()],
        }
    }

    /// Parse user public key from context or parameters
    fn extract_user_key(&self, context: &RequestContext, params: &HashMap<String, serde_cbor::Value>) -> Result<String> {
        // Try context first
        if let Some(user_id) = &context.user_id {
            return Ok(user_id.clone());
        }

        // Try parameters
        if let Some(serde_cbor::Value::Text(user_key)) = params.get("user_key") {
            return Ok(user_key.clone());
        }

        Err(anyhow::anyhow!("User key not found in request context or parameters"))
    }

    /// Increment operation counter
    async fn increment_operations(&self) {
        *self.operation_count.write().await += 1;
    }

    /// Increment error counter
    async fn increment_errors(&self) {
        *self.error_count.write().await += 1;
    }
}

#[async_trait]
impl AbstractTranslator for NamespaceTranslator {
    fn manifest(&self) -> &TranslatorManifest {
        &self.manifest
    }

    fn id(&self) -> Uuid {
        self.id
    }

    fn status(&self) -> TranslatorStatus {
        self.status.clone()
    }

    async fn initialize(&mut self) -> Result<()> {
        info!("🌐 Initializing namespace management translator");
        self.status = TranslatorStatus::Running;
        info!("✅ Namespace translator ready");
        Ok(())
    }

    async fn handle_synthetic_operation(&self, request: SyntheticRequest) -> Result<SyntheticResponse> {
        self.increment_operations().await;
        let start_time = SystemTime::now();

        let result = match request.file_path.as_str() {
            "namespaces/create.synth" => self.handle_create_namespace(request).await,
            "requests/join.synth" => self.handle_join_request(request).await,
            "requests/approve.synth" => self.handle_approve_request(request).await,
            "namespaces/list.synth" => self.handle_list_namespaces(request).await,
            "requests/pending.synth" => self.handle_pending_requests(request).await,
            "admin/status.synth" => self.handle_status_request(request).await,
            "discovery/global.synth" => self.handle_global_registry(request).await,
            "docs/help.txt" => self.handle_help_request(request).await,
            _ => {
                self.increment_errors().await;
                Ok(utils::create_error_response(
                    request.context.request_id,
                    format!("Unknown synthetic file: {}", request.file_path),
                    self.id.to_string(),
                ))
            }
        };

        // Add processing time to successful responses
        if let Ok(mut response) = result {
            if let Ok(elapsed) = start_time.elapsed() {
                response.context.processing_time_ms = elapsed.as_millis() as u64;
            }
            Ok(response)
        } else {
            self.increment_errors().await;
            result
        }
    }

    async fn stop(&mut self) -> Result<()> {
        info!("🛑 Stopping namespace translator");
        self.status = TranslatorStatus::Stopped;
        Ok(())
    }

    async fn health_check(&self) -> Result<HealthStatus> {
        let uptime = self.start_time.elapsed()
            .unwrap_or_default()
            .as_secs();

        Ok(HealthStatus {
            status: match self.status {
                TranslatorStatus::Running => "healthy".to_string(),
                TranslatorStatus::Starting => "starting".to_string(),
                TranslatorStatus::Failed(_) => "unhealthy".to_string(),
                TranslatorStatus::Stopped => "stopped".to_string(),
                TranslatorStatus::Restarting => "restarting".to_string(),
            },
            uptime_seconds: uptime,
            memory_usage: None,
            error_count: *self.error_count.read().await,
        })
    }
}

impl NamespaceTranslator {
    /// Handle namespace creation
    async fn handle_create_namespace(&self, request: SyntheticRequest) -> Result<SyntheticResponse> {
        if !matches!(request.operation, Operation::Write | Operation::Create) {
            return Ok(utils::create_error_response(
                request.context.request_id,
                "namespaces/create.synth only supports write operations".to_string(),
                self.id.to_string(),
            ));
        }

        // Parse creation request
        let data = request.data.ok_or_else(|| anyhow::anyhow!("No data provided"))?;
        let create_request: CreateNamespaceRequest = serde_cbor::from_slice(&data)
            .context("Failed to parse namespace creation request")?;

        // TODO: Parse user's public key from authentication context
        // For now, use a placeholder
        let creator_key = self.extract_user_key(&request.context, &request.params)?;

        // Create default policies if not provided
        let policies = create_request.policies.unwrap_or_else(|| NamespacePolicies {
            allow_sub_namespaces: true,
            allow_direct_invite: false,
            max_file_size: 100 * 1024 * 1024, // 100MB
            max_member_storage: 1024 * 1024 * 1024, // 1GB
            allowed_translators: ["wasm", "native"].iter().map(|s| s.to_string()).collect(),
            require_encryption: false,
            inactive_expiry_days: Some(90),
        });

        // Create threshold config
        let threshold = create_request.threshold.unwrap_or_else(|| ThresholdConfig {
            required: 1,
            total: 1,
            founder_veto: true,
            founders: std::collections::HashSet::new(), // Would add creator key
        });

        // TODO: Create namespace via manager
        // let namespace_id = self.namespace_manager.create_namespace(
        //     create_request.name,
        //     creator_pubkey,
        //     threshold,
        //     policies,
        // ).await?;

        // For now, return success with mock ID
        let namespace_id = format!("ns_{}", Uuid::new_v4().simple());

        let response_data = CreateNamespaceResponse {
            success: true,
            namespace_id: namespace_id.clone(),
            message: format!("Created namespace: {}", create_request.name),
        };

        let response_bytes = serde_cbor::to_vec(&response_data)?;

        // Event chain recording temporarily disabled due to thread safety issues
        // This will be re-enabled when mesh networking integration is complete
        // if let Some(event_chain) = &self.event_chain {
        //     let _ = crate::global_event_chain::track_file_operation(
        //         event_chain,
        //         format!("/ns/{}", namespace_id),
        //         "namespace_create".to_string(),
        //         "placeholder_hash".to_string(),
        //     ).await;
        // }

        Ok(utils::create_success_response(
            request.context.request_id,
            Some(response_bytes),
            self.id.to_string(),
            0,
        ))
    }

    /// Handle join requests
    async fn handle_join_request(&self, request: SyntheticRequest) -> Result<SyntheticResponse> {
        if !matches!(request.operation, Operation::Write | Operation::Create) {
            return Ok(utils::create_error_response(
                request.context.request_id,
                "requests/join.synth only supports write operations".to_string(),
                self.id.to_string(),
            ));
        }

        let data = request.data.ok_or_else(|| anyhow::anyhow!("No data provided"))?;
        let join_request: JoinNamespaceRequest = serde_cbor::from_slice(&data)?;

        let user_key = self.extract_user_key(&request.context, &request.params)?;

        // TODO: Submit join request to namespace manager
        // self.namespace_manager.request_join(
        //     join_request.namespace_id,
        //     user_pubkey,
        //     join_request.message,
        //     join_request.permissions,
        // ).await?;

        let response_data = JoinNamespaceResponse {
            success: true,
            message: format!("Join request submitted for namespace: {}", join_request.namespace_id),
            status: "pending".to_string(),
        };

        let response_bytes = serde_cbor::to_vec(&response_data)?;

        Ok(utils::create_success_response(
            request.context.request_id,
            Some(response_bytes),
            self.id.to_string(),
            0,
        ))
    }

    /// Handle approval signatures
    async fn handle_approve_request(&self, request: SyntheticRequest) -> Result<SyntheticResponse> {
        if !matches!(request.operation, Operation::Write | Operation::Create) {
            return Ok(utils::create_error_response(
                request.context.request_id,
                "requests/approve.synth only supports write operations".to_string(),
                self.id.to_string(),
            ));
        }

        let data = request.data.ok_or_else(|| anyhow::anyhow!("No data provided"))?;
        let approve_request: ApproveJoinRequest = serde_cbor::from_slice(&data)?;

        // TODO: Process approval with namespace manager
        let response_data = ApproveJoinResponse {
            success: true,
            message: "Approval signature processed".to_string(),
            threshold_met: false, // Would calculate from actual signatures
        };

        let response_bytes = serde_cbor::to_vec(&response_data)?;

        Ok(utils::create_success_response(
            request.context.request_id,
            Some(response_bytes),
            self.id.to_string(),
            0,
        ))
    }

    /// Handle namespace listing
    async fn handle_list_namespaces(&self, request: SyntheticRequest) -> Result<SyntheticResponse> {
        if !matches!(request.operation, Operation::Read | Operation::List) {
            return Ok(utils::create_error_response(
                request.context.request_id,
                "namespaces/list.synth only supports read operations".to_string(),
                self.id.to_string(),
            ));
        }

        let user_key = self.extract_user_key(&request.context, &request.params)?;

        // TODO: Get user's namespaces from manager
        // let namespaces = self.namespace_manager.list_user_namespaces(&user_pubkey).await;

        // Mock response for now
        let response_data = ListNamespacesResponse {
            namespaces: vec![
                NamespaceInfo {
                    id: "test_namespace_1".to_string(),
                    name: "Example Namespace".to_string(),
                    role: "owner".to_string(),
                    member_count: 3,
                    created_at: 1609459200, // 2021-01-01
                },
            ],
        };

        let response_bytes = serde_cbor::to_vec(&response_data)?;

        Ok(utils::create_success_response(
            request.context.request_id,
            Some(response_bytes),
            self.id.to_string(),
            0,
        ))
    }

    /// Handle pending requests
    async fn handle_pending_requests(&self, request: SyntheticRequest) -> Result<SyntheticResponse> {
        if !matches!(request.operation, Operation::Read | Operation::List) {
            return Ok(utils::create_error_response(
                request.context.request_id,
                "requests/pending.synth only supports read operations".to_string(),
                self.id.to_string(),
            ));
        }

        // Mock pending requests
        let response_data = PendingRequestsResponse {
            pending_requests: vec![
                PendingRequest {
                    namespace_id: "test_namespace_1".to_string(),
                    requester: "user_abc123".to_string(),
                    message: "Please let me join this namespace".to_string(),
                    requested_at: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    signatures_count: 1,
                    signatures_required: 2,
                },
            ],
        };

        let response_bytes = serde_cbor::to_vec(&response_data)?;

        Ok(utils::create_success_response(
            request.context.request_id,
            Some(response_bytes),
            self.id.to_string(),
            0,
        ))
    }

    /// Handle status requests
    async fn handle_status_request(&self, request: SyntheticRequest) -> Result<SyntheticResponse> {
        let health = self.health_check().await?;
        let operations = *self.operation_count.read().await;

        let status_data = HashMap::from([
            ("status".to_string(), serde_cbor::Value::Text(health.status)),
            ("uptime_seconds".to_string(), serde_cbor::Value::Integer(health.uptime_seconds as i128)),
            ("operations_processed".to_string(), serde_cbor::Value::Integer(operations as i128)),
            ("error_count".to_string(), serde_cbor::Value::Integer(health.error_count as i128)),
            ("translator_id".to_string(), serde_cbor::Value::Text(self.id.to_string())),
        ]);

        let response_bytes = serde_cbor::to_vec(&status_data)?;

        Ok(utils::create_success_response(
            request.context.request_id,
            Some(response_bytes),
            self.id.to_string(),
            0,
        ))
    }

    /// Handle global registry
    async fn handle_global_registry(&self, request: SyntheticRequest) -> Result<SyntheticResponse> {
        // Mock global registry - would query across mesh network
        let response_data = GlobalRegistryResponse {
            public_namespaces: vec![
                PublicNamespaceInfo {
                    id: "public_code".to_string(),
                    name: "Public Code Repository".to_string(),
                    description: "Open source code sharing".to_string(),
                    member_count: 150,
                    is_open: true,
                    mesh_nodes: vec!["node_1".to_string(), "node_2".to_string()],
                },
            ],
            total_namespaces: 1,
            last_updated: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        let response_bytes = serde_cbor::to_vec(&response_data)?;

        Ok(utils::create_success_response(
            request.context.request_id,
            Some(response_bytes),
            self.id.to_string(),
            0,
        ))
    }

    /// Handle help requests
    async fn handle_help_request(&self, request: SyntheticRequest) -> Result<SyntheticResponse> {
        let help_text = r#"
Namespace Management Translator - Help

Synthetic Files:
  namespaces/create.synth    - Create new namespace (write CBOR)
  requests/join.synth        - Request to join namespace (write CBOR)
  requests/approve.synth     - Approve join request (write CBOR)
  namespaces/list.synth      - List your namespaces (read CBOR)
  requests/pending.synth     - List pending requests (read CBOR)
  admin/status.synth         - Translator status (read CBOR)
  discovery/global.synth     - Global namespace registry (read CBOR)
  docs/help.txt              - This help text (read text)

Usage Examples:
  # Create namespace
  echo '{"name":"my-project","threshold":{"required":2,"total":3}}' | cbor-encode > /srv/settrans/namespace-manager/namespaces/create.synth

  # Join namespace
  echo '{"namespace_id":"abc123","message":"Please add me","permissions":{"can_read":true}}' | cbor-encode > /srv/settrans/namespace-manager/requests/join.synth

  # List namespaces
  cat /srv/settrans/namespace-manager/namespaces/list.synth | cbor-decode

For more information, see the 9P.e documentation.
"#;

        Ok(utils::create_success_response(
            request.context.request_id,
            Some(help_text.as_bytes().to_vec()),
            self.id.to_string(),
            0,
        ))
    }
}

/// Request/Response types for namespace operations
#[derive(Debug, Serialize, Deserialize)]
struct CreateNamespaceRequest {
    name: String,
    threshold: Option<ThresholdConfig>,
    policies: Option<NamespacePolicies>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CreateNamespaceResponse {
    success: bool,
    namespace_id: String,
    message: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct JoinNamespaceRequest {
    namespace_id: String,
    message: String,
    permissions: NamespacePermissions,
}

#[derive(Debug, Serialize, Deserialize)]
struct JoinNamespaceResponse {
    success: bool,
    message: String,
    status: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ApproveJoinRequest {
    namespace_id: String,
    requester: String,
    signature: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ApproveJoinResponse {
    success: bool,
    message: String,
    threshold_met: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct ListNamespacesResponse {
    namespaces: Vec<NamespaceInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
struct NamespaceInfo {
    id: String,
    name: String,
    role: String,
    member_count: u32,
    created_at: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct PendingRequestsResponse {
    pending_requests: Vec<PendingRequest>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PendingRequest {
    namespace_id: String,
    requester: String,
    message: String,
    requested_at: u64,
    signatures_count: usize,
    signatures_required: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct GlobalRegistryResponse {
    public_namespaces: Vec<PublicNamespaceInfo>,
    total_namespaces: u32,
    last_updated: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct PublicNamespaceInfo {
    id: String,
    name: String,
    description: String,
    member_count: u32,
    is_open: bool,
    mesh_nodes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::namespaces::GhostDAGConsensus;

    #[tokio::test]
    async fn test_namespace_translator_creation() {
        let consensus = Arc::new(GhostDAGConsensus {});
        let namespace_manager = Arc::new(NamespaceManager::new(consensus));

        let translator = NamespaceTranslator::new(namespace_manager, None);

        assert_eq!(translator.manifest().name, "namespace-manager");
        assert_eq!(translator.manifest().translator_type, TranslatorType::Builtin);
        assert!(!translator.manifest().synthetic_files.is_empty());
    }

    #[tokio::test]
    async fn test_help_request() {
        let consensus = Arc::new(GhostDAGConsensus {});
        let namespace_manager = Arc::new(NamespaceManager::new(consensus));
        let translator = NamespaceTranslator::new(namespace_manager, None);

        let request = SyntheticRequest {
            file_path: "help.txt".to_string(),
            operation: Operation::Read,
            data: None,
            params: HashMap::new(),
            context: utils::create_request_context(Some("test-123".to_string())),
        };

        let response = translator.handle_synthetic_operation(request).await.unwrap();

        assert!(response.success);
        assert!(response.data.is_some());

        let help_text = String::from_utf8(response.data.unwrap()).unwrap();
        assert!(help_text.contains("Namespace Management Translator"));
        assert!(help_text.contains("create.synth"));
    }
}