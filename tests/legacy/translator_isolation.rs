//! Translator System Isolation Property Tests
//! Ruthlessly validates Hurd-style microkernel translator sandboxing

use proptest::prelude::*;
use quickcheck::TestResult;
use quickcheck_macros::quickcheck;
use arbitrary::Arbitrary;
use std::collections::{HashMap, HashSet};
use quickcheck::{Arbitrary as QCArbitrary, Gen};

/// Translator execution context with isolation boundaries
#[derive(Debug, Clone, PartialEq, Arbitrary)]
pub struct TranslatorContext {
    pub translator_id: u32,
    pub code_hash: [u8; 32],
    pub memory_limit: usize,
    pub cpu_limit: u64, // microseconds
    pub file_permissions: u32, // Bit flags
    pub namespace_id: u32,
    pub parent_translator: Option<u32>,
    pub child_translators: Vec<u32>,
    pub allowed_syscalls: Vec<u8>, // Syscall numbers
    pub resource_usage: ResourceUsage,
}

fn qc_bytes<const N: usize>(g: &mut Gen) -> [u8; N] {
    let mut arr = [0u8; N];
    for byte in arr.iter_mut() {
        *byte = u8::arbitrary(g);
    }
    arr
}

impl proptest::arbitrary::Arbitrary for TranslatorContext {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
        use proptest::array::uniform32;
        use proptest::strategy::Strategy;
        (
            any::<u32>(),
            uniform32(any::<u8>()),
            1024usize..=1024 * 1024,
            10_000u64..=1_000_000u64,
            any::<u32>(),
            any::<u32>(),
            proptest::option::of(any::<u32>()),
            proptest::collection::vec(any::<u32>(), 0..8),
            proptest::collection::vec(any::<u8>(), 0..32),
            any::<ResourceUsage>(),
        )
            .prop_map(
                |(
                    translator_id,
                    code_hash,
                    memory_limit,
                    cpu_limit,
                    file_permissions,
                    namespace_id,
                    parent_translator,
                    child_translators,
                    allowed_syscalls,
                    resource_usage,
                )| TranslatorContext {
                    translator_id,
                    code_hash,
                    memory_limit,
                    cpu_limit,
                    file_permissions,
                    namespace_id,
                    parent_translator,
                    child_translators,
                    allowed_syscalls,
                    resource_usage,
                },
            )
            .boxed()
    }
}

impl QCArbitrary for TranslatorContext {
    fn arbitrary(g: &mut Gen) -> Self {
        TranslatorContext {
            translator_id: <u32 as QCArbitrary>::arbitrary(g),
            code_hash: qc_bytes::<32>(g),
            memory_limit: (<usize as QCArbitrary>::arbitrary(g) % (1024 * 1024)).max(1024),
            cpu_limit: (<u64 as QCArbitrary>::arbitrary(g) % 1_000_000).max(10_000),
            file_permissions: <u32 as QCArbitrary>::arbitrary(g),
            namespace_id: <u32 as QCArbitrary>::arbitrary(g),
            parent_translator: if <bool as QCArbitrary>::arbitrary(g) { Some(<u32 as QCArbitrary>::arbitrary(g)) } else { None },
            child_translators: {
                let len = usize::arbitrary(g) % 8;
                (0..len).map(|_| <u32 as QCArbitrary>::arbitrary(g)).collect()
            },
            allowed_syscalls: {
                let len = usize::arbitrary(g) % 32;
                (0..len).map(|_| <u8 as QCArbitrary>::arbitrary(g)).collect()
            },
            resource_usage: <ResourceUsage as QCArbitrary>::arbitrary(g),
        }
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        Box::new(std::iter::empty())
    }
}

/// Resource usage tracking for isolation enforcement
#[derive(Debug, Clone, PartialEq, Arbitrary)]
pub struct ResourceUsage {
    pub memory_used: usize,
    pub cpu_used: u64, // microseconds
    pub files_opened: u32,
    pub network_connections: u32,
    pub child_processes: u32,
    pub syscalls_made: HashMap<u8, u32>, // syscall -> count
}

