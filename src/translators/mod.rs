pub mod hypercore;
pub mod gemini;
pub mod tld_router;
pub mod html_renderer;
pub mod v8;
// Hurd-Style Translator System
//
// Implements microkernel translator architecture with:
// - Dynamic translator spawning and management
// - Sandboxed execution with resource limits
// - Inter-translator communication via message passing
/// - Capability-based security model

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
// Removed unused atomic imports
use tokio::sync::{mpsc, RwLock, Mutex};
use serde::{Deserialize, Serialize};

/// Translator unique identifier
///
/// 32-bit identifier for distinguishing between different translator instances
/// within the Hurd-style translator system.
pub type TranslatorId = u32;

/// Capability identifier for security model
///
/// 64-bit identifier for capabilities in the capability-based security system.
/// Capabilities grant specific permissions to translators.
pub type CapabilityId = u64;

/// Hurd-style translator system
///
/// Implements a microkernel-style translator architecture inspired by GNU Hurd.
/// Manages translator spawning, message passing, capability-based security,
/// and resource isolation with sandboxing.
#[derive(Debug)]
pub struct TranslatorSystem {
    /// Active translators indexed by ID
    translators: Arc<RwLock<HashMap<TranslatorId, Arc<Mutex<Translator>>>>>,

    /// Capability table mapping translators to their granted capabilities
    capability_table: Arc<RwLock<HashMap<TranslatorId, HashSet<CapabilityId>>>>,

    /// Message routing system for inter-translator communication
    message_router: Arc<Mutex<MessageRouter>>,

    /// System-wide resource limits and policies
    limits: TranslatorLimits,

    /// Atomic counter for generating unique translator IDs
    next_id: Arc<Mutex<TranslatorId>>,
}

/// Individual translator instance
///
/// Represents a single translator process with its code, state, resources,
/// and communication channels. Translators run in isolated sandboxes.
#[derive(Debug)]
pub struct Translator {
    /// Unique identifier for this translator
    pub id: TranslatorId,

    /// Translator bytecode or executable
    pub code: Vec<u8>,

    /// Configuration data for the translator
    pub config: Vec<u8>,

    /// Current execution state of the translator
    pub state: TranslatorState,

    /// Current resource usage statistics
    pub resource_usage: ResourceUsage,

    /// Incoming message queue
    pub message_queue: VecDeque<TranslatorMessage>,

    /// Channel for sending outbound messages
    pub message_sender: Option<mpsc::UnboundedSender<TranslatorMessage>>,

    /// Set of capabilities granted to this translator
    pub capabilities: HashSet<CapabilityId>,

    /// Parent translator that spawned this one (if any)
    pub parent: Option<TranslatorId>,

    /// Set of child translators spawned by this translator
    pub children: HashSet<TranslatorId>,

    /// Sandbox configuration for security isolation
    pub sandbox: SandboxConfig,

    /// When this translator was created (milliseconds)
    pub created_at: u64,

    /// When this translator was last active (milliseconds)
    pub last_active: u64,
}

/// Translator execution state
///
/// Represents the current lifecycle state of a translator instance,
/// from initialization through termination.
#[derive(Debug, Clone, PartialEq)]
pub enum TranslatorState {
    /// Translator is currently initializing
    Initializing,

    /// Translator is running and processing messages
    Running,

    /// Translator is suspended and not processing
    Suspended,

    /// Translator is in the process of terminating
    Terminating,

    /// Translator has completely terminated
    Terminated,

    /// Translator failed with error message
    Failed(String),
}

/// Resource usage tracking
#[derive(Debug, Clone)]
pub struct ResourceUsage {
    /// Memory allocated (bytes)
    pub memory_used: usize,

    /// CPU time consumed (microseconds)
    pub cpu_time: u64,

    /// Number of file descriptors open
    pub file_descriptors: u32,

    /// Number of network connections
    pub network_connections: u32,

    /// Number of system calls made
    pub syscall_count: u64,

    /// Message queue size
    pub message_queue_size: usize,
}

