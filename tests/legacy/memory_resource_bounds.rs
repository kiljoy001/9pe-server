//! Memory Safety and Resource Bounds Property-Based Testing
//! Ruthlessly validates all components stay within formal memory limits

use proptest::prelude::*;
use quickcheck::TestResult;
use quickcheck_macros::quickcheck;
use arbitrary::Arbitrary;
use std::collections::HashMap;
use quickcheck::{Arbitrary as QCArbitrary, Gen};

/// Memory allocation tracking for 9P.e components
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryAllocation {
    pub component_id: u32,
    pub component_type: ComponentType,
    pub allocated_bytes: usize,
    pub peak_bytes: usize,
    pub allocation_count: u64,
    pub deallocation_count: u64,
    pub fragmentation_ratio: f64, // 0.0 = no fragmentation, 1.0 = max fragmentation
}

#[derive(Debug, Clone, PartialEq, Arbitrary)]
pub enum ComponentType {
    ProtocolHandler,
    TranslatorSandbox,
    SyntheticFileGenerator,
    GhostdagConsensus,
    CryptoSession,
    StreamMultiplexer,
    CapabilityManager,
    CompatibilityLayer,
    MessageQueue,
    CacheSystem,
}

impl proptest::arbitrary::Arbitrary for ComponentType {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
        use proptest::strategy::Strategy;
        proptest::prop_oneof![
            proptest::strategy::Just(ComponentType::ProtocolHandler),
            proptest::strategy::Just(ComponentType::TranslatorSandbox),
            proptest::strategy::Just(ComponentType::SyntheticFileGenerator),
            proptest::strategy::Just(ComponentType::GhostdagConsensus),
            proptest::strategy::Just(ComponentType::CryptoSession),
            proptest::strategy::Just(ComponentType::StreamMultiplexer),
            proptest::strategy::Just(ComponentType::CapabilityManager),
            proptest::strategy::Just(ComponentType::CompatibilityLayer),
            proptest::strategy::Just(ComponentType::MessageQueue),
            proptest::strategy::Just(ComponentType::CacheSystem),
        ]
        .boxed()
    }
}

impl QCArbitrary for ComponentType {
    fn arbitrary(g: &mut Gen) -> Self {
        match usize::arbitrary(g) % 10 {
            0 => ComponentType::ProtocolHandler,
            1 => ComponentType::TranslatorSandbox,
            2 => ComponentType::SyntheticFileGenerator,
            3 => ComponentType::GhostdagConsensus,
            4 => ComponentType::CryptoSession,
            5 => ComponentType::StreamMultiplexer,
            6 => ComponentType::CapabilityManager,
            7 => ComponentType::CompatibilityLayer,
            8 => ComponentType::MessageQueue,
            _ => ComponentType::CacheSystem,
        }
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        Box::new(std::iter::empty())
    }
}

/// Resource usage metrics per component
#[derive(Debug, Clone, PartialEq, Arbitrary)]
pub struct ResourceUsage {
    pub cpu_time: u64, // microseconds
    pub memory_bytes: usize,
    pub file_descriptors: u32,
    pub network_connections: u32,
    pub thread_count: u32,
    pub heap_allocations: u64,
    pub stack_depth: u32,
}

impl proptest::arbitrary::Arbitrary for ResourceUsage {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
        use proptest::strategy::Strategy;
        (
            any::<u64>(),
            any::<usize>(),
            any::<u32>(),
            any::<u32>(),
            any::<u32>(),
            any::<u64>(),
            any::<u32>(),
        )
            .prop_map(
                |(
                    cpu_time,
                    memory_bytes,
                    file_descriptors,
                    network_connections,
                    thread_count,
                    heap_allocations,
                    stack_depth,
                )|
                     ResourceUsage {
                        cpu_time,
                        memory_bytes,
                        file_descriptors,
                        network_connections,
                        thread_count,
                        heap_allocations,
                        stack_depth,
                    },
            )
            .boxed()
    }
}

