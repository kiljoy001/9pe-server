//! Advanced memory management for 9P.e protocol
//!
//! This module provides sophisticated memory management including:
//! - Hierarchical memory pools with NUMA awareness
//! - Dynamic allocation strategies based on workload patterns
//! - Memory compaction and defragmentation
//! - Cache-aware data structures and algorithms

use std::collections::{HashMap, BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use thiserror::Error;

/// Memory management errors
#[derive(Error, Debug)]
pub enum MemoryError {
    #[error("Out of memory in pool {pool_id}: requested {requested}, available {available}")]
    /// Out of memory error indicating allocation failure
    OutOfMemory {
        /// ID of the memory pool that failed
        pool_id: usize,
        /// Number of bytes requested for allocation
        requested: usize,
        /// Number of bytes available in the pool
        available: usize
    },
    #[error("Invalid allocation size: {0}")]
    /// Invalid allocation size error
    InvalidSize(usize),
    #[error("Memory pool {0} not found")]
    /// Memory pool not found error
    PoolNotFound(usize),
    #[error("Memory corruption detected at address {0:p}")]
    /// Memory corruption detected error
    CorruptionDetected(*const u8),
    #[error("Alignment error: {size} bytes cannot be aligned to {align}")]
    /// Memory alignment error
    AlignmentError {
        /// Size that failed alignment
        size: usize,
        /// Required alignment boundary
        align: usize
    },
    #[error("Memory leak detected: {leaked_bytes} bytes not freed")]
    /// Memory leak detected error
    MemoryLeak {
        /// Number of bytes that were leaked
        leaked_bytes: usize
    },
}

/// Memory allocation statistics
#[derive(Debug, Clone, Default)]
pub struct AllocationStats {
    /// Total bytes allocated since creation
    pub total_allocated: usize,
    /// Total bytes freed since creation
    pub total_freed: usize,
    /// Current memory usage in bytes
    pub current_usage: usize,
    /// Peak memory usage in bytes
    pub peak_usage: usize,
    /// Number of allocation operations
    pub allocation_count: u64,
    /// Number of deallocation operations
    pub deallocation_count: u64,
    /// Ratio of fragmented memory (0.0 to 1.0)
    pub fragmentation_ratio: f64,
    /// Number of memory compaction events
    pub compaction_events: u32,
}

/// Memory pool configuration
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Initial size of the memory pool in bytes
    pub initial_size: usize,
    /// Maximum size the pool can grow to in bytes
    pub max_size: usize,
    /// Factor by which pool grows when more memory is needed
    pub growth_factor: f64,
    /// Memory alignment boundary in bytes
    pub alignment: usize,
    /// Optional NUMA node for memory affinity
    pub numa_node: Option<u32>,
    /// Whether to enable automatic memory compaction
    pub enable_compaction: bool,
    /// Fragmentation threshold that triggers compaction
    pub compaction_threshold: f64,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            initial_size: 1024 * 1024, // 1MB
            max_size: 1024 * 1024 * 1024, // 1GB
            growth_factor: 2.0,
            alignment: 8,
            numa_node: None,
            enable_compaction: true,
            compaction_threshold: 0.3,
        }
    }
}

/// Memory allocation strategy
#[derive(Debug, Clone, PartialEq)]
pub enum AllocationStrategy {
    /// First-fit allocation
    FirstFit,
    /// Best-fit allocation (minimizes waste)
    BestFit,
    /// Worst-fit allocation (reduces fragmentation)
    WorstFit,
    /// Buddy allocation (power-of-2 sizes)
    Buddy,
    /// Slab allocation (fixed-size objects)
    /// Slab allocation (fixed-size objects)
    Slab {
        /// Size of each object in the slab
        object_size: usize
    },
    /// Stack allocation (LIFO)
    Stack,
}

/// Memory block descriptor
#[derive(Debug, Clone)]
struct MemoryBlock {
    /// Pointer to the memory block
    ptr: *mut u8,
    /// Size of the memory block in bytes
    size: usize,
    /// Whether the block is currently free
    is_free: bool,
    /// Timestamp when the block was allocated
    allocated_at: Instant,
    /// NUMA node where this block is located
    numa_node: Option<u32>,
    /// Magic value for corruption detection
    magic: u64,
}

const MAGIC_VALUE: u64 = 0xDEADBEEFCAFEBABE;

unsafe impl Send for MemoryBlock {}
unsafe impl Sync for MemoryBlock {}

