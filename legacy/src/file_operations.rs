//! Unified File Operations - The hierarchy of computational files
//!
//! Normal files: read, write, execute
//! Synthetic files: read, write, execute, compute
//! Translators: read, write, execute, compute, compose

use std::sync::Arc;
use async_trait::async_trait;
use anyhow::Result;
use tokio::sync::RwLock;

/// Core file operations that all files support
#[async_trait]
pub trait FileOperations: Send + Sync {
    /// Read data from file
    async fn read(&self, offset: u64, count: u32) -> Result<Vec<u8>>;

    /// Write data to file
    async fn write(&self, offset: u64, data: &[u8]) -> Result<u32>;

    /// Execute file (for scripts/binaries)
    async fn execute(&self, _args: Vec<String>) -> Result<Vec<u8>> {
        Err(anyhow::anyhow!("Execute not supported"))
    }
}

/// Extended operations for synthetic (computed) files
#[async_trait]
pub trait ComputeOperations: FileOperations {
    /// Compute operation - transform input to output based on file's function
    /// This is triggered by write-then-read pattern
    async fn compute(&self, input: Vec<u8>) -> Result<Vec<u8>>;

    /// Get computation metadata
    async fn computation_info(&self) -> ComputationInfo {
        ComputationInfo::default()
    }
}

/// Composition operations for translator files
#[async_trait]
pub trait ComposeOperations: ComputeOperations {
    /// Compose this translator with another to create a pipeline
    async fn compose(&self, other: Arc<dyn ComposeOperations>) -> Result<Arc<dyn ComposeOperations>>;

    /// Get composition metadata
    async fn composition_info(&self) -> CompositionInfo {
        CompositionInfo::default()
    }

    /// Check if can compose with another translator
    async fn can_compose_with(&self, _other: &CompositionInfo) -> bool {
        true // Default: assume composable
    }
}

/// Information about a computation
#[derive(Clone, Debug, Default)]
pub struct ComputationInfo {
    /// Input type expected
    pub input_type: String,
    /// Output type produced
    pub output_type: String,
    /// Is computation pure (deterministic)?
    pub is_pure: bool,
    /// Estimated computation cost (relative units)
    pub cost: u64,
    /// Can be memoized?
    pub memoizable: bool,
}

/// Information about composition capabilities
#[derive(Clone, Debug, Default)]
pub struct CompositionInfo {
    /// What this accepts as input
    pub accepts: Vec<String>,
    /// What this produces as output
    pub produces: Vec<String>,
    /// Is associative? (a∘(b∘c) = (a∘b)∘c)
    pub associative: bool,
    /// Is commutative? (a∘b = b∘a)
    pub commutative: bool,
    /// Identity element (if any)
    pub identity: Option<String>,
}

/// Normal file implementation
pub struct NormalFile {
    path: String,
    content: Arc<RwLock<Vec<u8>>>,
}

