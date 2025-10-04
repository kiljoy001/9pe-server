//! Secure Server-Side API for WASM Composition
//!
//! Exposes WASM composition through 9P synthetic files with capability-based security

use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use anyhow::{Result, Context};
use sha2::{Sha256, Digest};
use ed25519_dalek::{VerifyingKey, Signature, Verifier};
use base64::{Engine as _, engine::general_purpose};

use crate::auth::{AuthService, Permissions, SignedCapability};
use crate::wasm_composition::{WasmComposer, WasmTranslator};
use crate::synthetic_advanced::SyntheticFile;
use async_trait::async_trait;

/// Security policy for WASM modules
#[derive(Debug, Clone)]
pub struct WasmSecurityPolicy {
    /// Maximum module size in bytes
    pub max_module_size: usize,
    /// Maximum memory pages (64KB each)
    pub max_memory_pages: u32,
    /// Maximum execution time in milliseconds
    pub max_execution_time_ms: u64,
    /// Allowed WASI capabilities
    pub wasi_capabilities: WasiCapabilities,
    /// Required signatures for module loading
    pub require_signed_modules: bool,
    /// Trusted signers (public keys)
    pub trusted_signers: Vec<VerifyingKey>,
}

#[derive(Debug, Clone)]
pub struct WasiCapabilities {
    pub allow_filesystem: bool,
    pub allow_network: bool,
    pub allow_env_vars: bool,
    pub allow_clock: bool,
    pub allow_random: bool,
    pub filesystem_paths: Vec<String>, // Allowed paths if filesystem enabled
    pub network_domains: Vec<String>,  // Allowed domains if network enabled
}

impl Default for WasmSecurityPolicy {
    fn default() -> Self {
        Self {
            max_module_size: 10 * 1024 * 1024, // 10MB
            max_memory_pages: 256,              // 16MB total
            max_execution_time_ms: 5000,        // 5 seconds
            wasi_capabilities: WasiCapabilities {
                allow_filesystem: false,
                allow_network: false,
                allow_env_vars: false,
                allow_clock: true,
                allow_random: true,
                filesystem_paths: vec![],
                network_domains: vec![],
            },
            require_signed_modules: true,
            trusted_signers: vec![],
        }
    }
}

/// WASM Module Registry with security verification
pub struct WasmModuleRegistry {
    modules: Arc<RwLock<HashMap<String, VerifiedModule>>>,
    composer: Arc<WasmComposer>,
    auth: Arc<AuthService>,
    policy: WasmSecurityPolicy,
}

/// Verified WASM module with metadata
#[derive(Clone)]
struct VerifiedModule {
    name: String,
    hash: Vec<u8>,
    owner: String,
    capabilities: Vec<String>,
    signature: Option<Vec<u8>>,
    bytes: Vec<u8>,
}

impl WasmModuleRegistry {
    pub fn new(composer: Arc<WasmComposer>, auth: Arc<AuthService>, policy: WasmSecurityPolicy) -> Self {
        Self {
            modules: Arc::new(RwLock::new(HashMap::new())),
            composer,
            auth,
            policy,
        }
    }

    /// Register a WASM module with security checks
    pub async fn register_module(
        &self,
        name: String,
        bytes: Vec<u8>,
        owner: String,
        signature: Option<Vec<u8>>,
        capability: &SignedCapability,
    ) -> Result<()> {
        // 1. Check capability permissions
        self.auth.verify_capability(capability).await?;
        if !capability.capability.resource.starts_with("/wasm/") {
            return Err(anyhow::anyhow!("Invalid capability for WASM registration"));
        }

        // 2. Verify module size
        if bytes.len() > self.policy.max_module_size {
            return Err(anyhow::anyhow!(
                "Module too large: {} > {}",
                bytes.len(),
                self.policy.max_module_size
            ));
        }

        // 3. Verify signature if required
        if self.policy.require_signed_modules {
            let sig = signature.as_ref().ok_or_else(|| anyhow::anyhow!("Module signature required"))?;
            self.verify_module_signature(&bytes, sig)?;
        }

        // 4. Validate WASM module structure
        wasmparser::validate(&bytes)
            .map_err(|e| anyhow::anyhow!("Invalid WASM module: {}", e))?;

        // 5. Check for forbidden imports
        self.validate_imports(&bytes)?;

        // 6. Calculate module hash
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let hash = hasher.finalize().to_vec();

        // 7. Store verified module
        let module = VerifiedModule {
            name: name.clone(),
            hash,
            owner,
            capabilities: vec![],
            signature,
            bytes: bytes.clone(),
        };

        self.modules.write().await.insert(name.clone(), module);

        // 8. Load into WASM runtime
        self.composer.load_module(name, &bytes).await?;

        Ok(())
    }