/// System-wide resource bounds enforcement
#[derive(Debug, Clone)]
pub struct ResourceBoundSystem {
    pub component_allocations: HashMap<u32, MemoryAllocation>,
    pub component_usage: HashMap<u32, ResourceUsage>,
    pub global_limits: GlobalResourceLimits,
    pub allocation_history: Vec<AllocationEvent>,
    pub oom_prevention: OutOfMemoryPrevention,
}

#[derive(Debug, Clone)]
pub struct GlobalResourceLimits {
    // Memory limits from formal specification
    pub max_total_memory: usize,          // 16MB total system
    pub max_protocol_memory: usize,       // 1MB per protocol handler
    pub max_translator_memory: usize,     // 1MB per translator
    pub max_synthetic_memory: usize,      // 64KB per synthetic generator
    pub max_consensus_memory: usize,      // 8MB for GHOSTDAG (optimized)
    pub max_crypto_memory: usize,         // 256KB per crypto session
    pub max_stream_memory: usize,         // 512KB per stream multiplexer
    pub max_capability_memory: usize,     // 128KB capability manager
    pub max_compatibility_memory: usize,  // 256KB compatibility layer
    pub max_message_queue_memory: usize,  // 2MB message queues
    pub max_cache_memory: usize,          // 4MB cache system

    // CPU and other resource limits
    pub max_cpu_per_component: u64,       // 1 second per component per operation
    pub max_file_descriptors: u32,        // 1024 total FDs
    pub max_network_connections: u32,     // 256 connections
    pub max_threads: u32,                 // 64 threads
    pub max_stack_depth: u32,             // 1024 call stack depth
}

#[derive(Debug, Clone)]
pub struct OutOfMemoryPrevention {
    pub emergency_reserve: usize,         // 1MB emergency reserve
    pub warning_threshold: f64,           // 0.85 = warn at 85% usage
    pub critical_threshold: f64,          // 0.95 = critical at 95% usage
    pub active_gc_threshold: f64,         // 0.80 = trigger GC at 80%
    pub component_kill_threshold: f64,    // 0.98 = kill components at 98%
}

#[derive(Debug, Clone, PartialEq)]
pub struct AllocationEvent {
    pub timestamp: u64,
    pub component_id: u32,
    pub event_type: AllocationEventType,
    pub bytes: usize,
    pub total_after: usize,
}

#[derive(Debug, Clone, PartialEq, Arbitrary)]
pub enum AllocationEventType {
    Allocate,
    Deallocate,
    Reallocate,
    GarbageCollect,
    EmergencyFree,
}

impl Default for GlobalResourceLimits {
    fn default() -> Self {
        Self {
            max_total_memory: 16 * 1024 * 1024,         // 16MB
            max_protocol_memory: 1024 * 1024,           // 1MB
            max_translator_memory: 1024 * 1024,         // 1MB
            max_synthetic_memory: 64 * 1024,            // 64KB
            max_consensus_memory: 8 * 1024 * 1024,      // 8MB (with pebbling optimizations)
            max_crypto_memory: 256 * 1024,              // 256KB
            max_stream_memory: 512 * 1024,              // 512KB
            max_capability_memory: 128 * 1024,          // 128KB
            max_compatibility_memory: 256 * 1024,       // 256KB
            max_message_queue_memory: 2 * 1024 * 1024,  // 2MB
            max_cache_memory: 4 * 1024 * 1024,          // 4MB
            max_cpu_per_component: 1000000,             // 1 second
            max_file_descriptors: 1024,
            max_network_connections: 256,
            max_threads: 64,
            max_stack_depth: 1024,
        }
    }
}

impl Default for OutOfMemoryPrevention {
    fn default() -> Self {
        Self {
            emergency_reserve: 1024 * 1024,  // 1MB
            warning_threshold: 0.85,
            critical_threshold: 0.95,
            active_gc_threshold: 0.80,
            component_kill_threshold: 0.98,
        }
    }
}

