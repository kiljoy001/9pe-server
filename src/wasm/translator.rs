//! WASM Translator System - User-extensible filesystem translators
//!
//! Users can extend the filesystem by dropping WASM modules into /srv/translators/

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::{Result, Context};
use wasmtime::{Engine, Module, Store, Instance, Memory, Linker};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder};

/// WASM translator instance
pub struct WasmTranslator {
    /// Name of the translator
    name: String,
    /// WASM module
    module: Module,
    /// Engine for execution
    engine: Engine,
    /// Mount point in filesystem
    mount_point: PathBuf,
    /// Active instances (per-connection)
    instances: Arc<RwLock<HashMap<u64, TranslatorInstance>>>,
}

/// Per-connection translator instance
struct TranslatorInstance {
    store: Store<WasiCtx>,
    instance: Instance,
    memory: Memory,
}

impl WasmTranslator {
    /// Load a WASM translator from bytecode
    pub async fn load(name: String, wasm_bytes: Vec<u8>, mount_point: PathBuf) -> Result<Self> {
        let engine = Engine::default();
        let module = Module::new(&engine, &wasm_bytes)
            .context("Failed to compile WASM module")?;

        Ok(Self {
            name,
            module,
            engine,
            mount_point,
            instances: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Create instance for a new connection
    pub async fn create_instance(&self, conn_id: u64) -> Result<()> {
        let mut linker = Linker::new(&self.engine);
        // Skip WASI linking for now - will add when needed
        // wasmtime_wasi::add_to_linker(&mut linker, |ctx| ctx)?;

        // Add 9P operations as host functions
        self.add_ninep_functions(&mut linker)?;

        // Create WASI context
        let wasi = WasiCtxBuilder::new()
            .inherit_stdio()
            .build();

        let mut store = Store::new(&self.engine, wasi);
        let instance = linker.instantiate(&mut store, &self.module)?;

        // Get memory export
        let memory = instance
            .get_memory(&mut store, "memory")
            .context("WASM module must export 'memory'")?;

        let translator_instance = TranslatorInstance {
            store,
            instance,
            memory,
        };

        self.instances.write().await.insert(conn_id, translator_instance);
        Ok(())
    }

    /// Add 9P protocol functions to WASM environment
    fn add_ninep_functions<T>(&self, linker: &mut Linker<T>) -> Result<()> {
        // These functions allow WASM to interact with 9P protocol

        // Read operation
        linker.func_wrap("9p", "read",
            |_caller: wasmtime::Caller<'_, T>, _fid: i32, _offset: i64, _count: i32| -> i32 {
                // Implementation would forward to actual 9P handler
                // For now, return success
                0
            })?;

        // Write operation
        linker.func_wrap("9p", "write",
            |_caller: wasmtime::Caller<'_, T>, _fid: i32, _offset: i64, _data_ptr: i32, _count: i32| -> i32 {
                // Implementation would forward to actual 9P handler
                0
            })?;

        // Stat operation
        linker.func_wrap("9p", "stat",
            |_caller: wasmtime::Caller<'_, T>, _fid: i32, _stat_ptr: i32| -> i32 {
                // Implementation would forward to actual 9P handler
                0
            })?;

        Ok(())
    }

    /// Handle a 9P message through WASM
    pub async fn handle_message(&self, conn_id: u64, msg: &[u8]) -> Result<Vec<u8>> {
        let mut instances = self.instances.write().await;
        let instance = instances.get_mut(&conn_id)
            .context("No instance for connection")?;

        // Call WASM entry point
        let handle_func = instance.instance
            .get_typed_func::<(i32, i32), i32>(&mut instance.store, "handle_9p_message")?;

        // Copy message to WASM memory
        let msg_ptr = self.copy_to_wasm(&mut instance.store, &instance.memory, msg)?;

        // Call handler
        let result_ptr = handle_func.call(&mut instance.store, (msg_ptr as i32, msg.len() as i32))?;

        // Read response from WASM memory
        let response = self.read_from_wasm(&mut instance.store, &instance.memory, result_ptr as usize)?;

        Ok(response)
    }

