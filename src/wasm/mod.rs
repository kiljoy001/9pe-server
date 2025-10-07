//! WASM Translator System
//!
//! Complete WASM-based translator system for 9P.e server
//! Users can extend the filesystem by dropping WASM modules into `/srv/translators/`
//!
//! NOTE: For GPU compute, use SYCL (src/sycl/) instead of the old OpenCL/OneAPI stubs.

pub mod translator;
pub mod composition;
pub mod threadsafe;
pub mod opencl_host;
pub mod consensus_host;

pub use translator::{WasmTranslator, TranslatorRegistry, TranslatorMetadata};
pub use composition::{WasmComposer, WasmFileHandlers, WasiState};
pub use threadsafe::{ThreadSafeTranslator, ThreadSafeTranslatorRegistry};
pub use opencl_host::{add_opencl_functions, initialize_opencl, get_opencl_info};
pub use consensus_host::{add_consensus_functions, update_consensus_state, get_consensus_diagnostics};