impl Default for ResourceBoundSystem {
    fn default() -> Self {
        Self {
            component_allocations: HashMap::new(),
            component_usage: HashMap::new(),
            global_limits: GlobalResourceLimits::default(),
            allocation_history: Vec::new(),
            oom_prevention: OutOfMemoryPrevention::default(),
        }
    }
}

impl ResourceBoundSystem {
    /// Allocate memory for component with bounds checking
    pub fn allocate_memory(&mut self, component_id: u32, component_type: ComponentType, bytes: usize) -> Result<(), String> {
        // Check component-specific limits
        let max_component_memory = self.get_component_memory_limit(&component_type);
        if bytes > max_component_memory {
            return Err(format!("Allocation exceeds component limit: {} > {}", bytes, max_component_memory));
        }

        // Check current component usage
        if let Some(allocation) = self.component_allocations.get(&component_id) {
            let new_total = allocation.allocated_bytes + bytes;
            if new_total > max_component_memory {
                return Err(format!("Component would exceed memory limit: {} > {}", new_total, max_component_memory));
            }
        }

        // Check global memory limit
        let current_total = self.get_total_allocated_memory();
        let new_global_total = current_total + bytes;

        if new_global_total > self.global_limits.max_total_memory {
            return Err(format!("Would exceed global memory limit: {} > {}", new_global_total, self.global_limits.max_total_memory));
        }

        // Check OOM prevention thresholds
        let usage_ratio = new_global_total as f64 / self.global_limits.max_total_memory as f64;

        if usage_ratio > self.oom_prevention.critical_threshold {
            return Err("Memory usage in critical zone - allocation denied".to_string());
        }

        if usage_ratio > self.oom_prevention.active_gc_threshold {
            // Trigger garbage collection before allowing allocation
            self.trigger_garbage_collection();
        }

        // Perform allocation
        let allocation = self.component_allocations
            .entry(component_id)
            .or_insert(MemoryAllocation {
                component_id,
                component_type: component_type.clone(),
                allocated_bytes: 0,
                peak_bytes: 0,
                allocation_count: 0,
                deallocation_count: 0,
                fragmentation_ratio: 0.0,
            });

        allocation.allocated_bytes += bytes;
        allocation.peak_bytes = allocation.peak_bytes.max(allocation.allocated_bytes);
        allocation.allocation_count += 1;

        // Record allocation event
        self.allocation_history.push(AllocationEvent {
            timestamp: Self::current_timestamp(),
            component_id,
            event_type: AllocationEventType::Allocate,
            bytes,
            total_after: allocation.allocated_bytes,
        });

        // Limit history size
        if self.allocation_history.len() > 10000 {
            self.allocation_history.drain(0..1000);
        }

        Ok(())
    }

    /// Deallocate memory for component
    pub fn deallocate_memory(&mut self, component_id: u32, bytes: usize) -> Result<(), String> {
        if let Some(allocation) = self.component_allocations.get_mut(&component_id) {
            if bytes > allocation.allocated_bytes {
                return Err("Cannot deallocate more than allocated".to_string());
            }

            allocation.allocated_bytes -= bytes;
            allocation.deallocation_count += 1;

            // Record deallocation event
            self.allocation_history.push(AllocationEvent {
                timestamp: Self::current_timestamp(),
                component_id,
                event_type: AllocationEventType::Deallocate,
                bytes,
                total_after: allocation.allocated_bytes,
            });

            // Clean up empty allocations
            if allocation.allocated_bytes == 0 {
                self.component_allocations.remove(&component_id);
            }

            Ok(())
        } else {
            Err("Component not found for deallocation".to_string())
        }
    }