/// Sandbox configuration for isolation
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Maximum memory allocation
    pub max_memory: usize,

    /// Maximum CPU time per operation
    pub max_cpu_time: u64,

    /// Maximum file descriptors
    pub max_file_descriptors: u32,

    /// Maximum network connections
    pub max_network_connections: u32,

    /// Allowed system calls
    pub allowed_syscalls: HashSet<u32>,

    /// Filesystem namespace restrictions
    pub filesystem_root: String,

    /// Network namespace isolation
    pub network_isolated: bool,

    /// Process group isolation
    pub process_isolated: bool,
}

/// Inter-translator message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslatorMessage {
    /// Sender translator ID
    pub from: TranslatorId,

    /// Recipient translator ID
    pub to: TranslatorId,

    /// Message type/operation
    pub message_type: MessageType,

    /// Message payload
    pub payload: Vec<u8>,

    /// Required capabilities to process
    pub required_capabilities: Vec<CapabilityId>,

    /// Message priority (0 = highest)
    pub priority: u8,

    /// Message timestamp
    pub timestamp: u64,

    /// Correlation ID for request/response
    pub correlation_id: Option<u64>,
}

/// Message types for translator communication
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MessageType {
    /// File system operation request
    FileOperation,

    /// File system operation response
    FileResponse,

    /// Resource allocation request
    ResourceRequest,

    /// Resource allocation response
    ResourceResponse,

    /// Control message (start, stop, etc.)
    Control,

    /// Data transfer
    Data,

    /// Error notification
    Error,

    /// Heartbeat/ping
    Heartbeat,

    /// Capability grant notification
    CapabilityGrant,

    /// Capability revoke notification
    CapabilityRevoke,
}

/// Message routing system
#[derive(Debug)]
pub struct MessageRouter {
    /// Message queues per translator
    queues: HashMap<TranslatorId, mpsc::UnboundedSender<TranslatorMessage>>,

    /// Message delivery statistics
    delivery_stats: HashMap<TranslatorId, MessageStats>,

    // Removed unused max_queue_size field
}

/// Message delivery statistics
#[derive(Debug, Clone)]
pub struct MessageStats {
    /// Number of messages sent by translator
    pub messages_sent: u64,
    /// Number of messages received by translator
    pub messages_received: u64,
    /// Number of messages dropped due to queue overflow
    pub messages_dropped: u64,
    /// Timestamp of last message activity
    pub last_message_time: u64,
}

/// System-wide translator limits
#[derive(Debug, Clone)]
pub struct TranslatorLimits {
    /// Maximum number of concurrent translators
    pub max_translators: usize,

    /// Maximum memory per translator
    pub max_memory_per_translator: usize,

    /// Maximum CPU time per translator operation
    pub max_cpu_time_per_operation: u64,

    /// Maximum file descriptors per translator
    pub max_file_descriptors_per_translator: u32,

    /// Maximum message queue size
    pub max_message_queue_size: usize,

    /// Maximum translator lifetime
    pub max_translator_lifetime: u64,

    /// Maximum child translators per parent
    pub max_children_per_translator: usize,
}

/// Translator system errors
#[derive(Debug, thiserror::Error)]
pub enum TranslatorError {
    #[error("Translator not found: {0}")]
    /// Translator with specified ID not found
    TranslatorNotFound(TranslatorId),

    #[error("Maximum translators reached")]
    /// Maximum number of translators reached
    MaxTranslatorsReached,

    #[error("Resource limit exceeded: {0}")]
    /// Resource limit exceeded
    ResourceLimitExceeded(String),

    #[error("Permission denied: missing capability {0}")]
    /// Permission denied - missing required capability
    PermissionDenied(CapabilityId),

    #[error("Translator failed: {0}")]
    /// Translator execution failed
    TranslatorFailed(String),

    #[error("Message delivery failed")]
    /// Message delivery to translator failed
    MessageDeliveryFailed,

    #[error("Sandbox violation: {0}")]
    /// Sandbox security violation detected
    SandboxViolation(String),

    #[error("Invalid translator code")]
    /// Invalid translator code provided
    InvalidCode,

    #[error("Translator already exists: {0}")]
    /// Translator with specified ID already exists
    TranslatorExists(TranslatorId),
}