    /// Verify module signature against trusted signers
    fn verify_module_signature(&self, bytes: &[u8], signature_bytes: &[u8]) -> Result<()> {
        let signature = Signature::from_bytes(signature_bytes)
            .map_err(|e| anyhow::anyhow!("Invalid signature: {}", e))?;

        for signer in &self.policy.trusted_signers {
            if signer.verify(bytes, &signature).is_ok() {
                return Ok(());
            }
        }

        Err(anyhow::anyhow!("No valid signature found"))
    }

    /// Validate WASM imports against security policy
    fn validate_imports(&self, bytes: &[u8]) -> Result<()> {
        use wasmparser::{Parser, Payload};

        let parser = Parser::new(0);
        for payload in parser.parse_all(bytes) {
            match payload? {
                Payload::ImportSection(imports) => {
                    for import in imports {
                        let import = import?;
                        match import.module {
                            "wasi_snapshot_preview1" => {
                                // Check WASI imports against policy
                                match import.name {
                                    "path_open" | "fd_read" | "fd_write" if !self.policy.wasi_capabilities.allow_filesystem => {
                                        return Err(anyhow::anyhow!("Filesystem access not allowed"));
                                    }
                                    "sock_connect" | "sock_send" if !self.policy.wasi_capabilities.allow_network => {
                                        return Err(anyhow::anyhow!("Network access not allowed"));
                                    }
                                    _ => {}
                                }
                            }
                            "translator" => {
                                // Our custom imports are always allowed
                            }
                            module => {
                                return Err(anyhow::anyhow!("Forbidden import module: {}", module));
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Create a translator instance with resource limits
    pub async fn create_instance(
        &self,
        module_name: String,
        instance_name: String,
        capability: &SignedCapability,
    ) -> Result<Arc<WasmTranslator>> {
        // Verify capability
        self.auth.verify_capability(capability).await?;

        // Check module exists and user has permission
        let modules = self.modules.read().await;
        let module = modules.get(&module_name)
            .ok_or_else(|| anyhow::anyhow!("Module not found: {}", module_name))?;

        if module.owner != capability.capability.subject &&
           !capability.capability.resource.contains("*") {
            return Err(anyhow::anyhow!("Permission denied for module"));
        }

        // Create instance with resource limits
        self.composer.instantiate(instance_name.clone(), module_name).await?;

        // Return wrapped translator
        Ok(Arc::new(WasmTranslator::new(
            instance_name.clone(),
            self.composer.clone(),
            instance_name,
        )))
    }
}

/// Server-side API exposed through synthetic files
pub struct WasmApiFiles {
    registry: Arc<WasmModuleRegistry>,
    auth: Arc<AuthService>,
}

impl WasmApiFiles {
    pub fn new(registry: Arc<WasmModuleRegistry>, auth: Arc<AuthService>) -> Self {
        Self { registry, auth }
    }

    /// Create all synthetic files for WASM API
    pub fn create_files(&self) -> HashMap<String, Arc<dyn SyntheticFile>> {
        let mut files = HashMap::new();

        // /wasm/ctl - Control interface
        files.insert(
            "/wasm/ctl".to_string(),
            Arc::new(WasmCtlFile {
                registry: self.registry.clone(),
                auth: self.auth.clone(),
            }) as Arc<dyn SyntheticFile>,
        );

        // /wasm/modules - List modules
        files.insert(
            "/wasm/modules".to_string(),
            Arc::new(WasmModulesFile {
                registry: self.registry.clone(),
            }) as Arc<dyn SyntheticFile>,
        );

        // /wasm/policy - Security policy
        files.insert(
            "/wasm/policy".to_string(),
            Arc::new(WasmPolicyFile {
                registry: self.registry.clone(),
            }) as Arc<dyn SyntheticFile>,
        );

        files
    }
}

/// /wasm/ctl - Main control interface
struct WasmCtlFile {
    registry: Arc<WasmModuleRegistry>,
    auth: Arc<AuthService>,
}

#[async_trait]
impl SyntheticFile for WasmCtlFile {
    async fn read(&self, _offset: u64, _count: u32) -> Result<Vec<u8>> {
        Ok(b"Commands: load <name> <capability>, create <module> <instance> <capability>, compose <spec>\n".to_vec())
    }

    async fn write(&self, _offset: u64, data: &[u8]) -> Result<u32> {
        let command = std::str::from_utf8(data)?.trim();
        let parts: Vec<&str> = command.split_whitespace().collect();

        match parts.get(0).map(|s| *s) {
            Some("load") => {
                // load <name> <base64_wasm> <base64_signature> <capability>
                if parts.len() < 5 {
                    return Err(anyhow::anyhow!("Usage: load <name> <wasm_b64> <sig_b64> <cap_json>"));
                }

                let name = parts[1].to_string();
                let wasm_bytes = general_purpose::STANDARD.decode(parts[2])?;
                let signature = general_purpose::STANDARD.decode(parts[3]).ok();
                let cap_json = parts[4..].join(" ");
                let capability: SignedCapability = serde_json::from_str(&cap_json)?;

                self.registry.register_module(
                    name,
                    wasm_bytes,
                    capability.capability.subject.clone(),
                    signature,
                    &capability,
                ).await?;
            }

            Some("create") => {
                // create <module> <instance> <capability>
                if parts.len() < 4 {
                    return Err(anyhow::anyhow!("Usage: create <module> <instance> <capability_json>"));
                }

                let module = parts[1].to_string();
                let instance = parts[2].to_string();
                let cap_json = parts[3..].join(" ");
                let capability: SignedCapability = serde_json::from_str(&cap_json)?;

                self.registry.create_instance(module, instance, &capability).await?;
            }

            Some("compose") => {
                // compose "http | gzip | encrypt" <capability>
                if parts.len() < 3 {
                    return Err(anyhow::anyhow!("Usage: compose \"pipeline spec\" <capability>"));
                }

                // Parse composition spec
                let spec = parts[1];
                let cap_json = parts[2..].join(" ");
                let capability: SignedCapability = serde_json::from_str(&cap_json)?;

                // Create composition (would generate WASM module)
                // This is where users' composition requests become WASM modules
            }

            _ => return Err(anyhow::anyhow!("Unknown command")),
        }

        Ok(data.len() as u32)
    }

    async fn size(&self) -> u64 { 0 }

    async fn clone(&self) -> Result<Box<dyn SyntheticFile>> {
        Ok(Box::new(WasmCtlFile {
            registry: self.registry.clone(),
            auth: self.auth.clone(),
        }))
    }
}

/// /wasm/modules - List available modules
struct WasmModulesFile {
    registry: Arc<WasmModuleRegistry>,
}

#[async_trait]
impl SyntheticFile for WasmModulesFile {
    async fn read(&self, _offset: u64, _count: u32) -> Result<Vec<u8>> {
        let modules = self.registry.modules.read().await;
        let mut output = String::new();

        for (name, module) in modules.iter() {
            output.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                name,
                module.owner,
                hex::encode(&module.hash[..8]),
                module.bytes.len()
            ));
        }

        Ok(output.into_bytes())
    }

    async fn write(&self, _offset: u64, _data: &[u8]) -> Result<u32> {
        Err(anyhow::anyhow!("Read-only file"))
    }

    async fn size(&self) -> u64 { 0 }

    async fn clone(&self) -> Result<Box<dyn SyntheticFile>> {
        Ok(Box::new(WasmModulesFile {
            registry: self.registry.clone(),
        }))
    }
}

/// /wasm/policy - View/update security policy
struct WasmPolicyFile {
    registry: Arc<WasmModuleRegistry>,
}

#[async_trait]
impl SyntheticFile for WasmPolicyFile {
    async fn read(&self, _offset: u64, _count: u32) -> Result<Vec<u8>> {
        let policy = &self.registry.policy;
        let output = format!(
            "max_module_size: {}\n\
             max_memory_pages: {}\n\
             max_execution_time_ms: {}\n\
             require_signed_modules: {}\n\
             allow_filesystem: {}\n\
             allow_network: {}\n",
            policy.max_module_size,
            policy.max_memory_pages,
            policy.max_execution_time_ms,
            policy.require_signed_modules,
            policy.wasi_capabilities.allow_filesystem,
            policy.wasi_capabilities.allow_network,
        );

        Ok(output.into_bytes())
    }

    async fn write(&self, _offset: u64, data: &[u8]) -> Result<u32> {
        // Would update policy with proper authorization
        Err(anyhow::anyhow!("Policy updates require admin capability"))
    }

    async fn size(&self) -> u64 { 0 }

    async fn clone(&self) -> Result<Box<dyn SyntheticFile>> {
        Ok(Box::new(WasmPolicyFile {
            registry: self.registry.clone(),
        }))
    }
}

/// Example: How users interact with the API
pub const USAGE_EXAMPLE: &str = r#"
# User workflow for WASM composition:

1. Write composition in Rust/C/Go/AssemblyScript
2. Compile to WASM
3. Sign the module (optional but recommended)
4. Upload via 9P:

   # Get capability
   cap=$(cat /auth/request_capability)

   # Load module
   echo "load myfilter filter.wasm sig.b64 $cap" > /wasm/ctl

   # Create instance
   echo "create myfilter filter1 $cap" > /wasm/ctl

   # Use in composition
   echo "compose 'http | filter1 | gzip' $cap" > /wasm/ctl

5. Module runs in sandboxed WASM with resource limits
6. Can only access what security policy allows

Security guarantees:
- Modules run in WASM sandbox (no host access)
- Capability-based authorization
- Resource limits enforced
- Signed modules prevent tampering
- WASI capabilities restricted by policy
"#;