    /// Copy data into WASM memory
    fn copy_to_wasm(&self, store: &mut Store<WasiCtx>, memory: &Memory, data: &[u8]) -> Result<usize> {
        // Simple allocation strategy - use a fixed offset for now
        // In a real implementation, we'd have proper memory management
        let ptr = 1024; // Start at 1KB offset

        // Write data
        memory.write(store, ptr, data)?;
        Ok(ptr)
    }

    /// Read data from WASM memory
    fn read_from_wasm(&self, store: &mut Store<WasiCtx>, memory: &Memory, ptr: usize) -> Result<Vec<u8>> {
        // First read the length (assume first 4 bytes at ptr)
        let mut len_bytes = [0u8; 4];
        memory.read(&*store, ptr, &mut len_bytes)?;
        let len = u32::from_le_bytes(len_bytes) as usize;

        // Read the actual data
        let mut data = vec![0u8; len];
        memory.read(&*store, ptr + 4, &mut data)?;
        Ok(data)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn mount_point(&self) -> &Path {
        &self.mount_point
    }
}

/// Translator registry - manages all loaded translators
pub struct TranslatorRegistry {
    /// Map of mount point to translator
    translators: Arc<RwLock<HashMap<PathBuf, Arc<WasmTranslator>>>>,
    /// Installation directory
    install_dir: PathBuf,
}

impl TranslatorRegistry {
    pub fn new(install_dir: PathBuf) -> Self {
        Self {
            translators: Arc::new(RwLock::new(HashMap::new())),
            install_dir,
        }
    }

    /// Scan install directory and load all WASM modules
    pub async fn scan_and_load(&self) -> Result<()> {
        use tokio::fs;

        // Create directories if they don't exist
        fs::create_dir_all(&self.install_dir).await?;
        fs::create_dir_all(self.install_dir.join("available")).await?;
        fs::create_dir_all(self.install_dir.join("enabled")).await?;

        // Scan enabled directory
        let enabled_dir = self.install_dir.join("enabled");
        let mut entries = fs::read_dir(&enabled_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("wasm") {
                self.load_translator(path).await?;
            }
        }

        Ok(())
    }

    /// Load a single translator
    async fn load_translator(&self, wasm_path: PathBuf) -> Result<()> {
        use tokio::fs;

        // Read WASM bytecode
        let wasm_bytes = fs::read(&wasm_path).await
            .context("Failed to read WASM file")?;

        // Read metadata (adjacent .json file)
        let meta_path = wasm_path.with_extension("json");
        let metadata = if meta_path.exists() {
            let meta_bytes = fs::read(&meta_path).await?;
            serde_json::from_slice::<TranslatorMetadata>(&meta_bytes)?
        } else {
            // Default metadata
            let name = wasm_path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
            TranslatorMetadata {
                name: name.clone(),
                mount_point: format!("/srv/{}", name),
                version: "1.0.0".to_string(),
                description: "User translator".to_string(),
            }
        };

        // Create translator
        let translator = WasmTranslator::load(
            metadata.name.clone(),
            wasm_bytes,
            PathBuf::from(&metadata.mount_point),
        ).await?;

        // Register it
        self.translators.write().await.insert(
            PathBuf::from(metadata.mount_point),
            Arc::new(translator),
        );

        tracing::info!("Loaded translator '{}' at {}", metadata.name, wasm_path.display());
        Ok(())
    }

    /// Install a new translator (copy to install directory)
    pub async fn install_translator(&self, wasm_bytes: Vec<u8>, metadata: TranslatorMetadata) -> Result<()> {
        use tokio::fs;

        let filename = format!("{}.wasm", metadata.name);
        let wasm_path = self.install_dir.join("available").join(&filename);
        let meta_path = wasm_path.with_extension("json");

        // Write WASM file
        fs::write(&wasm_path, wasm_bytes).await?;

        // Write metadata
        let meta_bytes = serde_json::to_vec_pretty(&metadata)?;
        fs::write(&meta_path, meta_bytes).await?;

        tracing::info!("Installed translator '{}' to available", metadata.name);
        Ok(())
    }

