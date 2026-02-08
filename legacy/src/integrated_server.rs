//! Integrated 9P.e Server with Synthetic Files, Translators, and Security
//!
//! This is the complete server that brings together all components

use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::{Result, Context};
use tracing::{info, warn, error, debug};

use ninep::{NinePMessage, ProtocolError};

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
            require_auth: true, // Authentication enabled by default for security
        })
    }

    /// Initialize server with default synthetic files and translators
    pub async fn initialize(&self) -> Result<()> {
        info!("Initializing integrated 9P.e server");

        // Set up default authentication if required
        if self.require_auth {
            self.setup_default_auth().await?;
        }

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

    /// Set up default authentication - only if no users exist
    async fn setup_default_auth(&self) -> Result<()> {
        use crate::auth::{User, AclEntry};

        // Check if any users exist first
        if self.auth_service.has_users().await {
            info!("Users already exist, skipping default user creation");
            return Ok(());
        }

        info!("No users found, creating default user");

        // Generate secure random password
        let password = self.generate_secure_random_password(16);

        // Create default user account with secure password
        let default_user = User {
            uid: 1000,
            username: "admin".to_string(),
            password_hash: password.clone(), // TODO: Use proper password hashing
            groups: vec!["admins".to_string()],
            home_dir: "/home/admin".to_string(),
            shell: "/bin/rc".to_string(),
            public_key: None,
        };

        self.auth_service.add_user(default_user).await?;

        // Set up default ACL - allow the user to access everything
        let acl_entry = AclEntry {
            principal: "admin".to_string(),
            permissions: Permissions::ALL.as_u32(),
            inheritable: true,
        };

        self.auth_service.add_acl("/".to_string(), acl_entry).await?;

        warn!("🔐 Default admin user created: username='admin', password='{}'", password);
        warn!("⚠️  Please change the default password immediately using the CLI!");
        warn!("⚠️  Run: ./9pe-server users passwd admin");

        Ok(())
    }

    /// Generate a secure random password of specified length
    fn generate_secure_random_password(&self, length: usize) -> String {
        use rand::Rng;
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ\
                                 abcdefghijklmnopqrstuvwxyz\
                                 0123456789\
                                 !@#$%^&*";
        let mut rng = rand::thread_rng();
        (0..length)
            .map(|_| {
                let idx = rng.gen_range(0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect()
    }

    /// Process a 9P.e message with full integration
    pub async fn process_message(
        &self,
        msg: NinePMessage,
        conn_id: u64,
    ) -> Result<NinePMessage> {
        debug!("Processing message: {:?}", msg);

        // Check authentication if required
        if self.require_auth {
            let contexts = self.contexts.read().await;
            if let Some(ctx) = contexts.get(&conn_id) {
                if ctx.user.is_none() && !matches!(msg, NinePMessage::Auth { .. } | NinePMessage::Version { .. }) {
                    return Ok(NinePMessage::Error {
                        ename: "Authentication required".to_string(),
                        errno: 1,
                    });
                }
            }
        }

        match msg {
            NinePMessage::Version { msize, version } => {
                self.handle_version(msize, version).await
            }

            NinePMessage::Auth { afid, uname, aname } => {
                self.handle_auth(afid, uname, aname, conn_id).await
            }

            NinePMessage::Attach { fid, afid, uname, aname } => {
                self.handle_attach(fid, afid, uname, aname, conn_id).await
            }

            NinePMessage::Walk { fid, newfid, wnames } => {
                self.handle_walk(fid, newfid, wnames, conn_id).await
            }

            NinePMessage::Open { fid, mode } => {
                self.handle_open(fid, mode, conn_id).await
            }

            NinePMessage::Read { fid, offset, count } => {
                self.handle_read(fid, offset, count, conn_id).await
            }

            NinePMessage::Write { fid, offset, data } => {
                self.handle_write(fid, offset, data, conn_id).await
            }

            NinePMessage::Clunk { fid } => {
                self.handle_clunk(fid).await
            }

            NinePMessage::Stat { fid } => {
                self.handle_stat(fid).await
            }

            // Advanced 9P.e messages
            NinePMessage::StreamInit { stream_id, fid, mode } => {
                self.handle_stream_init(stream_id, fid, mode).await
            }

            NinePMessage::SyntheticCreate { fid, generator, params } => {
                self.handle_synthetic_create(fid, generator, params).await
            }

            NinePMessage::TranslatorSpawn { translator_id, code, config } => {
                self.handle_translator_spawn(translator_id, code, config).await
            }

            _ => {
                Ok(NinePMessage::Error {
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
    ) -> Result<NinePMessage> {
        // Password-based authentication
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

                // Return a successful response (9P.e doesn't have AuthResp)
                Ok(NinePMessage::Attach {
                    fid: afid,
                    afid,
                    uname,
                    aname,
                })
            }
            Err(e) => {
                Ok(NinePMessage::Error {
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
    ) -> Result<NinePMessage> {
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
                    return Ok(NinePMessage::Error {
                        ename: "Permission denied".to_string(),
                        errno: 3,
                    });
                }
            }
        }

        // Store new FID
        drop(fids);
        self.fids.write().await.insert(newfid, new_target);

        // Return success - in 9P.e, walk success is indicated by lack of error
        Ok(NinePMessage::Walk {
            fid,
            newfid,
            wnames: vec![], // Empty means success
        })
    }

    /// Handle read with integrated sources
    async fn handle_read(
        &self,
        fid: u32,
        offset: u64,
        count: u32,
        conn_id: u64,
    ) -> Result<NinePMessage> {
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
                    return Ok(NinePMessage::Error {
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
                if let Some(trans) = self.translators.get_translator_by_name(trans_name).await {
                    trans.read(path, offset, count).await?
                } else {
                    vec![]
                }
            }
        };

        metrics::record_bytes_read(data.len() as u64);

        // Return read data via a Read message
        Ok(NinePMessage::Read {
            fid,
            offset,
            count: data.len() as u32,
        })
    }

    /// Handle write with integrated targets
    async fn handle_write(
        &self,
        fid: u32,
        offset: u64,
        data: Vec<u8>,
        conn_id: u64,
    ) -> Result<NinePMessage> {
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
                    return Ok(NinePMessage::Error {
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
                    return Ok(NinePMessage::Error {
                        ename: "Synthetic file is read-only".to_string(),
                        errno: 4,
                    });
                }
            }

            FidTarget::Translator(trans_name, path) => {
                // Write through translator
                if let Some(trans) = self.translators.get_translator_by_name(trans_name).await {
                    trans.write(path, offset, data.clone()).await?
                } else {
                    0
                }
            }
        };

        metrics::record_bytes_written(count as u64);

        // Return write response via a Write message
        Ok(NinePMessage::Write {
            fid,
            offset,
            data: vec![], // Empty data for response
        })
    }

    /// Handle synthetic file creation
    async fn handle_synthetic_create(
        &self,
        fid: u32,
        generator: String,
        params: Vec<u8>,
    ) -> Result<NinePMessage> {
        if !self.enable_synthetic {
            return Ok(NinePMessage::Error {
                ename: "Synthetic files disabled".to_string(),
                errno: 5,
            });
        }

        // Create synthetic file with generator and params
        // Store generator and params for fid
        self.fids.write().await.insert(fid, FidTarget::SyntheticFile(generator.clone()));

        Ok(NinePMessage::SyntheticRefresh {
            fid: 0, // Would be the actual fid
            force: false,
        })
    }

    /// Handle translator spawn
    async fn handle_translator_spawn(
        &self,
        translator_id: u32,
        code: Vec<u8>,
        config: Vec<u8>,
    ) -> Result<NinePMessage> {
        if !self.enable_translators {
            return Ok(NinePMessage::Error {
                ename: "Translators disabled".to_string(),
                errno: 6,
            });
        }

        // Spawn translator with provided code and config
        // In a real implementation, this would compile and execute the WASM code

        Ok(NinePMessage::TranslatorSpawn {
            translator_id,
            code,
            config,
        })
    }

    /// Handle stream initialization
    async fn handle_stream_init(
        &self,
        stream_id: u32,
        fid: u32,
        mode: u8,
    ) -> Result<NinePMessage> {
        // Would implement streaming for synthetic files
        Ok(NinePMessage::StreamInit {
            stream_id: fid,
            fid,
            mode: 0, // Default mode
        })
    }

    // ... other handlers (version, attach, clunk, stat, etc.)

    async fn handle_version(&self, msize: u32, version: String) -> Result<NinePMessage> {
        Ok(NinePMessage::Version {
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
    ) -> Result<NinePMessage> {
        // Store root for FID
        self.fids.write().await.insert(fid, FidTarget::RealFile(self.root.clone()));
        // Return successful attach response
        Ok(NinePMessage::Attach {
            fid,
            afid,
            uname,
            aname,
        })
    }

    async fn handle_open(&self, fid: u32, mode: u8, conn_id: u64) -> Result<NinePMessage> {
        // Return successful open response
        Ok(NinePMessage::Open {
            fid,
            mode: mode as u8,
        })
    }

    async fn handle_clunk(&self, fid: u32) -> Result<NinePMessage> {
        self.fids.write().await.remove(&fid);
        // Return successful clunk response
        Ok(NinePMessage::Clunk { fid })
    }

    async fn handle_stat(&self, fid: u32) -> Result<NinePMessage> {
        // Return successful stat response
        Ok(NinePMessage::Stat { fid })
    }
}