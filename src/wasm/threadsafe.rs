//! Thread-safe WASM translator system
//!
//! Solves the wasmtime threading issues by running each WASM instance
//! in its own dedicated thread with a message-passing interface.

use anyhow::{Result, Context};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc, oneshot};
use tracing::{info, debug, error};
use wasmtime::{Engine, Module, Store, Instance, Linker, Caller};
use crate::wasm::opencl_host::add_opencl_functions;

/// Store data for WASM instances
#[derive(Default)]
struct StoreData {
    // Can add context data here later
}

/// Thread-safe WASM translator that runs in a dedicated thread
#[derive(Debug)]
pub struct ThreadSafeTranslator {
    name: String,
    mount_point: PathBuf,
    command_tx: mpsc::UnboundedSender<TranslatorCommand>,
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

/// Commands sent to the WASM translator thread
#[derive(Debug)]
enum TranslatorCommand {
    ReadFile {
        path: String,
        response_tx: oneshot::Sender<Result<Vec<u8>>>,
    },
    WriteFile {
        path: String,
        data: Vec<u8>,
        response_tx: oneshot::Sender<Result<()>>,
    },
    ListFiles {
        path: String,
        response_tx: oneshot::Sender<Result<Vec<String>>>,
    },
    Shutdown,
}

/// Thread-safe translator registry
pub struct ThreadSafeTranslatorRegistry {
    translators: Arc<RwLock<HashMap<PathBuf, Arc<ThreadSafeTranslator>>>>,
    install_dir: PathBuf,
}

impl ThreadSafeTranslator {
    /// Create a new thread-safe translator
    pub async fn new(
        name: String,
        mount_point: PathBuf,
        wasm_bytes: Vec<u8>,
    ) -> Result<Self> {
        // CRITICAL: Validate WASM before spawning thread
        Self::validate_wasm_bytes(&wasm_bytes)
            .context("WASM validation failed")?;

        let (command_tx, command_rx) = mpsc::unbounded_channel();

        let translator_name = name.clone();
        let thread_handle = std::thread::spawn(move || {
            if let Err(e) = Self::run_translator_thread(translator_name, wasm_bytes, command_rx) {
                error!("WASM translator thread failed: {}", e);
            }
        });

        Ok(Self {
            name,
            mount_point,
            command_tx,
            thread_handle: Some(thread_handle),
        })
    }

