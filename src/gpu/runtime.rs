use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
use tokio::sync::mpsc::{UnboundedSender, UnboundedReceiver, unbounded_channel};
use uuid::Uuid;

/// Runtime state for a single GPU.
/// Holds free VRAM (in bytes) and a job queue.
#[derive(Clone)]
pub struct GpuRuntime {
    /// Total VRAM of the device (bytes)
    pub total_vram: u64,
    /// Currently free VRAM (bytes)
    pub free_vram: Arc<AtomicU64>,
    /// Job submission channel (fire‑and‑forget for this example)
    pub job_tx: UnboundedSender<GpuJob>,
    pub job_rx: Arc<std::sync::Mutex<UnboundedReceiver<GpuJob>>>,
}

impl GpuRuntime {
    pub fn new(total_vram: u64) -> Self {
        let (tx, rx) = unbounded_channel();
        GpuRuntime {
            total_vram,
            free_vram: Arc::new(AtomicU64::new(total_vram)),
            job_tx: tx,
            job_rx: Arc::new(std::sync::Mutex::new(rx)),
        }
    }

    /// Try to allocate `size` bytes of VRAM. Returns true on success.
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

    /// Release `size` bytes back to the pool.
    pub fn release(&self, size: u64) {
        self.free_vram.fetch_add(size, Ordering::SeqCst);
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
        GpuJob { id: Uuid::new_v4(), required_vram }
    }
}