impl proptest::arbitrary::Arbitrary for ResourceUsage {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
        use proptest::strategy::Strategy;
        (
            0usize..=1024 * 1024,
            0u64..=1_000_000,
            0u32..=128,
            0u32..=64,
            0u32..=16,
            proptest::collection::hash_map(any::<u8>(), 0u32..=1000, 0..16),
        )
            .prop_map(
                |(memory_used, cpu_used, files_opened, network_connections, child_processes, syscalls_made)| ResourceUsage {
                    memory_used,
                    cpu_used,
                    files_opened,
                    network_connections,
                    child_processes,
                    syscalls_made,
                },
            )
            .boxed()
    }
}

fn qc_syscalls(g: &mut Gen) -> HashMap<u8, u32> {
    let mut map = HashMap::new();
    let len = usize::arbitrary(g) % 8;
    for _ in 0..len {
        map.insert(<u8 as QCArbitrary>::arbitrary(g), <u32 as QCArbitrary>::arbitrary(g));
    }
    map
}

impl QCArbitrary for ResourceUsage {
    fn arbitrary(g: &mut Gen) -> Self {
        ResourceUsage {
            memory_used: <usize as QCArbitrary>::arbitrary(g) % (1024 * 1024),
            cpu_used: <u64 as QCArbitrary>::arbitrary(g) % 1_000_000,
            files_opened: <u32 as QCArbitrary>::arbitrary(g) % 128,
            network_connections: <u32 as QCArbitrary>::arbitrary(g) % 64,
            child_processes: <u32 as QCArbitrary>::arbitrary(g) % 16,
            syscalls_made: qc_syscalls(g),
        }
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        Box::new(std::iter::empty())
    }
}

/// Translator message with isolation controls
#[derive(Debug, Clone, PartialEq, Arbitrary)]
pub struct TranslatorMessage {
    pub from_id: u32,
    pub to_id: u32,
    pub message_type: MessageType,
    pub payload: Vec<u8>,
    pub capabilities: Vec<u32>, // Required capabilities
    pub priority: u8,
}

impl proptest::arbitrary::Arbitrary for TranslatorMessage {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
        use proptest::strategy::Strategy;
        (
            any::<u32>(),
            any::<u32>(),
            any::<MessageType>(),
            proptest::collection::vec(any::<u8>(), 0..512),
            proptest::collection::vec(any::<u32>(), 0..16),
            any::<u8>(),
        )
            .prop_map(
                |(from_id, to_id, message_type, payload, capabilities, priority)| TranslatorMessage {
                    from_id,
                    to_id,
                    message_type,
                    payload,
                    capabilities,
                    priority,
                },
            )
            .boxed()
    }
}

impl QCArbitrary for TranslatorMessage {
    fn arbitrary(g: &mut Gen) -> Self {
        let payload_len = usize::arbitrary(g) % 512;
        let capabilities_len = usize::arbitrary(g) % 16;
        TranslatorMessage {
            from_id: <u32 as QCArbitrary>::arbitrary(g),
            to_id: <u32 as QCArbitrary>::arbitrary(g),
            message_type: <MessageType as QCArbitrary>::arbitrary(g),
            payload: (0..payload_len).map(|_| <u8 as QCArbitrary>::arbitrary(g)).collect(),
            capabilities: (0..capabilities_len).map(|_| <u32 as QCArbitrary>::arbitrary(g)).collect(),
            priority: <u8 as QCArbitrary>::arbitrary(g),
        }
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        Box::new(std::iter::empty())
    }
}

#[derive(Debug, Clone, PartialEq, Arbitrary)]
pub enum MessageType {
    FileRequest,
    FileResponse,
    ResourceRequest,
    ResourceResponse,
    Control,
    Data,
    Error,
}

