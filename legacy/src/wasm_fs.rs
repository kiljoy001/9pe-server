//! WASM Filesystem Interface
//!
//! Users interact with WASM translators purely through filesystem operations:
//! - Write .wasm files to /wasm/modules/ to load modules
//! - Write to /wasm/compose/ to create compositions
//! - Read/write through /wasm/instances/[name]/ to use translators

use std::sync::Arc;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;
use anyhow::{Result, Context};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose};

use crate::wasm_composition::{WasmComposer, WasmTranslator};
use crate::synthetic_advanced::SyntheticFile;
use crate::auth::{AuthService, SecurityContext, Permissions};
use crate::translators::{Translator, FileInfo};

/// WASM filesystem that automatically manages modules
pub struct WasmFilesystem {
    composer: Arc<WasmComposer>,
    auth: Arc<AuthService>,
    modules: Arc<RwLock<HashMap<String, WasmModule>>>,
    instances: Arc<RwLock<HashMap<String, Arc<WasmTranslator>>>>,
    compositions: Arc<RwLock<HashMap<String, Composition>>>,
}

struct WasmModule {
    name: String,
    bytes: Vec<u8>,
    loaded: bool,
}

struct Composition {
    name: String,
    spec: String,  // e.g., "gzip | base64" or "cache + http"
    module_bytes: Option<Vec<u8>>,  // Generated WASM
}

