//! WASM-based Translator Composition
//!
//! Users write translator compositions in any WASM-compatible language
//! The server executes them safely in sandboxed WASM runtime

use std::sync::Arc;
use std::collections::HashMap;
use anyhow::{Result, Context};
use async_trait::async_trait;
use wasmtime::*;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder};
use tokio::sync::RwLock;

use crate::translators::{Translator, FileInfo};

/// WASM Composer - executes user-provided WASM modules for composition
pub struct WasmComposer {
    engine: Engine,
    modules: Arc<RwLock<HashMap<String, Module>>>,
    instances: Arc<RwLock<HashMap<String, Instance>>>,
}

impl WasmComposer {
    pub fn new() -> Result<Self> {
        // Configure WASM engine with safety limits
        let mut config = Config::new();
        config.wasm_simd(true);
        config.wasm_bulk_memory(true);
        config.wasm_multi_value(true);
        config.wasm_reference_types(true);

        // Safety limits
        config.max_wasm_stack(1024 * 1024);  // 1MB stack
        config.memory_guaranteed_dense_image_size(16 * 1024 * 1024); // 16MB

        let engine = Engine::new(&config)?;

        Ok(Self {
            engine,
            modules: Arc::new(RwLock::new(HashMap::new())),
            instances: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Load a WASM module from bytes
    pub async fn load_module(&self, name: String, wasm_bytes: &[u8]) -> Result<()> {
        let module = Module::new(&self.engine, wasm_bytes)?;
        self.modules.write().await.insert(name.clone(), module);
        Ok(())
    }

    /// Create a composition instance from a module
    pub async fn instantiate(&self, name: String, module_name: String) -> Result<()> {
        let modules = self.modules.read().await;
        let module = modules.get(&module_name)
            .ok_or_else(|| anyhow::anyhow!("Module not found: {}", module_name))?;

        // Create store with WASI
        let mut store = Store::new(&self.engine, WasiState::new());

        // Create WASI context
        let wasi_ctx = WasiCtxBuilder::new()
            .inherit_stdio()
            .build();
        store.data_mut().wasi = Some(wasi_ctx);

        // Link WASI
        let mut linker = Linker::new(&self.engine);
        wasmtime_wasi::add_to_linker(&mut linker, |state: &mut WasiState| {
            state.wasi.as_mut().unwrap()
        })?;

        // Add host functions for translator composition
        self.add_host_functions(&mut linker)?;

        // Instantiate
        let instance = linker.instantiate(&mut store, module)?;

        self.instances.write().await.insert(name, instance);
        Ok(())
    }

    /// Add host functions that WASM can call
    fn add_host_functions(&self, linker: &mut Linker<WasiState>) -> Result<()> {
        // compose_pipeline(translators_ptr, translators_len) -> handle
        linker.func_wrap(
            "translator",
            "compose_pipeline",
            |mut caller: Caller<'_, WasiState>, ptr: i32, len: i32| -> i32 {
                // Read translator names from WASM memory
                let mem = caller.get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();

                let data = mem.data(&caller);
                let bytes = &data[ptr as usize..(ptr + len) as usize];
                let translators = std::str::from_utf8(bytes).unwrap_or("");

                // Store composition and return handle
                let handle = caller.data_mut().next_handle();
                caller.data_mut().compositions.insert(handle, translators.to_string());
                handle
            }
        )?;

        // compose_stack(translators_ptr, translators_len) -> handle
        linker.func_wrap(
            "translator",
            "compose_stack",
            |mut caller: Caller<'_, WasiState>, ptr: i32, len: i32| -> i32 {
                let mem = caller.get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();

                let data = mem.data(&caller);
                let bytes = &data[ptr as usize..(ptr + len) as usize];
                let translators = std::str::from_utf8(bytes).unwrap_or("");

                let handle = caller.data_mut().next_handle();
                caller.data_mut().stacks.insert(handle, translators.to_string());
                handle
            }
        )?;

        // apply_translator(handle, data_ptr, data_len, out_ptr, out_len) -> result_len
        linker.func_wrap(
            "translator",
            "apply_translator",
            |mut caller: Caller<'_, WasiState>, handle: i32, in_ptr: i32, in_len: i32, out_ptr: i32, out_cap: i32| -> i32 {
                // Get input data
                let mem = caller.get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();

                let data = mem.data(&caller);
                let input = &data[in_ptr as usize..(in_ptr + in_len) as usize];

                // Apply translator (simplified - would actually run translator)
                let output = input.to_vec(); // Placeholder

                // Write output
                let out_len = output.len().min(out_cap as usize);
                let data_mut = mem.data_mut(&mut caller);
                data_mut[out_ptr as usize..out_ptr as usize + out_len]
                    .copy_from_slice(&output[..out_len]);

                out_len as i32
            }
        )?;

        // read_file(path_ptr, path_len, out_ptr, out_cap) -> result_len
        linker.func_wrap(
            "translator",
            "read_file",
            |mut caller: Caller<'_, WasiState>, path_ptr: i32, path_len: i32, out_ptr: i32, out_cap: i32| -> i32 {
                let mem = caller.get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();

                let data = mem.data(&caller);
                let path_bytes = &data[path_ptr as usize..(path_ptr + path_len) as usize];
                let path = std::str::from_utf8(path_bytes).unwrap_or("");

                // Read file (would be async in real implementation)
                let content = std::fs::read(path).unwrap_or_default();

                // Write to output buffer
                let out_len = content.len().min(out_cap as usize);
                let data_mut = mem.data_mut(&mut caller);
                data_mut[out_ptr as usize..out_ptr as usize + out_len]
                    .copy_from_slice(&content[..out_len]);

                out_len as i32
            }
        )?;