impl Default for TranslatorLimits {
    fn default() -> Self {
        Self {
            max_translators: 1024,
            max_memory_per_translator: 1024 * 1024, // 1MB
            max_cpu_time_per_operation: 1000000, // 1 second
            max_file_descriptors_per_translator: 64,
            max_message_queue_size: 10000,
            max_translator_lifetime: 24 * 60 * 60 * 1000, // 24 hours
            max_children_per_translator: 8,
        }
    }
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            max_memory: 1024 * 1024, // 1MB
            max_cpu_time: 1000000, // 1 second
            max_file_descriptors: 64,
            max_network_connections: 8,
            allowed_syscalls: HashSet::new(),
            filesystem_root: "/tmp/translator_sandbox".to_string(),
            network_isolated: true,
            process_isolated: true,
        }
    }
}

impl Default for ResourceUsage {
    fn default() -> Self {
        Self {
            memory_used: 0,
            cpu_time: 0,
            file_descriptors: 0,
            network_connections: 0,
            syscall_count: 0,
            message_queue_size: 0,
        }
    }
}

impl TranslatorSystem {
    /// Get system statistics
    pub fn get_stats(&self) -> TranslatorSystemStats {
        TranslatorSystemStats {
            total_translators: 0,
            running_translators: 0,
            suspended_translators: 0,
            terminated_translators: 0,
            failed_translators: 0,
            total_messages_sent: 0,
        }
    }
    /// Create new translator system with default limits
    ///
    /// Initializes a new Hurd-style translator system ready to spawn
    /// and manage translator instances with capability-based security.
    ///
    /// # Returns
    ///
    /// A new TranslatorSystem instance
    pub fn new() -> Self {
        Self {
            translators: Arc::new(RwLock::new(HashMap::new())),
            capability_table: Arc::new(RwLock::new(HashMap::new())),
            message_router: Arc::new(Mutex::new(MessageRouter::new())),
            limits: TranslatorLimits::default(),
            next_id: Arc::new(Mutex::new(1)),
        }
    }

    /// Spawn new translator instance
    ///
    /// Creates and starts a new translator with the specified code and configuration.
    /// Handles parent-child relationships, sandbox setup, and message routing.
    ///
    /// # Arguments
    ///
    /// * `code` - Translator bytecode or executable
    /// * `config` - Configuration data for the translator
    /// * `parent` - Optional parent translator ID
    /// * `sandbox` - Optional sandbox configuration (uses default if None)
    ///
    /// # Returns
    ///
    /// * `Ok(TranslatorId)` - Unique ID of the spawned translator
    /// * `Err(TranslatorError)` - Spawning failed (limits, validation, etc.)
    pub async fn spawn_translator(
        &self,
        code: Vec<u8>,
        config: Vec<u8>,
        parent: Option<TranslatorId>,
        sandbox: Option<SandboxConfig>
    ) -> Result<TranslatorId, TranslatorError> {
        // Check system limits
        let translators = self.translators.read().await;
        if translators.len() >= self.limits.max_translators {
            return Err(TranslatorError::MaxTranslatorsReached);
        }
        drop(translators);

        // Check parent limits if applicable
        if let Some(parent_id) = parent {
            let translators = self.translators.read().await;
            if let Some(parent_translator) = translators.get(&parent_id) {
                let parent_guard = parent_translator.lock().await;
                if parent_guard.children.len() >= self.limits.max_children_per_translator {
                    return Err(TranslatorError::ResourceLimitExceeded("Too many children".to_string()));
                }
            } else {
                return Err(TranslatorError::TranslatorNotFound(parent_id));
            }
        }

        // Generate new translator ID
        let id = {
            let mut next_id = self.next_id.lock().await;
            let id = *next_id;
            *next_id += 1;
            id
        };

        // Validate translator code (simplified)
        if code.is_empty() {
            return Err(TranslatorError::InvalidCode);
        }

        // Create message channel
        let (sender, receiver) = mpsc::unbounded_channel::<TranslatorMessage>();

        // Create translator instance
        let translator = Translator {
            id,
            code,
            config,
            state: TranslatorState::Initializing,
            resource_usage: ResourceUsage::default(),
            message_queue: VecDeque::new(),
            message_sender: Some(sender.clone()),
            capabilities: HashSet::new(),
            parent,
            children: HashSet::new(),
            sandbox: sandbox.unwrap_or_default(),
            created_at: current_timestamp(),
            last_active: current_timestamp(),
        };

        let translator_arc = Arc::new(Mutex::new(translator));

        // Register translator
        {
            let mut translators = self.translators.write().await;
            translators.insert(id, translator_arc.clone());
        }

        // Register message queue
        {
            let mut router = self.message_router.lock().await;
            router.register_translator(id, sender);
        }

        // Update parent's children list
        if let Some(parent_id) = parent {
            let translators = self.translators.read().await;
            if let Some(parent_translator) = translators.get(&parent_id) {
                let mut parent_guard = parent_translator.lock().await;
                parent_guard.children.insert(id);
            }
        }

        // Initialize empty capability set
        {
            let mut capabilities = self.capability_table.write().await;
            capabilities.insert(id, HashSet::new());
        }

        // Start translator execution (simplified)
        self.initialize_translator(translator_arc, receiver).await?;

        Ok(id)
    }