impl proptest::arbitrary::Arbitrary for MessageType {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
        use proptest::strategy::Strategy;
        proptest::prop_oneof![
            proptest::strategy::Just(MessageType::FileRequest),
            proptest::strategy::Just(MessageType::FileResponse),
            proptest::strategy::Just(MessageType::ResourceRequest),
            proptest::strategy::Just(MessageType::ResourceResponse),
            proptest::strategy::Just(MessageType::Control),
            proptest::strategy::Just(MessageType::Data),
            proptest::strategy::Just(MessageType::Error),
        ]
        .boxed()
    }
}

impl QCArbitrary for MessageType {
    fn arbitrary(g: &mut Gen) -> Self {
        match usize::arbitrary(g) % 7 {
            0 => MessageType::FileRequest,
            1 => MessageType::FileResponse,
            2 => MessageType::ResourceRequest,
            3 => MessageType::ResourceResponse,
            4 => MessageType::Control,
            5 => MessageType::Data,
            _ => MessageType::Error,
        }
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        Box::new(std::iter::empty())
    }
}

/// Translator sandbox enforcement system
#[derive(Debug, Clone)]
pub struct TranslatorSandbox {
    pub contexts: HashMap<u32, TranslatorContext>,
    pub active_namespaces: HashSet<u32>,
    pub capability_table: HashMap<u32, HashSet<u32>>, // translator_id -> capabilities
    pub message_queue: Vec<TranslatorMessage>,
    pub global_limits: GlobalLimits,
}

#[derive(Debug, Clone)]
pub struct GlobalLimits {
    pub max_translators: u32,
    pub max_memory_per_translator: usize,
    pub max_cpu_per_translator: u64,
    pub max_files_per_translator: u32,
    pub max_children_per_translator: u32,
    pub max_message_queue_size: usize,
}

impl Default for GlobalLimits {
    fn default() -> Self {
        Self {
            max_translators: 1024,
            max_memory_per_translator: 1024 * 1024, // 1MB
            max_cpu_per_translator: 1000000, // 1 second
            max_files_per_translator: 64,
            max_children_per_translator: 8,
            max_message_queue_size: 10000,
        }
    }
}

impl Default for TranslatorSandbox {
    fn default() -> Self {
        Self {
            contexts: HashMap::new(),
            active_namespaces: HashSet::new(),
            capability_table: HashMap::new(),
            message_queue: Vec::new(),
            global_limits: GlobalLimits::default(),
        }
    }
}

impl TranslatorSandbox {
    /// Spawn new translator with isolation constraints
    pub fn spawn_translator(&mut self, context: TranslatorContext) -> Result<(), String> {
        // Check global limits
        if self.contexts.len() >= self.global_limits.max_translators as usize {
            return Err("Maximum translators reached".to_string());
        }

        // Validate resource limits
        if context.memory_limit > self.global_limits.max_memory_per_translator {
            return Err("Memory limit exceeds global maximum".to_string());
        }

        if context.cpu_limit > self.global_limits.max_cpu_per_translator {
            return Err("CPU limit exceeds global maximum".to_string());
        }

        // Validate parent-child relationships
        if let Some(parent_id) = context.parent_translator {
            if !self.contexts.contains_key(&parent_id) {
                return Err("Parent translator does not exist".to_string());
            }

            let parent = self.contexts.get(&parent_id).unwrap();
            if parent.child_translators.len() >= self.global_limits.max_children_per_translator as usize {
                return Err("Parent has maximum children".to_string());
            }
        }

        // Register namespace
        self.active_namespaces.insert(context.namespace_id);

        // Initialize empty capability set
        self.capability_table.insert(context.translator_id, HashSet::new());

        // Add to contexts
        self.contexts.insert(context.translator_id, context);

        Ok(())
    }

