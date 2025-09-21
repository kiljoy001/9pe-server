//! Integrated 9P.e Server with Synthetic Files, Translators, and Security
//!
//! This is the complete server that brings together all components

use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::{Result, Context};
use tracing::{info, warn, error, debug};

use plan9e::protocol::{NinePeeMessage, ProtocolError};

use crate::synthetic::{SyntheticFileSystem, SyntheticGenerator};
use crate::synthetic_advanced::{Plan9SyntheticFS, SyntheticFile};
use crate::translators::{TranslatorManager, Translator};
use crate::auth::{AuthService, SecurityContext, AuthMethod, Permissions};
use crate::metrics;

/// FID state - tracks what each FID points to
#[derive(Debug, Clone)]
enum FidTarget {
    RealFile(PathBuf),           // Actual filesystem path
    SyntheticFile(String),       // Synthetic file path
    Translator(String, String),  // (translator_name, subpath)
}

/// Complete Integrated Server
pub struct IntegratedServer {
    // Core components
    root: PathBuf,
    fids: Arc<RwLock<HashMap<u32, FidTarget>>>,

    // Advanced features
    synthetic_fs: Arc<SyntheticFileSystem>,
    plan9_synthetic: Arc<Plan9SyntheticFS>,
    translators: Arc<TranslatorManager>,
    auth_service: Arc<AuthService>,

    // Connection contexts
    contexts: Arc<RwLock<HashMap<u64, SecurityContext>>>,

    // Configuration
    max_message_size: u32,
    enable_synthetic: bool,
    enable_translators: bool,
    require_auth: bool,
}

impl IntegratedServer {
    pub fn new(root: PathBuf) -> Result<Self> {
        if !root.exists() {
            return Err(anyhow::anyhow!("Root path does not exist: {:?}", root));
        }

        Ok(Self {
            root: root.canonicalize()?,
            fids: Arc::new(RwLock::new(HashMap::new())),
            synthetic_fs: Arc::new(SyntheticFileSystem::new()),
            plan9_synthetic: Arc::new(Plan9SyntheticFS::new()),
            translators: Arc::new(TranslatorManager::new()),
            auth_service: Arc::new(AuthService::new()),
            contexts: Arc::new(RwLock::new(HashMap::new())),
            max_message_size: 8192,
            enable_synthetic: true,
            enable_translators: true,
            require_auth: false, // Set to true for production
        })
    }

    /// Initialize server with default synthetic files and translators
    pub async fn initialize(&self) -> Result<()> {
        info!("Initializing integrated 9P.e server");

        // Register default translators
        use crate::translators::*;

        self.translators.register(
            "http".to_string(),
            Arc::new(HttpTranslator::new("http://localhost:8080".to_string()))
        ).await?;

        self.translators.register(
            "git".to_string(),
            Arc::new(GitTranslator::new(PathBuf::from("."), "main".to_string()))
        ).await?;

        // Mount translators at specific paths
        self.translators.mount(PathBuf::from("/http"), "http".to_string()).await?;
        self.translators.mount(PathBuf::from("/git"), "git".to_string()).await?;

        // Add current process to /proc
        let pid = std::process::id();
        self.plan9_synthetic.add_process(pid).await;

        info!("Server initialized with synthetic files and translators");
        Ok(())
    }

