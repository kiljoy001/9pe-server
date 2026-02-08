use crate::traits::{WasmProvider, Translator, WasmMetadata};
use crate::wasm::ThreadSafeTranslatorRegistry;
use crate::settrans::VirtualSettransSystem;
use async_trait::async_trait;
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

pub struct WasmRegistryAdapter {
    registry: Arc<ThreadSafeTranslatorRegistry>,
    settrans: Arc<VirtualSettransSystem>,
}

impl WasmRegistryAdapter {
    pub fn new(
        registry: Arc<ThreadSafeTranslatorRegistry>,
        settrans: Arc<VirtualSettransSystem>,
    ) -> Self {
        Self { registry, settrans }
    }
}

#[async_trait]
impl WasmProvider for WasmRegistryAdapter {
    async fn load_translator(&self, name: String, mount_point: &Path, bytecode: Vec<u8>) -> Result<()> {
        self.registry.load_translator(name, mount_point.to_path_buf(), bytecode).await
    }

    async fn remove_translator(&self, mount_point: &Path) -> Result<()> {
        self.registry.remove_translator(&mount_point.to_path_buf()).await
    }

    async fn get_translator(&self, path: &Path) -> Option<Arc<dyn Translator>> {
        let translator = self.registry.get_translator(&path.to_path_buf()).await?;
        Some(Arc::new(TranslatorAdapter { translator }))
    }

    async fn set_translator(&self, path: &str, translator_name: &str, args: Vec<String>) -> Result<()> {
        self.settrans.set_translator(path, translator_name, args).await
    }

    async fn list_translators(&self) -> Result<Vec<WasmMetadata>> {
        // This is a simplified implementation
        let names = self.registry.list_translators().await;
        let mut metadata = Vec::new();
        for name in names {
            metadata.push(WasmMetadata {
                name: name.clone(),
                version: "1.0.0".to_string(),
                description: format!("WASM translator: {}", name),
                mount_point: format!("/srv/{}", name),
                status: "Enabled".to_string(),
            });
        }
        Ok(metadata)
    }
}

pub struct TranslatorAdapter {
    translator: Arc<dyn crate::wasm::threadsafe::TranslatorBackend>,
}

#[async_trait]
impl Translator for TranslatorAdapter {
    async fn invoke_function(&self, function: &str, args: Vec<u8>) -> Result<Vec<u8>> {
        self.translator.invoke_function(function, args).await
    }

    fn name(&self) -> String {
        self.translator.name().to_string()
    }

    fn mount_point(&self) -> String {
        self.translator.mount_point().to_string_lossy().to_string()
    }
}
