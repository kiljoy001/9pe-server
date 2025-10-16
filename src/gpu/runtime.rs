use once_cell::sync::Lazy;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, RwLock,
};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use uuid::Uuid;

#[derive(Debug)]
pub struct DeviceState {
    total_vram: u64,
    free_vram: AtomicU64,
}

impl DeviceState {
    fn new(total_vram: u64) -> Self {
        Self {
            total_vram,
            free_vram: AtomicU64::new(total_vram),
        }
    }

    pub fn reset(&self) {
        self.free_vram.store(self.total_vram, Ordering::SeqCst);
    }

    pub fn total_vram(&self) -> u64 {
        self.total_vram
    }

    pub fn free_vram(&self) -> u64 {
        self.free_vram.load(Ordering::SeqCst)
    }

    pub fn allocate(&self, size: u64) -> bool {
        let mut cur = self.free_vram.load(Ordering::SeqCst);
        while cur >= size {
            match self.free_vram.compare_exchange_weak(
                cur,
                cur - size,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return true,
                Err(next) => cur = next,
            }
        }
        false
    }

    pub fn release(&self, size: u64) -> bool {
        let mut current = self.free_vram.load(Ordering::SeqCst);
        loop {
            let used = self.total_vram.saturating_sub(current.min(self.total_vram));
            let (new_value, released_fully) = if size > used {
                (self.total_vram, false)
            } else {
                ((current + size).min(self.total_vram), true)
            };

            match self.free_vram.compare_exchange(
                current,
                new_value,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return released_fully,
                Err(next) => current = next,
            }
        }
    }
}

static DEVICE_REGISTRY: Lazy<RwLock<HashMap<String, Arc<DeviceState>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

pub fn register_device_state(id: &str, total_vram: u64) -> Arc<DeviceState> {
    let mut registry = DEVICE_REGISTRY.write().unwrap();
    match registry.entry(id.to_string()) {
        Entry::Occupied(entry) => {
            let state = entry.get();
            state.reset();
            Arc::clone(state)
        }
        Entry::Vacant(entry) => entry.insert(Arc::new(DeviceState::new(total_vram))).clone(),
    }
}

pub fn get_device_state(id: &str) -> Option<Arc<DeviceState>> {
    let registry = DEVICE_REGISTRY.read().unwrap();
    registry.get(id).cloned()
}

/// Runtime state for a single GPU.
/// Holds free VRAM (in bytes) and a job queue.
#[derive(Clone, Debug)]
pub struct GpuRuntime {
    device_id: String,
    state: Arc<DeviceState>,
    /// Job submission channel (fire‑and‑forget for this example)
    pub job_tx: UnboundedSender<GpuJob>,
    pub job_rx: Arc<std::sync::Mutex<UnboundedReceiver<GpuJob>>>,
}

impl GpuRuntime {
    pub fn new(id: &str, total_vram: u64) -> Self {
        let (tx, rx) = unbounded_channel();
        let state = register_device_state(id, total_vram);
        GpuRuntime {
            device_id: id.to_string(),
            state,
            job_tx: tx,
            job_rx: Arc::new(std::sync::Mutex::new(rx)),
        }
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub fn total_vram(&self) -> u64 {
        self.state.total_vram()
    }

    pub fn free_vram(&self) -> u64 {
        self.state.free_vram()
    }

    /// Try to allocate `size` bytes of VRAM. Returns true on success.
    pub fn allocate(&self, size: u64) -> bool {
        self.state.allocate(size)
    }

    /// Release `size` bytes back to the pool.
    pub fn release(&self, size: u64) -> bool {
        self.state.release(size)
    }

    pub fn state(&self) -> Arc<DeviceState> {
        self.state.clone()
    }
}

/// Simple representation of a GPU job.
pub struct GpuJob {
    pub id: Uuid,
    pub required_vram: u64,
    // In a real implementation you would store kernel pointer, args, etc.
}

impl GpuJob {
    pub fn new(required_vram: u64) -> Self {
        GpuJob {
            id: Uuid::new_v4(),
            required_vram,
        }
    }
}