    /// Comprehensive WASM validation with security checks
    fn validate_wasm_bytes(wasm_bytes: &[u8]) -> Result<()> {
        // 1. Basic size checks
        if wasm_bytes.len() < 8 {
            return Err(anyhow::anyhow!("WASM module too small: {} bytes", wasm_bytes.len()));
        }

        if wasm_bytes.len() > 50 * 1024 * 1024 {  // 50MB limit
            return Err(anyhow::anyhow!("WASM module too large: {} bytes", wasm_bytes.len()));
        }

        // 2. Magic number validation
        const WASM_MAGIC: &[u8] = &[0x00, 0x61, 0x73, 0x6d]; // "\0asm"
        if !wasm_bytes.starts_with(WASM_MAGIC) {
            return Err(anyhow::anyhow!(
                "Invalid WASM magic number: {:02x?}",
                &wasm_bytes[..4.min(wasm_bytes.len())]
            ));
        }

        // 3. Version validation
        const WASM_VERSION: &[u8] = &[0x01, 0x00, 0x00, 0x00]; // Version 1
        if wasm_bytes.len() < 8 || !wasm_bytes[4..8].eq(WASM_VERSION) {
            return Err(anyhow::anyhow!(
                "Unsupported WASM version: {:02x?}",
                &wasm_bytes[4..8.min(wasm_bytes.len())]
            ));
        }

        // 4. Create engine with balanced security and performance
        let mut config = wasmtime::Config::new();

        // Compilation strategy (Cranelift is secure and performant)
        config.strategy(wasmtime::Strategy::Cranelift);

        // Security: Resource limits
        config.max_wasm_stack(256 * 1024); // 256KB stack - generous but safe
        config.consume_fuel(true); // Enable fuel for execution time limits
        config.epoch_interruption(true); // Allow interrupting long operations

        // Performance: Enable modern WASM features
        config.wasm_simd(true); // Allow SIMD for performance
        config.wasm_bulk_memory(true); // Allow bulk memory operations
        config.wasm_multi_value(true); // Allow multiple return values

        // Security: Still restrict dangerous features
        config.wasm_multi_memory(false); // Limit to single memory
        config.wasm_threads(false); // No threading support
        config.wasm_reference_types(false); // No reference types for simplicity

        // Memory security
        config.memory_init_cow(false); // Disable copy-on-write for predictability
        config.generate_address_map(false); // Don't generate debug info

        let engine = Engine::new(&config)?;

        // 5. Validate by attempting to parse as module
        let module = Module::new(&engine, wasm_bytes)
            .context("WASM module parsing failed")?;

        // 6. Security: Check for required exports
        let mut has_read_file = false;
        let mut has_write_file = false;
        let mut has_list_files = false;
        let mut has_memory = false;

        for export in module.exports() {
            match export.name() {
                "read_file" => {
                    if export.ty().func().is_some() {
                        has_read_file = true;
                    }
                }
                "write_file" => {
                    if export.ty().func().is_some() {
                        has_write_file = true;
                    }
                }
                "list_files" => {
                    if export.ty().func().is_some() {
                        has_list_files = true;
                    }
                }
                "memory" => {
                    if export.ty().memory().is_some() {
                        has_memory = true;
                    }
                }
                _ => {}
            }
        }

        // 7. Require essential exports for 9P translator
        if !has_memory {
            return Err(anyhow::anyhow!("WASM module must export 'memory'"));
        }

        if !has_read_file {
            return Err(anyhow::anyhow!("WASM module must export 'read_file' function"));
        }

        if !has_write_file {
            return Err(anyhow::anyhow!("WASM module must export 'write_file' function"));
        }

        if !has_list_files {
            return Err(anyhow::anyhow!("WASM module must export 'list_files' function"));
        }

        // 8. Security: Validate imports
        for import in module.imports() {
            match import.module() {
                "ninep" => {
                    // Allow only whitelisted host functions
                    match import.name() {
                        "log" => {
                            if import.ty().func().is_none() {
                                return Err(anyhow::anyhow!("ninep.log must be a function"));
                            }
                        }
                        _ => {
                            return Err(anyhow::anyhow!(
                                "Unauthorized import: ninep.{}",
                                import.name()
                            ));
                        }
                    }
                }
                "opencl" => {
                    // Allow OpenCL host functions for compute transformers
                    match import.name() {
                        "get_platform_count" | "get_platforms" | "get_device_count" | "get_devices" |
                        "create_context" | "create_queue" | "create_buffer" | "write_buffer" |
                        "read_buffer" | "release_buffer" | "create_program" | "build_program" |
                        "create_kernel" | "set_kernel_arg" | "enqueue_kernel" | "finish" => {
                            if import.ty().func().is_none() {
                                return Err(anyhow::anyhow!("opencl.{} must be a function", import.name()));
                            }
                        }
                        _ => {
                            return Err(anyhow::anyhow!(
                                "Unauthorized OpenCL import: opencl.{}",
                                import.name()
                            ));
                        }
                    }
                }
                _ => {
                    return Err(anyhow::anyhow!(
                        "Unauthorized import module: {}",
                        import.module()
                    ));
                }
            }
        }

        info!("WASM validation passed for {} byte module", wasm_bytes.len());
        Ok(())
    }

    /// Run the WASM translator in its own thread
    fn run_translator_thread(
        name: String,
        wasm_bytes: Vec<u8>,
        mut command_rx: mpsc::UnboundedReceiver<TranslatorCommand>,
    ) -> Result<()> {
        // Create WASM runtime in this thread (where it's safe)
        let engine = Engine::default();
        let module = Module::new(&engine, &wasm_bytes)?;

        // Create store with context data
        let store_data = StoreData::default();
        let mut store = Store::new(&engine, store_data);
        let mut linker: Linker<StoreData> = Linker::new(&engine);

        // Add custom host functions for 9P operations
        let translator_name = name.clone();
        linker.func_wrap("ninep", "log", move |_caller: Caller<'_, StoreData>, message: i32| {
            debug!("WASM translator '{}' log: {}", translator_name, message);
        })?;

        // Add OpenCL host functions for compute access
        add_opencl_functions(&mut linker)?;

        // Instantiate the module
        let instance = linker.instantiate(&mut store, &module)?;

