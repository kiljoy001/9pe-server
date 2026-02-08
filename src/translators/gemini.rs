//! Gemini Translator Bridge
//!
//! Bridges 9P filesystem operations to the Gemini protocol (the "Lightweight Web").
//! - Acts as a Gemini Client.
//! - Fetches content from `gemini://` URLs.
//! - Exposed as a StorageProvider where directories map to URLs and files map to content.

use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_rustls::rustls::pki_types::{ServerName, CertificateDer, UnixTime};
use tokio_rustls::rustls::client::danger::{ServerCertVerifier, ServerCertVerified, HandshakeSignatureValid};
use tokio_rustls::rustls::DigitallySignedStruct;
use tokio_rustls::TlsConnector;
use url::Url;
use std::path::{Path, PathBuf};
use async_trait::async_trait;

use crate::traits::{StorageProvider, FileAttr, DirEntry};
use crate::synth::SyntheticFilesystem;

/// Gemini Bridge handles the raw protocol interactions
#[derive(Clone)]
pub struct GeminiBridge {
    client_config: Arc<ClientConfig>,
}

impl GeminiBridge {
    pub fn new() -> Self {
        let root_store = RootCertStore::empty();
        
        let mut config = ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();
            
        // DANGEROUS: Accept any certificate for the purpose of "Gemini Browsing"
        // Self-signed certificates are the norm in Gemini space (TOFU model).
        #[derive(Debug)]
        struct Danger;
        
        impl ServerCertVerifier for Danger {
            fn verify_server_cert(
                &self,
                _end_entity: &CertificateDer<'_>,
                _intermediates: &[CertificateDer<'_>],
                _server_name: &ServerName<'_>,
                _ocsp_response: &[u8],
                _now: UnixTime,
            ) -> Result<ServerCertVerified, tokio_rustls::rustls::Error> {
                Ok(ServerCertVerified::assertion())
            }

            fn verify_tls12_signature(
                &self,
                _message: &[u8],
                _cert: &CertificateDer<'_>,
                _dss: &DigitallySignedStruct,
            ) -> Result<HandshakeSignatureValid, tokio_rustls::rustls::Error> {
                Ok(HandshakeSignatureValid::assertion())
            }

            fn verify_tls13_signature(
                &self,
                _message: &[u8],
                _cert: &CertificateDer<'_>,
                _dss: &DigitallySignedStruct,
            ) -> Result<HandshakeSignatureValid, tokio_rustls::rustls::Error> {
                Ok(HandshakeSignatureValid::assertion())
            }

            fn supported_verify_schemes(&self) -> Vec<tokio_rustls::rustls::SignatureScheme> {
                vec![
                    tokio_rustls::rustls::SignatureScheme::RSA_PKCS1_SHA1,
                    tokio_rustls::rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
                    tokio_rustls::rustls::SignatureScheme::RSA_PSS_SHA256,
                    tokio_rustls::rustls::SignatureScheme::RSA_PKCS1_SHA256,
                    tokio_rustls::rustls::SignatureScheme::ED25519,
                ]
            }
        }
        config.dangerous().set_certificate_verifier(Arc::new(Danger));

        Self {
            client_config: Arc::new(config),
        }
    }

    /// Fetch a Gemini URL
    pub async fn fetch(&self, url_str: &str) -> Result<(String, Vec<u8>)> {
        let url = Url::parse(url_str).context("Invalid Gemini URL")?;
        
        if url.scheme() != "gemini" {
            anyhow::bail!("Unsupported scheme: {}", url.scheme());
        }

        let host = url.host_str().context("Missing host")?;
        let port = url.port().unwrap_or(1965);
        let addr = format!("{}:{}", host, port);

        // Connect TCP
        let stream = TcpStream::connect(&addr).await.context("Simplified TCP connect failed")?;

        // Establish TLS
        let connector = TlsConnector::from(self.client_config.clone());
        let domain = ServerName::try_from(host)
            .map_err(|_| anyhow::anyhow!("Invalid DNS name"))?
            .to_owned();

        let mut tls_stream = connector.connect(domain, stream).await.context("TLS connect failed")?;

        // Send Request: <URL><CR><LF>
        let request = format!("{}\r\n", url_str);
        tls_stream.write_all(request.as_bytes()).await?;

        // Read Response
        let mut response_data = Vec::new();
        tls_stream.read_to_end(&mut response_data).await?;

        // Parse Header
        let header_end = response_data
            .windows(2)
            .position(|w| w == b"\r\n")
            .context("Invalid Gemini response (no CRLF)")?;

        let header_bytes = &response_data[..header_end];
        let body_bytes = &response_data[header_end + 2..];

        let header = String::from_utf8_lossy(header_bytes).to_string();
        
        Ok((header, body_bytes.to_vec()))
    }
}

/// Gemini Translator implements StorageProvider to allow mounting
pub struct GeminiTranslator {
    bridge: GeminiBridge,
    fs: SyntheticFilesystem,
}

impl GeminiTranslator {
    pub fn new() -> Self {
        Self {
            bridge: GeminiBridge::new(),
            fs: SyntheticFilesystem::new(),
        }
    }

    /// Convert a filesystem path to a Gemini URL
    /// e.g. /example.com/foo -> gemini://example.com/foo
    fn path_to_url(&self, path: &Path) -> Result<String> {
        let path_str = path.to_string_lossy();
        let trimmed = path_str.trim_start_matches('/');
        if trimmed.is_empty() {
            anyhow::bail!("Root path cannot be converted to URL");
        }
        Ok(format!("gemini://{}", trimmed))
    }

