//! Composable and Stackable Translators
//!
//! Allows translators to be composed together and stacked like Unix pipes

use std::collections::HashMap;
use std::sync::Arc;
use std::path::PathBuf;
use tokio::sync::RwLock;
use anyhow::{Result, Context};
use async_trait::async_trait;
use serde::{Serialize, Deserialize};

use crate::translators::{Translator, FileInfo};
use crate::synthetic_advanced::SyntheticFile;

/// A pipeline of translators that process data sequentially
pub struct TranslatorPipeline {
    name: String,
    stages: Vec<Arc<dyn Translator>>,
}

impl TranslatorPipeline {
    pub fn new(name: String) -> Self {
        Self {
            name,
            stages: Vec::new(),
        }
    }

    /// Add a translator to the pipeline
    pub fn add_stage(&mut self, translator: Arc<dyn Translator>) {
        self.stages.push(translator);
    }

    /// Create a pipeline from a specification
    pub fn from_spec(spec: &str) -> Result<Self> {
        // Parse spec like "http | json | filter:$.users | csv"
        let mut pipeline = Self::new(spec.to_string());

        for stage in spec.split('|').map(|s| s.trim()) {
            // Would create translators based on spec
            // pipeline.add_stage(create_translator(stage)?);
        }

        Ok(pipeline)
    }
}

#[async_trait]
impl Translator for TranslatorPipeline {
    fn name(&self) -> &str { &self.name }
    fn translator_type(&self) -> &str { "pipeline" }
    fn isolation(&self) -> crate::translators::IsolationLevel {
        // Use most restrictive isolation from stages
        crate::translators::IsolationLevel::Process
    }

    fn supports(&self, operation: &str) -> bool {
        // Pipeline supports operation if all stages do
        self.stages.iter().all(|t| t.supports(operation))
    }

    async fn init(&mut self) -> Result<()> {
        for stage in &mut self.stages {
            Arc::get_mut(stage).unwrap().init().await?;
        }
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        for stage in &mut self.stages {
            Arc::get_mut(stage).unwrap().shutdown().await?;
        }
        Ok(())
    }

    async fn read(&self, path: &str, offset: u64, count: u32) -> Result<Vec<u8>> {
        let mut data = Vec::new();

        // First stage reads from source
        if let Some(first) = self.stages.first() {
            data = first.read(path, offset, count).await?;
        }

        // Pass through remaining stages
        for stage in self.stages.iter().skip(1) {
            // Each stage transforms the data
            // For now, treat data as a "file" at path "data"
            data = stage.read("data", 0, data.len() as u32).await?;
        }

        Ok(data)
    }

    async fn write(&self, path: &str, offset: u64, data: Vec<u8>) -> Result<u32> {
        let mut current_data = data;

        // Pass through stages in reverse for writes
        for stage in self.stages.iter().rev().skip(1) {
            let result = stage.write("data", 0, current_data).await?;
            // Get transformed data for next stage
            current_data = stage.read("data", 0, result).await?;
        }

        // Last stage writes to destination
        if let Some(last) = self.stages.last() {
            last.write(path, offset, current_data).await
        } else {
            Ok(0)
        }
    }

    async fn list(&self, path: &str) -> Result<Vec<String>> {
        // List from first stage, potentially filtered by others
        if let Some(first) = self.stages.first() {
            let mut items = first.list(path).await?;

            // Let other stages filter the list
            for stage in self.stages.iter().skip(1) {
                // Stages can filter items
                items = stage.list(&items.join("\n")).await?;
            }

            Ok(items)
        } else {
            Ok(vec![])
        }
    }

    async fn stat(&self, path: &str) -> Result<FileInfo> {
        // Stat from first stage
        if let Some(first) = self.stages.first() {
            first.stat(path).await
        } else {
            Err(anyhow::anyhow!("Empty pipeline"))
        }
    }
}