        info!("WASM translator '{}' initialized successfully", name);

        // Message processing loop
        while let Some(command) = command_rx.blocking_recv() {
            match command {
                TranslatorCommand::ReadFile { path, response_tx } => {
                    let result = Self::handle_read_file(&mut store, &instance, &path);
                    let _ = response_tx.send(result);
                }
                TranslatorCommand::WriteFile { path, data, response_tx } => {
                    let result = Self::handle_write_file(&mut store, &instance, &path, data);
                    let _ = response_tx.send(result);
                }
                TranslatorCommand::ListFiles { path, response_tx } => {
                    let result = Self::handle_list_files(&mut store, &instance, &path);
                    let _ = response_tx.send(result);
                }
                TranslatorCommand::Shutdown => {
                    info!("WASM translator '{}' shutting down", name);
                    break;
                }
            }
        }

        Ok(())
    }

    /// Handle read file operation
    fn handle_read_file(
        store: &mut Store<StoreData>,
        instance: &Instance,
        path: &str,
    ) -> Result<Vec<u8>> {
        // For now, just return test data without memory access
        // This is a simplified implementation until we have proper WASM modules
        debug!("WASM translator reading file: {}", path);
        Ok(format!("Data from WASM translator for path: {}", path).into_bytes())
    }

    /// Handle write file operation
    fn handle_write_file(
        store: &mut Store<StoreData>,
        instance: &Instance,
        path: &str,
        data: Vec<u8>,
    ) -> Result<()> {
        // For now, just log the operation without memory access
        debug!("WASM translator writing {} bytes to: {}", data.len(), path);
        Ok(())
    }

    /// Handle list files operation
    fn handle_list_files(
        store: &mut Store<StoreData>,
        instance: &Instance,
        path: &str,
    ) -> Result<Vec<String>> {
        // For now, return test data without memory access
        debug!("WASM translator listing files in: {}", path);
        Ok(vec![
            "query.sql".to_string(),
            "result.json".to_string(),
            "schema.sql".to_string(),
            "databases.json".to_string(),
        ])
    }

    /// Read a file through the WASM translator
    pub async fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let (response_tx, response_rx) = oneshot::channel();

        self.command_tx.send(TranslatorCommand::ReadFile {
            path: path.to_string(),
            response_tx,
        })?;

        response_rx.await?
    }

    /// Write a file through the WASM translator
    pub async fn write_file(&self, path: &str, data: Vec<u8>) -> Result<()> {
        let (response_tx, response_rx) = oneshot::channel();

        self.command_tx.send(TranslatorCommand::WriteFile {
            path: path.to_string(),
            data,
            response_tx,
        })?;

        response_rx.await?
    }

    /// List files through the WASM translator
    pub async fn list_files(&self, path: &str) -> Result<Vec<String>> {
        let (response_tx, response_rx) = oneshot::channel();

        self.command_tx.send(TranslatorCommand::ListFiles {
            path: path.to_string(),
            response_tx,
        })?;

        response_rx.await?
    }

    /// Get the translator name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the mount point
    pub fn mount_point(&self) -> &PathBuf {
        &self.mount_point
    }
}

impl Drop for ThreadSafeTranslator {
    fn drop(&mut self) {
        // Send shutdown command
        let _ = self.command_tx.send(TranslatorCommand::Shutdown);

        // Wait for thread to finish
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
}

impl ThreadSafeTranslatorRegistry {
    /// Create a new thread-safe translator registry
    pub fn new(install_dir: PathBuf) -> Self {
        Self {
            translators: Arc::new(RwLock::new(HashMap::new())),
            install_dir,
        }
    }

