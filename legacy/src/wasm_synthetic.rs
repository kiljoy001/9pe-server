//! WASM-Created Synthetic Files
//!
//! Users can write WASM modules that create and serve synthetic files,
//! enabling custom dynamic filesystems written in any language

use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use anyhow::{Result, Context};
use async_trait::async_trait;
use wasmtime::*;

use crate::synthetic_advanced::SyntheticFile;
use crate::wasm_composition::WasmComposer;

/// Registry for WASM-created synthetic files
pub struct WasmSyntheticRegistry {
    files: Arc<RwLock<HashMap<String, Arc<WasmSyntheticFile>>>>,
    composer: Arc<WasmComposer>,
}

impl WasmSyntheticRegistry {
    pub fn new(composer: Arc<WasmComposer>) -> Self {
        Self {
            files: Arc::new(RwLock::new(HashMap::new())),
            composer,
        }
    }

    /// Register a synthetic file created by WASM
    pub async fn register_file(
        &self,
        path: String,
        instance_name: String,
        handlers: WasmFileHandlers,
    ) -> Result<()> {
        let file = Arc::new(WasmSyntheticFile {
            path: path.clone(),
            instance_name,
            handlers,
            composer: self.composer.clone(),
            state: Arc::new(RwLock::new(HashMap::new())),
        });

        self.files.write().await.insert(path, file);
        Ok(())
    }

    /// Get a WASM synthetic file
    pub async fn get_file(&self, path: &str) -> Option<Arc<WasmSyntheticFile>> {
        self.files.read().await.get(path).cloned()
    }

    /// List all registered synthetic files
    pub async fn list_files(&self) -> Vec<String> {
        self.files.read().await.keys().cloned().collect()
    }
}

/// Handlers that WASM exports for synthetic file operations
#[derive(Clone)]
pub struct WasmFileHandlers {
    pub on_read: String,   // Function name for read handler
    pub on_write: String,  // Function name for write handler
    pub on_stat: String,   // Function name for stat handler
}

/// A synthetic file backed by WASM functions
pub struct WasmSyntheticFile {
    path: String,
    instance_name: String,
    handlers: WasmFileHandlers,
    composer: Arc<WasmComposer>,
    state: Arc<RwLock<HashMap<String, Vec<u8>>>>, // File-specific state
}

#[async_trait]
impl SyntheticFile for WasmSyntheticFile {
    async fn read(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        // Call WASM read handler
        let input = format!("{}:{}:{}", self.path, offset, count);
        self.composer.execute(
            &self.instance_name,
            &self.handlers.on_read,
            input.as_bytes(),
        ).await
    }

    async fn write(&self, offset: u64, data: &[u8]) -> Result<u32> {
        // Prepare input with offset and data
        let mut input = format!("{}:{}:", self.path, offset).into_bytes();
        input.extend_from_slice(data);

        // Call WASM write handler
        let result = self.composer.execute(
            &self.instance_name,
            &self.handlers.on_write,
            &input,
        ).await?;

        // Parse result as u32
        if result.len() >= 4 {
            let bytes: [u8; 4] = result[..4].try_into()?;
            Ok(u32::from_le_bytes(bytes))
        } else {
            Ok(data.len() as u32)
        }
    }

    async fn size(&self) -> u64 {
        // Call WASM stat handler
        let input = self.path.as_bytes();
        if let Ok(result) = self.composer.execute(
            &self.instance_name,
            &self.handlers.on_stat,
            input,
        ).await {
            if result.len() >= 8 {
                let bytes: [u8; 8] = result[..8].try_into().unwrap_or([0; 8]);
                u64::from_le_bytes(bytes)
            } else {
                0
            }
        } else {
            0
        }
    }

    async fn clone(&self) -> Result<Box<dyn SyntheticFile>> {
        Ok(Box::new(WasmSyntheticFile {
            path: self.path.clone(),
            instance_name: self.instance_name.clone(),
            handlers: self.handlers.clone(),
            composer: self.composer.clone(),
            state: self.state.clone(),
        }))
    }
}

