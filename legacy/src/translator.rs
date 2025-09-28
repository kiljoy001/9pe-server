//! WASM Translator Management System
//!
//! Provides WASM-based translators with CBOR data exchange, synthetic file generation,
//! and full lifecycle management through /srv/settrans

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::{Result, Context, bail};
use serde::{Serialize, Deserialize};
use tracing::{info, warn, error, debug};
use uuid::Uuid;

#[cfg(feature = "wasm")]
use wasmtime::{Engine, Module, Store, Instance, Func, TypedFunc};

/// Translator manifest embedded in WASM modules
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslatorManifest {
    /// Translator name (must be unique)
    pub name: String,
    /// Version string
    pub version: String,
    /// Description
    pub description: String,
    /// Required directory structure to create
    pub required_dirs: Vec<String>,
    /// Synthetic files this translator provides
    pub synthetic_files: Vec<SyntheticFileSpec>,
    /// Permissions required
    pub permissions: Vec<Permission>,
    /// Restart policy
    pub restart_policy: RestartPolicy,
}

/// Synthetic file specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntheticFileSpec {
    /// File name (e.g., "status.synth")
    pub name: String,
    /// Access mode
    pub access: AccessMode,
    /// Data type for validation
    pub data_type: DataType,
    /// Whether this file can be cached
    pub cacheable: bool,
    /// Cache TTL in seconds
    pub ttl_seconds: Option<u64>,
    /// Schema for validation (JSON Schema)
    pub schema: Option<String>,
}

/// File access modes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccessMode {
    Read,
    Write,
    ReadWrite,
}

/// Data types for synthetic files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataType {
    Text,
    Binary,
    CBOR,
    JSON,
}

/// Permission types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Permission {
    FileRead,
    FileWrite,
    NetworkAccess,
    ProcessSpawn,
}

/// Restart policy for failed translators
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RestartPolicy {
    Never,
    Always,
    OnFailure,
    UpTo(u32), // Restart up to N times
}

