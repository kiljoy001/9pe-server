//! Function Files - Every file is a function
//!
//! This implements the paradigm: "Everything is a file, and every file is a function"
//! Files become lazily-evaluated functions that transform input to output

use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;

/// A file that acts as a function
#[async_trait]
pub trait FunctionFile: Send + Sync {
    /// Apply the function: read triggers evaluation
    /// The "input" is what was last written, "output" is what you read
    async fn apply(&self, input: Vec<u8>) -> Result<Vec<u8>>;

    /// Get function signature/type (optional metadata)
    async fn signature(&self) -> String {
        "Any -> Any".to_string()
    }

    /// Can this function be composed with others?
    fn is_composable(&self) -> bool {
        true
    }
}

/// Identity function implementation
pub struct IdentityFunction;

impl IdentityFunction {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl FunctionFile for IdentityFunction {
    async fn apply(&self, input: Vec<u8>) -> Result<Vec<u8>> {
        Ok(input)
    }

    async fn signature(&self) -> String {
        "Vec<u8> -> Vec<u8> (identity)".to_string()
    }

    fn is_composable(&self) -> bool {
        true
    }
}

/// Base64 encode function
pub struct Base64EncodeFunction;

impl Base64EncodeFunction {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl FunctionFile for Base64EncodeFunction {
    async fn apply(&self, input: Vec<u8>) -> Result<Vec<u8>> {
        use base64::{Engine as _, engine::general_purpose};
        let encoded = general_purpose::STANDARD.encode(input);
        Ok(encoded.into_bytes())
    }

    async fn signature(&self) -> String {
        "Vec<u8> -> Vec<u8> (base64)".to_string()
    }

    fn is_composable(&self) -> bool {
        true
    }
}

/// Function file manager
pub struct FunctionFileManager {
    functions: HashMap<String, Arc<dyn FunctionFile>>,
}

impl FunctionFileManager {
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
        }
    }

    pub fn register(&mut self, name: String, function: Arc<dyn FunctionFile>) {
        self.functions.insert(name, function);
    }

    pub fn get_function(&self, name: &str) -> Option<Arc<dyn FunctionFile>> {
        self.functions.get(name).cloned()
    }

    pub fn list_functions(&self) -> Vec<String> {
        self.functions.keys().cloned().collect()
    }
}

/// Function file instance - maintains state between write and read
pub struct FunctionFileInstance {
    /// The function implementation
    function: Arc<dyn FunctionFile>,
    /// Current input (what was written)
    input: Arc<RwLock<Vec<u8>>>,
    /// Cached output (lazy evaluation)
    output: Arc<RwLock<Option<Vec<u8>>>>,
    /// Is the output stale?
    dirty: Arc<RwLock<bool>>,
}

impl FunctionFileInstance {
    pub fn new(function: Arc<dyn FunctionFile>) -> Self {
        Self {
            function,
            input: Arc::new(RwLock::new(Vec::new())),
            output: Arc::new(RwLock::new(None)),
            dirty: Arc::new(RwLock::new(true)),
        }
    }

    /// Write = Set function input
    pub async fn write(&self, data: Vec<u8>) -> Result<u32> {
        let len = data.len();
        *self.input.write().await = data;
        *self.dirty.write().await = true;
        *self.output.write().await = None; // Invalidate cache
        Ok(len as u32)
    }

    /// Read = Get function output (lazy evaluation)
    pub async fn read(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        // Check if we need to recompute
        if *self.dirty.read().await {
            let input = self.input.read().await.clone();
            let result = self.function.apply(input).await?;
            *self.output.write().await = Some(result);
            *self.dirty.write().await = false;
        }

        // Return the requested portion
        let output = self.output.read().await;
        if let Some(ref data) = *output {
            let start = offset.min(data.len() as u64) as usize;
            let end = (start + count as usize).min(data.len());
            Ok(data[start..end].to_vec())
        } else {
            Ok(Vec::new())
        }
    }
}

/// Basic function file implementations