    /// Process a 9P.e message with full integration
    pub async fn process_message(
        &self,
        msg: NinePeeMessage,
        conn_id: u64,
    ) -> Result<NinePeeMessage> {
        debug!("Processing message: {:?}", msg);

        // Check authentication if required
        if self.require_auth {
            let contexts = self.contexts.read().await;
            if let Some(ctx) = contexts.get(&conn_id) {
                if ctx.user.is_none() && !matches!(msg, NinePeeMessage::Auth { .. } | NinePeeMessage::Version { .. }) {
                    return Ok(NinePeeMessage::Error {
                        ename: "Authentication required".to_string(),
                        errno: 1,
                    });
                }
            }
        }

        match msg {
            NinePeeMessage::Version { msize, version } => {
                self.handle_version(msize, version).await
            }

            NinePeeMessage::Auth { afid, uname, aname } => {
                self.handle_auth(afid, uname, aname, conn_id).await
            }

            NinePeeMessage::Attach { fid, afid, uname, aname } => {
                self.handle_attach(fid, afid, uname, aname, conn_id).await
            }

            NinePeeMessage::Walk { fid, newfid, wnames } => {
                self.handle_walk(fid, newfid, wnames, conn_id).await
            }

            NinePeeMessage::Open { fid, mode } => {
                self.handle_open(fid, mode, conn_id).await
            }

            NinePeeMessage::Read { fid, offset, count } => {
                self.handle_read(fid, offset, count, conn_id).await
            }

            NinePeeMessage::Write { fid, offset, data } => {
                self.handle_write(fid, offset, data, conn_id).await
            }

            NinePeeMessage::Clunk { fid } => {
                self.handle_clunk(fid).await
            }

            NinePeeMessage::Stat { fid } => {
                self.handle_stat(fid).await
            }

            // Advanced 9P.e messages
            NinePeeMessage::Stream { fid, stream_type } => {
                self.handle_stream(fid, stream_type).await
            }

            NinePeeMessage::Synthetic { path, operation } => {
                self.handle_synthetic(path, operation).await
            }

            NinePeeMessage::Translator { fid, trans_type, config } => {
                self.handle_translator(fid, trans_type, config).await
            }

            _ => {
                Ok(NinePeeMessage::Error {
                    ename: "Not implemented".to_string(),
                    errno: 1,
                })
            }
        }
    }

    /// Handle authentication
    async fn handle_auth(
        &self,
        afid: u32,
        uname: String,
        aname: String,
        conn_id: u64,
    ) -> Result<NinePeeMessage> {
        // Simple password auth for demo
        let auth_method = AuthMethod::Password(aname.clone());

        match self.auth_service.authenticate(&auth_method).await {
            Ok(user) => {
                // Create security context
                let ctx = SecurityContext {
                    user: Some(user),
                    auth_method,
                    capabilities: vec![],
                    session_key: None,
                    ip_address: "127.0.0.1".parse().unwrap(),
                    authenticated_at: Some(std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs()),
                    mfa_verified: false,
                };

                self.contexts.write().await.insert(conn_id, ctx);

                Ok(NinePeeMessage::AuthResp {
                    aqid: 0, // Auth qid
                })
            }
            Err(e) => {
                Ok(NinePeeMessage::Error {
                    ename: format!("Auth failed: {}", e),
                    errno: 2,
                })
            }
        }
    }

    /// Handle walk with integrated path resolution
    async fn handle_walk(
        &self,
        fid: u32,
        newfid: u32,
        wnames: Vec<String>,
        conn_id: u64,
    ) -> Result<NinePeeMessage> {
        let fids = self.fids.read().await;
        let base_target = fids.get(&fid)
            .ok_or_else(|| anyhow::anyhow!("Unknown fid"))?;

        let new_target = match base_target {
            FidTarget::RealFile(base_path) => {
                // Build new path
                let mut new_path = base_path.clone();
                for name in &wnames {
                    new_path.push(name);
                }

                // Check if it's a synthetic file
                if self.enable_synthetic {
                    let path_str = new_path.to_string_lossy();
                    if self.synthetic_fs.is_synthetic(&path_str).await {
                        FidTarget::SyntheticFile(path_str.to_string())
                    } else if let Some(_) = self.plan9_synthetic.get(&path_str).await {
                        FidTarget::SyntheticFile(path_str.to_string())
                    } else {
                        // Check for translator
                        if self.enable_translators {
                            if let Some(trans) = self.translators.get_translator(&new_path).await {
                                FidTarget::Translator(trans.name().to_string(), path_str.to_string())
                            } else {
                                FidTarget::RealFile(new_path)
                            }
                        } else {
                            FidTarget::RealFile(new_path)
                        }
                    }
                } else {
                    FidTarget::RealFile(new_path)
                }
            }

            FidTarget::SyntheticFile(base) => {
                let mut path = base.clone();
                for name in &wnames {
                    path.push('/');
                    path.push_str(name);
                }
                FidTarget::SyntheticFile(path)
            }

            FidTarget::Translator(trans_name, base) => {
                let mut path = base.clone();
                for name in &wnames {
                    path.push('/');
                    path.push_str(name);
                }
                FidTarget::Translator(trans_name.clone(), path)
            }
        };

        // Check authorization
        if self.require_auth {
            let contexts = self.contexts.read().await;
            if let Some(ctx) = contexts.get(&conn_id) {
                let resource = match &new_target {
                    FidTarget::RealFile(p) => p.to_string_lossy().to_string(),
                    FidTarget::SyntheticFile(p) => p.clone(),
                    FidTarget::Translator(_, p) => p.clone(),
                };

                if !self.auth_service.authorize(ctx, &resource, Permissions::TRAVERSE).await? {
                    return Ok(NinePeeMessage::Error {
                        ename: "Permission denied".to_string(),
                        errno: 3,
                    });
                }
            }
        }

        // Store new FID
        drop(fids);
        self.fids.write().await.insert(newfid, new_target);

        Ok(NinePeeMessage::WalkResp {
            qids: vec![], // Would return proper qids
        })
    }

