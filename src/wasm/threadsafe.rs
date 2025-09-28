//! Thread-safe WASM translator system
//!
//! Solves the wasmtime threading issues by running each WASM instance
//! in its own dedicated thread with a message-passing interface.

use anyhow::{Result, Context};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc, oneshot};
use tracing::{info, debug, error};
use wasmtime::{Engine, Module, Store, Instance, Linker};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder};

/// Thread-safe WASM translator that runs in a dedicated thread
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

    /// Run the WASM translator in its own thread
    fn run_translator_thread(
        name: String,
        wasm_bytes: Vec<u8>,
        mut command_rx: mpsc::UnboundedReceiver<TranslatorCommand>,
    ) -> Result<()> {
        // Create WASM runtime in this thread (where it's safe)
        let engine = Engine::default();
        let module = Module::new(&engine, &wasm_bytes)?;

        // Create store without WASI for now (simplified for threading test)
        let mut store = Store::new(&engine, ());
        let mut linker = Linker::new(&engine);

        // Add custom host functions for 9P operations
        linker.func_wrap("ninep", "log", |message: i32| {
            debug!("WASM log: {}", message);
        })?;

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
        store: &mut Store<()>,
        instance: &Instance,
        path: &str,
    ) -> Result<Vec<u8>> {
        // Call the WASM function for reading files
        if let Ok(read_func) = instance.get_typed_func::<(i32, i32), i32>(store, "read_file") {
            // For now, return simple test data
            // In real implementation, this would call the WASM function
            Ok(format!("Data from WASM translator for path: {}", path).into_bytes())
        } else {
            Ok(b"File not found".to_vec())
        }
    }

    /// Handle write file operation
    fn handle_write_file(
        store: &mut Store<()>,
        instance: &Instance,
        path: &str,
        data: Vec<u8>,
    ) -> Result<()> {
        // Call the WASM function for writing files
        if let Ok(write_func) = instance.get_typed_func::<(i32, i32, i32), i32>(store, "write_file") {
            // For now, just log the operation
            // In real implementation, this would call the WASM function
            debug!("Writing {} bytes to {}", data.len(), path);
        }
        Ok(())
    }

    /// Handle list files operation
    fn handle_list_files(
        store: &mut Store<()>,
        instance: &Instance,
        _path: &str,
    ) -> Result<Vec<String>> {
        // Call the WASM function for listing files
        if let Ok(list_func) = instance.get_typed_func::<i32, i32>(store, "list_files") {
            // For now, return test data
            // In real implementation, this would call the WASM function
            Ok(vec![
                "query.sql".to_string(),
                "result.json".to_string(),
                "schema.sql".to_string(),
                "databases.json".to_string(),
            ])
        } else {
            Ok(vec![])
        }
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

    #[tokio::test]
    async fn test_thread_safe_registry() {
        let temp_dir = TempDir::new().unwrap();
        let registry = ThreadSafeTranslatorRegistry::new(temp_dir.path().to_path_buf());

        // Test that registry can be created and is thread-safe
        assert_eq!(registry.list_translators().await.len(), 0);
    }

    #[tokio::test]
    async fn test_translator_creation() {
        // Simple test WASM module (empty for now)
        let wasm_bytes = vec![0x00, 0x61, 0x73, 0x6d]; // WASM magic number

        let result = ThreadSafeTranslator::new(
            "test".to_string(),
            PathBuf::from("/srv/test"),
            wasm_bytes,
        ).await;

        // Note: This will fail with invalid WASM, but tests the threading architecture
        assert!(result.is_err()); // Expected since we're using invalid WASM
    }
}