/// Composable translator using synthetic files
pub struct ComposableTranslator {
    name: String,
    config_file: Arc<RwLock<String>>,  // Synthetic file for configuration
    script_file: Arc<RwLock<Vec<u8>>>, // Synthetic file for transformation script
    cache: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl ComposableTranslator {
    pub fn new(name: String) -> Self {
        Self {
            name,
            config_file: Arc::new(RwLock::new(String::new())),
            script_file: Arc::new(RwLock::new(Vec::new())),
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create synthetic control files for this translator
    pub async fn create_control_files(&self) -> Vec<(String, Arc<dyn SyntheticFile>)> {
        let mut files = Vec::new();

        // Config file - users write JSON config here
        let config_clone = self.config_file.clone();
        files.push((
            format!("/trans/{}/config", self.name),
            Arc::new(TranslatorConfigFile { data: config_clone }) as Arc<dyn SyntheticFile>
        ));

        // Script file - users write transformation script
        let script_clone = self.script_file.clone();
        files.push((
            format!("/trans/{}/script", self.name),
            Arc::new(TranslatorScriptFile { data: script_clone }) as Arc<dyn SyntheticFile>
        ));

        // Status file - read-only status information
        files.push((
            format!("/trans/{}/status", self.name),
            Arc::new(TranslatorStatusFile {
                name: self.name.clone(),
                cache: self.cache.clone()
            }) as Arc<dyn SyntheticFile>
        ));

        files
    }

    /// Apply user-defined transformation
    async fn apply_script(&self, data: Vec<u8>) -> Result<Vec<u8>> {
        let script = self.script_file.read().await;

        if script.is_empty() {
            return Ok(data); // Pass through if no script
        }

        // Parse script (simple example - would use real scripting)
        let script_str = String::from_utf8_lossy(&script);

        if script_str.contains("uppercase") {
            Ok(String::from_utf8_lossy(&data).to_uppercase().into_bytes())
        } else if script_str.contains("reverse") {
            let mut result = data;
            result.reverse();
            Ok(result)
        } else if script_str.starts_with("filter:") {
            // Simple line filter
            let pattern = &script_str[7..].trim();
            let text = String::from_utf8_lossy(&data);
            let filtered: Vec<String> = text.lines()
                .filter(|line| line.contains(pattern))
                .map(String::from)
                .collect();
            Ok(filtered.join("\n").into_bytes())
        } else {
            Ok(data)
        }
    }
}

#[async_trait]
impl Translator for ComposableTranslator {
    fn name(&self) -> &str { &self.name }
    fn translator_type(&self) -> &str { "composable" }
    fn isolation(&self) -> crate::translators::IsolationLevel {
        crate::translators::IsolationLevel::Process
    }

    fn supports(&self, operation: &str) -> bool {
        true // Composable translators are flexible
    }

    async fn init(&mut self) -> Result<()> {
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        self.cache.write().await.clear();
        Ok(())
    }

    async fn read(&self, path: &str, offset: u64, count: u32) -> Result<Vec<u8>> {
        // Check cache
        if let Some(cached) = self.cache.read().await.get(path) {
            let start = offset.min(cached.len() as u64) as usize;
            let end = (start + count as usize).min(cached.len());
            return Ok(cached[start..end].to_vec());
        }

        // Generate data based on configuration
        let config = self.config_file.read().await;
        let data = format!("Generated from {}: {}", self.name, config).into_bytes();

        // Apply script transformation
        let transformed = self.apply_script(data).await?;

        // Cache result
        self.cache.write().await.insert(path.to_string(), transformed.clone());

        // Return requested range
        let start = offset.min(transformed.len() as u64) as usize;
        let end = (start + count as usize).min(transformed.len());
        Ok(transformed[start..end].to_vec())
    }

    async fn write(&self, path: &str, _offset: u64, data: Vec<u8>) -> Result<u32> {
        // Apply script transformation to incoming data
        let transformed = self.apply_script(data).await?;

        // Store in cache
        self.cache.write().await.insert(path.to_string(), transformed.clone());

        Ok(transformed.len() as u32)
    }

    async fn list(&self, _path: &str) -> Result<Vec<String>> {
        // List cached paths
        Ok(self.cache.read().await.keys().cloned().collect())
    }

    async fn stat(&self, path: &str) -> Result<FileInfo> {
        let cache = self.cache.read().await;
        let size = cache.get(path).map(|d| d.len() as u64).unwrap_or(0);

        Ok(FileInfo {
            name: path.split('/').last().unwrap_or("").to_string(),
            size,
            is_dir: false,
            modified: 0,
            permissions: 0o644,
        })
    }
}

/// Synthetic file for translator configuration
struct TranslatorConfigFile {
    data: Arc<RwLock<String>>,
}

#[async_trait]
impl SyntheticFile for TranslatorConfigFile {
    async fn read(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        let data = self.data.read().await;
        let bytes = data.as_bytes();
        let start = offset.min(bytes.len() as u64) as usize;
        let end = (start + count as usize).min(bytes.len());
        Ok(bytes[start..end].to_vec())
    }

    async fn write(&self, _offset: u64, data: &[u8]) -> Result<u32> {
        *self.data.write().await = String::from_utf8_lossy(data).to_string();
        Ok(data.len() as u32)
    }

    async fn size(&self) -> u64 {
        self.data.read().await.len() as u64
    }

    async fn clone(&self) -> Result<Box<dyn SyntheticFile>> {
        Ok(Box::new(TranslatorConfigFile {
            data: self.data.clone(),
        }))
    }
}

/// Synthetic file for translator script
struct TranslatorScriptFile {
    data: Arc<RwLock<Vec<u8>>>,
}

#[async_trait]
impl SyntheticFile for TranslatorScriptFile {
    async fn read(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        let data = self.data.read().await;
        let start = offset.min(data.len() as u64) as usize;
        let end = (start + count as usize).min(data.len());
        Ok(data[start..end].to_vec())
    }

    async fn write(&self, _offset: u64, data: &[u8]) -> Result<u32> {
        *self.data.write().await = data.to_vec();
        Ok(data.len() as u32)
    }

    async fn size(&self) -> u64 {
        self.data.read().await.len() as u64
    }

    async fn clone(&self) -> Result<Box<dyn SyntheticFile>> {
        Ok(Box::new(TranslatorScriptFile {
            data: self.data.clone(),
        }))
    }
}

/// Synthetic file for translator status
struct TranslatorStatusFile {
    name: String,
    cache: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

#[async_trait]
impl SyntheticFile for TranslatorStatusFile {
    async fn read(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        let cache = self.cache.read().await;
        let status = format!(
            "Translator: {}\nCached paths: {}\nMemory used: {} bytes\n",
            self.name,
            cache.len(),
            cache.values().map(|v| v.len()).sum::<usize>()
        );

        let bytes = status.as_bytes();
        let start = offset.min(bytes.len() as u64) as usize;
        let end = (start + count as usize).min(bytes.len());
        Ok(bytes[start..end].to_vec())
    }

    async fn write(&self, _offset: u64, data: &[u8]) -> Result<u32> {
        let command = String::from_utf8_lossy(data);

        if command.trim() == "clear" {
            self.cache.write().await.clear();
        }

        Ok(data.len() as u32)
    }

    async fn size(&self) -> u64 { 256 }

    async fn clone(&self) -> Result<Box<dyn SyntheticFile>> {
        Ok(Box::new(TranslatorStatusFile {
            name: self.name.clone(),
            cache: self.cache.clone(),
        }))
    }
}

/// Stacking translator - overlays multiple translators
pub struct StackedTranslator {
    layers: Vec<Arc<dyn Translator>>,
    write_through: bool,  // Write to all layers or just top?
}

impl StackedTranslator {
    pub fn new() -> Self {
        Self {
            layers: Vec::new(),
            write_through: false,
        }
    }

    /// Add a layer (bottom to top)
    pub fn push_layer(&mut self, translator: Arc<dyn Translator>) {
        self.layers.push(translator);
    }

    /// Enable write-through to all layers
    pub fn set_write_through(&mut self, enabled: bool) {
        self.write_through = enabled;
    }
}

#[async_trait]
impl Translator for StackedTranslator {
    fn name(&self) -> &str { "stacked" }
    fn translator_type(&self) -> &str { "stack" }
    fn isolation(&self) -> crate::translators::IsolationLevel {
        crate::translators::IsolationLevel::Process
    }

    fn supports(&self, operation: &str) -> bool {
        // Stack supports operation if any layer does
        self.layers.iter().any(|t| t.supports(operation))
    }

    async fn init(&mut self) -> Result<()> {
        for layer in &mut self.layers {
            Arc::get_mut(layer).unwrap().init().await?;
        }
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        for layer in &mut self.layers {
            Arc::get_mut(layer).unwrap().shutdown().await?;
        }
        Ok(())
    }

    async fn read(&self, path: &str, offset: u64, count: u32) -> Result<Vec<u8>> {
        // Try layers from top to bottom
        for layer in self.layers.iter().rev() {
            match layer.read(path, offset, count).await {
                Ok(data) if !data.is_empty() => return Ok(data),
                _ => continue,
            }
        }

        Ok(vec![])
    }

    async fn write(&self, path: &str, offset: u64, data: Vec<u8>) -> Result<u32> {
        if self.write_through {
            // Write to all layers
            let mut total = 0;
            for layer in &self.layers {
                if layer.supports("write") {
                    total = layer.write(path, offset, data.clone()).await?;
                }
            }
            Ok(total)
        } else {
            // Write only to top layer that supports it
            for layer in self.layers.iter().rev() {
                if layer.supports("write") {
                    return layer.write(path, offset, data).await;
                }
            }
            Ok(0)
        }
    }

    async fn list(&self, path: &str) -> Result<Vec<String>> {
        // Merge lists from all layers
        let mut all_items = HashMap::new();

        for layer in &self.layers {
            let items = layer.list(path).await?;
            for item in items {
                all_items.insert(item.clone(), item);
            }
        }

        Ok(all_items.into_values().collect())
    }

    async fn stat(&self, path: &str) -> Result<FileInfo> {
        // Try layers from top to bottom
        for layer in self.layers.iter().rev() {
            if let Ok(info) = layer.stat(path).await {
                return Ok(info);
            }
        }

        Err(anyhow::anyhow!("Path not found in any layer"))
    }
}

/// Factory for creating composed translators from specifications
pub struct TranslatorComposer {
    registry: Arc<RwLock<HashMap<String, Arc<dyn Translator>>>>,
}

impl TranslatorComposer {
    pub fn new() -> Self {
        Self {
            registry: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Parse and create a composed translator from a specification
    /// Examples:
    ///   "http | json"                    - Pipeline
    ///   "cache + http"                    - Stacked (cache on top of http)
    ///   "encrypt | compress | http"       - Pipeline with encryption
    ///   "(git + http) | filter"          - Stack then pipe
    pub async fn compose(&self, spec: &str) -> Result<Arc<dyn Translator>> {
        // Parse composition syntax
        if spec.contains('|') {
            // Pipeline composition
            self.create_pipeline(spec).await
        } else if spec.contains('+') {
            // Stack composition
            self.create_stack(spec).await
        } else {
            // Single translator
            self.create_single(spec).await
        }
    }

    async fn create_pipeline(&self, spec: &str) -> Result<Arc<dyn Translator>> {
        let mut pipeline = TranslatorPipeline::new(spec.to_string());

        for stage_spec in spec.split('|').map(|s| s.trim()) {
            let translator = self.compose(stage_spec).await?;
            pipeline.add_stage(translator);
        }

        Ok(Arc::new(pipeline))
    }

    async fn create_stack(&self, spec: &str) -> Result<Arc<dyn Translator>> {
        let mut stack = StackedTranslator::new();

        for layer_spec in spec.split('+').map(|s| s.trim()) {
            let translator = self.compose(layer_spec).await?;
            stack.push_layer(translator);
        }

        Ok(Arc::new(stack))
    }

    async fn create_single(&self, spec: &str) -> Result<Arc<dyn Translator>> {
        // Look up in registry or create new
        if let Some(trans) = self.registry.read().await.get(spec) {
            Ok(trans.clone())
        } else {
            // Create based on type
            match spec {
                "http" => Ok(Arc::new(crate::translators::HttpTranslator::new(
                    "http://localhost".to_string()
                ))),
                "git" => Ok(Arc::new(crate::translators::GitTranslator::new(
                    PathBuf::from("."),
                    "main".to_string()
                ))),
                "sql" => Ok(Arc::new(crate::translators::SqlTranslator::new(
                    "sqlite://data.db".to_string()
                ))),
                _ => {
                    // Create composable translator
                    Ok(Arc::new(ComposableTranslator::new(spec.to_string())))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pipeline() {
        let mut pipeline = TranslatorPipeline::new("test".to_string());
        // Add stages and test
    }

    #[tokio::test]
    async fn test_composable() {
        let trans = ComposableTranslator::new("filter".to_string());

        // Set script
        *trans.script_file.write().await = b"filter:test".to_vec();

        // Test filtering
        let input = b"test line\nother line\ntest again";
        let output = trans.apply_script(input.to_vec()).await.unwrap();
        let result = String::from_utf8_lossy(&output);

        assert!(result.contains("test"));
        assert!(!result.contains("other"));
    }

    #[tokio::test]
    async fn test_composition_syntax() {
        let composer = TranslatorComposer::new();

        // Test pipeline
        let _ = composer.compose("http | json | filter").await.unwrap();

        // Test stack
        let _ = composer.compose("cache + http").await.unwrap();
    }
}