    /// Initialize and start translator execution
    async fn initialize_translator(
        &self,
        translator: Arc<Mutex<Translator>>,
        mut receiver: mpsc::UnboundedReceiver<TranslatorMessage>
    ) -> Result<(), TranslatorError> {
        // Update state to running
        {
            let mut translator_guard = translator.lock().await;
            translator_guard.state = TranslatorState::Running;
            translator_guard.last_active = current_timestamp();
        }

        // Spawn message handling task
        let translator_clone = translator.clone();
        tokio::spawn(async move {
            while let Some(message) = receiver.recv().await {
                let mut translator_guard = translator_clone.lock().await;

                // Check if translator is still active
                if matches!(translator_guard.state, TranslatorState::Terminated | TranslatorState::Failed(_)) {
                    break;
                }

                // Process message
                translator_guard.message_queue.push_back(message);
                translator_guard.last_active = current_timestamp();

                // Limit message queue size
                while translator_guard.message_queue.len() > translator_guard.sandbox.max_memory / 1024 {
                    translator_guard.message_queue.pop_front();
                }
            }
        });

        Ok(())
    }

    /// Send message to translator
    pub async fn send_message(
        &self,
        from: TranslatorId,
        to: TranslatorId,
        message_type: MessageType,
        payload: Vec<u8>,
        required_capabilities: Vec<CapabilityId>
    ) -> Result<(), TranslatorError> {
        // Check sender exists
        {
            let translators = self.translators.read().await;
            if !translators.contains_key(&from) {
                return Err(TranslatorError::TranslatorNotFound(from));
            }
        }

        // Check sender has required capabilities
        self.check_capabilities(from, &required_capabilities).await?;

        let message = TranslatorMessage {
            from,
            to,
            message_type,
            payload,
            required_capabilities,
            priority: 5, // Default priority
            timestamp: current_timestamp(),
            correlation_id: None,
        };

        // Route message
        let mut router = self.message_router.lock().await;
        router.deliver_message(message).await
            .map_err(|_| TranslatorError::MessageDeliveryFailed)?;

        Ok(())
    }

    /// Grant capability to translator
    pub async fn grant_capability(&self, translator_id: TranslatorId, capability: CapabilityId) -> Result<(), TranslatorError> {
        // Check translator exists
        {
            let translators = self.translators.read().await;
            if !translators.contains_key(&translator_id) {
                return Err(TranslatorError::TranslatorNotFound(translator_id));
            }
        }

        // Grant capability
        let mut capabilities = self.capability_table.write().await;
        capabilities.entry(translator_id)
            .or_insert_with(HashSet::new)
            .insert(capability);

        // Update translator's capability set
        let translators = self.translators.read().await;
        if let Some(translator) = translators.get(&translator_id) {
            let mut translator_guard = translator.lock().await;
            translator_guard.capabilities.insert(capability);
        }

        Ok(())
    }