/// Extended WASM composer with synthetic file support
impl WasmComposer {
    /// Add synthetic file host functions to linker
    pub fn add_synthetic_file_functions(
        &self,
        linker: &mut Linker<WasmState>,
        registry: Arc<WasmSyntheticRegistry>,
    ) -> Result<()> {
        let reg = registry.clone();

        // register_synthetic(path_ptr, path_len, read_fn_ptr, read_fn_len, write_fn_ptr, write_fn_len, stat_fn_ptr, stat_fn_len) -> success
        linker.func_wrap(
            "synthetic",
            "register_file",
            move |mut caller: Caller<'_, WasmState>,
                  path_ptr: i32, path_len: i32,
                  read_ptr: i32, read_len: i32,
                  write_ptr: i32, write_len: i32,
                  stat_ptr: i32, stat_len: i32| -> i32 {

                let mem = caller.get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();

                let data = mem.data(&caller);

                // Read strings from memory
                let path = std::str::from_utf8(&data[path_ptr as usize..(path_ptr + path_len) as usize])
                    .unwrap_or("")
                    .to_string();

                let read_fn = std::str::from_utf8(&data[read_ptr as usize..(read_ptr + read_len) as usize])
                    .unwrap_or("synthetic_read")
                    .to_string();

                let write_fn = std::str::from_utf8(&data[write_ptr as usize..(write_ptr + write_len) as usize])
                    .unwrap_or("synthetic_write")
                    .to_string();

                let stat_fn = std::str::from_utf8(&data[stat_ptr as usize..(stat_ptr + stat_len) as usize])
                    .unwrap_or("synthetic_stat")
                    .to_string();

                // Store registration for later execution
                let handlers = WasmFileHandlers {
                    on_read: read_fn,
                    on_write: write_fn,
                    on_stat: stat_fn,
                };

                // Get instance name from caller data
                let instance_name = caller.data().instance_name.clone();

                // Register the file asynchronously (would need runtime handle in real impl)
                caller.data_mut().pending_registrations.push((path, instance_name, handlers));

                1 // Success
            }
        )?;

        // unregister_synthetic(path_ptr, path_len) -> success
        linker.func_wrap(
            "synthetic",
            "unregister_file",
            |mut caller: Caller<'_, WasmState>, path_ptr: i32, path_len: i32| -> i32 {
                let mem = caller.get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();

                let path = {
                    let data = mem.data(&caller);
                    std::str::from_utf8(&data[path_ptr as usize..(path_ptr + path_len) as usize])
                        .unwrap_or("").to_string()
                }; // immutable borrow ends here

                caller.data_mut().pending_unregistrations.push(path);

                1 // Success
            }
        )?;

        // emit_event(event_ptr, event_len) - for synthetic file events
        linker.func_wrap(
            "synthetic",
            "emit_event",
            |mut caller: Caller<'_, WasmState>, event_ptr: i32, event_len: i32| -> i32 {
                let mem = caller.get_export("memory")
                    .and_then(|e| e.into_memory())
                    .unwrap();

                let event = {
                    let data = mem.data(&caller);
                    data[event_ptr as usize..(event_ptr + event_len) as usize].to_vec()
                }; // immutable borrow ends here

                // Store event for processing
                caller.data_mut().events.push(event);

                1
            }
        )?;

        Ok(())
    }
}

/// WASM state with synthetic file support
pub struct WasmState {
    pub instance_name: String,
    pub pending_registrations: Vec<(String, String, WasmFileHandlers)>,
    pub pending_unregistrations: Vec<String>,
    pub events: Vec<Vec<u8>>,
}

/// Example WASM module in Rust that creates synthetic files
pub const EXAMPLE_WASM_SYNTHETIC: &str = r#"
// Example: WASM module that creates synthetic files
// Compile with: cargo build --target wasm32-wasi

use std::collections::HashMap;
use std::sync::Mutex;

// State for our synthetic files
static STATE: Mutex<HashMap<String, Vec<u8>>> = Mutex::new(HashMap::new());

#[no_mangle]
pub extern "C" fn init() {
    // Register synthetic files
    register_synthetic_file("/stats/cpu", "read_cpu", "write_cpu", "stat_cpu");
    register_synthetic_file("/stats/memory", "read_memory", "write_memory", "stat_memory");
    register_synthetic_file("/config.json", "read_config", "write_config", "stat_config");
    register_synthetic_file("/log", "read_log", "append_log", "stat_log");
}

// CPU stats file
#[no_mangle]
pub extern "C" fn read_cpu(path: *const u8, path_len: usize, offset: u64, count: u32) -> *const u8 {
    // Generate CPU stats dynamically
    let cpu_usage = get_cpu_usage();
    let data = format!("{:.2}%\n", cpu_usage);

    // Return requested range
    let bytes = data.as_bytes();
    let start = offset.min(bytes.len() as u64) as usize;
    let end = (start + count as usize).min(bytes.len());

    &bytes[start..end] as *const [u8] as *const u8
}

// Memory stats file
#[no_mangle]
pub extern "C" fn read_memory(path: *const u8, path_len: usize, offset: u64, count: u32) -> *const u8 {
    let mem_info = format!(
        "Total: {} MB\nUsed: {} MB\nFree: {} MB\n",
        get_total_memory(),
        get_used_memory(),
        get_free_memory()
    );

    let bytes = mem_info.as_bytes();
    let start = offset.min(bytes.len() as u64) as usize;
    let end = (start + count as usize).min(bytes.len());

    &bytes[start..end] as *const [u8] as *const u8
}