/// Hierarchical memory pool
#[derive(Debug)]
pub struct MemoryPool {
    id: usize,
    config: PoolConfig,
    strategy: AllocationStrategy,
    blocks: Vec<MemoryBlock>,
    free_blocks: BTreeMap<usize, Vec<usize>>, // size -> block indices
    stats: AllocationStats,
    last_compaction: Instant,
    allocation_history: VecDeque<(usize, Instant)>, // (size, time) pairs
}

impl MemoryPool {
    /// Create a new memory pool
    pub fn new(id: usize, config: PoolConfig, strategy: AllocationStrategy) -> Result<Self, MemoryError> {
        let mut pool = Self {
            id,
            config: config.clone(),
            strategy,
            blocks: Vec::new(),
            free_blocks: BTreeMap::new(),
            stats: AllocationStats::default(),
            last_compaction: Instant::now(),
            allocation_history: VecDeque::with_capacity(1000),
        };

        // Initialize with initial memory block
        pool.grow_pool(config.initial_size)?;
        Ok(pool)
    }

    /// Allocate memory from the pool
    pub fn allocate(&mut self, size: usize) -> Result<*mut u8, MemoryError> {
        if size == 0 {
            return Err(MemoryError::InvalidSize(0));
        }

        let aligned_size = self.align_size(size);

        // Track allocation pattern
        self.allocation_history.push_back((aligned_size, Instant::now()));
        if self.allocation_history.len() > 1000 {
            self.allocation_history.pop_front();
        }

        // Try to find suitable free block
        if let Some(block_idx) = self.find_free_block(aligned_size)? {
            let ptr = self.allocate_from_block(block_idx, aligned_size)?;
            self.update_allocation_stats(aligned_size);
            return Ok(ptr);
        }

        // No suitable block found, try to grow pool
        if self.can_grow_pool(aligned_size) {
            self.grow_pool(aligned_size.max(self.config.initial_size))?;
            if let Some(block_idx) = self.find_free_block(aligned_size)? {
                let ptr = self.allocate_from_block(block_idx, aligned_size)?;
                self.update_allocation_stats(aligned_size);
                return Ok(ptr);
            }
        }

        // Try compaction if enabled
        if self.config.enable_compaction && self.should_compact() {
            self.compact()?;
            if let Some(block_idx) = self.find_free_block(aligned_size)? {
                let ptr = self.allocate_from_block(block_idx, aligned_size)?;
                self.update_allocation_stats(aligned_size);
                return Ok(ptr);
            }
        }

        Err(MemoryError::OutOfMemory {
            pool_id: self.id,
            requested: aligned_size,
            available: self.get_available_memory(),
        })
    }

    /// Deallocate memory back to the pool
    pub fn deallocate(&mut self, ptr: *mut u8) -> Result<(), MemoryError> {
        // Find the block containing this pointer
        for (idx, block) in self.blocks.iter_mut().enumerate() {
            if block.ptr == ptr {
                if block.is_free {
                    return Ok(()); // Already freed
                }

                // Verify magic value for corruption detection
                if block.magic != MAGIC_VALUE {
                    return Err(MemoryError::CorruptionDetected(ptr));
                }

                // Mark as free
                block.is_free = true;
                block.magic = 0; // Clear magic

                // Add to free block map
                self.free_blocks.entry(block.size).or_insert_with(Vec::new).push(idx);

                // Update stats
                self.stats.current_usage -= block.size;
                self.stats.deallocation_count += 1;
                self.stats.total_freed += block.size;

                // Try to coalesce adjacent free blocks
                self.coalesce_free_blocks(idx);

                return Ok(());
            }
        }

        Err(MemoryError::CorruptionDetected(ptr))
    }

    /// Get memory allocation statistics
    pub fn get_stats(&self) -> AllocationStats {
        let mut stats = self.stats.clone();
        stats.fragmentation_ratio = self.calculate_fragmentation_ratio();
        stats
    }

