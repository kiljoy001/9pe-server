//! WASM Translator System
//!
//! Complete WASM-based translator system for 9P.e server
//! Users can extend the filesystem by dropping WASM modules into `/srv/translators/`

pub mod translator;
pub mod composition;
pub mod threadsafe;

pub use translator::{WasmTranslator, TranslatorRegistry, TranslatorMetadata};
pub use composition::{WasmComposer, WasmFileHandlers, WasiState};
pub use threadsafe::{ThreadSafeTranslator, ThreadSafeTranslatorRegistry};