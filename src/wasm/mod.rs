//! WASM Translator System
//!
//! Complete WASM-based translator system for 9P.e server
//! Users can extend the filesystem by dropping WASM modules into `/srv/translators/`
//!
//! NOTE: For GPU compute, use SYCL (src/sycl/) instead of the old OpenCL/OneAPI stubs.

pub mod composition;
pub mod consensus_host;
pub mod threadsafe;
pub mod translator;

pub use composition::{WasiState, WasmComposer, WasmFileHandlers};
pub use consensus_host::{
    add_consensus_functions, get_consensus_diagnostics, update_consensus_state,
};
pub use threadsafe::{ThreadSafeTranslator, ThreadSafeTranslatorRegistry};
pub use translator::{TranslatorMetadata, TranslatorRegistry, WasmTranslator};
