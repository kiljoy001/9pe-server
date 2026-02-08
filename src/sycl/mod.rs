//! SYCL GPU acceleration support with dual-backend architecture
//!
//! Provides high-level Rust API for SYCL compute operations with runtime
//! backend selection:
//! - Intel oneAPI for Intel GPUs (optimized, preferred)
//! - AdaptiveCpp for NVIDIA/AMD GPUs (universal fallback)

pub mod ffi;
pub mod backend_loader;
pub mod compat;  // Compatibility shim for old FFI interface
pub mod canvas;  // GPU-accelerated canvas rendering
pub mod scheduler;  // Priority-based job scheduler

pub use ffi::{SyclBackend, SyclDeviceInfo, SyclError};
pub use ffi::{SyclBuffer, SyclDevice, SyclEvent, SyclKernel, SyclQueue};
pub use backend_loader::{BackendType, DeviceInfo, SyclBackendLib, SyclBackendManager};
pub use canvas::{SyclCanvas, CanvasRenderer, BYTES_PER_PIXEL};
pub use scheduler::{JobScheduler, JobPriority, JobSubmitRequest, ScheduledJob, ScheduledJobStatus, SchedulerStats, NodeHeartbeat, JobCheckpointData};