/// CBOR-based request/response for synthetic files
#[derive(Debug, Serialize, Deserialize)]
pub struct SyntheticRequest {
    pub file_path: String,
    pub operation: Operation,
    pub data: Option<Vec<u8>>,
    pub params: HashMap<String, serde_cbor::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Operation {
    Read,
    Write,
    Create,
    Delete,
    List,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SyntheticResponse {
    pub success: bool,
    pub data: Option<Vec<u8>>,
    pub error: Option<String>,
    pub metadata: HashMap<String, serde_cbor::Value>,
}

/// Active translator instance
pub struct TranslatorInstance {
    pub manifest: TranslatorManifest,
    pub id: Uuid,
    pub base_path: PathBuf,
    pub restart_count: u32,
    pub status: TranslatorStatus,

    #[cfg(feature = "wasm")]
    pub wasm_instance: Option<Instance>,
    #[cfg(feature = "wasm")]
    pub store: Option<Store<()>>,
}

#[derive(Debug, Clone)]
pub enum TranslatorStatus {
    Starting,
    Running,
    Failed(String),
    Stopped,
    Restarting,
}

/// Main translator management system
pub struct TranslatorManager {
    /// Active translators
    translators: Arc<RwLock<HashMap<String, TranslatorInstance>>>,
    /// Base directory for translator installations (/srv/settrans)
    base_dir: PathBuf,
    /// WASM engine
    #[cfg(feature = "wasm")]
    engine: Engine,
}

impl TranslatorManager {
    /// Create new translator manager
    pub async fn new(base_dir: PathBuf) -> Result<Self> {
        // Create settrans directory structure
        let settrans_dir = base_dir.join("settrans");
        let install_dir = settrans_dir.join("install");

        tokio::fs::create_dir_all(&install_dir).await
            .context("Failed to create settrans/install directory")?;

        // Create synthetic control files
        Self::create_control_files(&settrans_dir).await?;

        info!("📁 Created translator management structure:");
        info!("   /srv/settrans -> {:?}", settrans_dir);
        info!("   /srv/settrans/install -> {:?}", install_dir);

        #[cfg(feature = "wasm")]
        let engine = Engine::default();

        Ok(Self {
            translators: Arc::new(RwLock::new(HashMap::new())),
            base_dir,
            #[cfg(feature = "wasm")]
            engine,
        })
    }

    /// Create control synthetic files
    async fn create_control_files(settrans_dir: &Path) -> Result<()> {
        let uninstall_file = settrans_dir.join("uninstall.synth");
        let restart_file = settrans_dir.join("restart.synth");
        let status_file = settrans_dir.join("status.synth");

        // These are placeholder files - actual handling is done in synthetic file system
        for file in &[&uninstall_file, &restart_file, &status_file] {
            if !file.exists() {
                tokio::fs::write(file, "").await?;
            }
        }

        Ok(())
    }

    /// Install a translator from WASM bytecode
    pub async fn install_translator(&mut self, wasm_bytes: Vec<u8>) -> Result<String> {
        // 1. Validate and load WASM module
        #[cfg(feature = "wasm")]
        let module = Module::new(&self.engine, &wasm_bytes)
            .context("Failed to compile WASM module")?;

        // 2. Extract manifest
        let manifest = self.extract_manifest(&wasm_bytes).await?;

        // 3. Check for conflicts
        if self.translators.read().await.contains_key(&manifest.name) {
            bail!("Translator '{}' already installed", manifest.name);
        }

        // 4. Create translator directory structure
        let translator_dir = self.base_dir.join(&manifest.name);
        self.create_translator_directories(&translator_dir, &manifest).await?;

        // 5. Create and start WASM instance
        let id = Uuid::new_v4();
        let mut instance = TranslatorInstance {
            manifest: manifest.clone(),
            id,
            base_path: translator_dir.clone(),
            restart_count: 0,
            status: TranslatorStatus::Starting,
            #[cfg(feature = "wasm")]
            wasm_instance: None,
            #[cfg(feature = "wasm")]
            store: None,
        };

        #[cfg(feature = "wasm")]
        {
            let mut store = Store::new(&self.engine, ());
            let wasm_instance = Instance::new(&mut store, &module, &[])
                .context("Failed to instantiate WASM module")?;

            // Call translator_init if available
            if let Ok(init_func) = wasm_instance.get_typed_func::<(), i32>(&mut store, "translator_init") {
                let result = init_func.call(&mut store, ())
                    .context("Failed to initialize translator")?;
                if result != 0 {
                    bail!("Translator initialization failed with code: {}", result);
                }
            }

            instance.wasm_instance = Some(wasm_instance);
            instance.store = Some(store);
        }

        instance.status = TranslatorStatus::Running;

        // 6. Register translator
        self.translators.write().await.insert(manifest.name.clone(), instance);

        info!("✅ Installed translator: {} v{}", manifest.name, manifest.version);
        Ok(manifest.name)
    }

    /// Extract manifest from WASM module
    async fn extract_manifest(&self, _wasm_bytes: &[u8]) -> Result<TranslatorManifest> {
        // For now, return a default manifest
        // In a real implementation, this would extract from WASM custom sections
        Ok(TranslatorManifest {
            name: format!("translator_{}", Uuid::new_v4().simple()),
            version: "1.0.0".to_string(),
            description: "Example translator".to_string(),
            required_dirs: vec!["input".to_string(), "output".to_string(), "config".to_string()],
            synthetic_files: vec![
                SyntheticFileSpec {
                    name: "status.synth".to_string(),
                    access: AccessMode::Read,
                    data_type: DataType::CBOR,
                    cacheable: true,
                    ttl_seconds: Some(5),
                    schema: None,
                },
                SyntheticFileSpec {
                    name: "config.synth".to_string(),
                    access: AccessMode::ReadWrite,
                    data_type: DataType::CBOR,
                    cacheable: false,
                    ttl_seconds: None,
                    schema: None,
                },
            ],
            permissions: vec![Permission::FileRead, Permission::FileWrite],
            restart_policy: RestartPolicy::OnFailure,
        })
    }

    /// Create translator directory structure
    async fn create_translator_directories(&self, base_path: &Path, manifest: &TranslatorManifest) -> Result<()> {
        tokio::fs::create_dir_all(base_path).await?;

        // Create required directories
        for dir_name in &manifest.required_dirs {
            let dir_path = base_path.join(dir_name);
            tokio::fs::create_dir_all(&dir_path).await?;
            debug!("Created translator directory: {:?}", dir_path);
        }

        // Create synthetic files
        for synth_file in &manifest.synthetic_files {
            let file_path = base_path.join(&synth_file.name);
            if !file_path.exists() {
                tokio::fs::write(&file_path, "").await?;
                debug!("Created synthetic file: {:?}", file_path);
            }
        }

        // Write manifest
        let manifest_path = base_path.join("manifest.cbor");
        let manifest_data = serde_cbor::to_vec(manifest)?;
        tokio::fs::write(manifest_path, manifest_data).await?;

        Ok(())
    }

    /// Uninstall a translator
    pub async fn uninstall_translator(&mut self, name: &str) -> Result<()> {
        let mut translators = self.translators.write().await;

        if let Some(instance) = translators.remove(name) {
            info!("🗑️ Uninstalling translator: {}", name);

            // Stop the translator if running
            // (WASM instance will be dropped automatically)

            // Remove directory structure
            if instance.base_path.exists() {
                tokio::fs::remove_dir_all(&instance.base_path).await
                    .context("Failed to remove translator directory")?;
            }

            info!("✅ Uninstalled translator: {}", name);
            Ok(())
        } else {
            bail!("Translator '{}' not found", name);
        }
    }

    /// Restart a translator
    pub async fn restart_translator(&mut self, name: &str) -> Result<()> {
        let mut translators = self.translators.write().await;

        if let Some(instance) = translators.get_mut(name) {
            info!("🔄 Restarting translator: {}", name);

            instance.status = TranslatorStatus::Restarting;
            instance.restart_count += 1;

            // Check restart policy
            match instance.manifest.restart_policy {
                RestartPolicy::Never => {
                    bail!("Translator '{}' has restart policy 'Never'", name);
                }
                RestartPolicy::UpTo(max) if instance.restart_count > max => {
                    bail!("Translator '{}' exceeded maximum restarts ({})", name, max);
                }
                _ => {}
            }

            // Restart WASM instance
            #[cfg(feature = "wasm")]
            {
                // Re-create store and instance
                // This would involve reloading the WASM module
                instance.status = TranslatorStatus::Running;
            }

            #[cfg(not(feature = "wasm"))]
            {
                instance.status = TranslatorStatus::Running;
            }

            info!("✅ Restarted translator: {}", name);
            Ok(())
        } else {
            bail!("Translator '{}' not found", name);
        }
    }

    /// Handle synthetic file operation
    pub async fn handle_synthetic_file(&self, translator_name: &str, request: SyntheticRequest) -> Result<SyntheticResponse> {
        let translators = self.translators.read().await;

        if let Some(instance) = translators.get(translator_name) {
            match instance.status {
                TranslatorStatus::Running => {
                    self.execute_synthetic_operation(instance, request).await
                }
                _ => {
                    Ok(SyntheticResponse {
                        success: false,
                        data: None,
                        error: Some(format!("Translator '{}' is not running", translator_name)),
                        metadata: HashMap::new(),
                    })
                }
            }
        } else {
            Ok(SyntheticResponse {
                success: false,
                data: None,
                error: Some(format!("Translator '{}' not found", translator_name)),
                metadata: HashMap::new(),
            })
        }
    }

    /// Execute synthetic file operation
    async fn execute_synthetic_operation(&self, _instance: &TranslatorInstance, request: SyntheticRequest) -> Result<SyntheticResponse> {
        // For now, return mock responses based on file type
        match request.operation {
            Operation::Read => {
                if request.file_path.ends_with("status.synth") {
                    let status_data = HashMap::from([
                        ("status".to_string(), serde_cbor::Value::Text("running".to_string())),
                        ("uptime".to_string(), serde_cbor::Value::Integer(3600)),
                        ("errors".to_string(), serde_cbor::Value::Integer(0)),
                    ]);
                    let data = serde_cbor::to_vec(&status_data)?;

                    Ok(SyntheticResponse {
                        success: true,
                        data: Some(data),
                        error: None,
                        metadata: HashMap::new(),
                    })
                } else {
                    Ok(SyntheticResponse {
                        success: false,
                        data: None,
                        error: Some("File not found".to_string()),
                        metadata: HashMap::new(),
                    })
                }
            }
            Operation::Write => {
                // Handle write operations
                Ok(SyntheticResponse {
                    success: true,
                    data: None,
                    error: None,
                    metadata: HashMap::new(),
                })
            }
            _ => {
                Ok(SyntheticResponse {
                    success: false,
                    data: None,
                    error: Some("Operation not supported".to_string()),
                    metadata: HashMap::new(),
                })
            }
        }
    }

    /// List all active translators
    pub async fn list_translators(&self) -> Vec<(String, TranslatorStatus)> {
        let translators = self.translators.read().await;
        translators.iter()
            .map(|(name, instance)| (name.clone(), instance.status.clone()))
            .collect()
    }

    /// Get translator status
    pub async fn get_translator_status(&self, name: &str) -> Option<TranslatorStatus> {
        let translators = self.translators.read().await;
        translators.get(name).map(|instance| instance.status.clone())
    }
}

/// Built-in synthetic file generators
pub mod generators {
    use super::*;

    /// Generate status information
    pub async fn generate_status(translator_name: &str, params: &HashMap<String, serde_cbor::Value>) -> Result<Vec<u8>> {
        let status = HashMap::from([
            ("translator".to_string(), serde_cbor::Value::Text(translator_name.to_string())),
            ("timestamp".to_string(), serde_cbor::Value::Integer(chrono::Utc::now().timestamp().into())),
            ("format".to_string(), params.get("format").cloned().unwrap_or(serde_cbor::Value::Text("cbor".to_string()))),
        ]);

        serde_cbor::to_vec(&status).context("Failed to serialize status")
    }

    /// Generate directory listing
    pub async fn generate_directory_listing(path: &Path) -> Result<Vec<u8>> {
        let mut entries = Vec::new();

        if path.exists() && path.is_dir() {
            let mut dir = tokio::fs::read_dir(path).await?;
            while let Some(entry) = dir.next_entry().await? {
                let name = entry.file_name().to_string_lossy().to_string();
                let is_dir = entry.file_type().await?.is_dir();

                entries.push(HashMap::from([
                    ("name".to_string(), serde_cbor::Value::Text(name)),
                    ("type".to_string(), serde_cbor::Value::Text(if is_dir { "directory".to_string() } else { "file".to_string() })),
                ]));
            }
        }

        let listing = HashMap::from([
            ("entries".to_string(), serde_cbor::Value::Array(
                entries.into_iter().map(|entry| {
                    let btree: std::collections::BTreeMap<serde_cbor::Value, serde_cbor::Value> = entry
                        .into_iter()
                        .map(|(k, v)| (serde_cbor::Value::Text(k), v))
                        .collect();
                    serde_cbor::Value::Map(btree)
                }).collect()
            )),
        ]);

        serde_cbor::to_vec(&listing).context("Failed to serialize directory listing")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_translator_manager_creation() {
        let temp_dir = tempdir().unwrap();
        let manager = TranslatorManager::new(temp_dir.path().to_path_buf()).await.unwrap();

        // Check that settrans directory was created
        let settrans_dir = temp_dir.path().join("settrans");
        assert!(settrans_dir.exists());
        assert!(settrans_dir.join("install").exists());
        assert!(settrans_dir.join("uninstall.synth").exists());
    }

    #[tokio::test]
    async fn test_synthetic_request_cbor() {
        let request = SyntheticRequest {
            file_path: "status.synth".to_string(),
            operation: Operation::Read,
            data: None,
            params: HashMap::from([
                ("format".to_string(), serde_cbor::Value::Text("json".to_string())),
            ]),
        };

        // Test CBOR serialization/deserialization
        let cbor_data = serde_cbor::to_vec(&request).unwrap();
        let decoded: SyntheticRequest = serde_cbor::from_slice(&cbor_data).unwrap();

        assert_eq!(decoded.file_path, "status.synth");
        assert!(matches!(decoded.operation, Operation::Read));
    }
}