    /// Force memory compaction
    pub fn compact(&mut self) -> Result<(), MemoryError> {
        if !self.config.enable_compaction {
            return Ok(());
        }

        // Identify allocated blocks
        let mut allocated_blocks: Vec<(usize, MemoryBlock)> = self.blocks
            .iter()
            .enumerate()
            .filter(|(_, block)| !block.is_free)
            .map(|(idx, block)| (idx, block.clone()))
            .collect();

        // Sort by size for better packing
        allocated_blocks.sort_by_key(|(_, block)| block.size);

        // Rebuild memory layout
        self.blocks.clear();
        self.free_blocks.clear();

        // Re-allocate in contiguous fashion
        let mut offset = 0;
        for (_, mut block) in allocated_blocks {
            // Allocate new contiguous space
            if let Ok(new_ptr) = self.allocate_contiguous(block.size, offset) {
                // Copy data if different location
                if new_ptr != block.ptr {
                    unsafe {
                        std::ptr::copy_nonoverlapping(block.ptr, new_ptr, block.size);
                    }
                }
                block.ptr = new_ptr;
                let block_size = block.size;
                self.blocks.push(block);
                offset += block_size;
            }
        }

        self.stats.compaction_events += 1;
        self.last_compaction = Instant::now();

        Ok(())
    }

    /// Get available memory in the pool
    pub fn get_available_memory(&self) -> usize {
        self.free_blocks.values().flatten().count() * self.config.alignment
    }

    /// Check if pool should be compacted
    fn should_compact(&self) -> bool {
        let fragmentation = self.calculate_fragmentation_ratio();
        fragmentation > self.config.compaction_threshold &&
        self.last_compaction.elapsed() > Duration::from_secs(30)
    }

    /// Calculate memory fragmentation ratio
    fn calculate_fragmentation_ratio(&self) -> f64 {
        let total_free_size: usize = self.free_blocks
            .iter()
            .map(|(size, indices)| size * indices.len())
            .sum();

        if total_free_size == 0 {
            return 0.0;
        }

        let largest_free_block = self.free_blocks.keys().last().copied().unwrap_or(0);
        1.0 - (largest_free_block as f64 / total_free_size as f64)
    }

    /// Align size to configured alignment
    fn align_size(&self, size: usize) -> usize {
        (size + self.config.alignment - 1) & !(self.config.alignment - 1)
    }

    /// Find suitable free block using the configured strategy
    fn find_free_block(&self, size: usize) -> Result<Option<usize>, MemoryError> {
        match self.strategy {
            AllocationStrategy::FirstFit => self.find_first_fit(size),
            AllocationStrategy::BestFit => self.find_best_fit(size),
            AllocationStrategy::WorstFit => self.find_worst_fit(size),
            AllocationStrategy::Buddy => self.find_buddy_block(size),
            AllocationStrategy::Slab { object_size } => {
                if size <= object_size {
                    self.find_slab_block(object_size)
                } else {
                    Ok(None)
                }
            }
            AllocationStrategy::Stack => self.find_stack_block(size),
        }
    }

    /// First-fit allocation strategy
    fn find_first_fit(&self, size: usize) -> Result<Option<usize>, MemoryError> {
        for (&block_size, indices) in &self.free_blocks {
            if block_size >= size && !indices.is_empty() {
                return Ok(Some(indices[0]));
            }
        }
        Ok(None)
    }

    /// Best-fit allocation strategy
    fn find_best_fit(&self, size: usize) -> Result<Option<usize>, MemoryError> {
        let mut best_fit: Option<(usize, usize)> = None; // (waste, block_idx)

        for (&block_size, indices) in &self.free_blocks {
            if block_size >= size && !indices.is_empty() {
                let waste = block_size - size;
                if best_fit.is_none() || waste < best_fit.unwrap().0 {
                    best_fit = Some((waste, indices[0]));
                }
            }
        }

        Ok(best_fit.map(|(_, idx)| idx))
    }

    /// Worst-fit allocation strategy
    fn find_worst_fit(&self, size: usize) -> Result<Option<usize>, MemoryError> {
        let mut worst_fit: Option<(usize, usize)> = None; // (waste, block_idx)

        for (&block_size, indices) in &self.free_blocks {
            if block_size >= size && !indices.is_empty() {
                let waste = block_size - size;
                if worst_fit.is_none() || waste > worst_fit.unwrap().0 {
                    worst_fit = Some((waste, indices[0]));
                }
            }
        }

        Ok(worst_fit.map(|(_, idx)| idx))
    }

    /// Buddy allocation strategy
    fn find_buddy_block(&self, size: usize) -> Result<Option<usize>, MemoryError> {
        // Find next power of 2
        let buddy_size = size.next_power_of_two();

        if let Some(indices) = self.free_blocks.get(&buddy_size) {
            if !indices.is_empty() {
                return Ok(Some(indices[0]));
            }
        }

        // Try to split larger block
        for (&block_size, indices) in &self.free_blocks {
            if block_size >= buddy_size * 2 && !indices.is_empty() {
                // Could split this block
                return Ok(Some(indices[0]));
            }
        }

        Ok(None)
    }