    /// Handle read with integrated sources
    async fn handle_read(
        &self,
        fid: u32,
        offset: u64,
        count: u32,
        conn_id: u64,
    ) -> Result<NinePeeMessage> {
        let fids = self.fids.read().await;
        let target = fids.get(&fid)
            .ok_or_else(|| anyhow::anyhow!("Unknown fid"))?;

        // Check authorization
        if self.require_auth {
            let contexts = self.contexts.read().await;
            if let Some(ctx) = contexts.get(&conn_id) {
                let resource = match target {
                    FidTarget::RealFile(p) => p.to_string_lossy().to_string(),
                    FidTarget::SyntheticFile(p) => p.clone(),
                    FidTarget::Translator(_, p) => p.clone(),
                };

                if !self.auth_service.authorize(ctx, &resource, Permissions::READ).await? {
                    return Ok(NinePeeMessage::Error {
                        ename: "Permission denied".to_string(),
                        errno: 3,
                    });
                }
            }
        }

        let data = match target {
            FidTarget::RealFile(path) => {
                // Read from real filesystem
                use tokio::fs::File;
                use tokio::io::{AsyncReadExt, AsyncSeekExt};

                let mut file = File::open(path).await?;
                file.seek(std::io::SeekFrom::Start(offset)).await?;

                let mut buffer = vec![0u8; count as usize];
                let n = file.read(&mut buffer).await?;
                buffer.truncate(n);
                buffer
            }

            FidTarget::SyntheticFile(path) => {
                // Try simple synthetic files first
                if let Some(gen) = self.synthetic_fs.get_generator(path).await {
                    gen.generate(offset, count).await?
                } else if let Some(file) = self.plan9_synthetic.get(path).await {
                    // Try advanced synthetic files
                    file.read(offset, count).await?
                } else {
                    vec![]
                }
            }

            FidTarget::Translator(trans_name, path) => {
                // Read through translator
                if let Some(trans) = self.translators.translators.read().await.get(trans_name) {
                    trans.read(path, offset, count).await?
                } else {
                    vec![]
                }
            }
        };

        metrics::record_bytes_read(data.len() as u64);

        Ok(NinePeeMessage::ReadResp { data })
    }

    /// Handle write with integrated targets
    async fn handle_write(
        &self,
        fid: u32,
        offset: u64,
        data: Vec<u8>,
        conn_id: u64,
    ) -> Result<NinePeeMessage> {
        let fids = self.fids.read().await;
        let target = fids.get(&fid)
            .ok_or_else(|| anyhow::anyhow!("Unknown fid"))?;

        // Check authorization
        if self.require_auth {
            let contexts = self.contexts.read().await;
            if let Some(ctx) = contexts.get(&conn_id) {
                let resource = match target {
                    FidTarget::RealFile(p) => p.to_string_lossy().to_string(),
                    FidTarget::SyntheticFile(p) => p.clone(),
                    FidTarget::Translator(_, p) => p.clone(),
                };

                if !self.auth_service.authorize(ctx, &resource, Permissions::WRITE).await? {
                    return Ok(NinePeeMessage::Error {
                        ename: "Permission denied".to_string(),
                        errno: 3,
                    });
                }
            }
        }

        let count = match target {
            FidTarget::RealFile(path) => {
                // Write to real filesystem
                use tokio::fs::File;
                use tokio::io::{AsyncWriteExt, AsyncSeekExt};

                let mut file = File::options()
                    .write(true)
                    .open(path)
                    .await?;
                file.seek(std::io::SeekFrom::Start(offset)).await?;
                file.write_all(&data).await?;
                data.len() as u32
            }

            FidTarget::SyntheticFile(path) => {
                // Write to advanced synthetic file (bidirectional)
                if let Some(file) = self.plan9_synthetic.get(path).await {
                    file.write(offset, &data).await?
                } else {
                    return Ok(NinePeeMessage::Error {
                        ename: "Synthetic file is read-only".to_string(),
                        errno: 4,
                    });
                }
            }

            FidTarget::Translator(trans_name, path) => {
                // Write through translator
                if let Some(trans) = self.translators.translators.read().await.get(trans_name) {
                    trans.write(path, offset, data.clone()).await?
                } else {
                    0
                }
            }
        };

        metrics::record_bytes_written(count as u64);

        Ok(NinePeeMessage::WriteResp { count })
    }