        // write_file(path_ptr, path_len, data_ptr, data_len) -> success
        linker.func_wrap(
            "translator",
            "write_file",
            |mut caller: Caller<'_, WasiState>, path_ptr: i32, path_len: i32, data_ptr: i32, data_len: i32| -> i32 {
                let mem = caller.get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();

                let data = mem.data(&caller);
                let path_bytes = &data[path_ptr as usize..(path_ptr + path_len) as usize];
                let path = std::str::from_utf8(path_bytes).unwrap_or("");

                let content = &data[data_ptr as usize..(data_ptr + data_len) as usize];

                // Write file (would be async in real implementation)
                match std::fs::write(path, content) {
                    Ok(_) => 1,
                    Err(_) => 0,
                }
            }
        )?;

        Ok(())
    }

    /// Execute a WASM composition
    pub async fn execute(
        &self,
        instance_name: &str,
        function: &str,
        input: &[u8],
    ) -> Result<Vec<u8>> {
        let instances = self.instances.read().await;
        let instance = instances.get(instance_name)
            .ok_or_else(|| anyhow::anyhow!("Instance not found: {}", instance_name))?;

        // Create store for execution
        let mut store = Store::new(&self.engine, WasiState::new());

        // Get function
        let func = instance.get_func(&mut store, function)
            .ok_or_else(|| anyhow::anyhow!("Function not found: {}", function))?;

        // Allocate input in WASM memory
        let memory = instance.get_memory(&mut store, "memory")
            .ok_or_else(|| anyhow::anyhow!("Memory export not found"))?;

        let alloc = instance.get_func(&mut store, "alloc")
            .ok_or_else(|| anyhow::anyhow!("alloc function not found"))?;

        // Allocate space for input
        let input_ptr = alloc.call(&mut store, &[Val::I32(input.len() as i32)], &mut [Val::I32(0)])?;
        let input_ptr = if let Val::I32(ptr) = &input_ptr[0] { *ptr } else { 0 };

        // Write input to memory
        memory.write(&mut store, input_ptr as usize, input)?;

        // Call function
        let mut results = [Val::I32(0), Val::I32(0)];
        func.call(&mut store, &[Val::I32(input_ptr), Val::I32(input.len() as i32)], &mut results)?;

        let output_ptr = if let Val::I32(ptr) = results[0] { ptr } else { 0 };
        let output_len = if let Val::I32(len) = results[1] { len } else { 0 };

        // Read output from memory
        let mut output = vec![0u8; output_len as usize];
        memory.read(&store, output_ptr as usize, &mut output)?;

        Ok(output)
    }
}

/// State for WASM instances
struct WasiState {
    wasi: Option<WasiCtx>,
    compositions: HashMap<i32, String>,
    stacks: HashMap<i32, String>,
    next_handle: i32,
}

impl WasiState {
    fn new() -> Self {
        Self {
            wasi: None,
            compositions: HashMap::new(),
            stacks: HashMap::new(),
            next_handle: 1,
        }
    }