/// Uppercase function - converts text to uppercase
pub struct UppercaseFunction;

#[async_trait]
impl FunctionFile for UppercaseFunction {
    async fn apply(&self, input: Vec<u8>) -> Result<Vec<u8>> {
        let text = String::from_utf8_lossy(&input);
        Ok(text.to_uppercase().into_bytes())
    }

    async fn signature(&self) -> String {
        "String -> String".to_string()
    }
}

/// Hash function (SHA-256)
pub struct HashFunction;

#[async_trait]
impl FunctionFile for HashFunction {
    async fn apply(&self, input: Vec<u8>) -> Result<Vec<u8>> {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(input);
        let result = hasher.finalize();
        Ok(format!("{:x}", result).into_bytes())
    }

    async fn signature(&self) -> String {
        "Bytes -> SHA256Hash".to_string()
    }
}

/// Function composition - pipe output of one function to another
pub struct ComposedFunction {
    first: Arc<dyn FunctionFile>,
    second: Arc<dyn FunctionFile>,
}

impl ComposedFunction {
    pub fn new(first: Arc<dyn FunctionFile>, second: Arc<dyn FunctionFile>) -> Self {
        Self { first, second }
    }
}

#[async_trait]
impl FunctionFile for ComposedFunction {
    async fn apply(&self, input: Vec<u8>) -> Result<Vec<u8>> {
        let intermediate = self.first.apply(input).await?;
        self.second.apply(intermediate).await
    }

    async fn signature(&self) -> String {
        format!("({}) ∘ ({})", self.first.signature().await, self.second.signature().await)
    }
}

/// Map function - apply function to each line
pub struct MapFunction {
    function: Arc<dyn FunctionFile>,
}

impl MapFunction {
    pub fn new(function: Arc<dyn FunctionFile>) -> Self {
        Self { function }
    }
}

#[async_trait]
impl FunctionFile for MapFunction {
    async fn apply(&self, input: Vec<u8>) -> Result<Vec<u8>> {
        let text = String::from_utf8_lossy(&input);
        let mut results = Vec::new();

        for line in text.lines() {
            let line_result = self.function.apply(line.as_bytes().to_vec()).await?;
            results.extend_from_slice(&line_result);
            results.push(b'\n');
        }

        Ok(results)
    }

    async fn signature(&self) -> String {
        format!("List<a> -> List<{}>", self.function.signature().await)
    }
}

/// Filter function - only output lines matching predicate
pub struct FilterFunction {
    predicate: Arc<dyn FunctionFile>,
}

impl FilterFunction {
    pub fn new(predicate: Arc<dyn FunctionFile>) -> Self {
        Self { predicate }
    }
}

#[async_trait]
impl FunctionFile for FilterFunction {
    async fn apply(&self, input: Vec<u8>) -> Result<Vec<u8>> {
        let text = String::from_utf8_lossy(&input);
        let mut results = Vec::new();

        for line in text.lines() {
            let pred_result = self.predicate.apply(line.as_bytes().to_vec()).await?;
            // If predicate returns "true", include the line
            if pred_result == b"true" {
                results.extend_from_slice(line.as_bytes());
                results.push(b'\n');
            }
        }

        Ok(results)
    }

    async fn signature(&self) -> String {
        "List<a> -> List<a> (filtered)".to_string()
    }
}

/// Reduce/fold function
pub struct ReduceFunction {
    function: Arc<dyn FunctionFile>,
    initial: Vec<u8>,
}

impl ReduceFunction {
    pub fn new(function: Arc<dyn FunctionFile>, initial: Vec<u8>) -> Self {
        Self { function, initial }
    }
}

#[async_trait]
impl FunctionFile for ReduceFunction {
    async fn apply(&self, input: Vec<u8>) -> Result<Vec<u8>> {
        let text = String::from_utf8_lossy(&input);
        let mut accumulator = self.initial.clone();

        for line in text.lines() {
            // Combine accumulator with current line
            let combined = [accumulator, b"\n".to_vec(), line.as_bytes().to_vec()].concat();
            accumulator = self.function.apply(combined).await?;
        }

        Ok(accumulator)
    }

