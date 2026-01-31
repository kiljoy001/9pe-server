use crate::traits::{StorageProvider, FileAttr, DirEntry};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use std::path::{Path, PathBuf};

/// HtmlRenderer wraps a StorageProvider and converts certain files to HTML
pub struct HtmlRenderer {
    source: Arc<dyn StorageProvider>,
}

impl HtmlRenderer {
    pub fn new(source: Arc<dyn StorageProvider>) -> Self {
        Self { source }
    }

    fn is_gemini_body(&self, path: &Path) -> bool {
        path.ends_with("body")
    }

    fn gmi_to_html(&self, gmi: &str) -> String {
        let mut html = String::from("<html><body>\n");
        for line in gmi.lines() {
            if line.starts_with("# ") {
                html.push_str(&format!("<h1>{}</h1>\n", &line[2..]));
            } else if line.starts_with("## ") {
                html.push_str(&format!("<h2>{}</h2>\n", &line[3..]));
            } else if line.starts_with("=> ") {
                let parts: Vec<&str> = line[3..].splitn(2, ' ').collect();
                let url = parts[0];
                let label = if parts.len() > 1 { parts[1] } else { url };
                html.push_str(&format!("<p><a href=\"{}\">{}</a></p>\n", url, label));
            } else {
                html.push_str(&format!("<p>{}</p>\n", line));
            }
        }
        html.push_str("</body></html>");
        html
    }
}

#[async_trait]
impl StorageProvider for HtmlRenderer {
    async fn read(&self, path: &Path, offset: u64, size: u32) -> Result<Vec<u8>> {
        if path.extension().and_then(|s| s.to_str()) == Some("html") {
            // Try to read the non-html version
            let base_path = path.with_extension("");

            // Check if this is a Gemini response by looking at the header
            let header_path = base_path.parent().unwrap_or(base_path.as_path()).join("header");
            let html = if let Ok(header_data) = self.source.read(&header_path, 0, 1024).await {
                let header_str = String::from_utf8_lossy(&header_data);
                let status_code = header_str.split_whitespace().next().unwrap_or("");

                // Check for Gemini redirect (3x status codes)
                if status_code.starts_with('3') {
                    let redirect_url = header_str.trim().split_whitespace().skip(1).collect::<Vec<_>>().join(" ");
                    format!(
                        "<html><head><meta http-equiv=\"refresh\" content=\"0; url={}\"></head><body>\
                        <h1>Redirect</h1><p>This page has moved to: <a href=\"{}\">{}</a></p>\
                        </body></html>",
                        redirect_url, redirect_url, redirect_url
                    )
                } else if status_code.starts_with('2') {
                    // Success - render the body
                    let gmi_data = self.source.read(&base_path, 0, 1024 * 1024).await?;
                    let gmi_str = String::from_utf8_lossy(&gmi_data);
                    self.gmi_to_html(&gmi_str)
                } else {
                    // Error or other status
                    format!(
                        "<html><body><h1>Gemini Status: {}</h1><pre>{}</pre></body></html>",
                        status_code, header_str.trim()
                    )
                }
            } else {
                // No header file - just try to render body as-is
                let gmi_data = self.source.read(&base_path, 0, 1024 * 1024).await?;
                let gmi_str = String::from_utf8_lossy(&gmi_data);
                self.gmi_to_html(&gmi_str)
            };

            let bytes = html.as_bytes();
            let start = offset.min(bytes.len() as u64) as usize;
            let end = (offset + size as u64).min(bytes.len() as u64) as usize;
            return Ok(bytes[start..end].to_vec());
        }

        self.source.read(path, offset, size).await
    }

    async fn write(&self, path: &Path, offset: u64, data: &[u8]) -> Result<u32> {
        self.source.write(path, offset, data).await
    }

    async fn stat(&self, path: &Path) -> Result<FileAttr> {
        if path.extension().and_then(|s| s.to_str()) == Some("html") {
            let base_path = path.with_extension("");
            let mut attr = self.source.stat(&base_path).await?;
            // Size is unknown until rendered, but let's approximate or just say it's large
            attr.size = 1024 * 1024; 
            return Ok(attr);
        }
        self.source.stat(path).await
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let mut entries = self.source.read_dir(path).await?;
        // For every file, add a .html version
        let mut extra = Vec::new();
        for entry in &entries {
            if entry.name == "body" {
                 extra.push(DirEntry {
                     name: "body.html".to_string(),
                     is_dir: false,
                 });
            }
        }
        entries.extend(extra);
        Ok(entries)
    }

    async fn create_dir(&self, path: &Path, mode: u32) -> Result<()> {
        self.source.create_dir(path, mode).await
    }

    async fn create_file(&self, path: &Path, mode: u32) -> Result<()> {
        self.source.create_file(path, mode).await
    }

    async fn remove_file(&self, path: &Path) -> Result<()> {
        self.source.remove_file(path).await
    }

    async fn remove_dir(&self, path: &Path) -> Result<()> {
        self.source.remove_dir(path).await
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        self.source.rename(from, to).await
    }

    async fn truncate(&self, path: &Path, size: u64) -> Result<()> {
        self.source.truncate(path, size).await
    }

    async fn set_permissions(&self, path: &Path, mode: u32) -> Result<()> {
        self.source.set_permissions(path, mode).await
    }
}
