//! Revolutionary filesystem-based translator management system
//!
//! The settrans system provides Plan 9 style translator management through synthetic files.
//! Drop WASM files into /settrans/install/ and control translators through filesystem operations.

use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::{RwLock, mpsc};
use tokio::time::{Duration, interval};
use tracing::{info, debug, error};

use crate::wasm::ThreadSafeTranslatorRegistry;

/// Translator state and metadata
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranslatorInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub mount_point: String,
    pub wasm_path: PathBuf,
    pub config_path: Option<PathBuf>,
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
    Installing,
    Uninstalling,
}

impl std::fmt::Display for TranslatorStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TranslatorStatus::Available => write!(f, "available"),
            TranslatorStatus::Enabled => write!(f, "enabled"),
            TranslatorStatus::Disabled => write!(f, "disabled"),
            TranslatorStatus::Error(err) => write!(f, "error: {}", err),
            TranslatorStatus::Installing => write!(f, "installing"),
            TranslatorStatus::Uninstalling => write!(f, "uninstalling"),
        }
    }
}

/// Commands for translator management
#[derive(Debug, Clone)]
pub enum SettransCommand {
    Enable(String),
    Disable(String),
    Uninstall(String),
    Refresh,
    Status,
}

/// The settrans system - revolutionary filesystem-based translator management
pub struct SettransSystem {
    /// Base directory (/settrans)
    base_dir: PathBuf,
    /// Registry for WASM translators
    translator_registry: Arc<ThreadSafeTranslatorRegistry>,
    /// Known translators and their state
    translators: Arc<RwLock<HashMap<String, TranslatorInfo>>>,
    /// Command channel for control operations
    command_tx: mpsc::UnboundedSender<SettransCommand>,
    /// Install watcher handle
    install_watcher: Option<InstallWatcher>,
}

impl SettransSystem {
    /// Create new settrans system
    pub async fn new(
        base_dir: PathBuf,
        translator_registry: Arc<ThreadSafeTranslatorRegistry>,
    ) -> Result<Self> {
        // Create directory structure
        Self::create_directories(&base_dir).await?;

        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let translators = Arc::new(RwLock::new(HashMap::new()));

        let mut system = Self {
            base_dir: base_dir.clone(),
            translator_registry: translator_registry.clone(),
            translators: translators.clone(),
            command_tx,
            install_watcher: None,
        };

        // Start command processor
        let base_dir_clone = base_dir.clone();
        let translators_clone = translators.clone();
        let registry_clone = translator_registry.clone();
        tokio::spawn(async move {
            Self::command_processor(
                command_rx,
                base_dir_clone,
                translators_clone,
                registry_clone,
            ).await;
        });

        // Start install watcher
        let install_watcher = InstallWatcher::new(
            base_dir.join("install"),
            system.command_tx.clone(),
        ).await?;
        system.install_watcher = Some(install_watcher);

        // Initial scan
        system.scan_available_translators().await?;

        info!("Settrans system initialized at {:?}", base_dir);
        Ok(system)
    }

    /// Create the /settrans directory structure
    async fn create_directories(base_dir: &Path) -> Result<()> {
        let directories = [
            "install",      // Drop WASM files here
            "available",    // List installed translators
            "enabled",      // Currently active translators
            "disabled",     // Disabled translators
            "status",       // Status information
        ];

        fs::create_dir_all(base_dir).await?;

        for dir in &directories {
            fs::create_dir_all(base_dir.join(dir)).await?;
        }

        // Create control files
        let control_files = [
            ("enable", "Write translator name to enable"),
            ("disable", "Write translator name to disable"),
            ("uninstall", "Write translator name to uninstall"),
            ("refresh", "Write anything to refresh translator list"),
            ("status", "Read current status of all translators"),
        ];

        for (file, description) in &control_files {
            let file_path = base_dir.join(file);
            if !file_path.exists() {
                fs::write(&file_path, format!("# {}\n", description)).await?;
            }
        }

        Ok(())
    }

    /// Scan for available translators
    async fn scan_available_translators(&self) -> Result<()> {
        let available_dir = self.base_dir.join("available");
        let mut translators = self.translators.write().await;

        // Clear existing available translators
        translators.retain(|_, info| info.status != TranslatorStatus::Available);

        // Scan available directory
        if available_dir.exists() {
            let mut entries = fs::read_dir(&available_dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                if entry.file_type().await?.is_file() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.ends_with(".json") {
                        if let Ok(info) = self.load_translator_info(&entry.path()).await {
                            translators.insert(info.name.clone(), info);
                        }
                    }
                }
            }
        }