// Config file - persistent across reads/writes
#[no_mangle]
pub extern "C" fn read_config(path: *const u8, path_len: usize, offset: u64, count: u32) -> *const u8 {
    let mut state = STATE.lock().unwrap();

    let config = state.entry("/config.json".to_string())
        .or_insert_with(|| br"{"debug": false, "level": 1}".to_vec());

    let start = offset.min(config.len() as u64) as usize;
    let end = (start + count as usize).min(config.len());

    &config[start..end] as *const [u8] as *const u8
}

#[no_mangle]
pub extern "C" fn write_config(path: *const u8, path_len: usize, data: *const u8, data_len: usize) -> u32 {
    let mut state = STATE.lock().unwrap();

    let new_data = unsafe {
        std::slice::from_raw_parts(data, data_len).to_vec()
    };

    state.insert("/config.json".to_string(), new_data);
    data_len as u32
}

// External functions we expect the host to provide
extern "C" {
    fn register_synthetic_file(
        path_ptr: *const u8, path_len: usize,
        read_fn: *const u8, read_fn_len: usize,
        write_fn: *const u8, write_fn_len: usize,
        stat_fn: *const u8, stat_fn_len: usize,
    );

    fn get_cpu_usage() -> f32;
    fn get_total_memory() -> u64;
    fn get_used_memory() -> u64;
    fn get_free_memory() -> u64;
}
"#;
/// Example in AssemblyScript (TypeScript-like)
pub const EXAMPLE_ASSEMBLYSCRIPT_SYNTHETIC: &str = r#"
// AssemblyScript example for creating synthetic files
// Compile with: asc synthetic.ts --target release

import { register_file, emit_event } from "./synthetic";

// State storage
const state = new Map<string, Uint8Array>();
const counters = new Map<string, i32>();

// Initialize and register files
export function init(): void {
    register_file("/counter", "read_counter", "write_counter", "stat_counter");
    register_file("/random", "read_random", "write_random", "stat_random");
    register_file("/time", "read_time", "write_time", "stat_time");
}

// Counter file - increments on each read
export function read_counter(path: string, offset: i64, count: i32): Uint8Array {
    let value = counters.has("/counter") ? counters.get("/counter") : 0;
    value++;
    counters.set("/counter", value);

    const data = value.toString();
    return Uint8Array.wrap(String.UTF8.encode(data));
}

export function write_counter(path: string, data: Uint8Array): i32 {
    const value = parseInt(String.UTF8.decode(data.buffer));
    counters.set("/counter", value);
    emit_event("counter_updated");
    return data.length;
}

// Random number generator file
export function read_random(path: string, offset: i64, count: i32): Uint8Array {
    const random = Math.random() * 1000000;
    const data = random.toString();
    return Uint8Array.wrap(String.UTF8.encode(data));
}

// Time file - always returns current timestamp
export function read_time(path: string, offset: i64, count: i32): Uint8Array {
    const timestamp = Date.now().toString();
    return Uint8Array.wrap(String.UTF8.encode(timestamp));
}

// Stat implementations
export function stat_counter(path: string): i64 {
    const value = counters.has("/counter") ? counters.get("/counter") : 0;
    return value.toString().length;
}

export function stat_random(path: string): i64 {
    return 10; // Random numbers are ~10 chars
}

export function stat_time(path: string): i64 {
    return 13; // Unix timestamp in ms
}
"#;

/// Usage examples
pub const USAGE_EXAMPLES: &str = r#"
# WASM-Created Synthetic Files

## 1. Write WASM module that creates synthetic files:
```rust
// my_synthetic.rs
#[no_mangle]
pub extern "C" fn init() {
    register_synthetic_file("/my/data", "on_read", "on_write", "on_stat");
}

#[no_mangle]
pub extern "C" fn on_read(path: &str, offset: u64, count: u32) -> Vec<u8> {
    // Generate data dynamically
    format!("Generated at {}", timestamp()).into_bytes()
}
```

## 2. Compile to WASM:
```bash
cargo build --target wasm32-wasi --release
```

## 3. Load into server:
```bash
cat my_synthetic.wasm > /wasm/modules/synthetic.wasm
echo "synthetic" > /wasm/instances/synth1
```

## 4. Use the synthetic files:
```bash
cat /wasm/synthetic/my/data         # Calls your on_read function
echo "config" > /wasm/synthetic/my/data  # Calls your on_write function
```

## Advanced Examples:

### Dynamic API wrapper:
- WASM creates `/api/users/[id]` synthetic files
- Each read makes HTTP request
- Caches responses

### Computed values:
- `/math/pi/[digits]` - computes Pi to N digits
- `/hash/sha256/[input]` - computes hash on read

### Stateful files:
- `/session/token` - generates new token on each read
- `/metrics/counter` - increments on access

### Reactive files:
- `/watch/config` - updates when config changes
- `/stream/logs` - continuous log stream

## Benefits:
- Any language that compiles to WASM
- Sandboxed execution
- Dynamic file generation
- Stateful or stateless
- Can call external services
- Composes with translators
"#;