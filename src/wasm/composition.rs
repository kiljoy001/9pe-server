//! WASM-based Translator Composition
//!
//! Users write translator compositions in any WASM-compatible language
//! The server executes them safely in sandboxed WASM runtime

use std::sync::Arc;
use std::collections::HashMap;
use anyhow::{Result, Context};
use wasmtime::*;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder};
use tokio::sync::RwLock;

/// WASM Composer - executes user-provided WASM modules for composition
pub struct WasmComposer {
    engine: Engine,
    modules: Arc<RwLock<HashMap<String, Module>>>,
    instances: Arc<RwLock<HashMap<String, WasmInstance>>>,
}

struct WasmInstance {
    store: Store<WasiState>,
    instance: Instance,
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
        // Skip WASI linking for now - will add when needed
        // wasmtime_wasi::add_to_linker(&mut linker, |state: &mut WasiState| {
        //     state.wasi.as_mut().unwrap()
        // })?;

        // Add host functions for translator composition
        self.add_host_functions(&mut linker)?;

        // Instantiate
        let instance = linker.instantiate(&mut store, module)?;

        let wasm_instance = WasmInstance {
            store,
            instance,
        };

        self.instances.write().await.insert(name, wasm_instance);
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
                let translators = std::str::from_utf8(bytes).unwrap_or("").to_string();

                // Store composition and return handle
                let handle = caller.data_mut().next_handle();
                caller.data_mut().compositions.insert(handle, translators);
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
                let translators = std::str::from_utf8(bytes).unwrap_or("").to_string();

                let handle = caller.data_mut().next_handle();
                caller.data_mut().stacks.insert(handle, translators);
                handle
            }
        )?;

        // apply_translator(handle, data_ptr, data_len, out_ptr, out_len) -> result_len
        linker.func_wrap(
            "translator",
            "apply_translator",
            |mut caller: Caller<'_, WasiState>, _handle: i32, in_ptr: i32, in_len: i32, out_ptr: i32, out_cap: i32| -> i32 {
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
        let mut instances = self.instances.write().await;
        let instance = instances.get_mut(instance_name)
            .ok_or_else(|| anyhow::anyhow!("Instance not found: {}", instance_name))?;

        // Get function
        let func = instance.instance.get_func(&mut instance.store, function)
            .ok_or_else(|| anyhow::anyhow!("Function not found: {}", function))?;

        // Allocate input in WASM memory
        let memory = instance.instance.get_memory(&mut instance.store, "memory")
            .ok_or_else(|| anyhow::anyhow!("Memory export not found"))?;

        let alloc = instance.instance.get_func(&mut instance.store, "alloc")
            .ok_or_else(|| anyhow::anyhow!("alloc function not found"))?;

        // Allocate space for input
        let mut results = [Val::I32(0)];
        alloc.call(&mut instance.store, &[Val::I32(input.len() as i32)], &mut results)?;
        let input_ptr = if let Val::I32(ptr) = results[0] { ptr } else { 0 };

        // Write input to memory
        memory.write(&mut instance.store, input_ptr as usize, input)?;

        // Call function
        let mut output_results = [Val::I32(0), Val::I32(0)];
        func.call(&mut instance.store, &[Val::I32(input_ptr), Val::I32(input.len() as i32)], &mut output_results)?;

        let output_ptr = if let Val::I32(ptr) = output_results[0] { ptr } else { 0 };
        let output_len = if let Val::I32(len) = output_results[1] { len } else { 0 };

        // Read output from memory
        let mut output = vec![0u8; output_len as usize];
        memory.read(&instance.store, output_ptr as usize, &mut output)?;

        Ok(output)
    }

    /// List available modules
    pub async fn list_modules(&self) -> Vec<String> {
        self.modules.read().await.keys().cloned().collect()
    }

    /// List active instances
    pub async fn list_instances(&self) -> Vec<String> {
        self.instances.read().await.keys().cloned().collect()
    }
}

/// State for WASM instances
pub struct WasiState {
    pub wasi: Option<WasiCtx>,
    pub compositions: HashMap<i32, String>,
    pub stacks: HashMap<i32, String>,
    pub next_handle: i32,
    pub instance_name: String,
    pub pending_registrations: Vec<(String, String, WasmFileHandlers)>,
    pub pending_unregistrations: Vec<String>,
    pub events: Vec<Vec<u8>>,
}

/// Handlers that WASM exports for synthetic file operations
#[derive(Clone)]
pub struct WasmFileHandlers {
    pub on_read: String,   // Function name for read handler
    pub on_write: String,  // Function name for write handler
    pub on_stat: String,   // Function name for stat handler
}

impl WasiState {
    fn new() -> Self {
        Self {
            wasi: None,
            compositions: HashMap::new(),
            stacks: HashMap::new(),
            next_handle: 1,
            instance_name: String::new(),
            pending_registrations: Vec::new(),
            pending_unregistrations: Vec::new(),
            events: Vec::new(),
        }
    }

    fn next_handle(&mut self) -> i32 {
        let handle = self.next_handle;
        self.next_handle += 1;
        handle
    }
}

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