        self.update_status_files().await?;
        Ok(())
    }

    /// Load translator info from JSON file
    async fn load_translator_info(&self, config_path: &Path) -> Result<TranslatorInfo> {
        let content = fs::read_to_string(config_path).await?;
        let mut info: TranslatorInfo = serde_json::from_str(&content)?;

        // Verify WASM file exists
        if !info.wasm_path.exists() {
            info.status = TranslatorStatus::Error("WASM file not found".to_string());
        }

        Ok(info)
    }

    /// Update synthetic status files
    async fn update_status_files(&self) -> Result<()> {
        let translators = self.translators.read().await;

        // Update available list
        let available: Vec<_> = translators
            .values()
            .filter(|t| t.status == TranslatorStatus::Available)
            .map(|t| t.name.clone())
            .collect();
        fs::write(
            self.base_dir.join("available").join("list"),
            available.join("\n")
        ).await?;

        // Update enabled list
        let enabled: Vec<_> = translators
            .values()
            .filter(|t| t.status == TranslatorStatus::Enabled)
            .map(|t| t.name.clone())
            .collect();
        fs::write(
            self.base_dir.join("enabled").join("list"),
            enabled.join("\n")
        ).await?;

        // Update disabled list
        let disabled: Vec<_> = translators
            .values()
            .filter(|t| t.status == TranslatorStatus::Disabled)
            .map(|t| t.name.clone())
            .collect();
        fs::write(
            self.base_dir.join("disabled").join("list"),
            disabled.join("\n")
        ).await?;

        // Update comprehensive status
        let mut status_content = String::new();
        status_content.push_str("# Translator Status Report\n");
        status_content.push_str(&format!("Generated: {}\n\n", chrono::Utc::now()));

        for info in translators.values() {
            status_content.push_str(&format!(
                "{}: {} ({})\n  Mount: {}\n  Version: {}\n  Errors: {}\n",
                info.name, info.status, info.description,
                info.mount_point, info.version, info.error_count
            ));
            if let Some(error) = &info.last_error {
                status_content.push_str(&format!("  Last Error: {}\n", error));
            }
            status_content.push('\n');
        }

        fs::write(self.base_dir.join("status"), status_content).await?;

        Ok(())
    }

    /// Command processor loop
    async fn command_processor(
        mut command_rx: mpsc::UnboundedReceiver<SettransCommand>,
        base_dir: PathBuf,
        translators: Arc<RwLock<HashMap<String, TranslatorInfo>>>,
        registry: Arc<ThreadSafeTranslatorRegistry>,
    ) {
        while let Some(command) = command_rx.recv().await {
            match command {
                SettransCommand::Enable(name) => {
                    if let Err(e) = Self::enable_translator(&name, &translators, &registry).await {
                        error!("Failed to enable translator {}: {}", name, e);
                    }
                }
                SettransCommand::Disable(name) => {
                    if let Err(e) = Self::disable_translator(&name, &translators, &registry).await {
                        error!("Failed to disable translator {}: {}", name, e);
                    }
                }
                SettransCommand::Uninstall(name) => {
                    if let Err(e) = Self::uninstall_translator(&name, &base_dir, &translators).await {
                        error!("Failed to uninstall translator {}: {}", name, e);
                    }
                }
                SettransCommand::Refresh => {
                    info!("Refreshing translator list");
                }
                SettransCommand::Status => {
                    debug!("Status requested");
                }
            }
        }
    }

    /// Enable a translator
    async fn enable_translator(
        name: &str,
        translators: &Arc<RwLock<HashMap<String, TranslatorInfo>>>,
        registry: &Arc<ThreadSafeTranslatorRegistry>,
    ) -> Result<()> {
        let mut translators_guard = translators.write().await;

        if let Some(info) = translators_guard.get_mut(name) {
            if info.status == TranslatorStatus::Available {
                // Note: ThreadSafeTranslatorRegistry doesn't have enable_translator method yet
                // For now, we just mark as enabled in our local state
                info.status = TranslatorStatus::Enabled;
                info!("Enabled translator: {}", name);
            }
        }

        Ok(())
    }

    /// Disable a translator
    async fn disable_translator(
        name: &str,
        translators: &Arc<RwLock<HashMap<String, TranslatorInfo>>>,
        _registry: &Arc<ThreadSafeTranslatorRegistry>,
    ) -> Result<()> {
        let mut translators_guard = translators.write().await;

        if let Some(info) = translators_guard.get_mut(name) {
            if info.status == TranslatorStatus::Enabled {
                // Note: In current implementation, we just mark as disabled
                // The registry doesn't have an unload method yet
                info.status = TranslatorStatus::Disabled;
                info!("Disabled translator: {}", name);
            }
        }

        Ok(())
    }

    /// Uninstall a translator
    async fn uninstall_translator(
        name: &str,
        base_dir: &Path,
        translators: &Arc<RwLock<HashMap<String, TranslatorInfo>>>,
    ) -> Result<()> {
        let mut translators_guard = translators.write().await;

        if let Some(info) = translators_guard.remove(name) {
            // Remove files
            if info.wasm_path.exists() {
                fs::remove_file(&info.wasm_path).await?;
            }
            if let Some(config_path) = &info.config_path {
                if config_path.exists() {
                    fs::remove_file(config_path).await?;
                }
            }

            // Remove from available directory
            let available_path = base_dir.join("available").join(format!("{}.json", name));
            if available_path.exists() {
                fs::remove_file(available_path).await?;
            }

            info!("Uninstalled translator: {}", name);
        }

        Ok(())
    }

    /// Get command sender for external control
    pub fn command_sender(&self) -> mpsc::UnboundedSender<SettransCommand> {
        self.command_tx.clone()
    }

    /// Get current translator status
    pub async fn get_status(&self) -> HashMap<String, TranslatorInfo> {
        self.translators.read().await.clone()
    }
}