impl WasmFilesystem {
    pub fn new(composer: Arc<WasmComposer>, auth: Arc<AuthService>) -> Self {
        Self {
            composer,
            auth,
            modules: Arc::new(RwLock::new(HashMap::new())),
            instances: Arc::new(RwLock::new(HashMap::new())),
            compositions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Handle filesystem operations for WASM
    pub async fn handle_operation(
        &self,
        path: &str,
        op: FsOperation,
        context: &SecurityContext,
    ) -> Result<FsResponse> {
        // Parse path structure: /wasm/category/name/...
        let parts: Vec<&str> = path.trim_start_matches("/wasm/").split('/').collect();

        match parts.get(0).map(|s| *s) {
            Some("modules") => self.handle_modules(parts.get(1..), op, context).await,
            Some("compose") => self.handle_compose(parts.get(1..), op, context).await,
            Some("instances") => self.handle_instances(parts.get(1..), op, context).await,
            Some("run") => self.handle_run(parts.get(1..), op, context).await,
            _ => Err(anyhow::anyhow!("Unknown WASM path")),
        }
    }

    /// Handle /wasm/modules/ operations
    async fn handle_modules(
        &self,
        path: &[&str],
        op: FsOperation,
        context: &SecurityContext,
    ) -> Result<FsResponse> {
        match op {
            FsOperation::Write(data) => {
                // Writing a .wasm file loads it as a module
                let name = path.get(0)
                    .ok_or_else(|| anyhow::anyhow!("Module name required"))?
                    .trim_end_matches(".wasm");

                // Verify WASM
                wasmparser::validate(&data)
                    .map_err(|e| anyhow::anyhow!("Invalid WASM: {}", e))?;

                // Check permissions
                if !self.auth.authorize(context, &format!("/wasm/modules/{}", name), Permissions::WRITE).await? {
                    return Err(anyhow::anyhow!("Permission denied"));
                }

                // Load module
                self.composer.load_module(name.to_string(), &data).await?;

                // Store module info
                self.modules.write().await.insert(name.to_string(), WasmModule {
                    name: name.to_string(),
                    bytes: data.clone(),
                    loaded: true,
                });

                Ok(FsResponse::Written(data.len() as u32))
            }

            FsOperation::Read(offset, count) => {
                // Reading lists modules or gets module bytes
                if path.is_empty() {
                    // List modules
                    let modules = self.modules.read().await;
                    let list = modules.keys()
                        .map(|k| format!("{}.wasm", k))
                        .collect::<Vec<_>>()
                        .join("\n");
                    Ok(FsResponse::Data(list.into_bytes()))
                } else {
                    // Get specific module
                    let name = path[0].trim_end_matches(".wasm");
                    let modules = self.modules.read().await;
                    let module = modules.get(name)
                        .ok_or_else(|| anyhow::anyhow!("Module not found"))?;

                    let start = offset.min(module.bytes.len() as u64) as usize;
                    let end = (start + count as usize).min(module.bytes.len());
                    Ok(FsResponse::Data(module.bytes[start..end].to_vec()))
                }
            }

            FsOperation::Delete => {
                let name = path.get(0)
                    .ok_or_else(|| anyhow::anyhow!("Module name required"))?
                    .trim_end_matches(".wasm");

                if !self.auth.authorize(context, &format!("/wasm/modules/{}", name), Permissions::DELETE).await? {
                    return Err(anyhow::anyhow!("Permission denied"));
                }

                self.modules.write().await.remove(name);
                Ok(FsResponse::Deleted)
            }

            _ => Err(anyhow::anyhow!("Unsupported operation")),
        }
    }

    /// Handle /wasm/compose/ operations
    async fn handle_compose(
        &self,
        path: &[&str],
        op: FsOperation,
        context: &SecurityContext,
    ) -> Result<FsResponse> {
        match op {
            FsOperation::Write(data) => {
                // Writing a composition spec generates a WASM module
                let name = path.get(0)
                    .ok_or_else(|| anyhow::anyhow!("Composition name required"))?;

                let spec = std::str::from_utf8(&data)?;

                // Parse and validate spec
                let wasm_bytes = self.generate_composition_wasm(spec).await?;

                // Load as module
                self.composer.load_module(format!("comp_{}", name), &wasm_bytes).await?;

                // Store composition
                self.compositions.write().await.insert(name.to_string(), Composition {
                    name: name.to_string(),
                    spec: spec.to_string(),
                    module_bytes: Some(wasm_bytes),
                });

                Ok(FsResponse::Written(data.len() as u32))
            }

            FsOperation::Read(_offset, _count) => {
                // Read composition spec or list
                if path.is_empty() {
                    let comps = self.compositions.read().await;
                    let list = comps.keys().cloned().collect::<Vec<_>>().join("\n");
                    Ok(FsResponse::Data(list.into_bytes()))
                } else {
                    let name = path[0];
                    let comps = self.compositions.read().await;
                    let comp = comps.get(name)
                        .ok_or_else(|| anyhow::anyhow!("Composition not found"))?;
                    Ok(FsResponse::Data(comp.spec.as_bytes().to_vec()))
                }
            }

            _ => Err(anyhow::anyhow!("Unsupported operation")),
        }
    }

    /// Handle /wasm/instances/ operations
    async fn handle_instances(
        &self,
        path: &[&str],
        op: FsOperation,
        context: &SecurityContext,
    ) -> Result<FsResponse> {
        let instance_name = path.get(0)
            .ok_or_else(|| anyhow::anyhow!("Instance name required"))?;

        match op {
            FsOperation::Write(data) if path.len() == 1 => {
                // Create instance by writing module name
                let module_name = std::str::from_utf8(&data)?.trim();

                // Check permissions
                if !self.auth.authorize(context, "/wasm/instances", Permissions::WRITE).await? {
                    return Err(anyhow::anyhow!("Permission denied"));
                }

                // Create instance
                self.composer.instantiate(instance_name.to_string(), module_name.to_string()).await?;

                // Create translator wrapper
                let translator = Arc::new(WasmTranslator::new(
                    instance_name.to_string(),
                    self.composer.clone(),
                    instance_name.to_string(),
                ));

                self.instances.write().await.insert(instance_name.to_string(), translator);

                Ok(FsResponse::Written(data.len() as u32))
            }

            _ if path.len() > 1 => {
                // Operate on instance files
                let instances = self.instances.read().await;
                let translator = instances.get(*instance_name)
                    .ok_or_else(|| anyhow::anyhow!("Instance not found"))?;

                let file_path = path[1..].join("/");

                match op {
                    FsOperation::Read(offset, count) => {
                        let data = translator.read(&file_path, offset, count).await?;
                        Ok(FsResponse::Data(data))
                    }
                    FsOperation::Write(data) => {
                        let written = translator.write(&file_path, 0, data).await?;
                        Ok(FsResponse::Written(written))
                    }
                    FsOperation::List => {
                        let entries = translator.list(&file_path).await?;
                        Ok(FsResponse::DirList(entries))
                    }
                    _ => Err(anyhow::anyhow!("Unsupported operation")),
                }
            }

            _ => Err(anyhow::anyhow!("Invalid instance operation")),
        }
    }

    /// Handle /wasm/run/ for immediate execution
    async fn handle_run(
        &self,
        path: &[&str],
        op: FsOperation,
        _context: &SecurityContext,
    ) -> Result<FsResponse> {
        match op {
            FsOperation::Write(data) => {
                // Run WASM directly without saving
                // Input format: first line is WASM (base64), rest is input data
                let input_str = std::str::from_utf8(&data)?;
                let mut lines = input_str.lines();

                let wasm_b64 = lines.next()
                    .ok_or_else(|| anyhow::anyhow!("Missing WASM code"))?;
                let wasm_bytes = general_purpose::STANDARD.decode(wasm_b64)?;
                let input_data = lines.collect::<Vec<_>>().join("\n");

                // Create temporary instance
                let temp_name = format!("temp_{}", uuid::Uuid::new_v4());
                self.composer.load_module(temp_name.clone(), &wasm_bytes).await?;
                self.composer.instantiate(temp_name.clone(), temp_name.clone()).await?;

                // Execute
                let output = self.composer.execute(&temp_name, "process", input_data.as_bytes()).await?;

                Ok(FsResponse::Data(output))
            }

            _ => Err(anyhow::anyhow!("Write-only interface")),
        }
    }

    /// Generate WASM module from composition spec
    async fn generate_composition_wasm(&self, spec: &str) -> Result<Vec<u8>> {
        // This would generate actual WASM bytecode
        // For now, return a placeholder

        // Parse spec: "filter1 | filter2" or "layer1 + layer2"
        let is_pipeline = spec.contains('|');
        let is_stack = spec.contains('+');

        if is_pipeline {
            // Generate pipeline WASM
            self.generate_pipeline_wasm(spec.split('|').map(str::trim).collect()).await
        } else if is_stack {
            // Generate stack WASM
            self.generate_stack_wasm(spec.split('+').map(str::trim).collect()).await
        } else {
            // Single translator reference
            Ok(vec![]) // Would look up existing module
        }
    }

    async fn generate_pipeline_wasm(&self, stages: Vec<&str>) -> Result<Vec<u8>> {
        // Would use wasm-encoder to build actual module
        // This is a simplified version
        use wasm_encoder::{Module, CodeSection, Function, Instruction};

        let mut module = Module::new();

        // Add imports for each stage translator
        for stage in &stages {
            // Import stage's process function
        }

        // Add main process function that chains stages
        let mut function = Function::new(vec![]);
        function.instruction(&Instruction::LocalGet(0));
        // Chain calls to each stage
        for _stage in &stages {
            function.instruction(&Instruction::Call(0));
        }
        function.instruction(&Instruction::End);

        let mut code = CodeSection::new();
        code.function(&function);
        module.section(&code);

        Ok(module.finish())
    }

    async fn generate_stack_wasm(&self, layers: Vec<&str>) -> Result<Vec<u8>> {
        // Generate WASM that runs layers in parallel
        use wasm_encoder::Module;

        let module = Module::new();
        // Would add parallel execution logic
        Ok(module.finish())
    }
}

/// Filesystem operation types
pub enum FsOperation {
    Read(u64, u32),     // offset, count
    Write(Vec<u8>),
    Delete,
    List,
    Stat,
}

/// Filesystem response types
pub enum FsResponse {
    Data(Vec<u8>),
    Written(u32),
    Deleted,
    DirList(Vec<String>),
    FileInfo(FileInfo),
}

/// Usage examples
pub const EXAMPLES: &str = r#"
# WASM Filesystem Usage

## Load a module:
cat my_translator.wasm > /wasm/modules/mytrans.wasm

## Create a composition:
echo "gzip | base64 | encrypt" > /wasm/compose/secure_pipe

## Create an instance:
echo "mytrans" > /wasm/instances/t1

## Use the instance:
cat data.txt > /wasm/instances/t1/process
cat /wasm/instances/t1/result

## Direct execution:
(echo $WASM_BASE64; cat input.txt) > /wasm/run/

## Pipeline through multiple instances:
cat data | /wasm/instances/compress/in > /wasm/instances/encrypt/in > output

## The beauty: it's all just files!
"#;

/// Create synthetic file for a WASM path
pub struct WasmSyntheticFile {
    fs: Arc<WasmFilesystem>,
    path: String,
}

#[async_trait]
impl SyntheticFile for WasmSyntheticFile {
    async fn read(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        let context = SecurityContext {
            user: None,
            auth_method: crate::auth::AuthMethod::None,
            capabilities: vec![],
            session_key: None,
            ip_address: "127.0.0.1".parse().unwrap(),
            authenticated_at: None,
            mfa_verified: false,
        };

        match self.fs.handle_operation(&self.path, FsOperation::Read(offset, count), &context).await? {
            FsResponse::Data(data) => Ok(data),
            _ => Err(anyhow::anyhow!("Unexpected response")),
        }
    }

    async fn write(&self, _offset: u64, data: &[u8]) -> Result<u32> {
        let context = SecurityContext {
            user: None,
            auth_method: crate::auth::AuthMethod::None,
            capabilities: vec![],
            session_key: None,
            ip_address: "127.0.0.1".parse().unwrap(),
            authenticated_at: None,
            mfa_verified: false,
        };

        match self.fs.handle_operation(&self.path, FsOperation::Write(data.to_vec()), &context).await? {
            FsResponse::Written(n) => Ok(n),
            _ => Err(anyhow::anyhow!("Unexpected response")),
        }
    }

    async fn size(&self) -> u64 { 0 }

    async fn clone(&self) -> Result<Box<dyn SyntheticFile>> {
        Ok(Box::new(WasmSyntheticFile {
            fs: self.fs.clone(),
            path: self.path.clone(),
        }))
    }
}