    /// Revoke capability from translator
    pub async fn revoke_capability(&self, translator_id: TranslatorId, capability: CapabilityId) -> Result<(), TranslatorError> {
        let mut capabilities = self.capability_table.write().await;
        if let Some(caps) = capabilities.get_mut(&translator_id) {
            caps.remove(&capability);
        }

        // Update translator's capability set
        let translators = self.translators.read().await;
        if let Some(translator) = translators.get(&translator_id) {
            let mut translator_guard = translator.lock().await;
            translator_guard.capabilities.remove(&capability);
        }

        Ok(())
    }

    /// Check if translator has required capabilities
    async fn check_capabilities(&self, translator_id: TranslatorId, required: &[CapabilityId]) -> Result<(), TranslatorError> {
        let capabilities = self.capability_table.read().await;
        if let Some(translator_caps) = capabilities.get(&translator_id) {
            for &required_cap in required {
                if !translator_caps.contains(&required_cap) {
                    return Err(TranslatorError::PermissionDenied(required_cap));
                }
            }
        } else {
            return Err(TranslatorError::TranslatorNotFound(translator_id));
        }
        Ok(())
    }

    /// Kill translator and cleanup resources
    pub async fn kill_translator(&self, translator_id: TranslatorId) -> Result<(), TranslatorError> {
        // Get translator
        let translator_arc = {
            let mut translators = self.translators.write().await;
            translators.remove(&translator_id)
                .ok_or(TranslatorError::TranslatorNotFound(translator_id))?
        };

        // Update state to terminated
        {
            let mut translator_guard = translator_arc.lock().await;
            translator_guard.state = TranslatorState::Terminated;

            // Kill all child translators
            let children: Vec<_> = translator_guard.children.iter().cloned().collect();
            drop(translator_guard);

            for child_id in children {
                let _ = Box::pin(self.kill_translator(child_id)).await;
            }
        }

        // Remove from parent's children list
        {
            let translator_guard = translator_arc.lock().await;
            if let Some(parent_id) = translator_guard.parent {
                let translators = self.translators.read().await;
                if let Some(parent_translator) = translators.get(&parent_id) {
                    let mut parent_guard = parent_translator.lock().await;
                    parent_guard.children.remove(&translator_id);
                }
            }
        }

        // Remove capabilities
        {
            let mut capabilities = self.capability_table.write().await;
            capabilities.remove(&translator_id);
        }

        // Remove from message router
        {
            let mut router = self.message_router.lock().await;
            router.unregister_translator(translator_id);
        }

        Ok(())
    }

    /// Get translator information
    pub async fn get_translator_info(&self, translator_id: TranslatorId) -> Option<TranslatorInfo> {
        let translators = self.translators.read().await;
        if let Some(translator) = translators.get(&translator_id) {
            let translator_guard = translator.lock().await;
            Some(TranslatorInfo {
                id: translator_guard.id,
                state: translator_guard.state.clone(),
                resource_usage: translator_guard.resource_usage.clone(),
                capabilities: translator_guard.capabilities.clone(),
                parent: translator_guard.parent,
                children: translator_guard.children.clone(),
                created_at: translator_guard.created_at,
                last_active: translator_guard.last_active,
                message_queue_size: translator_guard.message_queue.len(),
            })
        } else {
            None
        }
    }

    /// Get system statistics
    pub async fn get_system_stats(&self) -> TranslatorSystemStats {
        let translators = self.translators.read().await;
        let total_translators = translators.len();

        let mut running_count = 0;
        let mut suspended_count = 0;
        let mut terminated_count = 0;
        let mut failed_count = 0;

        for translator in translators.values() {
            let translator_guard = translator.lock().await;
            match translator_guard.state {
                TranslatorState::Running => running_count += 1,
                TranslatorState::Suspended => suspended_count += 1,
                TranslatorState::Terminated => terminated_count += 1,
                TranslatorState::Failed(_) => failed_count += 1,
                _ => {}
            }
        }

        let router = self.message_router.lock().await;
        let total_messages_sent = router.delivery_stats.values()
            .map(|stats| stats.messages_sent)
            .sum();

        TranslatorSystemStats {
            total_translators,
            running_translators: running_count,
            suspended_translators: suspended_count,
            terminated_translators: terminated_count,
            failed_translators: failed_count,
            total_messages_sent,
        }
    }