    /// Send message between translators with capability checking
    pub fn send_message(&mut self, message: TranslatorMessage) -> Result<(), String> {
        // Check message queue size
        if self.message_queue.len() >= self.global_limits.max_message_queue_size {
            return Err("Message queue full".to_string());
        }

        // Validate sender exists
        if !self.contexts.contains_key(&message.from_id) {
            return Err("Sender translator does not exist".to_string());
        }

        // Validate receiver exists
        if !self.contexts.contains_key(&message.to_id) {
            return Err("Receiver translator does not exist".to_string());
        }

        // Check capabilities
        if let Some(sender_caps) = self.capability_table.get(&message.from_id) {
            for required_cap in &message.capabilities {
                if !sender_caps.contains(required_cap) {
                    return Err(format!("Sender lacks capability {}", required_cap));
                }
            }
        }

        // Check namespace isolation
        let sender_ns = self.contexts[&message.from_id].namespace_id;
        let receiver_ns = self.contexts[&message.to_id].namespace_id;
        if sender_ns != receiver_ns {
            return Err("Cross-namespace communication not allowed".to_string());
        }

        self.message_queue.push(message);
        Ok(())
    }

    /// Update resource usage and enforce limits
    pub fn update_resource_usage(&mut self, translator_id: u32, usage: ResourceUsage) -> Result<(), String> {
        if let Some(context) = self.contexts.get_mut(&translator_id) {
            // Check memory limit
            if usage.memory_used > context.memory_limit {
                return Err("Memory limit exceeded".to_string());
            }

            // Check CPU limit
            if usage.cpu_used > context.cpu_limit {
                return Err("CPU limit exceeded".to_string());
            }

            // Check file limit
            if usage.files_opened > self.global_limits.max_files_per_translator {
                return Err("File limit exceeded".to_string());
            }

            context.resource_usage = usage;
            Ok(())
        } else {
            Err("Translator not found".to_string())
        }
    }

    /// Grant capability to translator
    pub fn grant_capability(&mut self, translator_id: u32, capability: u32) -> Result<(), String> {
        if self.contexts.contains_key(&translator_id) {
            self.capability_table
                .entry(translator_id)
                .or_insert_with(HashSet::new)
                .insert(capability);
            Ok(())
        } else {
            Err("Translator not found".to_string())
        }
    }

    /// Kill translator and cleanup resources
    pub fn kill_translator(&mut self, translator_id: u32) -> Result<(), String> {
        if let Some(context) = self.contexts.remove(&translator_id) {
            // Remove from capability table
            self.capability_table.remove(&translator_id);

            // Remove from parent's children list
            if let Some(parent_id) = context.parent_translator {
                if let Some(parent) = self.contexts.get_mut(&parent_id) {
                    parent.child_translators.retain(|&id| id != translator_id);
                }
            }

            // Kill all child translators
            for child_id in context.child_translators {
                let _ = self.kill_translator(child_id);
            }

            // Remove messages from/to this translator
            self.message_queue.retain(|msg| msg.from_id != translator_id && msg.to_id != translator_id);

            Ok(())
        } else {
            Err("Translator not found".to_string())
        }
    }
}

/// Translator isolation property tests
pub struct TranslatorIsolationProperties;

impl TranslatorIsolationProperties {
    /// THEOREM 1: Resource limits are strictly enforced
    pub fn resource_limits_enforced(sandbox: &TranslatorSandbox) -> bool {
        for context in sandbox.contexts.values() {
            // Memory usage must not exceed limit
            if context.resource_usage.memory_used > context.memory_limit {
                return false;
            }

            // CPU usage must not exceed limit
            if context.resource_usage.cpu_used > context.cpu_limit {
                return false;
            }

            // Files opened must not exceed global limit
            if context.resource_usage.files_opened > sandbox.global_limits.max_files_per_translator {
                return false;
            }
        }
        true
    }

