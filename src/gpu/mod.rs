// GPU module exposing device discovery and synthetic registration
pub mod info;
pub mod registry;
pub mod runtime;
pub mod synthetic;

pub use info::{discover_gpus, GpuInfo};
pub use runtime::{get_device_state, register_device_state, DeviceState, GpuRuntime};

// Intel XMX tensor core optimizations
#[cfg(feature = "xmx")]
pub mod xmx;

#[cfg(feature = "xmx")]
pub use xmx::{XmxHardware, XmxPrecision, detect_xmx_capability};

#[cfg(not(feature = "xmx"))]
pub mod xmx_stub;

#[cfg(not(feature = "xmx"))]
pub use xmx_stub::{XmxHardware, XmxPrecision, detect_xmx_capability};
