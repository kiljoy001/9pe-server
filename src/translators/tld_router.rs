use crate::traits::{StorageProvider, FileAttr, DirEntry};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use std::path::{Path, PathBuf};
use tracing::debug;

/// TLD Router handles magic paths based on extensions (.9, .gem, .bloat)
pub struct TldRouter {
    gemini: Arc<dyn StorageProvider>,
    hyper: Arc<dyn StorageProvider>,
    // bloat: Arc<dyn StorageProvider>, // Future: HTTP bridge
    fallback: Arc<dyn StorageProvider>,
}

impl TldRouter {
    pub fn new(
        gemini: Arc<dyn StorageProvider>,
        hyper: Arc<dyn StorageProvider>,
        fallback: Arc<dyn StorageProvider>,
    ) -> Self {
        Self {
            gemini,
            hyper,
            fallback,
        }
    }

    fn resolve_tld(&self, path: &Path) -> (PathBuf, Arc<dyn StorageProvider>) {
        let path_str = path.to_string_lossy();
        
        // Very basic TLD detection: look for .gem, .9, .bloat in the first component after mount point
        // In our case, we assume this is mounted at /n/web
        // So path might be /foo.gem/bar
        
        let components: Vec<_> = path.components().collect();
        if components.len() > 1 {
            // components[0] is root /
            // components[1] is the "domain.tld" part
            if let std::path::Component::Normal(os_str) = components[1] {
                let s = os_str.to_string_lossy();
                if s.ends_with(".gem") {
                    debug!("TLD Router: Routing {} to Gemini", path_str);
                    // Strip the domain part for the translator? 
                    // No, most translators expect the domain as the root.
                    // Actually, GeminiTranslator expect /domain.com/path
                    return (path.to_path_buf(), self.gemini.clone());
                } else if s.ends_with(".9") || s.ends_with(".hyper") {
                    debug!("TLD Router: Routing {} to Hypercore", path_str);
                    return (path.to_path_buf(), self.hyper.clone());
                }
            }
        }

        (path.to_path_buf(), self.fallback.clone())
    }
}

#[async_trait]
impl StorageProvider for TldRouter {
    async fn read(&self, path: &Path, offset: u64, size: u32) -> Result<Vec<u8>> {
        let (p, provider) = self.resolve_tld(path);
        provider.read(&p, offset, size).await
    }

    async fn write(&self, path: &Path, offset: u64, data: &[u8]) -> Result<u32> {
        let (p, provider) = self.resolve_tld(path);
        provider.write(&p, offset, data).await
    }

    async fn stat(&self, path: &Path) -> Result<FileAttr> {
        let (p, provider) = self.resolve_tld(path);
        provider.stat(&p).await
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let (p, provider) = self.resolve_tld(path);
        provider.read_dir(&p).await
    }

    async fn create_dir(&self, path: &Path, mode: u32) -> Result<()> {
        let (p, provider) = self.resolve_tld(path);
        provider.create_dir(&p, mode).await
    }

    async fn create_file(&self, path: &Path, mode: u32) -> Result<()> {
        let (p, provider) = self.resolve_tld(path);
        provider.create_file(&p, mode).await
    }

    async fn remove_file(&self, path: &Path) -> Result<()> {
        let (p, provider) = self.resolve_tld(path);
        provider.remove_file(&p).await
    }

    async fn remove_dir(&self, path: &Path) -> Result<()> {
        let (p, provider) = self.resolve_tld(path);
        provider.remove_dir(&p).await
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let (p_from, provider_from) = self.resolve_tld(from);
        let (p_to, _) = self.resolve_tld(to);
        provider_from.rename(&p_from, &p_to).await
    }

    async fn truncate(&self, path: &Path, size: u64) -> Result<()> {
        let (p, provider) = self.resolve_tld(path);
        provider.truncate(&p, size).await
    }

    async fn set_permissions(&self, path: &Path, mode: u32) -> Result<()> {
        let (p, provider) = self.resolve_tld(path);
        provider.set_permissions(&p, mode).await
    }
}