    /// Cleanup terminated translators
    pub async fn cleanup_terminated(&self) {
        let mut to_remove = Vec::new();

        {
            let translators = self.translators.read().await;
            for (id, translator) in translators.iter() {
                let translator_guard = translator.lock().await;
                if matches!(translator_guard.state, TranslatorState::Terminated | TranslatorState::Failed(_)) {
                    let age = current_timestamp() - translator_guard.created_at;
                    if age > 60000 { // Cleanup after 1 minute
                        to_remove.push(*id);
                    }
                }
            }
        }

        for id in to_remove {
            let _ = self.kill_translator(id).await;
        }
    }
}

impl MessageRouter {
    fn new() -> Self {
        Self {
            queues: HashMap::new(),
            delivery_stats: HashMap::new(),
        }
    }

    fn register_translator(&mut self, id: TranslatorId, sender: mpsc::UnboundedSender<TranslatorMessage>) {
        self.queues.insert(id, sender);
        self.delivery_stats.insert(id, MessageStats {
            messages_sent: 0,
            messages_received: 0,
            messages_dropped: 0,
            last_message_time: current_timestamp(),
        });
    }

    fn unregister_translator(&mut self, id: TranslatorId) {
        self.queues.remove(&id);
        self.delivery_stats.remove(&id);
    }

    async fn deliver_message(&mut self, message: TranslatorMessage) -> Result<(), TranslatorError> {
        if let Some(sender) = self.queues.get(&message.to) {
            if let Err(_) = sender.send(message.clone()) {
                // Update stats - message dropped
                if let Some(stats) = self.delivery_stats.get_mut(&message.to) {
                    stats.messages_dropped += 1;
                }
                return Err(TranslatorError::MessageDeliveryFailed);
            }

            // Update stats - message sent
            if let Some(stats) = self.delivery_stats.get_mut(&message.from) {
                stats.messages_sent += 1;
                stats.last_message_time = current_timestamp();
            }

            // Update stats - message received
            if let Some(stats) = self.delivery_stats.get_mut(&message.to) {
                stats.messages_received += 1;
                stats.last_message_time = current_timestamp();
            }

            Ok(())
        } else {
            Err(TranslatorError::TranslatorNotFound(message.to))
        }
    }
}

/// Translator information for status queries
#[derive(Debug, Clone)]
pub struct TranslatorInfo {
    /// Unique translator identifier
    pub id: TranslatorId,
    /// Current execution state of translator
    pub state: TranslatorState,
    /// Current resource usage statistics
    pub resource_usage: ResourceUsage,
    /// Set of capabilities granted to translator
    pub capabilities: HashSet<CapabilityId>,
    /// Optional parent translator ID
    pub parent: Option<TranslatorId>,
    /// Set of child translator IDs
    pub children: HashSet<TranslatorId>,
    /// Timestamp when translator was created
    pub created_at: u64,
    /// Timestamp of last activity
    pub last_active: u64,
    /// Current size of message queue
    pub message_queue_size: usize,
}

/// System-wide translator statistics
#[derive(Debug, Clone)]
pub struct TranslatorSystemStats {
    /// Total number of translators in system
    pub total_translators: usize,
    /// Number of currently running translators
    pub running_translators: usize,
    /// Number of suspended translators
    pub suspended_translators: usize,
    /// Number of terminated translators
    pub terminated_translators: usize,
    /// Number of failed translators
    pub failed_translators: usize,
    /// Total number of messages sent across all translators
    pub total_messages_sent: u64,
}

