//! SYCL GPU acceleration support via AdaptiveCpp
//!
//! Provides high-level Rust API for SYCL compute operations

pub mod ffi;

pub use ffi::{SyclDevice, SyclQueue, SyclBuffer, SyclKernel, SyclEvent};
pub use ffi::{SyclDeviceInfo, SyclError, SyclBackend};