    /// Ensure a path is populated in the synthetic filesystem
    async fn ensure_populated(&self, path: &Path) -> Result<()> {
        if self.fs.exists(path).await {
            return Ok(());
        }

        // Try to fetch as a directory/page
        if let Ok(url) = self.path_to_url(path) {
            // Attempt fetch
            match self.bridge.fetch(&url).await {
                Ok((header, body)) => {
                    // Create directory for this path
                    self.fs.create_directory(path).await?;
                    
                    // Add "header" file
                    self.fs.create_file(&path.join("header"), header.as_bytes().to_vec(), false).await?;
                    
                    // Add "body" file
                    self.fs.create_file(&path.join("body"), body.clone(), false).await?;

                    // Simple parsing for links if text/gemini
                    if header.starts_with("20 text/gemini") {
                         let body_str = String::from_utf8_lossy(&body);
                         for line in body_str.lines() {
                             if line.starts_with("=>") {
                                 let parts: Vec<&str> = line.split_whitespace().collect();
                                 if parts.len() >= 2 {
                                     let link_url = parts[1];
                                     // Very basic link handling - good enough for prototype
                                     // Create a placeholder directory/file for the link
                                     // to allow "cd" into it.
                                     // We sanitize the name to be FS friendly
                                     let name = Path::new(link_url)
                                         .file_name()
                                         .map(|s| s.to_string_lossy().to_string())
                                         .unwrap_or_else(|| "link".to_string());
                                     
                                     // Avoid creating ".." or "."
                                     if name != ".." && name != "." {
                                         // Just create an empty dir to signify it exists
                                         // When accessed, ensure_populated will trigger fetch
                                         self.fs.create_directory(&path.join(&name)).await.ok();
                                     }
                                 }
                             }
                         }
                    }
                }
                Err(e) => {
                    // Could not fetch, maybe it's just a new domain user wants to 'cd' into
                    // We allow creating the directory IF it's a top-level domain-like path
                    // e.g. /google.com
                    if path.components().count() == 2 {
                         self.fs.create_directory(path).await?;
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        
        Ok(())
    }
}

#[async_trait]
impl StorageProvider for GeminiTranslator {
    async fn read(&self, path: &Path, offset: u64, size: u32) -> Result<Vec<u8>> {
        if !self.fs.exists(path).await {
            if let Some(parent) = path.parent() {
                 self.ensure_populated(parent).await.ok();
            }
        }
        self.fs.read_file(path).await.map(|content| {
            // Primitive offset/size handling
            if offset as usize >= content.len() {
                return Vec::new();
            }
            let end = std::cmp::min(offset as usize + size as usize, content.len());
            content[offset as usize..end].to_vec()
        })
    }

    async fn write(&self, path: &Path, _offset: u64, data: &[u8]) -> Result<u32> {
        self.fs.write_file(path, data.to_vec()).await?;
        Ok(data.len() as u32)
    }

    async fn stat(&self, path: &Path) -> Result<FileAttr> {
        if !self.fs.exists(path).await {
            self.ensure_populated(path).await.ok();
        }
        
        let node = self.fs.get_node(path).await.ok_or_else(|| anyhow::anyhow!("File not found"))?;
        
        // Map SynthNode to FileAttr
        // Note: is_dir calculation depends on SynthNode type
        let is_dir = matches!(node.node_type, crate::synth::SynthNodeType::Directory { .. });
        
        Ok(FileAttr {
            size: if is_dir { 0 } else { 
                match node.node_type {
                    crate::synth::SynthNodeType::File { content, .. } => content.len() as u64,
                    _ => 0,
                }
            },
            mode: node.permissions,
            mtime: node.modified.timestamp() as u64,
            is_dir,
        })
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        if !self.fs.exists(path).await {
            self.ensure_populated(path).await.ok();
        }
        
        let names = self.fs.list_directory(path).await?;
        let mut entries = Vec::new();
        
        // For each child name, check if it's a directory?
        // SyntheticFilesystem list_directory only returns names.
        // We might need to look them up to see if they are dirs.
        // For now, assume everything is a DirEntry if it exists.
        
        for name in names {
            let child_path = path.join(&name);
            let is_dir = if let Some(node) = self.fs.get_node(&child_path).await {
                 matches!(node.node_type, crate::synth::SynthNodeType::Directory { .. })
            } else {
                false
            };
            
            entries.push(DirEntry {
                name,
                is_dir,
            });
        }
        
        Ok(entries)
    }

    async fn create_dir(&self, path: &Path, _mode: u32) -> Result<()> {
        self.fs.create_directory(path).await
    }

    async fn create_file(&self, path: &Path, _mode: u32) -> Result<()> {
        // Assume empty file
        self.fs.create_file(path, Vec::new(), true).await
    }

    async fn remove_file(&self, path: &Path) -> Result<()> {
        self.fs.remove_node(path).await
    }

    async fn remove_dir(&self, path: &Path) -> Result<()> {
        self.fs.remove_node(path).await
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        self.fs.rename_node(from, to).await
    }

    async fn truncate(&self, path: &Path, size: u64) -> Result<()> {
        self.fs.truncate_file(path, size).await
    }

    async fn set_permissions(&self, path: &Path, mode: u32) -> Result<()> {
        self.fs.set_permissions(path, mode).await
    }
}
