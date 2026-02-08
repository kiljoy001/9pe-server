use crate::memory::{MemoryManager, PoolConfig, AllocationStrategy};
use anyhow::{Result, anyhow};
use std::sync::{Arc, Mutex, RwLock};
use std::collections::HashMap;
use std::path::PathBuf;
use std::ops::{Deref, DerefMut};

/// State of a shared memory borrow
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorrowState {
    /// Memory is available for any borrow
    Available,
    /// Memory is exclusively borrowed for writing
    Exclusive,
    /// Memory is borrowed for reading by N participants
    Shared(usize),
}

/// Information about a shared memory region
#[derive(Debug)]
pub struct MemoryRegion {
    pub ptr: *mut u8,
    pub size: usize,
    pub pool_id: usize,
    pub state: BorrowState,
}

unsafe impl Send for MemoryRegion {}
unsafe impl Sync for MemoryRegion {}

/// Manager for shared memory with borrow-checker semantics
pub struct SharedMemoryManager {
    memory_manager: Arc<MemoryManager>,
    default_pool: usize,
    regions: RwLock<HashMap<String, Arc<Mutex<MemoryRegion>>>>,
}

impl SharedMemoryManager {
    pub fn new(memory_manager: Arc<MemoryManager>) -> Result<Self> {
        let pool_id = memory_manager.create_pool(PoolConfig::default(), AllocationStrategy::FirstFit)
            .map_err(|e| anyhow!("Failed to create memory pool for SHM: {}", e))?;
        
        Ok(Self {
            memory_manager,
            default_pool: pool_id,
            regions: RwLock::new(HashMap::new()),
        })
    }

    /// Allocate a new shared memory region
    pub fn allocate(&self, id: String, size: usize) -> Result<()> {
        let ptr = self.memory_manager.allocate_from_pool(self.default_pool, size)
            .map_err(|e| anyhow!("SHM allocation failed: {}", e))?;
        
        let region = MemoryRegion {
            ptr,
            size,
            pool_id: self.default_pool,
            state: BorrowState::Available,
        };

        let mut regions = self.regions.write().unwrap();
        if regions.contains_key(&id) {
            return Err(anyhow!("Region already exists: {}", id));
        }
        
        regions.insert(id, Arc::new(Mutex::new(region)));
        Ok(())
    }

    /// Request a read-only borrow
    pub fn borrow_read(&self, id: &str) -> Result<SharedMemoryHandle> {
        let regions = self.regions.read().unwrap();
        let region_arc = regions.get(id).ok_or_else(|| anyhow!("Region not found: {}", id))?;
        let mut region = region_arc.lock().unwrap();

        match region.state {
            BorrowState::Available => {
                region.state = BorrowState::Shared(1);
            }
            BorrowState::Shared(count) => {
                region.state = BorrowState::Shared(count + 1);
            }
            BorrowState::Exclusive => {
                return Err(anyhow!("Region {} is exclusively borrowed for writing", id));
            }
        }

        Ok(SharedMemoryHandle {
            id: id.to_string(),
            region: Arc::clone(region_arc),
            writable: false,
            manager_regions: None, // Only needed for cleanup if we wanted to remove from map
        })
    }

    /// Request an exclusive write borrow
    pub fn borrow_write(&self, id: &str) -> Result<SharedMemoryHandle> {
        let regions = self.regions.read().unwrap();
        let region_arc = regions.get(id).ok_or_else(|| anyhow!("Region not found: {}", id))?;
        let mut region = region_arc.lock().unwrap();

        match region.state {
            BorrowState::Available => {
                region.state = BorrowState::Exclusive;
            }
            _ => {
                return Err(anyhow!("Region {} is already borrowed (State: {:?})", id, region.state));
            }
        }

        Ok(SharedMemoryHandle {
            id: id.to_string(),
            region: Arc::clone(region_arc),
            writable: true,
            manager_regions: None,
        })
    }
}

/// An RAII handle to shared memory that releases the borrow on drop
#[derive(Clone, Debug)]
pub struct SharedMemoryHandle {
    id: String,
    region: Arc<Mutex<MemoryRegion>>,
    writable: bool,
    // Optional back-reference for explicit removal if needed
    manager_regions: Option<Arc<RwLock<HashMap<String, Arc<Mutex<MemoryRegion>>>>>>,
}

impl SharedMemoryHandle {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn size(&self) -> usize {
        self.region.lock().unwrap().size
    }

    pub fn as_slice(&self) -> &[u8] {
        let region = self.region.lock().unwrap();
        unsafe { std::slice::from_raw_parts(region.ptr, region.size) }
    }

    pub fn as_mut_slice(&mut self) -> Result<&mut [u8]> {
        if !self.writable {
            return Err(anyhow!("Cannot get mutable slice from read-only borrow"));
        }
        let region = self.region.lock().unwrap();
        Ok(unsafe { std::slice::from_raw_parts_mut(region.ptr, region.size) })
    }
}

impl Drop for SharedMemoryHandle {
    fn drop(&mut self) {
        let mut region = self.region.lock().unwrap();
        match region.state {
            BorrowState::Exclusive => {
                region.state = BorrowState::Available;
            }
            BorrowState::Shared(count) => {
                if count > 1 {
                    region.state = BorrowState::Shared(count - 1);
                } else {
                    region.state = BorrowState::Available;
                }
            }
            BorrowState::Available => {
                // Should not happen if logic is correct
                tracing::error!("Attempted to drop borrow on available region {}", self.id);
            }
        }
    }
}

// Support for transparent access
impl Deref for SharedMemoryHandle {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}