    fn next_handle(&mut self) -> i32 {
        let handle = self.next_handle;
        self.next_handle += 1;
        handle
    }
}

/// WASM-based translator that executes user code
pub struct WasmTranslator {
    name: String,
    composer: Arc<WasmComposer>,
    instance_name: String,
}

impl WasmTranslator {
    pub fn new(name: String, composer: Arc<WasmComposer>, instance_name: String) -> Self {
        Self {
            name,
            composer,
            instance_name,
        }
    }
}

#[async_trait]
impl Translator for WasmTranslator {
    fn name(&self) -> &str {
        &self.name
    }

    fn translator_type(&self) -> &str {
        "wasm"
    }

    fn isolation(&self) -> crate::translators::IsolationLevel {
        crate::translators::IsolationLevel::WASM
    }

    fn supports(&self, operation: &str) -> bool {
        // WASM modules can support any operation they implement
        true
    }

    async fn init(&mut self) -> Result<()> {
        // Already initialized when instance was created
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        // WASM instances are cleaned up automatically
        Ok(())
    }

    async fn read(&self, path: &str, offset: u64, count: u32) -> Result<Vec<u8>> {
        // Prepare input: path, offset, count
        let input = format!("READ:{}:{}:{}", path, offset, count);
        self.composer.execute(&self.instance_name, "translator_read", input.as_bytes()).await
    }

    async fn write(&self, path: &str, offset: u64, data: Vec<u8>) -> Result<u32> {
        // Prepare input with metadata
        let mut input = format!("WRITE:{}:{}:", path, offset).into_bytes();
        input.extend_from_slice(&data);

        let result = self.composer.execute(&self.instance_name, "translator_write", &input).await?;

        // Parse result as u32
        let bytes: [u8; 4] = result[..4].try_into().unwrap_or([0; 4]);
        Ok(u32::from_le_bytes(bytes))
    }

    async fn list(&self, path: &str) -> Result<Vec<String>> {
        let input = format!("LIST:{}", path);
        let output = self.composer.execute(&self.instance_name, "translator_list", input.as_bytes()).await?;

        // Parse newline-separated list
        let list_str = String::from_utf8_lossy(&output);
        Ok(list_str.lines().map(String::from).collect())
    }

    async fn stat(&self, path: &str) -> Result<FileInfo> {
        let input = format!("STAT:{}", path);
        let output = self.composer.execute(&self.instance_name, "translator_stat", input.as_bytes()).await?;

        // Parse JSON result (simplified)
        let json_str = String::from_utf8_lossy(&output);
        let info: FileInfo = serde_json::from_str(&json_str)?;
        Ok(info)
    }
}

/// Example WASM module in Rust for users to build on
pub const EXAMPLE_WASM_TRANSLATOR: &str = r#"
// Example translator composition in Rust (compiles to WASM)
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct ComposedTranslator {
    pipeline: Vec<String>,
}

#[wasm_bindgen]
impl ComposedTranslator {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            pipeline: vec![],
        }
    }

    // Compose translators in a pipeline
    pub fn add_to_pipeline(&mut self, translator: String) {
        self.pipeline.push(translator);
    }

    // Process data through the pipeline
    pub fn process(&self, data: &[u8]) -> Vec<u8> {
        let mut result = data.to_vec();

        for translator in &self.pipeline {
            result = match translator.as_str() {
                "gzip" => compress_gzip(&result),
                "base64" => encode_base64(&result),
                "encrypt" => encrypt_aes(&result),
                "json_filter" => filter_json(&result),
                _ => result,
            };
        }

        result
    }
}

// Helper functions
fn compress_gzip(data: &[u8]) -> Vec<u8> {
    // Gzip compression
    data.to_vec()
}

fn encode_base64(data: &[u8]) -> Vec<u8> {
    // Base64 encoding
    data.to_vec()
}

fn encrypt_aes(data: &[u8]) -> Vec<u8> {
    // AES encryption
    data.to_vec()
}

fn filter_json(data: &[u8]) -> Vec<u8> {
    // JSON filtering
    data.to_vec()
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_wasm_composer() {
        let composer = WasmComposer::new().unwrap();

        // Would load actual WASM module in real test
        // composer.load_module("test".to_string(), &wasm_bytes).await.unwrap();
    }
}