    async fn signature(&self) -> String {
        format!("List<a> -> b (via {})", self.function.signature().await)
    }
}

/// Cache function results
pub struct MemoizedFunction {
    function: Arc<dyn FunctionFile>,
    cache: Arc<RwLock<HashMap<Vec<u8>, Vec<u8>>>>,
}

impl MemoizedFunction {
    pub fn new(function: Arc<dyn FunctionFile>) -> Self {
        Self {
            function,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl FunctionFile for MemoizedFunction {
    async fn apply(&self, input: Vec<u8>) -> Result<Vec<u8>> {
        // Check cache
        if let Some(cached) = self.cache.read().await.get(&input) {
            return Ok(cached.clone());
        }

        // Compute and cache
        let result = self.function.apply(input.clone()).await?;
        self.cache.write().await.insert(input, result.clone());
        Ok(result)
    }

    async fn signature(&self) -> String {
        format!("{} (memoized)", self.function.signature().await)
    }
}

/// Function registry - manages all function files
pub struct FunctionRegistry {
    functions: Arc<RwLock<HashMap<String, Arc<dyn FunctionFile>>>>,
}

impl FunctionRegistry {
    pub fn new() -> Self {
        Self {
            functions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register built-in functions
    pub async fn register_builtins(&self) {
        let mut funcs = self.functions.write().await;

        // Basic functions
        funcs.insert("identity".to_string(), Arc::new(IdentityFunction));
        funcs.insert("uppercase".to_string(), Arc::new(UppercaseFunction));
        funcs.insert("base64".to_string(), Arc::new(Base64EncodeFunction));
        funcs.insert("sha256".to_string(), Arc::new(HashFunction));
    }

    /// Create a function file at a path
    pub async fn create_function(&self, path: String, func_name: String) -> Result<()> {
        let functions = self.functions.read().await;
        if let Some(_func) = functions.get(&func_name) {
            // Here we'd integrate with the filesystem to create the function file
            tracing::info!("Created function file at {} with function {}", path, func_name);
        } else {
            return Err(anyhow::anyhow!("Unknown function: {}", func_name));
        }
        Ok(())
    }

    /// Compose two functions
    pub async fn compose(&self, first: String, second: String) -> Result<Arc<dyn FunctionFile>> {
        let functions = self.functions.read().await;
        let f1 = functions.get(&first)
            .ok_or_else(|| anyhow::anyhow!("Unknown function: {}", first))?;
        let f2 = functions.get(&second)
            .ok_or_else(|| anyhow::anyhow!("Unknown function: {}", second))?;

        Ok(Arc::new(ComposedFunction::new(f1.clone(), f2.clone())))
    }
}

/// Special directory: /func/ - where function files live
///
/// /func/
///   builtin/
///     uppercase    - Built-in uppercase function
///     base64       - Built-in base64 encoder
///     sha256       - Built-in hash function
///   user/
///     my_func.wasm - User-defined WASM function
///   compose/
///     pipeline     - Write "func1|func2|func3" to create pipeline
///   apply/
///     input        - Write here
///     output       - Read result here

pub const FUNCTION_FILE_EXAMPLES: &str = r#"
# Function Files - Everything is a Function

## Basic Usage
echo "hello world" > /func/uppercase
cat /func/uppercase
# Output: HELLO WORLD

## Function Composition
echo "uppercase|base64" > /func/compose/pipeline
echo "hello" > /func/compose/pipeline/input
cat /func/compose/pipeline/output
# Output: SEVMTE8=

## Map over lines
echo "line1\nline2\nline3" > /func/map/uppercase
cat /func/map/uppercase
# Output: LINE1\nLINE2\nLINE3

## Custom WASM functions
cp my_function.wasm /func/install/
echo "data" > /func/user/my_function
cat /func/user/my_function

## Lazy evaluation
# Functions only compute when read, not when written
# This allows building complex pipelines efficiently
"#;