impl NormalFile {
    pub fn new(path: String) -> Self {
        Self {
            path,
            content: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

#[async_trait]
impl FileOperations for NormalFile {
    async fn read(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        let content = self.content.read().await;
        let start = offset.min(content.len() as u64) as usize;
        let end = (start + count as usize).min(content.len());
        Ok(content[start..end].to_vec())
    }

    async fn write(&self, offset: u64, data: &[u8]) -> Result<u32> {
        let mut content = self.content.write().await;
        let start = offset as usize;

        // Extend if necessary
        if start > content.len() {
            content.resize(start, 0);
        }

        // Write data
        if start + data.len() > content.len() {
            content.resize(start + data.len(), 0);
        }
        content[start..start + data.len()].copy_from_slice(data);

        Ok(data.len() as u32)
    }

    async fn execute(&self, _args: Vec<String>) -> Result<Vec<u8>> {
        // Could implement script execution here
        Err(anyhow::anyhow!("Not executable"))
    }
}

/// Synthetic file - computes output from input
pub struct SyntheticFile {
    name: String,
    compute_fn: Arc<dyn Fn(Vec<u8>) -> Vec<u8> + Send + Sync>,
    last_input: Arc<RwLock<Vec<u8>>>,
    cached_output: Arc<RwLock<Option<Vec<u8>>>>,
}

impl SyntheticFile {
    pub fn new<F>(name: String, compute_fn: F) -> Self
    where
        F: Fn(Vec<u8>) -> Vec<u8> + Send + Sync + 'static,
    {
        Self {
            name,
            compute_fn: Arc::new(compute_fn),
            last_input: Arc::new(RwLock::new(Vec::new())),
            cached_output: Arc::new(RwLock::new(None)),
        }
    }
}

#[async_trait]
impl FileOperations for SyntheticFile {
    async fn read(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        // Lazy evaluation - compute if needed
        if self.cached_output.read().await.is_none() {
            let input = self.last_input.read().await.clone();
            let output = (self.compute_fn)(input);
            *self.cached_output.write().await = Some(output);
        }

        let output = self.cached_output.read().await;
        if let Some(ref data) = *output {
            let start = offset.min(data.len() as u64) as usize;
            let end = (start + count as usize).min(data.len());
            Ok(data[start..end].to_vec())
        } else {
            Ok(Vec::new())
        }
    }

    async fn write(&self, _offset: u64, data: &[u8]) -> Result<u32> {
        *self.last_input.write().await = data.to_vec();
        *self.cached_output.write().await = None; // Invalidate cache
        Ok(data.len() as u32)
    }
}

#[async_trait]
impl ComputeOperations for SyntheticFile {
    async fn compute(&self, input: Vec<u8>) -> Result<Vec<u8>> {
        Ok((self.compute_fn)(input))
    }

    async fn computation_info(&self) -> ComputationInfo {
        ComputationInfo {
            input_type: "bytes".to_string(),
            output_type: "bytes".to_string(),
            is_pure: true,
            cost: 1,
            memoizable: true,
        }
    }
}

/// Translator file - can compute and compose
pub struct TranslatorFile {
    name: String,
    transform: Arc<dyn Fn(Vec<u8>) -> Vec<u8> + Send + Sync>,
    input_type: String,
    output_type: String,
}

impl TranslatorFile {
    pub fn new<F>(name: String, transform: F, input_type: String, output_type: String) -> Self
    where
        F: Fn(Vec<u8>) -> Vec<u8> + Send + Sync + 'static,
    {
        Self {
            name,
            transform: Arc::new(transform),
            input_type,
            output_type,
        }
    }
}

#[async_trait]
impl FileOperations for TranslatorFile {
    async fn read(&self, _offset: u64, _count: u32) -> Result<Vec<u8>> {
        // Translators don't store data, they transform it
        Ok(format!("Translator: {} ({} -> {})", self.name, self.input_type, self.output_type).into_bytes())
    }

    async fn write(&self, _offset: u64, data: &[u8]) -> Result<u32> {
        // Writing to a translator means sending data through it
        Ok(data.len() as u32)
    }
}

#[async_trait]
impl ComputeOperations for TranslatorFile {
    async fn compute(&self, input: Vec<u8>) -> Result<Vec<u8>> {
        Ok((self.transform)(input))
    }

    async fn computation_info(&self) -> ComputationInfo {
        ComputationInfo {
            input_type: self.input_type.clone(),
            output_type: self.output_type.clone(),
            is_pure: true,
            cost: 10,
            memoizable: true,
        }
    }
}

#[async_trait]
impl ComposeOperations for TranslatorFile {
    async fn compose(&self, other: Arc<dyn ComposeOperations>) -> Result<Arc<dyn ComposeOperations>> {
        // Create a composed translator
        let other_info = other.composition_info().await;

        // Check type compatibility
        if !other_info.accepts.contains(&self.output_type) {
            return Err(anyhow::anyhow!(
                "Type mismatch: {} produces {}, but next expects {:?}",
                self.name, self.output_type, other_info.accepts
            ));
        }

        // Create composed translator
        let composed = ComposedTranslator {
            first: Arc::new(self.clone()),
            second: other,
            name: format!("{}∘{}", self.name, "other"),
        };

        Ok(Arc::new(composed))
    }

    async fn composition_info(&self) -> CompositionInfo {
        CompositionInfo {
            accepts: vec![self.input_type.clone()],
            produces: vec![self.output_type.clone()],
            associative: false,
            commutative: false,
            identity: None,
        }
    }
}

impl Clone for TranslatorFile {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            transform: self.transform.clone(),
            input_type: self.input_type.clone(),
            output_type: self.output_type.clone(),
        }
    }
}

/// Composed translator - pipeline of transformations
struct ComposedTranslator {
    first: Arc<dyn ComposeOperations>,
    second: Arc<dyn ComposeOperations>,
    name: String,
}

#[async_trait]
impl FileOperations for ComposedTranslator {
    async fn read(&self, _offset: u64, _count: u32) -> Result<Vec<u8>> {
        Ok(format!("Composed: {}", self.name).into_bytes())
    }

