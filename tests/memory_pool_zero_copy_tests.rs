//! Tests for Memory Pool Zero-Copy System (#3)
//! High-performance memory management for QUIC integration

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, AtomicU64, AtomicBool, Ordering};
use std::collections::VecDeque;
use std::ptr;
use std::alloc::{alloc, dealloc, Layout};

/// A zero-copy buffer that can be shared between components
#[derive(Debug)]
pub struct ZeroCopyBuffer {
    data: *mut u8,
    len: usize,
    capacity: usize,
    refcount: Arc<AtomicUsize>,
    pool: Option<Arc<MemoryPool>>,
}

impl ZeroCopyBuffer {
    /// Create a new buffer from raw parts
    pub unsafe fn from_raw_parts(data: *mut u8, len: usize, capacity: usize) -> Self {
        Self {
            data,
            len,
            capacity,
            refcount: Arc::new(AtomicUsize::new(1)),
            pool: None,
        }
    }

    /// Get a slice view of the buffer
    pub fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.data, self.len) }
    }

    /// Get a mutable slice view (only if refcount is 1)
    pub fn as_mut_slice(&mut self) -> Option<&mut [u8]> {
        if self.refcount.load(Ordering::Acquire) == 1 {
            Some(unsafe { std::slice::from_raw_parts_mut(self.data, self.len) })
        } else {
            None
        }
    }

    /// Clone the buffer (increments refcount, doesn't copy data)
    pub fn clone_ref(&self) -> Self {
        self.refcount.fetch_add(1, Ordering::AcqRel);
        Self {
            data: self.data,
            len: self.len,
            capacity: self.capacity,
            refcount: Arc::clone(&self.refcount),
            pool: self.pool.clone(),
        }
    }

    /// Split the buffer at the given index
    pub fn split_at(&self, index: usize) -> Option<(Self, Self)> {
        if index > self.len {
            return None;
        }

        let left = Self {
            data: self.data,
            len: index,
            capacity: index,
            refcount: Arc::clone(&self.refcount),
            pool: self.pool.clone(),
        };

        let right = Self {
            data: unsafe { self.data.add(index) },
            len: self.len - index,
            capacity: self.capacity - index,
            refcount: Arc::clone(&self.refcount),
            pool: self.pool.clone(),
        };

        self.refcount.fetch_add(1, Ordering::AcqRel); // One extra ref for the split
        Some((left, right))
    }
}

impl Drop for ZeroCopyBuffer {
    fn drop(&mut self) {
        let count = self.refcount.fetch_sub(1, Ordering::AcqRel);
        if count == 1 {
            // Last reference, return to pool or deallocate
            if let Some(pool) = &self.pool {
                pool.return_buffer(self.data, self.capacity);
            } else {
                unsafe {
                    let layout = Layout::from_size_align(self.capacity, 1).unwrap();
                    dealloc(self.data, layout);
                }
            }
        }
    }
}

// Safety: ZeroCopyBuffer can be sent between threads
unsafe impl Send for ZeroCopyBuffer {}
unsafe impl Sync for ZeroCopyBuffer {}

/// Memory pool for efficient buffer allocation
pub struct MemoryPool {
    // Pools for different size classes
    small_pool: Mutex<VecDeque<(*mut u8, usize)>>,  // 4KB buffers
    medium_pool: Mutex<VecDeque<(*mut u8, usize)>>, // 64KB buffers
    large_pool: Mutex<VecDeque<(*mut u8, usize)>>,  // 1MB buffers

    // Statistics
    allocations: AtomicU64,
    deallocations: AtomicU64,
    bytes_allocated: AtomicU64,
    bytes_in_use: AtomicU64,
    pool_hits: AtomicU64,
    pool_misses: AtomicU64,
}