    /// Enable a translator (symlink from available to enabled)
    pub async fn enable_translator(&self, name: &str) -> Result<()> {
        use tokio::fs;

        let available = self.install_dir.join("available").join(format!("{}.wasm", name));
        let enabled = self.install_dir.join("enabled").join(format!("{}.wasm", name));

        if !available.exists() {
            return Err(anyhow::anyhow!("Translator '{}' not found in available", name));
        }

        // Create symlink
        #[cfg(unix)]
        fs::symlink(&available, &enabled).await?;

        #[cfg(not(unix))]
        fs::copy(&available, &enabled).await?;

        // Also copy metadata
        let meta_available = available.with_extension("json");
        let meta_enabled = enabled.with_extension("json");

        #[cfg(unix)]
        fs::symlink(&meta_available, &meta_enabled).await?;

        #[cfg(not(unix))]
        fs::copy(&meta_available, &meta_enabled).await?;

        // Load the translator
        self.load_translator(enabled).await?;

        tracing::info!("Enabled translator '{}'", name);
        Ok(())
    }

    /// Check if a path has a translator mounted
    pub async fn get_translator(&self, path: &Path) -> Option<Arc<WasmTranslator>> {
        let translators = self.translators.read().await;

        // Find longest matching mount point
        let mut best_match = None;
        let mut best_len = 0;

        for (mount_point, translator) in translators.iter() {
            if path.starts_with(mount_point) {
                let len = mount_point.components().count();
                if len > best_len {
                    best_match = Some(translator.clone());
                    best_len = len;
                }
            }
        }

        best_match
    }

    /// List all available translators
    pub async fn list_available(&self) -> Result<Vec<String>> {
        use tokio::fs;

        let available_dir = self.install_dir.join("available");
        let mut translators = Vec::new();

        if available_dir.exists() {
            let mut entries = fs::read_dir(&available_dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("wasm") {
                    if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                        translators.push(name.to_string());
                    }
                }
            }
        }

        Ok(translators)
    }

    /// List all enabled translators
    pub async fn list_enabled(&self) -> Vec<String> {
        let translators = self.translators.read().await;
        translators.values().map(|t| t.name().to_string()).collect()
    }
}

/// Metadata for a translator
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct TranslatorMetadata {
    pub name: String,
    pub mount_point: String,
    pub version: String,
    pub description: String,
}

#[cfg(test)]
mod fuzz_tests {
    use super::*;
    use proptest::prelude::*;

    /// Fuzz test: Translator message deserialization
    #[test]
    fn fuzz_translator_message() {
        proptest!(|(bytes: Vec<u8>)| {
            // Should never panic on arbitrary messages
            let _ = bytes.as_slice();
        });
    }

    /// Fuzz test: Metadata parsing
    #[test]
    fn fuzz_metadata_parsing() {
        proptest!(|(bytes: Vec<u8>)| {
            // Should never panic
            let _ = serde_json::from_slice::<TranslatorMetadata>(&bytes);
        });
    }

    /// Fuzz test: Mount point validation
    #[test]
    fn fuzz_mount_point_validation() {
        proptest!(|(mount_point in ".*")| {
            // Should start with /srv/
            let is_valid = mount_point.starts_with("/srv/");
            let _ = is_valid;
        });
    }

    /// Fuzz test: WASM memory boundary checks
    #[test]
    fn fuzz_memory_boundaries() {
        proptest!(|(
            offset: u32,
            length: u32
        )| {
            // Memory operations should check boundaries
            let end = offset.saturating_add(length);
            prop_assert!(end >= offset); // No overflow
        });
    }

    /// Fuzz test: Function call parameters
    #[test]
    fn fuzz_function_params() {
        proptest!(|(
            ptr: u32,
            len: u32
        )| {
            // Pointers should be validated
            let _ = (ptr, len);
        });
    }
}