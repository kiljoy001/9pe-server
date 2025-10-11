// GPU module exposing device discovery and synthetic registration
pub mod info;
pub mod registry;
pub mod runtime;
pub mod synthetic;

pub use info::GpuInfo;
pub use runtime::GpuRuntime;