    /// THEOREM 2: Namespace isolation is maintained
    pub fn namespace_isolation(sandbox: &TranslatorSandbox) -> bool {
        for message in &sandbox.message_queue {
            if let (Some(sender), Some(receiver)) = (
                sandbox.contexts.get(&message.from_id),
                sandbox.contexts.get(&message.to_id)
            ) {
                // Messages cannot cross namespace boundaries
                if sender.namespace_id != receiver.namespace_id {
                    return false;
                }
            }
        }
        true
    }

    /// THEOREM 3: Parent-child hierarchy is acyclic
    pub fn acyclic_hierarchy(sandbox: &TranslatorSandbox) -> bool {
        for (id, context) in &sandbox.contexts {
            if Self::has_cycle(sandbox, *id, HashSet::new()) {
                return false;
            }
        }
        true
    }

    /// THEOREM 4: Capability requirements are enforced
    pub fn capability_enforcement(sandbox: &TranslatorSandbox) -> bool {
        for message in &sandbox.message_queue {
            if let Some(sender_caps) = sandbox.capability_table.get(&message.from_id) {
                // All required capabilities must be possessed by sender
                for required_cap in &message.capabilities {
                    if !sender_caps.contains(required_cap) {
                        return false;
                    }
                }
            } else {
                // If no capabilities registered, message cannot require any
                if !message.capabilities.is_empty() {
                    return false;
                }
            }
        }
        true
    }

    /// THEOREM 5: Global limits are respected
    pub fn global_limits_respected(sandbox: &TranslatorSandbox) -> bool {
        // Total translators limit
        if sandbox.contexts.len() > sandbox.global_limits.max_translators as usize {
            return false;
        }

        // Message queue limit
        if sandbox.message_queue.len() > sandbox.global_limits.max_message_queue_size {
            return false;
        }

        // Children limits
        for context in sandbox.contexts.values() {
            if context.child_translators.len() > sandbox.global_limits.max_children_per_translator as usize {
                return false;
            }
        }

        true
    }

    /// THEOREM 6: Resource cleanup on termination
    pub fn resource_cleanup_property(sandbox: &TranslatorSandbox, terminated_ids: &HashSet<u32>) -> bool {
        for terminated_id in terminated_ids {
            // Terminated translator should not exist in contexts
            if sandbox.contexts.contains_key(terminated_id) {
                return false;
            }

            // Should not exist in capability table
            if sandbox.capability_table.contains_key(terminated_id) {
                return false;
            }

            // Should not have pending messages
            for message in &sandbox.message_queue {
                if message.from_id == *terminated_id || message.to_id == *terminated_id {
                    return false;
                }
            }
        }
        true
    }

    /// Helper: Detect cycles in parent-child hierarchy
    fn has_cycle(sandbox: &TranslatorSandbox, start_id: u32, mut visited: HashSet<u32>) -> bool {
        if visited.contains(&start_id) {
            return true; // Cycle detected
        }

        visited.insert(start_id);

        if let Some(context) = sandbox.contexts.get(&start_id) {
            for &child_id in &context.child_translators {
                if Self::has_cycle(sandbox, child_id, visited.clone()) {
                    return true;
                }
            }
        }

        false
    }
}

/// QuickCheck properties
#[quickcheck]
fn prop_resource_limits(contexts: Vec<TranslatorContext>) -> TestResult {
    if contexts.len() > 20 {
        return TestResult::discard();
    }

    let mut sandbox = TranslatorSandbox::default();

    for context in contexts {
        let _ = sandbox.spawn_translator(context);
    }

    TestResult::from_bool(TranslatorIsolationProperties::resource_limits_enforced(&sandbox))
}

#[quickcheck]
fn prop_namespace_isolation(contexts: Vec<TranslatorContext>, messages: Vec<TranslatorMessage>) -> TestResult {
    if contexts.len() > 15 || messages.len() > 30 {
        return TestResult::discard();
    }

    let mut sandbox = TranslatorSandbox::default();

    // Spawn translators
    for context in contexts {
        let _ = sandbox.spawn_translator(context);
    }

    // Send messages (ignore failures for this property test)
    for message in messages {
        let _ = sandbox.send_message(message);
    }

    TestResult::from_bool(TranslatorIsolationProperties::namespace_isolation(&sandbox))
}

