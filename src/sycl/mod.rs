//! SYCL GPU acceleration support via AdaptiveCpp
//!
//! Provides high-level Rust API for SYCL compute operations

pub mod ffi;

pub use ffi::{SyclBackend, SyclDeviceInfo, SyclError};
pub use ffi::{SyclBuffer, SyclDevice, SyclEvent, SyclKernel, SyclQueue};
