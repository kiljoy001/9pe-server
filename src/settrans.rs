//! Virtual settrans system using synthetic filesystem
//!
//! Provides translator management through virtual directories and files
//! that exist only in the 9P namespace, not on physical disk.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tracing::{info, error};

use crate::synth::{SyntheticFilesystem, ControlHandler};
use crate::wasm::ThreadSafeTranslatorRegistry;

/// Translator state and metadata
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranslatorInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub mount_point: String,
    pub wasm_data: Vec<u8>,
    pub status: TranslatorStatus,
    pub installed_at: chrono::DateTime<chrono::Utc>,
    pub last_accessed: Option<chrono::DateTime<chrono::Utc>>,
    pub error_count: u32,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TranslatorStatus {
    Available,
    Enabled,
    Disabled,
    Error(String),
}

/// Commands for translator management
#[derive(Debug, Clone)]
pub enum SettransCommand {
    Enable(String),
    Disable(String),
    Uninstall(String),
    Install { name: String, data: Vec<u8> },
    Refresh,
    Status,
}

/// Virtual settrans system - translator management through synthetic filesystem
pub struct VirtualSettransSystem {
    /// Base directory (/srv/settrans) - virtual only
    #[allow(dead_code)]
    base_dir: PathBuf,
    /// Synthetic filesystem for virtual directories
    synth_fs: Arc<SyntheticFilesystem>,
    /// Registry for WASM translators
    #[allow(dead_code)]
    translator_registry: Arc<ThreadSafeTranslatorRegistry>,
    /// Known translators and their state
    #[allow(dead_code)]
    translators: Arc<RwLock<HashMap<String, TranslatorInfo>>>,
    /// Command channel for control operations
    command_tx: mpsc::UnboundedSender<SettransCommand>,
}

impl VirtualSettransSystem {
    /// Create new virtual settrans system
    pub async fn new(
        synth_fs: Arc<SyntheticFilesystem>,
        translator_registry: Arc<ThreadSafeTranslatorRegistry>,
    ) -> Result<Self> {
        let base_dir = PathBuf::from("/srv/settrans");

        // Create virtual directory structure in synthetic filesystem
        Self::create_virtual_structure(&base_dir, &synth_fs).await?;

        let (command_tx, mut command_rx) = mpsc::unbounded_channel();
        let translators = Arc::new(RwLock::new(HashMap::new()));

        // Create control file handlers
        let cmd_tx = command_tx.clone();

        // Enable control file handler
        struct EnableHandler {
            cmd_tx: mpsc::UnboundedSender<SettransCommand>,
        }
        impl ControlHandler for EnableHandler {
            fn read(&self) -> Result<Vec<u8>> {
                Ok(b"Write translator name to enable\n".to_vec())
            }
            fn write(&self, data: &[u8]) -> Result<()> {
                let name = String::from_utf8_lossy(data).trim().to_string();
                self.cmd_tx.send(SettransCommand::Enable(name))?;
                Ok(())
            }
        }

        synth_fs.create_control_file(
            &base_dir.join("enable"),
            Arc::new(EnableHandler { cmd_tx: cmd_tx.clone() })
        ).await?;

        // Disable control file handler
        struct DisableHandler {
            cmd_tx: mpsc::UnboundedSender<SettransCommand>,
        }
        impl ControlHandler for DisableHandler {
            fn read(&self) -> Result<Vec<u8>> {
                Ok(b"Write translator name to disable\n".to_vec())
            }
            fn write(&self, data: &[u8]) -> Result<()> {
                let name = String::from_utf8_lossy(data).trim().to_string();
                self.cmd_tx.send(SettransCommand::Disable(name))?;
                Ok(())
            }
        }

        synth_fs.create_control_file(
            &base_dir.join("disable"),
            Arc::new(DisableHandler { cmd_tx: cmd_tx.clone() })
        ).await?;

        // Status control file handler
        struct StatusHandler {
            translators: Arc<RwLock<HashMap<String, TranslatorInfo>>>,
        }
        impl ControlHandler for StatusHandler {
            fn read(&self) -> Result<Vec<u8>> {
                // This would need to be async in real implementation
                let translators = futures::executor::block_on(self.translators.read());
                let mut status = String::new();
                for (name, info) in translators.iter() {
                    status.push_str(&format!("{}: {:?}\n", name, info.status));
                }
                if status.is_empty() {
                    status = "No translators installed\n".to_string();
                }
                Ok(status.into_bytes())
            }
            fn write(&self, _data: &[u8]) -> Result<()> {
                Ok(()) // Status is read-only
            }
        }

        synth_fs.create_control_file(
            &base_dir.join("status"),
            Arc::new(StatusHandler { translators: Arc::clone(&translators) })
        ).await?;

        // Start command processor
        let translators_clone = Arc::clone(&translators);
        let registry_clone = Arc::clone(&translator_registry);
        let synth_fs_clone = Arc::clone(&synth_fs);
        let base_dir_clone = base_dir.clone();

        tokio::spawn(async move {
            while let Some(command) = command_rx.recv().await {
                match command {
                    SettransCommand::Enable(name) => {
                        Self::handle_enable(&name, &translators_clone, &registry_clone).await;
                    }
                    SettransCommand::Disable(name) => {
                        Self::handle_disable(&name, &translators_clone, &registry_clone).await;
                    }
                    SettransCommand::Install { name, data } => {
                        Self::handle_install(
                            &name,
                            data,
                            &translators_clone,
                            &synth_fs_clone,
                            &base_dir_clone
                        ).await;
                    }
                    _ => {}
                }
            }
        });

        Ok(Self {
            base_dir,
            synth_fs,
            translator_registry,
            translators,
            command_tx,
        })
    }