impl std::fmt::Debug for MemoryPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryPool")
            .field("allocations", &self.allocations.load(Ordering::Relaxed))
            .field("deallocations", &self.deallocations.load(Ordering::Relaxed))
            .field("bytes_allocated", &self.bytes_allocated.load(Ordering::Relaxed))
            .field("bytes_in_use", &self.bytes_in_use.load(Ordering::Relaxed))
            .field("pool_hits", &self.pool_hits.load(Ordering::Relaxed))
            .field("pool_misses", &self.pool_misses.load(Ordering::Relaxed))
            .finish()
    }
}

// Safety: raw pointers are only accessed while the mutex protecting each pool is held.
unsafe impl Send for MemoryPool {}
unsafe impl Sync for MemoryPool {}

impl MemoryPool {
    const SMALL_SIZE: usize = 4 * 1024;      // 4KB
    const MEDIUM_SIZE: usize = 64 * 1024;    // 64KB
    const LARGE_SIZE: usize = 1024 * 1024;   // 1MB

    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            small_pool: Mutex::new(VecDeque::new()),
            medium_pool: Mutex::new(VecDeque::new()),
            large_pool: Mutex::new(VecDeque::new()),
            allocations: AtomicU64::new(0),
            deallocations: AtomicU64::new(0),
            bytes_allocated: AtomicU64::new(0),
            bytes_in_use: AtomicU64::new(0),
            pool_hits: AtomicU64::new(0),
            pool_misses: AtomicU64::new(0),
        })
    }

    /// Allocate a buffer of at least the given size
    pub fn allocate(self: &Arc<Self>, size: usize) -> ZeroCopyBuffer {
        let (ptr, capacity) = self.get_or_allocate_buffer(size);

        self.allocations.fetch_add(1, Ordering::Relaxed);
        self.bytes_in_use.fetch_add(capacity as u64, Ordering::Relaxed);

        let mut buffer = unsafe { ZeroCopyBuffer::from_raw_parts(ptr, size, capacity) };
        buffer.pool = Some(Arc::clone(self));
        buffer
    }

    fn get_or_allocate_buffer(&self, size: usize) -> (*mut u8, usize) {
        // Determine size class
        let (pool, alloc_size) = if size <= Self::SMALL_SIZE {
            (&self.small_pool, Self::SMALL_SIZE)
        } else if size <= Self::MEDIUM_SIZE {
            (&self.medium_pool, Self::MEDIUM_SIZE)
        } else if size <= Self::LARGE_SIZE {
            (&self.large_pool, Self::LARGE_SIZE)
        } else {
            // Too large for pools, allocate directly
            let layout = Layout::from_size_align(size, 1).unwrap();
            let ptr = unsafe { alloc(layout) };
            self.pool_misses.fetch_add(1, Ordering::Relaxed);
            self.bytes_allocated.fetch_add(size as u64, Ordering::Relaxed);
            return (ptr, size);
        };

        // Try to get from pool
        if let Some((ptr, cap)) = pool.lock().unwrap().pop_front() {
            self.pool_hits.fetch_add(1, Ordering::Relaxed);
            (ptr, cap)
        } else {
            // Allocate new
            let layout = Layout::from_size_align(alloc_size, 1).unwrap();
            let ptr = unsafe { alloc(layout) };
            self.pool_misses.fetch_add(1, Ordering::Relaxed);
            self.bytes_allocated.fetch_add(alloc_size as u64, Ordering::Relaxed);
            (ptr, alloc_size)
        }
    }

    fn return_buffer(&self, ptr: *mut u8, capacity: usize) {
        self.bytes_in_use.fetch_sub(capacity as u64, Ordering::Relaxed);
        self.deallocations.fetch_add(1, Ordering::Relaxed);

        // Determine which pool to return to
        let pool = if capacity == Self::SMALL_SIZE {
            &self.small_pool
        } else if capacity == Self::MEDIUM_SIZE {
            &self.medium_pool
        } else if capacity == Self::LARGE_SIZE {
            &self.large_pool
        } else {
            // Not a pooled size, deallocate directly
            unsafe {
                let layout = Layout::from_size_align(capacity, 1).unwrap();
                dealloc(ptr, layout);
            }
            self.bytes_allocated.fetch_sub(capacity as u64, Ordering::Relaxed);
            return;
        };

        // Return to pool (with a limit to prevent unbounded growth)
        let mut pool_lock = pool.lock().unwrap();
        if pool_lock.len() < 32 {
            pool_lock.push_back((ptr, capacity));
        } else {
            // Pool is full, deallocate
            unsafe {
                let layout = Layout::from_size_align(capacity, 1).unwrap();
                dealloc(ptr, layout);
            }
            self.bytes_allocated.fetch_sub(capacity as u64, Ordering::Relaxed);
        }
    }

    /// Get pool statistics
    pub fn get_stats(&self) -> PoolStats {
        PoolStats {
            allocations: self.allocations.load(Ordering::Relaxed),
            deallocations: self.deallocations.load(Ordering::Relaxed),
            bytes_allocated: self.bytes_allocated.load(Ordering::Relaxed),
            bytes_in_use: self.bytes_in_use.load(Ordering::Relaxed),
            pool_hits: self.pool_hits.load(Ordering::Relaxed),
            pool_misses: self.pool_misses.load(Ordering::Relaxed),
            small_pool_size: self.small_pool.lock().unwrap().len(),
            medium_pool_size: self.medium_pool.lock().unwrap().len(),
            large_pool_size: self.large_pool.lock().unwrap().len(),
        }
    }

    /// Pre-warm the pools with buffers
    pub fn prewarm(&self, small: usize, medium: usize, large: usize) {
        let mut small_pool = self.small_pool.lock().unwrap();
        for _ in 0..small {
            let layout = Layout::from_size_align(Self::SMALL_SIZE, 1).unwrap();
            let ptr = unsafe { alloc(layout) };
            small_pool.push_back((ptr, Self::SMALL_SIZE));
            self.bytes_allocated.fetch_add(Self::SMALL_SIZE as u64, Ordering::Relaxed);
        }

        let mut medium_pool = self.medium_pool.lock().unwrap();
        for _ in 0..medium {
            let layout = Layout::from_size_align(Self::MEDIUM_SIZE, 1).unwrap();
            let ptr = unsafe { alloc(layout) };
            medium_pool.push_back((ptr, Self::MEDIUM_SIZE));
            self.bytes_allocated.fetch_add(Self::MEDIUM_SIZE as u64, Ordering::Relaxed);
        }

        let mut large_pool = self.large_pool.lock().unwrap();
        for _ in 0..large {
            let layout = Layout::from_size_align(Self::LARGE_SIZE, 1).unwrap();
            let ptr = unsafe { alloc(layout) };
            large_pool.push_back((ptr, Self::LARGE_SIZE));
            self.bytes_allocated.fetch_add(Self::LARGE_SIZE as u64, Ordering::Relaxed);
        }
    }
}