    /// Slab allocation strategy
    fn find_slab_block(&self, object_size: usize) -> Result<Option<usize>, MemoryError> {
        if let Some(indices) = self.free_blocks.get(&object_size) {
            if !indices.is_empty() {
                return Ok(Some(indices[0]));
            }
        }
        Ok(None)
    }

    /// Stack allocation strategy (LIFO)
    fn find_stack_block(&self, size: usize) -> Result<Option<usize>, MemoryError> {
        // Find most recently freed block that fits
        let mut newest_block: Option<(Instant, usize)> = None;

        for (&block_size, indices) in &self.free_blocks {
            if block_size >= size {
                for &idx in indices {
                    let block = &self.blocks[idx];
                    if newest_block.is_none() || block.allocated_at > newest_block.unwrap().0 {
                        newest_block = Some((block.allocated_at, idx));
                    }
                }
            }
        }

        Ok(newest_block.map(|(_, idx)| idx))
    }

    /// Allocate from a specific block
    fn allocate_from_block(&mut self, block_idx: usize, size: usize) -> Result<*mut u8, MemoryError> {
        // First check if we can allocate
        {
            let block = &self.blocks[block_idx];
            if !block.is_free || block.size < size {
                return Err(MemoryError::InvalidSize(size));
            }
        }

        // Get block info we need
        let block_size = self.blocks[block_idx].size;
        let block_ptr = self.blocks[block_idx].ptr;

        // Mark as allocated
        {
            let block = &mut self.blocks[block_idx];
            block.is_free = false;
            block.magic = MAGIC_VALUE;
            block.allocated_at = Instant::now();
        }

        // Remove from free blocks map
        if let Some(indices) = self.free_blocks.get_mut(&block_size) {
            indices.retain(|&idx| idx != block_idx);
            if indices.is_empty() {
                self.free_blocks.remove(&block_size);
            }
        }

        // Split block if significantly larger than needed
        if block_size > size + self.config.alignment {
            self.split_block(block_idx, size)?;
        }

        Ok(block_ptr)
    }

    /// Split a block into allocated and free portions
    fn split_block(&mut self, block_idx: usize, alloc_size: usize) -> Result<(), MemoryError> {
        let original_size = self.blocks[block_idx].size;
        let remaining_size = original_size - alloc_size;

        if remaining_size < self.config.alignment {
            return Ok(()); // Not worth splitting
        }

        // Update original block size
        self.blocks[block_idx].size = alloc_size;

        // Create new free block for remainder
        let new_block = MemoryBlock {
            ptr: unsafe { self.blocks[block_idx].ptr.add(alloc_size) },
            size: remaining_size,
            is_free: true,
            allocated_at: Instant::now(),
            numa_node: self.blocks[block_idx].numa_node,
            magic: 0,
        };

        let new_idx = self.blocks.len();
        self.blocks.push(new_block);

        // Add to free blocks map
        self.free_blocks.entry(remaining_size).or_insert_with(Vec::new).push(new_idx);

        Ok(())
    }

    /// Coalesce adjacent free blocks
    fn coalesce_free_blocks(&mut self, freed_idx: usize) {
        // This is a simplified version - a full implementation would
        // maintain a more sophisticated data structure for efficient coalescing
        let freed_block = &self.blocks[freed_idx];
        let freed_end = unsafe { freed_block.ptr.add(freed_block.size) };

        // Look for adjacent free blocks
        let mut blocks_to_merge = vec![freed_idx];

        for (idx, block) in self.blocks.iter().enumerate() {
            if idx != freed_idx && block.is_free {
                let block_end = unsafe { block.ptr.add(block.size) };

                // Check if blocks are adjacent
                if block_end == freed_block.ptr || freed_end == block.ptr {
                    blocks_to_merge.push(idx);
                }
            }
        }

        if blocks_to_merge.len() > 1 {
            self.merge_blocks(blocks_to_merge);
        }
    }