    /// Create virtual directory structure
    async fn create_virtual_structure(
        base_dir: &Path,
        synth_fs: &Arc<SyntheticFilesystem>
    ) -> Result<()> {
        // Create base directory
        synth_fs.create_directory(base_dir).await?;

        // Create subdirectories
        let directories = [
            "install",      // Drop WASM files here
            "available",    // List installed translators
            "enabled",      // Currently active translators
            "disabled",     // Disabled translators
        ];

        for dir in &directories {
            synth_fs.create_directory(&base_dir.join(dir)).await?;
        }

        info!("Virtual settrans structure created at {:?} (synthetic filesystem only)", base_dir);
        Ok(())
    }

    /// Handle enable command
    async fn handle_enable(
        name: &str,
        translators: &Arc<RwLock<HashMap<String, TranslatorInfo>>>,
        registry: &Arc<ThreadSafeTranslatorRegistry>,
    ) {
        let mut trans = translators.write().await;
        if let Some(info) = trans.get_mut(name) {
            // Load translator into registry
            match registry.load_translator(
                info.name.clone(),
                PathBuf::from(&info.mount_point),
                info.wasm_data.clone(),
            ).await {
                Ok(_) => {
                    info.status = TranslatorStatus::Enabled;
                    info!("Enabled translator: {}", name);
                }
                Err(e) => {
                    error!("Failed to enable translator {}: {}", name, e);
                    info.status = TranslatorStatus::Error(e.to_string());
                }
            }
        }
    }

    /// Handle disable command
    async fn handle_disable(
        name: &str,
        translators: &Arc<RwLock<HashMap<String, TranslatorInfo>>>,
        registry: &Arc<ThreadSafeTranslatorRegistry>,
    ) {
        let mut trans = translators.write().await;
        if let Some(info) = trans.get_mut(name) {
            // Remove from registry
            let mount_path = PathBuf::from(&info.mount_point);
            match registry.remove_translator(&mount_path).await {
                Ok(_) => {
                    info.status = TranslatorStatus::Disabled;
                    info!("Disabled translator: {}", name);
                }
                Err(e) => {
                    error!("Failed to disable translator {}: {}", name, e);
                }
            }
        }
    }

    /// Handle install command (when WASM is dropped into /install)
    async fn handle_install(
        name: &str,
        data: Vec<u8>,
        translators: &Arc<RwLock<HashMap<String, TranslatorInfo>>>,
        synth_fs: &Arc<SyntheticFilesystem>,
        base_dir: &Path,
    ) {
        let info = TranslatorInfo {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            description: format!("WASM translator: {}", name),
            mount_point: format!("/srv/{}", name),
            wasm_data: data,
            status: TranslatorStatus::Available,
            installed_at: chrono::Utc::now(),
            last_accessed: None,
            error_count: 0,
            last_error: None,
        };

        // Create virtual file in /available
        synth_fs.create_file(
            &base_dir.join("available").join(name),
            name.as_bytes().to_vec(),
            false
        ).await.ok();

        // Store translator info
        translators.write().await.insert(name.to_string(), info);
        info!("Installed translator: {}", name);
    }

