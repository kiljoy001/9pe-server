//! Settrans - Filesystem-based translator management
//!
//! Install and manage WASM translators through the filesystem itself

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use async_trait::async_trait;

use crate::synthetic::SyntheticGenerator;
use crate::wasm_translator::{TranslatorRegistry, TranslatorMetadata};

/// The /settrans directory structure:
///
/// /settrans/
///   install/       - Drop WASM files here to install
///   available/     - List of installed translators
///   enabled/       - Currently active translators
///   disable/       - Write translator name to disable
///   enable/        - Write translator name to enable
///   status/        - Current status of all translators

/// Installation watcher - monitors /settrans/install for new WASM files
pub struct InstallWatcher {
    registry: Arc<TranslatorRegistry>,
    install_dir: PathBuf,
}

impl InstallWatcher {
    pub fn new(registry: Arc<TranslatorRegistry>, base_path: PathBuf) -> Self {
        Self {
            registry,
            install_dir: base_path.join("settrans").join("install"),
        }
    }

    /// Watch for new WASM files and auto-install them
    pub async fn watch(&self) -> Result<()> {
        use tokio::fs;
        use tokio::time::{interval, Duration};

        // Create install directory if it doesn't exist
        fs::create_dir_all(&self.install_dir).await?;

        let mut check_interval = interval(Duration::from_secs(5));

        loop {
            check_interval.tick().await;
            self.check_for_new_translators().await?;
        }
    }

    async fn check_for_new_translators(&self) -> Result<()> {
        use tokio::fs;

        let mut entries = fs::read_dir(&self.install_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            // Check for WASM files
            if path.extension().and_then(|s| s.to_str()) == Some("wasm") {
                // Read the file
                let wasm_bytes = fs::read(&path).await?;

                // Look for metadata file
                let meta_path = path.with_extension("meta");
                let metadata = if meta_path.exists() {
                    let meta_content = fs::read_to_string(&meta_path).await?;
                    parse_metadata(&meta_content)?
                } else {
                    // Generate default metadata
                    let name = path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .to_string();

                    TranslatorMetadata {
                        name: name.clone(),
                        mount_point: format!("/trans/{}", name),
                        version: "1.0.0".to_string(),
                        description: "Auto-installed translator".to_string(),
                    }
                };

                // Install the translator
                self.registry.install_translator(wasm_bytes, metadata).await?;

                // Remove the source files
                fs::remove_file(&path).await?;
                if meta_path.exists() {
                    fs::remove_file(&meta_path).await?;
                }

                tracing::info!("Auto-installed translator from {}", path.display());
            }
        }

        Ok(())
    }
}

/// Parse metadata from simple text format
fn parse_metadata(content: &str) -> Result<TranslatorMetadata> {
    let mut name = None;
    let mut mount_point = None;
    let mut version = None;
    let mut description = None;

    for line in content.lines() {
        let parts: Vec<&str> = line.splitn(2, ':').collect();
        if parts.len() == 2 {
            let key = parts[0].trim();
            let value = parts[1].trim();

            match key {
                "name" => name = Some(value.to_string()),
                "mount" => mount_point = Some(value.to_string()),
                "version" => version = Some(value.to_string()),
                "description" => description = Some(value.to_string()),
                _ => {}
            }
        }
    }

    Ok(TranslatorMetadata {
        name: name.unwrap_or_else(|| "unknown".to_string()),
        mount_point: mount_point.unwrap_or_else(|| "/trans/unknown".to_string()),
        version: version.unwrap_or_else(|| "1.0.0".to_string()),
        description: description.unwrap_or_else(|| "No description".to_string()),
    })
}

/// Synthetic file for enabling translators
pub struct EnableFile {
    registry: Arc<TranslatorRegistry>,
}

impl EnableFile {
    pub fn new(registry: Arc<TranslatorRegistry>) -> Self {
        Self { registry }
    }

    pub async fn write(&self, data: &[u8]) -> Result<u32> {
        let name = String::from_utf8_lossy(data).trim().to_string();
        self.registry.enable_translator(&name).await?;
        Ok(data.len() as u32)
    }
}

/// Synthetic file for disabling translators
pub struct DisableFile {
    registry: Arc<TranslatorRegistry>,
}

impl DisableFile {
    pub fn new(registry: Arc<TranslatorRegistry>) -> Self {
        Self { registry }
    }

    pub async fn write(&self, data: &[u8]) -> Result<u32> {
        let name = String::from_utf8_lossy(data).trim().to_string();
        // TODO: Implement disable functionality
        tracing::info!("Would disable translator: {}", name);
        Ok(data.len() as u32)
    }
}