    /// Handle synthetic file operations
    async fn handle_synthetic(
        &self,
        path: String,
        operation: String,
    ) -> Result<NinePeeMessage> {
        if !self.enable_synthetic {
            return Ok(NinePeeMessage::Error {
                ename: "Synthetic files disabled".to_string(),
                errno: 5,
            });
        }

        // List synthetic files
        if operation == "list" {
            let mut files = self.synthetic_fs.list().await;
            // Add Plan 9 synthetic files
            // files.extend(self.plan9_synthetic.list().await);

            let data = files.join("\n").into_bytes();
            return Ok(NinePeeMessage::SyntheticResp { data });
        }

        Ok(NinePeeMessage::SyntheticResp { data: vec![] })
    }

    /// Handle translator operations
    async fn handle_translator(
        &self,
        fid: u32,
        trans_type: String,
        config: Vec<u8>,
    ) -> Result<NinePeeMessage> {
        if !self.enable_translators {
            return Ok(NinePeeMessage::Error {
                ename: "Translators disabled".to_string(),
                errno: 6,
            });
        }

        // Mount translator at FID's path
        let fids = self.fids.read().await;
        if let Some(FidTarget::RealFile(path)) = fids.get(&fid) {
            // Create and register translator based on type
            match trans_type.as_str() {
                "http" => {
                    let url = String::from_utf8(config)?;
                    let trans = Arc::new(crate::translators::HttpTranslator::new(url));
                    self.translators.register(format!("http_{}", fid), trans).await?;
                    self.translators.mount(path.clone(), format!("http_{}", fid)).await?;
                }
                _ => {
                    return Ok(NinePeeMessage::Error {
                        ename: format!("Unknown translator type: {}", trans_type),
                        errno: 7,
                    });
                }
            }
        }

        Ok(NinePeeMessage::TranslatorResp { success: true })
    }

    /// Handle streaming operations
    async fn handle_stream(
        &self,
        fid: u32,
        stream_type: String,
    ) -> Result<NinePeeMessage> {
        // Would implement streaming for synthetic files
        Ok(NinePeeMessage::StreamResp {
            stream_id: fid,
            ready: true,
        })
    }

    // ... other handlers (version, attach, clunk, stat, etc.)

    async fn handle_version(&self, msize: u32, version: String) -> Result<NinePeeMessage> {
        Ok(NinePeeMessage::Version {
            msize: msize.min(self.max_message_size),
            version: if version.starts_with("9P.e") { "9P.e".to_string() } else { version },
        })
    }

    async fn handle_attach(
        &self,
        fid: u32,
        afid: u32,
        uname: String,
        aname: String,
        conn_id: u64,
    ) -> Result<NinePeeMessage> {
        // Store root for FID
        self.fids.write().await.insert(fid, FidTarget::RealFile(self.root.clone()));
        Ok(NinePeeMessage::AttachResp { qid: 0 })
    }

    async fn handle_open(&self, fid: u32, mode: u32, conn_id: u64) -> Result<NinePeeMessage> {
        Ok(NinePeeMessage::OpenResp { qid: 0, iounit: 8192 })
    }

    async fn handle_clunk(&self, fid: u32) -> Result<NinePeeMessage> {
        self.fids.write().await.remove(&fid);
        Ok(NinePeeMessage::ClunkResp)
    }

    async fn handle_stat(&self, fid: u32) -> Result<NinePeeMessage> {
        Ok(NinePeeMessage::StatResp {
            stat: vec![], // Would return proper stat
        })
    }
}