    async fn write(&self, _offset: u64, data: &[u8]) -> Result<u32> {
        Ok(data.len() as u32)
    }
}

#[async_trait]
impl ComputeOperations for ComposedTranslator {
    async fn compute(&self, input: Vec<u8>) -> Result<Vec<u8>> {
        let intermediate = self.first.compute(input).await?;
        self.second.compute(intermediate).await
    }
}

#[async_trait]
impl ComposeOperations for ComposedTranslator {
    async fn compose(&self, other: Arc<dyn ComposeOperations>) -> Result<Arc<dyn ComposeOperations>> {
        // Create a three-way composition
        let composed = ComposedTranslator {
            first: Arc::new(ComposedTranslator {
                first: self.first.clone(),
                second: self.second.clone(),
                name: self.name.clone(),
            }),
            second: other,
            name: format!("{}∘other", self.name),
        };
        Ok(Arc::new(composed))
    }

    async fn composition_info(&self) -> CompositionInfo {
        let first_info = self.first.composition_info().await;
        let second_info = self.second.composition_info().await;

        CompositionInfo {
            accepts: first_info.accepts,
            produces: second_info.produces,
            associative: first_info.associative && second_info.associative,
            commutative: false, // Composition is rarely commutative
            identity: None,
        }
    }
}

/// File type detection
pub enum FileType {
    Normal(Arc<dyn FileOperations>),
    Synthetic(Arc<dyn ComputeOperations>),
    Translator(Arc<dyn ComposeOperations>),
}

impl FileType {
    /// Get the appropriate file based on path
    pub fn from_path(path: &str) -> Self {
        if path.starts_with("/trans/") {
            // Translator
            FileType::Translator(Arc::new(TranslatorFile::new(
                path.to_string(),
                |data| data, // Identity for now
                "any".to_string(),
                "any".to_string(),
            )))
        } else if path.starts_with("/sys/") || path.starts_with("/proc/") {
            // Synthetic
            FileType::Synthetic(Arc::new(SyntheticFile::new(
                path.to_string(),
                |data| format!("Computed: {}", String::from_utf8_lossy(&data)).into_bytes(),
            )))
        } else {
            // Normal file
            FileType::Normal(Arc::new(NormalFile::new(path.to_string())))
        }
    }

    /// Execute operation based on file type
    pub async fn execute_operation(&self, op: &str, args: Vec<u8>) -> Result<Vec<u8>> {
        match (self, op) {
            (FileType::Normal(f), "read") => f.read(0, args.len() as u32).await,
            (FileType::Normal(f), "write") => {
                let written = f.write(0, &args).await?;
                Ok(written.to_le_bytes().to_vec())
            }

            (FileType::Synthetic(f), "compute") => f.compute(args).await,

            (FileType::Translator(f), "compute") => f.compute(args).await,
            (FileType::Translator(_f), "compose") => {
                // This would need another translator reference
                Err(anyhow::anyhow!("Compose requires another translator"))
            }

            _ => Err(anyhow::anyhow!("Operation {} not supported for this file type", op)),
        }
    }
}