#[derive(Debug)]
pub struct PoolStats {
    pub allocations: u64,
    pub deallocations: u64,
    pub bytes_allocated: u64,
    pub bytes_in_use: u64,
    pub pool_hits: u64,
    pub pool_misses: u64,
    pub small_pool_size: usize,
    pub medium_pool_size: usize,
    pub large_pool_size: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_zero_copy_buffer_basic() {
        let pool = MemoryPool::new();
        let mut buffer = pool.allocate(1024);

        // Write to buffer
        if let Some(slice) = buffer.as_mut_slice() {
            slice[0] = 42;
            slice[1] = 43;
        }

        // Read from buffer
        let slice = buffer.as_slice();
        assert_eq!(slice[0], 42);
        assert_eq!(slice[1], 43);
    }

    #[test]
    fn test_buffer_refcounting() {
        let pool = MemoryPool::new();
        let buffer1 = pool.allocate(1024);
        let buffer2 = buffer1.clone_ref();

        assert_eq!(buffer1.refcount.load(Ordering::Relaxed), 2);
        assert_eq!(buffer2.refcount.load(Ordering::Relaxed), 2);

        drop(buffer1);
        assert_eq!(buffer2.refcount.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_buffer_splitting() {
        let pool = MemoryPool::new();
        let mut buffer = pool.allocate(1024);

        // Fill buffer with test data
        if let Some(slice) = buffer.as_mut_slice() {
            for (i, byte) in slice.iter_mut().enumerate() {
                *byte = (i % 256) as u8;
            }
        }

        // Split the buffer
        if let Some((left, right)) = buffer.split_at(512) {
            assert_eq!(left.len, 512);
            assert_eq!(right.len, 512);

            // Verify data integrity
            assert_eq!(left.as_slice()[0], 0);
            assert_eq!(right.as_slice()[0], 0); // 512 % 256 = 0
        }
    }

    #[test]
    fn test_pool_size_classes() {
        let pool = MemoryPool::new();

        // Small allocation
        let small = pool.allocate(1024);
        assert_eq!(small.capacity, MemoryPool::SMALL_SIZE);

        // Medium allocation
        let medium = pool.allocate(32 * 1024);
        assert_eq!(medium.capacity, MemoryPool::MEDIUM_SIZE);

        // Large allocation
        let large = pool.allocate(512 * 1024);
        assert_eq!(large.capacity, MemoryPool::LARGE_SIZE);
    }

    #[test]
    fn test_pool_reuse() {
        let pool = MemoryPool::new();

        // Allocate and deallocate
        let buffer1 = pool.allocate(1024);
        drop(buffer1);

        let stats = pool.get_stats();
        assert_eq!(stats.small_pool_size, 1);

        // Next allocation should reuse
        let _buffer2 = pool.allocate(1024);
        let stats = pool.get_stats();
        assert_eq!(stats.pool_hits, 1);
    }

    #[test]
    fn test_pool_prewarm() {
        let pool = MemoryPool::new();
        pool.prewarm(5, 3, 2);

        let stats = pool.get_stats();
        assert_eq!(stats.small_pool_size, 5);
        assert_eq!(stats.medium_pool_size, 3);
        assert_eq!(stats.large_pool_size, 2);
    }

    #[test]
    fn test_concurrent_allocation() {
        let pool = MemoryPool::new();
        let mut handles = vec![];

        for i in 0..10 {
            let pool_clone = Arc::clone(&pool);
            let handle = thread::spawn(move || {
                let size = 1024 * (1 + i % 5);
                let mut buffers: Vec<ZeroCopyBuffer> = vec![];

                for _ in 0..10 {
                    buffers.push(pool_clone.allocate(size));
                }

                // Return half of them
                buffers.truncate(5);
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let stats = pool.get_stats();
        assert_eq!(stats.allocations, 100);
        assert!(stats.deallocations > 0);
    }

    #[test]
    fn test_memory_efficiency() {
        let pool = MemoryPool::new();

        // Allocate many small buffers
        let mut buffers = vec![];
        for _ in 0..100 {
            buffers.push(pool.allocate(100));
        }

        let stats = pool.get_stats();
        let efficiency = stats.bytes_in_use as f64 / stats.bytes_allocated as f64;

        // Even small allocations should use pooled buffers efficiently
        assert!(efficiency > 0.0);
        println!("Memory efficiency: {:.2}%", efficiency * 100.0);
    }

    #[test]
    fn test_fragmentation_resistance() {
        let pool = MemoryPool::new();

        // Allocate and free in a pattern that could cause fragmentation
        for _ in 0..10 {
            let mut buffers = vec![];

            // Allocate various sizes
            buffers.push(pool.allocate(1024));
            buffers.push(pool.allocate(8192));
            buffers.push(pool.allocate(512));
            buffers.push(pool.allocate(32768));

            // Free in different order
            buffers.remove(1);
            buffers.remove(0);
        }

        let stats = pool.get_stats();

        // Pool should prevent fragmentation by reusing buffers
        assert!(stats.pool_hits > 0);
        assert!(stats.small_pool_size > 0 || stats.medium_pool_size > 0);
    }
}