    /// Get the synthetic filesystem
    pub fn get_synth_fs(&self) -> &Arc<SyntheticFilesystem> {
        &self.synth_fs
    }

    /// Install a WASM translator
    pub async fn install_translator(&self, name: String, wasm_data: Vec<u8>) -> Result<()> {
        self.command_tx.send(SettransCommand::Install { name, data: wasm_data })?;
        Ok(())
    }

    /// Enable a translator
    pub async fn enable_translator(&self, name: &str) -> Result<()> {
        self.command_tx.send(SettransCommand::Enable(name.to_string()))?;
        Ok(())
    }

    /// Disable a translator
    pub async fn disable_translator(&self, name: &str) -> Result<()> {
        self.command_tx.send(SettransCommand::Disable(name.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_translator_info_serialization() {
        let info = TranslatorInfo {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: "Test translator".to_string(),
            mount_point: "/srv/test".to_string(),
            wasm_data: vec![1, 2, 3],
            status: TranslatorStatus::Available,
            installed_at: chrono::Utc::now(),
            last_accessed: None,
            error_count: 0,
            last_error: None,
        };

        // Should serialize/deserialize without error
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: TranslatorInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info.name, deserialized.name);
    }

    #[test]
    fn test_translator_status_variants() {
        let statuses = vec![
            TranslatorStatus::Available,
            TranslatorStatus::Enabled,
            TranslatorStatus::Disabled,
            TranslatorStatus::Error("test error".to_string()),
        ];

        for status in statuses {
            // Should serialize without panic
            let json = serde_json::to_string(&status).unwrap();
            let _: TranslatorStatus = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn test_translator_info_equality() {
        let info1 = TranslatorInfo {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: "Test".to_string(),
            mount_point: "/srv/test".to_string(),
            wasm_data: vec![1, 2, 3],
            status: TranslatorStatus::Available,
            installed_at: chrono::Utc::now(),
            last_accessed: None,
            error_count: 0,
            last_error: None,
        };

        let mut info2 = info1.clone();
        assert_eq!(info1, info2);

        info2.error_count = 5;
        assert_ne!(info1, info2);
    }

    #[test]
    fn test_settrans_command_variants() {
        let commands = vec![
            SettransCommand::Enable("test".to_string()),
            SettransCommand::Disable("test".to_string()),
            SettransCommand::Uninstall("test".to_string()),
            SettransCommand::Install { name: "test".to_string(), data: vec![1, 2, 3] },
            SettransCommand::Refresh,
            SettransCommand::Status,
        ];

        // All commands should be constructable
        assert_eq!(commands.len(), 6);
    }

    #[test]
    fn test_translator_status_error_message() {
        let error_msg = "Failed to load WASM module".to_string();
        let status = TranslatorStatus::Error(error_msg.clone());

        if let TranslatorStatus::Error(msg) = status {
            assert_eq!(msg, error_msg);
        } else {
            panic!("Expected Error variant");
        }
    }

    /// Fuzz test: TranslatorInfo should handle arbitrary data
    #[test]
    fn fuzz_translator_info() {
        use proptest::prelude::*;

        proptest!(|(name in ".*", version in ".*", data: Vec<u8>)| {
            let info = TranslatorInfo {
                name,
                version,
                description: "Test".to_string(),
                mount_point: "/srv/test".to_string(),
                wasm_data: data,
                status: TranslatorStatus::Available,
                installed_at: chrono::Utc::now(),
                last_accessed: None,
                error_count: 0,
                last_error: None,
            };
            // Should not panic with arbitrary strings/data
            let _ = info.name.len();
        });
    }

    /// Fuzz test: Status serialization with arbitrary error messages
    #[test]
    fn fuzz_status_errors() {
        use proptest::prelude::*;

        proptest!(|(error_msg in ".*")| {
            let status = TranslatorStatus::Error(error_msg);
            let json = serde_json::to_string(&status).unwrap();
            let _: TranslatorStatus = serde_json::from_str(&json).unwrap();
        });
    }
}