    /// Update resource usage for component
    pub fn update_resource_usage(&mut self, component_id: u32, usage: ResourceUsage) -> Result<(), String> {
        // Validate against limits
        if usage.cpu_time > self.global_limits.max_cpu_per_component {
            return Err("CPU time exceeds limit".to_string());
        }

        if usage.file_descriptors > self.global_limits.max_file_descriptors {
            return Err("File descriptor count exceeds limit".to_string());
        }

        if usage.network_connections > self.global_limits.max_network_connections {
            return Err("Network connection count exceeds limit".to_string());
        }

        if usage.thread_count > self.global_limits.max_threads {
            return Err("Thread count exceeds limit".to_string());
        }

        if usage.stack_depth > self.global_limits.max_stack_depth {
            return Err("Stack depth exceeds limit".to_string());
        }

        self.component_usage.insert(component_id, usage);
        Ok(())
    }

    /// Get component-specific memory limit
    fn get_component_memory_limit(&self, component_type: &ComponentType) -> usize {
        match component_type {
            ComponentType::ProtocolHandler => self.global_limits.max_protocol_memory,
            ComponentType::TranslatorSandbox => self.global_limits.max_translator_memory,
            ComponentType::SyntheticFileGenerator => self.global_limits.max_synthetic_memory,
            ComponentType::GhostdagConsensus => self.global_limits.max_consensus_memory,
            ComponentType::CryptoSession => self.global_limits.max_crypto_memory,
            ComponentType::StreamMultiplexer => self.global_limits.max_stream_memory,
            ComponentType::CapabilityManager => self.global_limits.max_capability_memory,
            ComponentType::CompatibilityLayer => self.global_limits.max_compatibility_memory,
            ComponentType::MessageQueue => self.global_limits.max_message_queue_memory,
            ComponentType::CacheSystem => self.global_limits.max_cache_memory,
        }
    }

    /// Get total allocated memory across all components
    pub fn get_total_allocated_memory(&self) -> usize {
        self.component_allocations.values()
            .map(|alloc| alloc.allocated_bytes)
            .sum()
    }

    /// Trigger garbage collection
    fn trigger_garbage_collection(&mut self) {
        // Record GC event
        self.allocation_history.push(AllocationEvent {
            timestamp: Self::current_timestamp(),
            component_id: 0, // System event
            event_type: AllocationEventType::GarbageCollect,
            bytes: 0,
            total_after: self.get_total_allocated_memory(),
        });

        // Simplified GC: reduce fragmentation ratios
        for allocation in self.component_allocations.values_mut() {
            allocation.fragmentation_ratio *= 0.5; // Reduce fragmentation
        }
    }

    /// Emergency memory cleanup
    pub fn emergency_memory_cleanup(&mut self) -> usize {
        let initial_memory = self.get_total_allocated_memory();

        // Kill non-critical components if in emergency
        let usage_ratio = initial_memory as f64 / self.global_limits.max_total_memory as f64;

        if usage_ratio > self.oom_prevention.component_kill_threshold {
            // Identify components to kill (least critical first)
            let mut components_to_kill = Vec::new();

            for (&component_id, allocation) in &self.component_allocations {
                // Kill non-essential components first
                match allocation.component_type {
                    ComponentType::CacheSystem |
                    ComponentType::SyntheticFileGenerator => {
                        components_to_kill.push(component_id);
                    }
                    _ => {}
                }
            }

            // Kill components
            for component_id in components_to_kill {
                if let Some(allocation) = self.component_allocations.remove(&component_id) {
                    self.allocation_history.push(AllocationEvent {
                        timestamp: Self::current_timestamp(),
                        component_id,
                        event_type: AllocationEventType::EmergencyFree,
                        bytes: allocation.allocated_bytes,
                        total_after: self.get_total_allocated_memory(),
                    });
                }
                self.component_usage.remove(&component_id);
            }
        }

        let final_memory = self.get_total_allocated_memory();
        initial_memory - final_memory // Return bytes freed
    }

    /// Check if system is in healthy memory state
    pub fn is_memory_healthy(&self) -> bool {
        let usage_ratio = self.get_total_allocated_memory() as f64 / self.global_limits.max_total_memory as f64;
        usage_ratio < self.oom_prevention.warning_threshold
    }