#[quickcheck]
fn prop_global_limits(contexts: Vec<TranslatorContext>) -> TestResult {
    if contexts.len() > 50 {
        return TestResult::discard();
    }

    let mut sandbox = TranslatorSandbox::default();

    for context in contexts {
        let _ = sandbox.spawn_translator(context);
    }

    TestResult::from_bool(TranslatorIsolationProperties::global_limits_respected(&sandbox))
}

#[quickcheck]
fn prop_capability_enforcement(contexts: Vec<TranslatorContext>, messages: Vec<TranslatorMessage>) -> TestResult {
    if contexts.len() > 10 || messages.len() > 20 {
        return TestResult::discard();
    }

    let mut sandbox = TranslatorSandbox::default();

    for context in contexts {
        let translator_id = context.translator_id;
        if sandbox.spawn_translator(context).is_ok() {
            // Grant some random capabilities
            for cap in 1..=5u32 {
                if translator_id % cap == 0 {
                    let _ = sandbox.grant_capability(translator_id, cap);
                }
            }
        }
    }

    for message in messages {
        let _ = sandbox.send_message(message);
    }

    TestResult::from_bool(TranslatorIsolationProperties::capability_enforcement(&sandbox))
}

/// Proptest specifications
proptest! {
    #![proptest_config(ProptestConfig::with_cases(3000))]

    #[test]
    fn proptest_resource_enforcement(contexts in prop::collection::vec(any::<TranslatorContext>(), 1..10)) {
        let mut sandbox = TranslatorSandbox::default();

        for context in contexts {
            let _ = sandbox.spawn_translator(context);
        }

        prop_assert!(TranslatorIsolationProperties::resource_limits_enforced(&sandbox));
        prop_assert!(TranslatorIsolationProperties::global_limits_respected(&sandbox));
    }

    #[test]
    fn proptest_hierarchy_acyclic(contexts in prop::collection::vec(any::<TranslatorContext>(), 1..8)) {
        let mut sandbox = TranslatorSandbox::default();

        for context in contexts {
            let _ = sandbox.spawn_translator(context);
        }

        prop_assert!(TranslatorIsolationProperties::acyclic_hierarchy(&sandbox));
    }

    #[test]
    fn proptest_isolation_boundaries(
        contexts in prop::collection::vec(any::<TranslatorContext>(), 1..12),
        messages in prop::collection::vec(any::<TranslatorMessage>(), 1..25)
    ) {
        let mut sandbox = TranslatorSandbox::default();

        for context in contexts {
            let _ = sandbox.spawn_translator(context);
        }

        for message in messages {
            let _ = sandbox.send_message(message);
        }

        prop_assert!(TranslatorIsolationProperties::namespace_isolation(&sandbox));
        prop_assert!(TranslatorIsolationProperties::capability_enforcement(&sandbox));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_translator_lifecycle() {
        let mut sandbox = TranslatorSandbox::default();

        let context = TranslatorContext {
            translator_id: 1,
            code_hash: [1u8; 32],
            memory_limit: 1024,
            cpu_limit: 1000,
            file_permissions: 0b111, // read/write/execute
            namespace_id: 1,
            parent_translator: None,
            child_translators: vec![],
            allowed_syscalls: vec![1, 2, 3],
            resource_usage: ResourceUsage {
                memory_used: 0,
                cpu_used: 0,
                files_opened: 0,
                network_connections: 0,
                child_processes: 0,
                syscalls_made: HashMap::new(),
            },
        };

        // Spawn translator
        assert!(sandbox.spawn_translator(context).is_ok());
        assert_eq!(sandbox.contexts.len(), 1);

        // Kill translator
        assert!(sandbox.kill_translator(1).is_ok());
        assert_eq!(sandbox.contexts.len(), 0);
    }

    #[test]
    fn test_resource_limit_enforcement() {
        let mut sandbox = TranslatorSandbox::default();

        let context = TranslatorContext {
            translator_id: 1,
            code_hash: [1u8; 32],
            memory_limit: 1024,
            cpu_limit: 1000,
            file_permissions: 0b111,
            namespace_id: 1,
            parent_translator: None,
            child_translators: vec![],
            allowed_syscalls: vec![],
            resource_usage: ResourceUsage {
                memory_used: 0,
                cpu_used: 0,
                files_opened: 0,
                network_connections: 0,
                child_processes: 0,
                syscalls_made: HashMap::new(),
            },
        };

        assert!(sandbox.spawn_translator(context).is_ok());

        // Exceed memory limit
        let excessive_usage = ResourceUsage {
            memory_used: 2048, // Exceeds 1024 limit
            cpu_used: 500,
            files_opened: 0,
            network_connections: 0,
            child_processes: 0,
            syscalls_made: HashMap::new(),
        };

        assert!(sandbox.update_resource_usage(1, excessive_usage).is_err());
    }

    #[test]
    fn test_namespace_isolation() {
        let mut sandbox = TranslatorSandbox::default();

        // Create translators in different namespaces
        let context1 = TranslatorContext {
            translator_id: 1,
            namespace_id: 1,
            code_hash: [1u8; 32],
            memory_limit: 1024,
            cpu_limit: 1000,
            file_permissions: 0,
            parent_translator: None,
            child_translators: vec![],
            allowed_syscalls: vec![],
            resource_usage: ResourceUsage {
                memory_used: 0,
                cpu_used: 0,
                files_opened: 0,
                network_connections: 0,
                child_processes: 0,
                syscalls_made: HashMap::new(),
            },
        };

        let context2 = TranslatorContext {
            translator_id: 2,
            namespace_id: 2, // Different namespace
            ..context1.clone()
        };

        assert!(sandbox.spawn_translator(context1).is_ok());
        assert!(sandbox.spawn_translator(context2).is_ok());

        // Try cross-namespace message (should fail)
        let cross_ns_message = TranslatorMessage {
            from_id: 1,
            to_id: 2,
            message_type: MessageType::Data,
            payload: vec![1, 2, 3],
            capabilities: vec![],
            priority: 1,
        };

        assert!(sandbox.send_message(cross_ns_message).is_err());
    }

    #[test]
    fn test_capability_based_messaging() {
        let mut sandbox = TranslatorSandbox::default();

        let context1 = TranslatorContext {
            translator_id: 1,
            namespace_id: 1,
            code_hash: [1u8; 32],
            memory_limit: 1024,
            cpu_limit: 1000,
            file_permissions: 0,
            parent_translator: None,
            child_translators: vec![],
            allowed_syscalls: vec![],
            resource_usage: ResourceUsage {
                memory_used: 0,
                cpu_used: 0,
                files_opened: 0,
                network_connections: 0,
                child_processes: 0,
                syscalls_made: HashMap::new(),
            },
        };

        let context2 = TranslatorContext {
            translator_id: 2,
            namespace_id: 1, // Same namespace
            ..context1.clone()
        };

        assert!(sandbox.spawn_translator(context1).is_ok());
        assert!(sandbox.spawn_translator(context2).is_ok());

        // Message requiring capability 42
        let cap_message = TranslatorMessage {
            from_id: 1,
            to_id: 2,
            message_type: MessageType::FileRequest,
            payload: vec![],
            capabilities: vec![42],
            priority: 1,
        };

        // Should fail without capability
        assert!(sandbox.send_message(cap_message.clone()).is_err());

        // Grant capability and retry
        assert!(sandbox.grant_capability(1, 42).is_ok());
        assert!(sandbox.send_message(cap_message).is_ok());
    }
}