/// Watches /settrans/install for new WASM files
pub struct InstallWatcher {
    install_dir: PathBuf,
    command_tx: mpsc::UnboundedSender<SettransCommand>,
}

impl InstallWatcher {
    /// Create new install watcher
    pub async fn new(
        install_dir: PathBuf,
        command_tx: mpsc::UnboundedSender<SettransCommand>,
    ) -> Result<Self> {
        fs::create_dir_all(&install_dir).await?;

        let watcher = Self {
            install_dir: install_dir.clone(),
            command_tx: command_tx.clone(),
        };

        // Start watching loop
        let install_dir_clone = install_dir.clone();
        let command_tx_clone = command_tx.clone();
        tokio::spawn(async move {
            Self::watch_loop(install_dir_clone, command_tx_clone).await;
        });

        Ok(watcher)
    }

    /// Main watching loop
    async fn watch_loop(
        install_dir: PathBuf,
        command_tx: mpsc::UnboundedSender<SettransCommand>,
    ) {
        let mut interval = interval(Duration::from_secs(2));
        let mut known_files = std::collections::HashSet::new();

        loop {
            interval.tick().await;

            // Scan directory for new WASM files
            if let Ok(mut entries) = fs::read_dir(&install_dir).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();
                    if path.extension().and_then(|s| s.to_str()) == Some("wasm") {
                        let file_name = path.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("")
                            .to_string();

                        if !known_files.contains(&file_name) {
                            known_files.insert(file_name.clone());

                            if let Err(e) = Self::install_wasm_file(&path, &command_tx).await {
                                error!("Failed to install {}: {}", file_name, e);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Install a new WASM file
    async fn install_wasm_file(
        wasm_path: &Path,
        command_tx: &mpsc::UnboundedSender<SettransCommand>,
    ) -> Result<()> {
        let file_name = wasm_path.file_stem()
            .and_then(|s| s.to_str())
            .context("Invalid file name")?;

        info!("Installing WASM translator: {}", file_name);

        // Look for accompanying JSON config
        let config_path = wasm_path.with_extension("json");
        let translator_info = if config_path.exists() {
            let content = fs::read_to_string(&config_path).await?;
            serde_json::from_str::<serde_json::Value>(&content)?
        } else {
            // Create default config
            serde_json::json!({
                "name": file_name,
                "version": "1.0.0",
                "description": format!("Auto-discovered translator: {}", file_name),
                "mount_point": format!("/srv/{}", file_name)
            })
        };

        // Create translator info
        let info = TranslatorInfo {
            name: translator_info["name"].as_str().unwrap_or(file_name).to_string(),
            version: translator_info["version"].as_str().unwrap_or("1.0.0").to_string(),
            description: translator_info["description"].as_str().unwrap_or("").to_string(),
            mount_point: translator_info["mount_point"].as_str()
                .unwrap_or(&format!("/srv/{}", file_name)).to_string(),
            wasm_path: wasm_path.to_path_buf(),
            config_path: if config_path.exists() { Some(config_path) } else { None },
            status: TranslatorStatus::Available,
            installed_at: chrono::Utc::now(),
            last_accessed: None,
            error_count: 0,
            last_error: None,
        };

        // Move to available directory
        let available_dir = wasm_path.parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("available");

        let new_wasm_path = available_dir.join(wasm_path.file_name().unwrap());
        let new_config_path = available_dir.join(format!("{}.json", info.name));

        fs::rename(wasm_path, &new_wasm_path).await?;

        // Update info with new path
        let updated_info = TranslatorInfo {
            wasm_path: new_wasm_path,
            config_path: Some(new_config_path.clone()),
            ..info
        };

        // Save config
        fs::write(&new_config_path, serde_json::to_string_pretty(&updated_info)?).await?;

        // Trigger refresh
        let _ = command_tx.send(SettransCommand::Refresh);

        info!("Successfully installed translator: {}", updated_info.name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_settrans_creation() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path().to_path_buf();

        let registry = Arc::new(TranslatorRegistry::new(base_path.join("translators")));
        let settrans = SettransSystem::new(base_path.join("settrans"), registry).await;

        assert!(settrans.is_ok());
    }

    #[tokio::test]
    async fn test_directory_structure() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path().join("settrans");

        SettransSystem::create_directories(&base_path).await.unwrap();

        assert!(base_path.join("install").exists());
        assert!(base_path.join("available").exists());
        assert!(base_path.join("enabled").exists());
        assert!(base_path.join("status").exists());
    }
}