/// Get current timestamp in milliseconds
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_translator_system_creation() {
        let system = TranslatorSystem::new();
        let stats = system.get_system_stats().await;
        assert_eq!(stats.total_translators, 0);
    }

    #[tokio::test]
    async fn test_spawn_translator() {
        let system = TranslatorSystem::new();

        let translator_id = system.spawn_translator(
            b"test code".to_vec(),
            b"test config".to_vec(),
            None,
            None
        ).await.unwrap();

        assert!(translator_id > 0);

        let info = system.get_translator_info(translator_id).await.unwrap();
        assert_eq!(info.id, translator_id);
        assert_eq!(info.state, TranslatorState::Running);
    }

    #[tokio::test]
    async fn test_capability_management() {
        let system = TranslatorSystem::new();

        let translator_id = system.spawn_translator(
            b"test code".to_vec(),
            b"config".to_vec(),
            None,
            None
        ).await.unwrap();

        // Grant capability
        system.grant_capability(translator_id, 42).await.unwrap();

        let info = system.get_translator_info(translator_id).await.unwrap();
        assert!(info.capabilities.contains(&42));

        // Revoke capability
        system.revoke_capability(translator_id, 42).await.unwrap();

        let info = system.get_translator_info(translator_id).await.unwrap();
        assert!(!info.capabilities.contains(&42));
    }

    #[tokio::test]
    async fn test_parent_child_relationship() {
        let system = TranslatorSystem::new();

        // Spawn parent translator
        let parent_id = system.spawn_translator(
            b"parent code".to_vec(),
            b"parent config".to_vec(),
            None,
            None
        ).await.unwrap();

        // Spawn child translator
        let child_id = system.spawn_translator(
            b"child code".to_vec(),
            b"child config".to_vec(),
            Some(parent_id),
            None
        ).await.unwrap();

        let parent_info = system.get_translator_info(parent_id).await.unwrap();
        assert!(parent_info.children.contains(&child_id));

        let child_info = system.get_translator_info(child_id).await.unwrap();
        assert_eq!(child_info.parent, Some(parent_id));
    }

    #[tokio::test]
    async fn test_message_passing() {
        let system = TranslatorSystem::new();

        let translator1 = system.spawn_translator(
            b"code1".to_vec(),
            b"config1".to_vec(),
            None,
            None
        ).await.unwrap();

        let translator2 = system.spawn_translator(
            b"code2".to_vec(),
            b"config2".to_vec(),
            None,
            None
        ).await.unwrap();

        // Send message without required capabilities (should work)
        let result = system.send_message(
            translator1,
            translator2,
            MessageType::Data,
            b"test message".to_vec(),
            vec![]
        ).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_capability_enforcement() {
        let system = TranslatorSystem::new();

        let translator1 = system.spawn_translator(
            b"code1".to_vec(),
            b"config1".to_vec(),
            None,
            None
        ).await.unwrap();

        let translator2 = system.spawn_translator(
            b"code2".to_vec(),
            b"config2".to_vec(),
            None,
            None
        ).await.unwrap();

        // Send message requiring capability that sender doesn't have
        let result = system.send_message(
            translator1,
            translator2,
            MessageType::FileOperation,
            b"test".to_vec(),
            vec![999] // Required capability
        ).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            TranslatorError::PermissionDenied(cap) => assert_eq!(cap, 999),
            _ => panic!("Expected PermissionDenied"),
        }
    }

    #[tokio::test]
    async fn test_translator_cleanup() {
        let system = TranslatorSystem::new();

        let translator_id = system.spawn_translator(
            b"test code".to_vec(),
            b"config".to_vec(),
            None,
            None
        ).await.unwrap();

        // Kill translator
        system.kill_translator(translator_id).await.unwrap();

        // Should not be able to find it anymore
        let info = system.get_translator_info(translator_id).await;
        assert!(info.is_none());
    }

    #[tokio::test]
    async fn test_system_limits() {
        let mut system = TranslatorSystem::new();
        system.limits.max_translators = 2;

        // Spawn maximum translators
        let _t1 = system.spawn_translator(b"code1".to_vec(), b"config1".to_vec(), None, None).await.unwrap();
        let _t2 = system.spawn_translator(b"code2".to_vec(), b"config2".to_vec(), None, None).await.unwrap();

        // Third should fail
        let result = system.spawn_translator(b"code3".to_vec(), b"config3".to_vec(), None, None).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            TranslatorError::MaxTranslatorsReached => {} // Expected
            _ => panic!("Expected MaxTranslatorsReached"),
        }
    }
}