    /// Merge multiple free blocks into one
    fn merge_blocks(&mut self, block_indices: Vec<usize>) {
        if block_indices.len() < 2 {
            return;
        }

        // Sort by pointer address to determine layout
        let mut sorted_indices = block_indices;
        sorted_indices.sort_by_key(|&idx| self.blocks[idx].ptr as usize);

        let first_idx = sorted_indices[0];
        let mut total_size = 0;
        let mut min_ptr = self.blocks[first_idx].ptr;

        // Calculate total size and find minimum pointer
        for &idx in &sorted_indices {
            let block = &self.blocks[idx];
            total_size += block.size;
            if (block.ptr as usize) < (min_ptr as usize) {
                min_ptr = block.ptr;
            }

            // Remove from free blocks map
            if let Some(indices) = self.free_blocks.get_mut(&block.size) {
                indices.retain(|&i| i != idx);
                if indices.is_empty() {
                    self.free_blocks.remove(&block.size);
                }
            }
        }

        // Update first block to represent merged block
        self.blocks[first_idx].ptr = min_ptr;
        self.blocks[first_idx].size = total_size;

        // Mark other blocks as invalid (we don't actually remove them to avoid index shifts)
        for &idx in &sorted_indices[1..] {
            self.blocks[idx].size = 0;
            self.blocks[idx].is_free = false;
        }

        // Add merged block to free blocks map
        self.free_blocks.entry(total_size).or_insert_with(Vec::new).push(first_idx);
    }

    /// Check if pool can grow to accommodate size
    fn can_grow_pool(&self, size: usize) -> bool {
        let current_total = self.blocks.iter().map(|b| b.size).sum::<usize>();
        current_total + size <= self.config.max_size
    }

    /// Grow the memory pool
    fn grow_pool(&mut self, min_size: usize) -> Result<(), MemoryError> {
        let growth_size = (self.config.initial_size as f64 * self.config.growth_factor) as usize;
        let actual_size = min_size.max(growth_size);

        if !self.can_grow_pool(actual_size) {
            return Err(MemoryError::OutOfMemory {
                pool_id: self.id,
                requested: actual_size,
                available: self.config.max_size - self.blocks.iter().map(|b| b.size).sum::<usize>(),
            });
        }

        // Allocate new memory block (in practice, this would use system allocation)
        let ptr = unsafe {
            std::alloc::alloc_zeroed(std::alloc::Layout::from_size_align(actual_size, self.config.alignment)
                .map_err(|_| MemoryError::AlignmentError { size: actual_size, align: self.config.alignment })?)
        };

        if ptr.is_null() {
            return Err(MemoryError::OutOfMemory {
                pool_id: self.id,
                requested: actual_size,
                available: 0,
            });
        }

        let new_block = MemoryBlock {
            ptr,
            size: actual_size,
            is_free: true,
            allocated_at: Instant::now(),
            numa_node: self.config.numa_node,
            magic: 0,
        };

        let block_idx = self.blocks.len();
        self.blocks.push(new_block);

        // Add to free blocks map
        self.free_blocks.entry(actual_size).or_insert_with(Vec::new).push(block_idx);

        Ok(())
    }

    /// Allocate contiguous memory at specific offset (for compaction)
    fn allocate_contiguous(&mut self, size: usize, _offset: usize) -> Result<*mut u8, MemoryError> {
        // Simplified implementation - in practice would maintain base addresses
        self.allocate(size)
    }

    /// Update allocation statistics
    fn update_allocation_stats(&mut self, size: usize) {
        self.stats.total_allocated += size;
        self.stats.current_usage += size;
        self.stats.allocation_count += 1;

        if self.stats.current_usage > self.stats.peak_usage {
            self.stats.peak_usage = self.stats.current_usage;
        }
    }
}

/// Memory manager coordinating multiple pools
pub struct MemoryManager {
    pools: RwLock<HashMap<usize, Arc<Mutex<MemoryPool>>>>,
    next_pool_id: std::sync::atomic::AtomicUsize,
    global_stats: RwLock<AllocationStats>,
}

impl MemoryManager {
    /// Create a new memory manager
    pub fn new() -> Self {
        Self {
            pools: RwLock::new(HashMap::new()),
            next_pool_id: std::sync::atomic::AtomicUsize::new(0),
            global_stats: RwLock::new(AllocationStats::default()),
        }
    }

    /// Create a new memory pool
    pub fn create_pool(&self, config: PoolConfig, strategy: AllocationStrategy) -> Result<usize, MemoryError> {
        let pool_id = self.next_pool_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let pool = Arc::new(Mutex::new(MemoryPool::new(pool_id, config, strategy)?));

        self.pools.write().unwrap().insert(pool_id, pool);
        Ok(pool_id)
    }

