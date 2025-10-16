// GPU module exposing device discovery and synthetic registration
pub mod info;
pub mod registry;
pub mod runtime;
pub mod synthetic;

pub use info::{discover_gpus, GpuInfo};
pub use runtime::{get_device_state, register_device_state, DeviceState, GpuRuntime};