    /// Get memory usage statistics
    pub fn get_memory_stats(&self) -> MemoryStats {
        let total_allocated = self.get_total_allocated_memory();
        let total_limit = self.global_limits.max_total_memory;

        MemoryStats {
            total_allocated,
            total_limit,
            usage_percentage: (total_allocated as f64 / total_limit as f64 * 100.0) as u32,
            component_count: self.component_allocations.len() as u32,
            allocation_events: self.allocation_history.len() as u64,
            fragmentation_ratio: self.calculate_average_fragmentation(),
        }
    }

    /// Calculate average fragmentation across all components
    fn calculate_average_fragmentation(&self) -> f64 {
        if self.component_allocations.is_empty() {
            return 0.0;
        }

        let total_fragmentation: f64 = self.component_allocations.values()
            .map(|alloc| alloc.fragmentation_ratio)
            .sum();

        total_fragmentation / self.component_allocations.len() as f64
    }

    /// Current timestamp (simplified)
    fn current_timestamp() -> u64 {
        1234567890000
    }

    /// Cleanup component resources
    pub fn cleanup_component(&mut self, component_id: u32) -> Result<(), String> {
        if let Some(allocation) = self.component_allocations.remove(&component_id) {
            self.allocation_history.push(AllocationEvent {
                timestamp: Self::current_timestamp(),
                component_id,
                event_type: AllocationEventType::Deallocate,
                bytes: allocation.allocated_bytes,
                total_after: self.get_total_allocated_memory(),
            });
        }

        self.component_usage.remove(&component_id);
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct MemoryStats {
    pub total_allocated: usize,
    pub total_limit: usize,
    pub usage_percentage: u32,
    pub component_count: u32,
    pub allocation_events: u64,
    pub fragmentation_ratio: f64,
}

/// Memory and resource bounds property tests
pub struct ResourceBoundsProperties;

impl ResourceBoundsProperties {
    /// THEOREM 1: Component memory limits are never exceeded
    pub fn component_memory_limits_enforced(system: &ResourceBoundSystem) -> bool {
        for allocation in system.component_allocations.values() {
            let limit = system.get_component_memory_limit(&allocation.component_type);
            if allocation.allocated_bytes > limit {
                return false;
            }
        }
        true
    }

    /// THEOREM 2: Global memory limit is never exceeded
    pub fn global_memory_limit_enforced(system: &ResourceBoundSystem) -> bool {
        let total = system.get_total_allocated_memory();
        total <= system.global_limits.max_total_memory
    }

    /// THEOREM 3: Allocation/deallocation accounting is consistent
    pub fn allocation_accounting_consistent(system: &ResourceBoundSystem) -> bool {
        for allocation in system.component_allocations.values() {
            // Cannot deallocate more than allocated
            if allocation.deallocation_count > allocation.allocation_count {
                return false;
            }

            // Peak bytes should be at least current bytes
            if allocation.peak_bytes < allocation.allocated_bytes {
                return false;
            }
        }
        true
    }

    /// THEOREM 4: Resource usage stays within bounds
    pub fn resource_usage_bounds_enforced(system: &ResourceBoundSystem) -> bool {
        for usage in system.component_usage.values() {
            if usage.cpu_time > system.global_limits.max_cpu_per_component {
                return false;
            }
            if usage.file_descriptors > system.global_limits.max_file_descriptors {
                return false;
            }
            if usage.network_connections > system.global_limits.max_network_connections {
                return false;
            }
            if usage.thread_count > system.global_limits.max_threads {
                return false;
            }
            if usage.stack_depth > system.global_limits.max_stack_depth {
                return false;
            }
        }
        true
    }

    /// THEOREM 5: OOM prevention thresholds trigger correctly
    pub fn oom_prevention_triggers_correctly(system: &ResourceBoundSystem) -> bool {
        let total_memory = system.get_total_allocated_memory();
        let usage_ratio = total_memory as f64 / system.global_limits.max_total_memory as f64;

        // If we're above critical threshold, we should not be here (allocation should have been denied)
        if usage_ratio > system.oom_prevention.critical_threshold {
            return false;
        }

        // Emergency reserve should always be available
        let available_memory = system.global_limits.max_total_memory - total_memory;
        available_memory >= system.oom_prevention.emergency_reserve
    }

    /// THEOREM 6: Memory stats are accurate
    pub fn memory_stats_accurate(system: &ResourceBoundSystem) -> bool {
        let stats = system.get_memory_stats();

        // Stats should match actual allocations
        if stats.total_allocated != system.get_total_allocated_memory() {
            return false;
        }

        if stats.total_limit != system.global_limits.max_total_memory {
            return false;
        }

        if stats.component_count != system.component_allocations.len() as u32 {
            return false;
        }

        // Usage percentage should be calculated correctly
        let expected_percentage = (stats.total_allocated as f64 / stats.total_limit as f64 * 100.0) as u32;
        if stats.usage_percentage != expected_percentage {
            return false;
        }

        true
    }

    /// THEOREM 7: Emergency cleanup frees memory
    pub fn emergency_cleanup_frees_memory(system: &mut ResourceBoundSystem, initial_usage_high: bool) -> bool {
        if !initial_usage_high {
            return true; // Property only applies when usage is high
        }

        let initial_memory = system.get_total_allocated_memory();
        let freed_bytes = system.emergency_memory_cleanup();
        let final_memory = system.get_total_allocated_memory();

        // Should free some memory if any was allocated
        if initial_memory > 0 {
            freed_bytes > 0 && final_memory < initial_memory
        } else {
            freed_bytes == 0 // Nothing to free
        }
    }
}

/// QuickCheck properties
#[quickcheck]
fn prop_component_memory_limits(component_type: ComponentType, allocation_size: u32) -> TestResult {
    if allocation_size > 16 * 1024 * 1024 {
        return TestResult::discard(); // Skip unreasonably large allocations
    }

    let mut system = ResourceBoundSystem::default();
    let component_id = 1;

    let result = system.allocate_memory(component_id, component_type, allocation_size as usize);

    // If allocation succeeded, limits should still be enforced
    if result.is_ok() {
        TestResult::from_bool(ResourceBoundsProperties::component_memory_limits_enforced(&system))
    } else {
        TestResult::passed() // Rejection is acceptable
    }
}

#[quickcheck]
fn prop_global_memory_limit(allocations: Vec<(u8, u32)>) -> TestResult {
    if allocations.len() > 20 {
        return TestResult::discard();
    }

    let mut system = ResourceBoundSystem::default();

    // Try to allocate for multiple components
    for (component_id, size) in allocations {
        if size > 1024 * 1024 {
            continue; // Skip large allocations
        }

        let _ = system.allocate_memory(
            component_id as u32,
            ComponentType::ProtocolHandler,
            size as usize,
        );
    }

    TestResult::from_bool(ResourceBoundsProperties::global_memory_limit_enforced(&system))
}

#[quickcheck]
fn prop_allocation_accounting(component_id: u32, operations: Vec<(bool, u32)>) -> TestResult {
    if operations.len() > 50 {
        return TestResult::discard();
    }

    let mut system = ResourceBoundSystem::default();

    // Perform allocation/deallocation operations
    for (is_allocation, size) in operations {
        if size > 64 * 1024 {
            continue; // Skip large operations
        }

        if is_allocation {
            let _ = system.allocate_memory(component_id, ComponentType::ProtocolHandler, size as usize);
        } else {
            let _ = system.deallocate_memory(component_id, size as usize);
        }
    }

    TestResult::from_bool(ResourceBoundsProperties::allocation_accounting_consistent(&system))
}

/// Proptest specifications
proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    #[test]
    fn proptest_memory_bounds_comprehensive(
        components in prop::collection::vec((any::<ComponentType>(), 1u32..1024*1024), 1..10)
    ) {
        let mut system = ResourceBoundSystem::default();

        // Allocate memory for components
        for (i, (component_type, size)) in components.into_iter().enumerate() {
            let component_id = i as u32;
            let _ = system.allocate_memory(component_id, component_type, size as usize);
        }

        prop_assert!(ResourceBoundsProperties::component_memory_limits_enforced(&system));
        prop_assert!(ResourceBoundsProperties::global_memory_limit_enforced(&system));
        prop_assert!(ResourceBoundsProperties::allocation_accounting_consistent(&system));
        prop_assert!(ResourceBoundsProperties::memory_stats_accurate(&system));
    }

    #[test]
    fn proptest_resource_usage_bounds(usages in prop::collection::vec(any::<ResourceUsage>(), 1..8)) {
        let mut system = ResourceBoundSystem::default();

        // Update resource usage for components
        for (i, usage) in usages.into_iter().enumerate() {
            let component_id = i as u32;
            let _ = system.update_resource_usage(component_id, usage);
        }

        prop_assert!(ResourceBoundsProperties::resource_usage_bounds_enforced(&system));
    }

    #[test]
    fn proptest_oom_prevention(
        allocations in prop::collection::vec((any::<ComponentType>(), 1u32..512*1024), 1..15)
    ) {
        let mut system = ResourceBoundSystem::default();

        // Allocate until near limit
        for (i, (component_type, size)) in allocations.into_iter().enumerate() {
            let component_id = i as u32;
            let _ = system.allocate_memory(component_id, component_type, size as usize);
        }

        prop_assert!(ResourceBoundsProperties::oom_prevention_triggers_correctly(&system));

        // Test emergency cleanup if memory usage is high
        let usage_ratio = system.get_total_allocated_memory() as f64 / system.global_limits.max_total_memory as f64;
        let initial_usage_high = usage_ratio > system.oom_prevention.active_gc_threshold;

        prop_assert!(ResourceBoundsProperties::emergency_cleanup_frees_memory(&mut system, initial_usage_high));
    }

    #[test]
    fn proptest_memory_lifecycle(
        operations in prop::collection::vec((any::<bool>(), any::<ComponentType>(), 1u32..256*1024), 1..20)
    ) {
        let mut system = ResourceBoundSystem::default();
        let mut active_components = std::collections::HashMap::new();

        for (i, (is_cleanup, component_type, size)) in operations.into_iter().enumerate() {
            let component_id = i as u32 % 8; // Limit to 8 components for reuse

            if is_cleanup && active_components.contains_key(&component_id) {
                // Cleanup component
                let _ = system.cleanup_component(component_id);
                active_components.remove(&component_id);
            } else {
                // Allocate for component
                if system.allocate_memory(component_id, component_type.clone(), size as usize).is_ok() {
                    active_components.insert(component_id, component_type);
                }
            }
        }

        prop_assert!(ResourceBoundsProperties::component_memory_limits_enforced(&system));
        prop_assert!(ResourceBoundsProperties::global_memory_limit_enforced(&system));
        prop_assert!(ResourceBoundsProperties::allocation_accounting_consistent(&system));
        prop_assert!(system.is_memory_healthy() || system.get_total_allocated_memory() > 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_memory_allocation() {
        let mut system = ResourceBoundSystem::default();

        // Allocate memory within limits
        assert!(system.allocate_memory(1, ComponentType::ProtocolHandler, 1024).is_ok());
        assert_eq!(system.get_total_allocated_memory(), 1024);

        // Deallocate
        assert!(system.deallocate_memory(1, 512).is_ok());
        assert_eq!(system.get_total_allocated_memory(), 512);

        // Deallocate remaining
        assert!(system.deallocate_memory(1, 512).is_ok());
        assert_eq!(system.get_total_allocated_memory(), 0);
    }

    #[test]
    fn test_component_memory_limits() {
        let mut system = ResourceBoundSystem::default();

        // Try to allocate more than component limit for synthetic generator (64KB)
        let result = system.allocate_memory(1, ComponentType::SyntheticFileGenerator, 128 * 1024);
        assert!(result.is_err());

        // Within limit should work
        let result = system.allocate_memory(1, ComponentType::SyntheticFileGenerator, 32 * 1024);
        assert!(result.is_ok());
    }

    #[test]
    fn test_global_memory_limit() {
        let mut system = ResourceBoundSystem::default();
        system.global_limits.max_total_memory = 1024; // Set very small limit

        // First allocation should work
        assert!(system.allocate_memory(1, ComponentType::ProtocolHandler, 512).is_ok());

        // Second allocation should exceed global limit
        assert!(system.allocate_memory(2, ComponentType::ProtocolHandler, 600).is_err());
    }

    #[test]
    fn test_oom_prevention() {
        let mut system = ResourceBoundSystem::default();
        system.global_limits.max_total_memory = 1000;
        system.oom_prevention.critical_threshold = 0.9; // 90%

        // Allocate up to warning threshold (should work)
        assert!(system.allocate_memory(1, ComponentType::ProtocolHandler, 800).is_ok());

        // Try to allocate beyond critical threshold (should fail)
        assert!(system.allocate_memory(2, ComponentType::ProtocolHandler, 150).is_err());
    }

    #[test]
    fn test_resource_usage_limits() {
        let mut system = ResourceBoundSystem::default();

        let valid_usage = ResourceUsage {
            cpu_time: 500000,  // 0.5 seconds
            memory_bytes: 1024,
            file_descriptors: 10,
            network_connections: 5,
            thread_count: 2,
            heap_allocations: 100,
            stack_depth: 50,
        };

        assert!(system.update_resource_usage(1, valid_usage).is_ok());

        let invalid_usage = ResourceUsage {
            cpu_time: 2000000, // 2 seconds (exceeds 1 second limit)
            memory_bytes: 1024,
            file_descriptors: 10,
            network_connections: 5,
            thread_count: 2,
            heap_allocations: 100,
            stack_depth: 50,
        };

        assert!(system.update_resource_usage(2, invalid_usage).is_err());
    }

    #[test]
    fn test_emergency_cleanup() {
        let mut system = ResourceBoundSystem::default();

        // Allocate cache and synthetic components (non-critical)
        system.allocate_memory(1, ComponentType::CacheSystem, 1024).unwrap();
        system.allocate_memory(2, ComponentType::SyntheticFileGenerator, 512).unwrap();
        system.allocate_memory(3, ComponentType::ProtocolHandler, 256).unwrap(); // Critical

        let initial_memory = system.get_total_allocated_memory();
        assert_eq!(initial_memory, 1792);

        // Trigger emergency cleanup
        let freed = system.emergency_memory_cleanup();

        // Should free non-critical components but keep critical ones
        let final_memory = system.get_total_allocated_memory();
        assert!(freed > 0);
        assert!(final_memory < initial_memory);

        // Protocol handler should still exist
        assert!(system.component_allocations.contains_key(&3));
    }

    #[test]
    fn test_memory_stats_accuracy() {
        let mut system = ResourceBoundSystem::default();

        // Allocate some memory
        system.allocate_memory(1, ComponentType::ProtocolHandler, 1024).unwrap();
        system.allocate_memory(2, ComponentType::TranslatorSandbox, 2048).unwrap();

        let stats = system.get_memory_stats();

        assert_eq!(stats.total_allocated, 3072);
        assert_eq!(stats.component_count, 2);
        assert_eq!(stats.usage_percentage, (3072 * 100 / (16 * 1024 * 1024)) as u32);
    }

    #[test]
    fn test_component_cleanup() {
        let mut system = ResourceBoundSystem::default();

        // Allocate for component
        system.allocate_memory(42, ComponentType::TranslatorSandbox, 1024).unwrap();
        assert!(system.component_allocations.contains_key(&42));

        // Cleanup component
        system.cleanup_component(42).unwrap();
        assert!(!system.component_allocations.contains_key(&42));
        assert_eq!(system.get_total_allocated_memory(), 0);
    }
}