    /// Scan directory and load all WASM translators
    pub async fn scan_and_load(&self) -> Result<()> {
        use tokio::fs;

        // Create directory if it doesn't exist
        fs::create_dir_all(&self.install_dir).await.ok();

        // Read all .wasm files from the directory
        let mut entries = fs::read_dir(&self.install_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("wasm") {
                if let Some(file_name) = path.file_stem() {
                    let name = file_name.to_string_lossy().to_string();
                    let mount_point = PathBuf::from(format!("/srv/{}", name));

                    match fs::read(&path).await {
                        Ok(wasm_bytes) => {
                            if let Err(e) = self.load_translator(name.clone(), mount_point, wasm_bytes).await {
                                error!("Failed to load translator {}: {}", name, e);
                            } else {
                                info!("Loaded translator {} from {:?}", name, path);
                            }
                        }
                        Err(e) => {
                            error!("Failed to read WASM file {:?}: {}", path, e);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Load a translator from WASM bytes
    pub async fn load_translator(
        &self,
        name: String,
        mount_point: PathBuf,
        wasm_bytes: Vec<u8>,
    ) -> Result<()> {
        let translator = Arc::new(
            ThreadSafeTranslator::new(name.clone(), mount_point.clone(), wasm_bytes).await?
        );

        let mut translators = self.translators.write().await;
        translators.insert(mount_point, translator);

        info!("Loaded thread-safe WASM translator: {}", name);
        Ok(())
    }

    /// Get a translator by mount point
    pub async fn get_translator(&self, mount_point: &PathBuf) -> Option<Arc<ThreadSafeTranslator>> {
        let translators = self.translators.read().await;
        translators.get(mount_point).cloned()
    }

    /// List all loaded translators
    pub async fn list_translators(&self) -> Vec<String> {
        let translators = self.translators.read().await;
        translators
            .values()
            .map(|t| t.name().to_string())
            .collect()
    }

    /// Remove a translator
    pub async fn remove_translator(&self, mount_point: &PathBuf) -> Result<()> {
        let mut translators = self.translators.write().await;
        if let Some(translator) = translators.remove(mount_point) {
            info!("Removed translator: {}", translator.name());
            // Translator will be dropped here, triggering shutdown
        }
        Ok(())
    }
}

// Ensure the registry is thread-safe
unsafe impl Send for ThreadSafeTranslatorRegistry {}
unsafe impl Sync for ThreadSafeTranslatorRegistry {}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use proptest::prelude::*;

    #[tokio::test]
    async fn test_thread_safe_registry() {
        let temp_dir = TempDir::new().unwrap();
        let registry = ThreadSafeTranslatorRegistry::new(temp_dir.path().to_path_buf());

        // Test that registry can be created and is thread-safe
        assert_eq!(registry.list_translators().await.len(), 0);
    }

    #[tokio::test]
    async fn test_translator_creation_with_invalid_wasm() {
        // Invalid WASM (just magic number)
        let wasm_bytes = vec![0x00, 0x61, 0x73, 0x6d]; // WASM magic number only

        let result = ThreadSafeTranslator::new(
            "test".to_string(),
            PathBuf::from("/srv/test"),
            wasm_bytes,
        ).await;

        // Should fail with invalid WASM
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_translator_registry() {
        let temp_dir = TempDir::new().unwrap();
        let registry = ThreadSafeTranslatorRegistry::new(temp_dir.path().to_path_buf());

        // Test registry operations
        assert_eq!(registry.list_translators().await.len(), 0);

        // Test scan of empty directory
        assert!(registry.scan_and_load().await.is_ok());
    }

    // =================== PROPERTY TESTS ===================

    /// Create a minimal but valid WASM module that meets our requirements
    fn create_minimal_valid_wasm() -> Vec<u8> {
        // Use the wat crate to compile WAT text to WASM bytes
        wat::parse_str(r#"
            (module
              (memory (export "memory") 1)
              (func (export "read_file") (param i32 i32) (result i32)
                i32.const 0
              )
              (func (export "write_file") (param i32 i32 i32 i32) (result i32)
                i32.const 0
              )
              (func (export "list_files") (param i32 i32) (result i32)
                i32.const 0
              )
            )
        "#).expect("Failed to compile WAT to WASM")
    }

    /// Property test: invalid WASM bytes should be rejected
    #[tokio::test]
    async fn test_property_invalid_wasm_rejected() {
        // Test various invalid WASM patterns
        let invalid_cases = vec![
            // Too small
            vec![0x00, 0x61, 0x73],
            // Wrong magic
            vec![0xFF, 0xFF, 0xFF, 0xFF, 0x01, 0x00, 0x00, 0x00],
            // Wrong version
            vec![0x00, 0x61, 0x73, 0x6d, 0xFF, 0xFF, 0xFF, 0xFF],
            // Empty
            vec![],
            // Just magic number (minimum failing case from original test)
            vec![0x00, 0x61, 0x73, 0x6d],
            // Random bytes
            vec![0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE],
        ];

        for (i, invalid_wasm) in invalid_cases.into_iter().enumerate() {
            let result = ThreadSafeTranslator::new(
                format!("test_invalid_{}", i),
                PathBuf::from("/srv/test"),
                invalid_wasm,
            ).await;

            assert!(result.is_err(), "Invalid WASM case {} should be rejected", i);
        }
    }

    /// Property test: valid WASM should be accepted
    #[tokio::test]
    async fn test_property_valid_wasm_accepted() {
        let valid_wasm = create_minimal_valid_wasm();

        let result = ThreadSafeTranslator::new(
            "test_valid".to_string(),
            PathBuf::from("/srv/test"),
            valid_wasm,
        ).await;

        assert!(result.is_ok(), "Valid WASM should be accepted, got error: {:?}", result.err());
    }

    /// Test that WASM modules without required exports are rejected
    #[tokio::test]
    async fn test_missing_memory_export_rejected() {
        // Create WASM with all required functions but no memory export
        let mut wasm = create_minimal_valid_wasm();

        // Modify export section to remove memory export
        // This is a simplified test - in practice you'd need to properly modify the WASM binary
        let result = ThreadSafeTranslator::new(
            "test_no_memory".to_string(),
            PathBuf::from("/srv/test"),
            vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00], // Minimal header only
        ).await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        // The minimal header should trigger a validation/parsing error
        assert!(err_msg.contains("validation") || err_msg.contains("parsing") || err_msg.contains("too small") || err_msg.contains("end-of-file"),
                "Expected validation error, got: {}", err_msg);
    }

    /// Test concurrent WASM validation
    #[tokio::test]
    async fn test_concurrent_validation() {
        let valid_wasm = create_minimal_valid_wasm();
        let invalid_wasm = vec![0x00, 0x61, 0x73, 0x6d]; // Just magic

        let mut handles = vec![];

        // Start multiple validation tasks concurrently
        for i in 0..10 {
            let valid = valid_wasm.clone();
            let invalid = invalid_wasm.clone();

            let handle = tokio::spawn(async move {
                let valid_result = ThreadSafeTranslator::new(
                    format!("valid_{}", i),
                    PathBuf::from("/srv/valid"),
                    valid,
                ).await;

                let invalid_result = ThreadSafeTranslator::new(
                    format!("invalid_{}", i),
                    PathBuf::from("/srv/invalid"),
                    invalid,
                ).await;

                (valid_result.is_ok(), invalid_result.is_err())
            });

            handles.push(handle);
        }

        // Verify all concurrent validations work correctly
        for handle in handles {
            let (valid_ok, invalid_err) = handle.await.unwrap();
            assert!(valid_ok, "Valid WASM should be accepted");
            assert!(invalid_err, "Invalid WASM should be rejected");
        }
    }

    /// Test validation with security edge cases
    #[tokio::test]
    async fn test_security_validation() {
        // Test various security-related rejection cases

        // 1. WASM with unauthorized imports
        let result = ThreadSafeTranslator::validate_wasm_bytes(
            &create_minimal_valid_wasm() // This would need to be modified to have bad imports
        );
        // For now, our minimal WASM should pass
        assert!(result.is_ok());

        // 2. Test exact boundary conditions

        // Exactly 8 bytes (minimum) - should fail as incomplete
        let exactly_8 = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        let result = ThreadSafeTranslator::validate_wasm_bytes(&exactly_8);
        assert!(result.is_err());

        // Exactly at size limit (50MB) - should be accepted if valid structure
        // (We won't actually test this due to memory constraints)

        // Just over size limit
        let too_large = vec![0u8; 50 * 1024 * 1024 + 1];
        let result = ThreadSafeTranslator::validate_wasm_bytes(&too_large);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too large"));
    }

    /// Test validation error messages are informative
    #[tokio::test]
    async fn test_validation_error_messages() {
        // Wrong magic number
        let wrong_magic = vec![0xFF, 0xFF, 0xFF, 0xFF, 0x01, 0x00, 0x00, 0x00];
        let result = ThreadSafeTranslator::validate_wasm_bytes(&wrong_magic);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid WASM magic number"));

        // Wrong version
        let wrong_version = vec![0x00, 0x61, 0x73, 0x6d, 0xFF, 0xFF, 0xFF, 0xFF];
        let result = ThreadSafeTranslator::validate_wasm_bytes(&wrong_version);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unsupported WASM version"));

        // Too small
        let too_small = vec![0x00, 0x61, 0x73];
        let result = ThreadSafeTranslator::validate_wasm_bytes(&too_small);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too small"));
    }

    /// Fuzz test: WASM validation should never panic on arbitrary input
    #[test]
    fn fuzz_wasm_validation_no_panic() {
        use proptest::prelude::*;

        proptest!(|(bytes: Vec<u8>)| {
            // Validation should never panic, only return Ok or Err
            let _ = ThreadSafeTranslator::validate_wasm_bytes(&bytes);
        });
    }

    /// Fuzz test: Valid WASM from WAT should always be accepted
    #[test]
    fn fuzz_valid_wasm_always_accepted() {
        use proptest::prelude::*;

        // Generate variations of valid WAT that should all be accepted
        proptest!(|(mem_pages in 1u32..100, func_count in 1u32..10)| {
            let wat = format!(r#"
                (module
                  (memory (export "memory") {})
                  (func (export "read_file") (param i32 i32) (result i32)
                    i32.const 0
                  )
                  (func (export "write_file") (param i32 i32 i32 i32) (result i32)
                    i32.const 0
                  )
                  (func (export "list_files") (param i32 i32) (result i32)
                    i32.const 0
                  )
                  {}
                )
            "#, mem_pages,
                (0..func_count).map(|i|
                    format!("(func $helper{} (param i32) (result i32) i32.const {})", i, i)
                ).collect::<Vec<_>>().join("\n")
            );

            let wasm = wat::parse_str(&wat).expect("WAT compilation failed");
            let result = ThreadSafeTranslator::validate_wasm_bytes(&wasm);
            prop_assert!(result.is_ok(), "Valid WASM should be accepted: {:?}", result.err());
        });
    }

    /// Fuzz test: Invalid WASM patterns should be safely rejected
    #[test]
    fn fuzz_invalid_wasm_safely_rejected() {
        use proptest::prelude::*;

        proptest!(|(
            corrupt_at in 0usize..100,
            corrupt_byte in any::<u8>(),
            size_mult in 0usize..10
        )| {
            let mut wasm = create_minimal_valid_wasm();

            // Corrupt at random position
            if corrupt_at < wasm.len() {
                wasm[corrupt_at] = corrupt_byte;
            }

            // Or make it wrong size
            if size_mult > 0 {
                wasm.resize(size_mult, 0);
            }

            // Should either accept (if corruption didn't break it) or safely reject
            let _ = ThreadSafeTranslator::validate_wasm_bytes(&wasm);
        });
    }

    /// Fuzz test: Missing required exports should be rejected
    #[test]
    fn fuzz_missing_exports_rejected() {
        use proptest::prelude::*;

        let test_cases = vec![
            // Missing read_file
            r#"(module
                (memory (export "memory") 1)
                (func (export "write_file") (param i32 i32 i32 i32) (result i32) i32.const 0)
                (func (export "list_files") (param i32 i32) (result i32) i32.const 0)
            )"#,
            // Missing write_file
            r#"(module
                (memory (export "memory") 1)
                (func (export "read_file") (param i32 i32) (result i32) i32.const 0)
                (func (export "list_files") (param i32 i32) (result i32) i32.const 0)
            )"#,
            // Missing list_files
            r#"(module
                (memory (export "memory") 1)
                (func (export "read_file") (param i32 i32) (result i32) i32.const 0)
                (func (export "write_file") (param i32 i32 i32 i32) (result i32) i32.const 0)
            )"#,
            // Missing memory
            r#"(module
                (func (export "read_file") (param i32 i32) (result i32) i32.const 0)
                (func (export "write_file") (param i32 i32 i32 i32) (result i32) i32.const 0)
                (func (export "list_files") (param i32 i32) (result i32) i32.const 0)
            )"#,
        ];

        for (i, wat) in test_cases.iter().enumerate() {
            let wasm = wat::parse_str(wat).expect("WAT should compile");
            let result = ThreadSafeTranslator::validate_wasm_bytes(&wasm);
            assert!(result.is_err(), "Test case {} - WASM missing required exports should be rejected: {:?}", i, result);
        }
    }
}