/// Status file showing all translators
#[derive(Clone)]
pub struct StatusFile {
    // Don't store the registry directly to avoid Sync issues
    // Instead, generate static status for now
}

impl StatusFile {
    pub fn new(_registry: Arc<TranslatorRegistry>) -> Self {
        Self { }
    }
}

#[async_trait]
impl SyntheticGenerator for StatusFile {
    async fn generate(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        let status = format!(
            "WASM Translator Status\n\
            =====================\n\
            \n\
            Enabled Translators:\n\
            - hello_world @ /trans/hello\n\
            - ai_chat @ /ai\n\
            - git_fs @ /git\n\
            \n\
            Available (not enabled):\n\
            - markdown_render\n\
            - json_query\n\
            \n\
            Total: 3 enabled, 2 available\n\
            \n\
            To install: cp translator.wasm /settrans/install/\n\
            To enable: echo 'translator_name' > /settrans/enable\n\
            To disable: echo 'translator_name' > /settrans/disable\n"
        );

        let bytes = status.as_bytes();
        let start = offset.min(bytes.len() as u64) as usize;
        let end = (start + count as usize).min(bytes.len());
        Ok(bytes[start..end].to_vec())
    }

    async fn size(&self) -> u64 {
        512 // Approximate
    }
}

/// Example: A simple "Hello World" translator in WASM
/// Users would compile this to WASM and drop in /settrans/install/
pub const EXAMPLE_TRANSLATOR_SOURCE: &str = r#"
// hello_translator.c - Compile with: clang --target=wasm32 -nostdlib -o hello.wasm

// Memory allocator (required)
__attribute__((export_name("malloc")))
void* malloc(int size) {
    static char heap[65536];
    static int heap_ptr = 0;
    void* ptr = &heap[heap_ptr];
    heap_ptr += size;
    return ptr;
}

// 9P message handler (required entry point)
__attribute__((export_name("handle_9p_message")))
int handle_9p_message(char* msg, int len) {
    // Parse 9P message and generate response
    // This would implement actual 9P protocol handling

    // For now, return a simple response
    char* response = (char*)malloc(100);
    response[0] = 0; // Response length (4 bytes)
    response[1] = 0;
    response[2] = 0;
    response[3] = 10;

    // Simple response data
    const char* data = "Hello 9P!";
    for (int i = 0; i < 10; i++) {
        response[4 + i] = data[i];
    }

    return (int)response;
}

// File operations that translators can implement
__attribute__((export_name("translator_read")))
int translator_read(int fid, long offset, int count) {
    // Implement custom read behavior
    return 0;
}

__attribute__((export_name("translator_write")))
int translator_write(int fid, long offset, char* data, int count) {
    // Implement custom write behavior
    return 0;
}

// Metadata for the translator
__attribute__((export_name("get_metadata")))
const char* get_metadata() {
    return "name: hello\n"
           "mount: /trans/hello\n"
           "version: 1.0.0\n"
           "description: Hello World translator\n";
}
"#;

/// Register settrans management files
pub async fn register_settrans_files(
    fs: &crate::synthetic::SyntheticFileSystem,
    registry: Arc<TranslatorRegistry>,
) {
    // Status file
    fs.register(
        "/settrans/status".to_string(),
        Arc::new(StatusFile::new(registry.clone())),
    ).await;

    // Note: Enable/Disable would need bidirectional file support
    // For now, these would be handled specially by the server
}

/// Create the settrans directory structure
pub async fn create_settrans_structure(base_path: PathBuf) -> Result<()> {
    use tokio::fs;

    let settrans = base_path.join("settrans");

    // Create all directories
    fs::create_dir_all(&settrans).await?;
    fs::create_dir_all(settrans.join("install")).await?;
    fs::create_dir_all(settrans.join("available")).await?;
    fs::create_dir_all(settrans.join("enabled")).await?;

    // Create README
    let readme = "WASM Translator Management\n\
                  =========================\n\
                  \n\
                  To install a translator:\n\
                  cp your_translator.wasm /settrans/install/\n\
                  \n\
                  Optional: Include metadata file:\n\
                  cp your_translator.meta /settrans/install/\n\
                  \n\
                  Metadata format:\n\
                  name: translator_name\n\
                  mount: /mount/point\n\
                  version: 1.0.0\n\
                  description: What it does\n\
                  \n\
                  The translator will be auto-installed and appear in /settrans/available/\n";

    fs::write(settrans.join("README"), readme).await?;

    Ok(())
}