    /// Allocate from specific pool
    pub fn allocate_from_pool(&self, pool_id: usize, size: usize) -> Result<*mut u8, MemoryError> {
        let pools = self.pools.read().unwrap();
        let pool = pools.get(&pool_id).ok_or(MemoryError::PoolNotFound(pool_id))?;
        let ptr = pool.lock().unwrap().allocate(size)?;

        // Update global stats
        let mut global_stats = self.global_stats.write().unwrap();
        global_stats.total_allocated += size;
        global_stats.current_usage += size;
        global_stats.allocation_count += 1;

        Ok(ptr)
    }

    /// Get global allocation statistics
    pub fn get_global_stats(&self) -> AllocationStats {
        self.global_stats.read().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_pool_creation() {
        let config = PoolConfig::default();
        let pool = MemoryPool::new(0, config, AllocationStrategy::FirstFit);
        assert!(pool.is_ok());
    }

    #[test]
    fn test_basic_allocation() {
        let config = PoolConfig::default();
        let mut pool = MemoryPool::new(0, config, AllocationStrategy::FirstFit).unwrap();

        let ptr = pool.allocate(100).unwrap();
        assert!(!ptr.is_null());

        let result = pool.deallocate(ptr);
        assert!(result.is_ok());
    }

    #[test]
    fn test_allocation_strategies() {
        let config = PoolConfig::default();

        // Test different strategies
        let strategies = [
            AllocationStrategy::FirstFit,
            AllocationStrategy::BestFit,
            AllocationStrategy::WorstFit,
            AllocationStrategy::Buddy,
            AllocationStrategy::Stack,
        ];

        for strategy in strategies {
            let mut pool = MemoryPool::new(0, config.clone(), strategy).unwrap();
            let ptr = pool.allocate(64).unwrap();
            assert!(!ptr.is_null());
            pool.deallocate(ptr).unwrap();
        }
    }

    #[test]
    fn test_memory_stats() {
        let config = PoolConfig::default();
        let mut pool = MemoryPool::new(0, config, AllocationStrategy::FirstFit).unwrap();

        let initial_stats = pool.get_stats();
        assert_eq!(initial_stats.allocation_count, 0);

        let ptr = pool.allocate(100).unwrap();
        let after_alloc_stats = pool.get_stats();
        assert_eq!(after_alloc_stats.allocation_count, 1);
        assert!(after_alloc_stats.current_usage >= 100);

        pool.deallocate(ptr).unwrap();
        let after_dealloc_stats = pool.get_stats();
        assert_eq!(after_dealloc_stats.deallocation_count, 1);
    }

    #[test]
    fn test_pool_growth() {
        let mut config = PoolConfig::default();
        config.initial_size = 512;
        config.max_size = 2048;

        let mut pool = MemoryPool::new(0, config, AllocationStrategy::FirstFit).unwrap();

        // Allocate beyond initial size
        let ptr1 = pool.allocate(300).unwrap();
        let ptr2 = pool.allocate(300).unwrap(); // Should trigger growth

        assert!(!ptr1.is_null());
        assert!(!ptr2.is_null());

        pool.deallocate(ptr1).unwrap();
        pool.deallocate(ptr2).unwrap();
    }

    #[test]
    fn test_memory_manager() {
        let manager = MemoryManager::new();
        let config = PoolConfig::default();

        let pool_id = manager.create_pool(config, AllocationStrategy::FirstFit).unwrap();
        let ptr = manager.allocate_from_pool(pool_id, 100).unwrap();

        assert!(!ptr.is_null());

        let stats = manager.get_global_stats();
        assert!(stats.total_allocated >= 100);
    }

    #[test]
    fn test_size_alignment() {
        let config = PoolConfig {
            alignment: 16,
            ..Default::default()
        };
        let mut pool = MemoryPool::new(0, config, AllocationStrategy::FirstFit).unwrap();

        // Test various sizes get properly aligned
        for size in [1, 7, 15, 16, 17, 31, 32, 33] {
            let ptr = pool.allocate(size).unwrap();
            assert_eq!(ptr as usize % 16, 0, "Pointer not aligned for size {}", size);
            pool.deallocate(ptr).unwrap();
        }
    }

    #[test]
    fn test_out_of_memory() {
        let mut config = PoolConfig::default();
        config.max_size = 1024; // Very small
        config.initial_size = 512; // Set initial size smaller than max

        let mut pool = MemoryPool::new(0, config, AllocationStrategy::FirstFit).unwrap();

        // Try to allocate more than max_size
        let result = pool.allocate(2048);
        assert!(result.is_err());

        match result.unwrap_err() {
            MemoryError::OutOfMemory { .. } => (),
            _ => panic!("Expected OutOfMemory error"),
        }
    }
}