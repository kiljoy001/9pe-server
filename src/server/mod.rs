//! Server module with clean separation of concerns

use anyhow::{Result, Context};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, debug, error};

use crate::network::NetworkConfig;
use crate::transport::{TransportType, TransportFactory, ConnectionListener};
use crate::wasm::ThreadSafeTranslatorRegistry;
use crate::settrans::VirtualSettransSystem;
use crate::synth::SyntheticFilesystem;
use crate::auto_mount::AutoMountDaemon;

pub mod builder;
pub mod handler;
pub mod session;

pub use builder::ServerBuilder;
use session::SessionManager;

/// The main 9P.e server struct - no more God Object!
pub struct Server {
    config: ServerConfig,
    listener: Box<dyn ConnectionListener>,
    session_manager: Arc<SessionManager>,
    translator_registry: Arc<ThreadSafeTranslatorRegistry>,
    settrans_system: Arc<VirtualSettransSystem>,
    synth_fs: Arc<SyntheticFilesystem>,
    auto_mount_daemon: Option<AutoMountDaemon>,
    consensus_coordinator: Option<Arc<crate::consensus::ConsensusCoordinator>>,
}

/// Server configuration
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub network: NetworkConfig,
    pub transport: TransportType,
    pub root_directory: PathBuf,
    pub max_message_size: u32,
    pub worker_threads: Option<usize>,
    pub mesh_enabled: bool,
    pub mesh_port: u16,
    pub metrics_enabled: bool,
    pub metrics_port: u16,
    pub translator_directory: PathBuf,
    pub settrans_directory: PathBuf,
    pub auto_mount_enabled: bool,
    pub consensus_config: Option<crate::config::ConsensusConfig>,
    pub node_id: String,
}

impl Server {
    /// Create a new server using the builder pattern
    pub fn builder() -> ServerBuilder {
        ServerBuilder::new()
    }

    /// Internal constructor called by builder
    pub(crate) async fn new(config: ServerConfig) -> Result<Self> {
        // Create transport
        let transport = TransportFactory::create(config.transport.clone())?;

        // Get socket address
        let addr = config.network.socket_addr()?;

        // Start listening
        let listener = transport
            .listen(addr)
            .await
            .context("Failed to start listener")?;

        // Create session manager
        let session_manager = Arc::new(SessionManager::new());

        // Initialize synthetic filesystem for virtual directories
        let synth_fs = Arc::new(SyntheticFilesystem::new());
        info!("Synthetic filesystem initialized for virtual directories");

        // Initialize thread-safe translator registry and load existing translators
        let translator_registry = Arc::new(ThreadSafeTranslatorRegistry::new(config.translator_directory.clone()));
        info!("Thread-safe WASM translator registry initialized at {:?}", config.translator_directory);

        // Scan and load existing WASM translators from disk
        if let Err(e) = translator_registry.scan_and_load().await {
            error!("Failed to load existing translators: {}", e);
        }

        // Initialize virtual settrans system with synthetic filesystem
        let settrans_system = Arc::new(
            VirtualSettransSystem::new(
                synth_fs.clone(),
                translator_registry.clone(),
            ).await.context("Failed to initialize virtual settrans system")?
        );
        info!("Virtual settrans system initialized at /srv/settrans (virtual only, no physical directories)");

        // Initialize consensus coordinator if enabled
        let consensus_coordinator = if let Some(ref consensus_cfg) = config.consensus_config {
            info!("Initializing consensus coordinator for node: {}", config.node_id);

            // Create crypto provider
            let crypto = Arc::new(crate::consensus::crypto::Ed25519Provider::new()?);

            // Create consensus coordinator
            let coordinator = Arc::new(crate::consensus::ConsensusCoordinator::new(
                config.node_id.clone(),
                crypto,
            ));

            // Initialize the coordinator
            coordinator.initialize().await?;
            info!("Consensus coordinator initialized successfully");

            Some(coordinator)
        } else {
            info!("Consensus disabled in config");
            None
        };

        // Start mesh networking if enabled
        if config.mesh_enabled {
            info!("Starting mesh networking on port {}", config.mesh_port);
            // TODO: Actual mesh networking implementation
            // For now, just bind the port to prove it works
            let mesh_addr = std::net::SocketAddr::from(([0, 0, 0, 0], config.mesh_port));
            tokio::spawn(async move {
                match tokio::net::TcpListener::bind(mesh_addr).await {
                    Ok(_listener) => {
                        info!("Mesh networking bound to port {}", config.mesh_port);
                        // TODO: Implement actual mesh protocol
                    }
                    Err(e) => error!("Failed to bind mesh port: {}", e),
                }
            });
        }

        // Start metrics server if enabled
        if config.metrics_enabled {
            let metrics_port = config.metrics_port;
            tokio::spawn(async move {
                use std::net::SocketAddr;
                use tokio::net::TcpListener;
                use tokio::io::AsyncWriteExt;

                let addr = SocketAddr::from(([0, 0, 0, 0], metrics_port));
                match TcpListener::bind(addr).await {
                    Ok(listener) => {
                        info!("Metrics server started on port {}", metrics_port);
                        loop {
                            match listener.accept().await {
                                Ok((mut stream, _)) => {
                                    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n# 9P.e Metrics\nninep_server_running 1\nninep_connections_total 0\n";
                                    let _ = stream.write_all(response.as_bytes()).await;
                                }
                                Err(e) => error!("Failed to accept metrics connection: {}", e),
                            }
                        }
                    }
                    Err(e) => error!("Failed to start metrics server: {}", e),
                }
            });
        }

        // Initialize auto-mount daemon if enabled
        let auto_mount_daemon = if config.auto_mount_enabled {
            info!("Auto-mount enabled - starting transparent /n/ namespace daemon");
            match crate::auto_mount::initialize_auto_mount().await {
                Ok(daemon) => {
                    info!("Auto-mount daemon started successfully");
                    Some(daemon)
                }
                Err(e) => {
                    error!("Failed to start auto-mount daemon: {}", e);
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            config,
            listener,
            session_manager,
            translator_registry,
            settrans_system,
            synth_fs,
            auto_mount_daemon,
            consensus_coordinator,
        })
    }

    /// Get the server's listening address
    pub fn address(&self) -> String {
        self.config.network.display_address()
    }

    /// Run the server
    pub async fn run(self) -> Result<()> {
        info!(
            "9P.e server running on {} with root {:?}",
            self.address(),
            self.config.root_directory
        );

        // Accept loop with proper error handling
        loop {
            match self.listener.accept().await {
                Ok(connection) => {
                    let root_path = self.config.root_directory.clone();
                    let max_message_size = self.config.max_message_size;
                    let session_mgr = Arc::clone(&self.session_manager);

                    // Clone components for the handler
                    let translator_registry = Arc::clone(&self.translator_registry);
                    let settrans_system = Arc::clone(&self.settrans_system);
                    let synth_fs = Arc::clone(&self.synth_fs);

                    // Spawn handler task
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_connection(
                            connection,
                            root_path,
                            max_message_size,
                            session_mgr,
                            translator_registry,
                            settrans_system,
                            synth_fs,
                        )
                        .await
                        {
                            error!("Connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Accept error: {}", e);
                    // Continue accepting other connections
                }
            }
        }
    }

    /// Handle a single connection with real 9P message processing
    async fn handle_connection(
        mut connection: Box<dyn crate::transport::Connection>,
        root_path: std::path::PathBuf,
        max_message_size: u32,
        session_mgr: Arc<SessionManager>,
        translator_registry: Arc<ThreadSafeTranslatorRegistry>,
        settrans_system: Arc<VirtualSettransSystem>,
        synth_fs: Arc<SyntheticFilesystem>,
    ) -> Result<()> {
        let peer = connection.peer_addr()?;
        info!(
            "New {} connection from {}",
            connection.protocol(),
            peer
        );

        // Create session
        let session_id = session_mgr.create_session(peer).await?;

        // Create message handler for this connection with all components
        let mut handler = crate::server::handler::MessageHandler::new(
            root_path,
            max_message_size,
            translator_registry,
            settrans_system,
            synth_fs,
        )?;

        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        // Handle messages
        loop {
            // Read message size header (4 bytes)
            let mut size_buf = [0u8; 4];
            match connection.read_exact(&mut size_buf).await {
                Ok(_) => {}
                Err(e) => {
                    debug!("Connection closed or read error: {}", e);
                    break;
                }
            }

            let message_size = u32::from_le_bytes(size_buf);
            if message_size > max_message_size || message_size < 4 {
                error!("Invalid message size: {}", message_size);
                break;
            }

            // Read the rest of the message
            let mut message_buf = vec![0u8; (message_size - 4) as usize];
            if let Err(e) = connection.read_exact(&mut message_buf).await {
                error!("Failed to read message body: {}", e);
                break;
            }

            // Deserialize 9P message
            let message = match handler.deserialize_ninepee_message(message_buf).await {
                Ok(msg) => msg,
                Err(e) => {
                    error!("Failed to deserialize message: {}", e);
                    continue;
                }
            };

            debug!("Received message - deserializing as NinePeeMessage");

            // Process message and get response
            let response = match handler.handle_message(message).await {
                Ok(resp) => resp,
                Err(e) => {
                    error!("Failed to handle message: {}", e);
                    crate::protocol::NinePeeMessage::Error {
                        ename: format!("Internal error: {}", e),
                        errno: 5, // EIO
                    }
                }
            };

            debug!("Sending response: {:?}", response);

            // Serialize response
            // Serialize response using bincode for now
            let response_data = match bincode::serialize(&response) {
                Ok(data) => data,
                Err(e) => {
                    error!("Failed to serialize response: {}", e);
                    continue;
                }
            };

            // Send response with size header
            let response_size = (response_data.len() + 4) as u32;
            let mut response_with_header = response_size.to_le_bytes().to_vec();
            response_with_header.extend_from_slice(&response_data);

            if let Err(e) = connection.write_all(&response_with_header).await {
                error!("Failed to send response: {}", e);
                break;
            }

            if let Err(e) = connection.flush().await {
                error!("Failed to flush connection: {}", e);
                break;
            }
        }

        // Clean up session
        session_mgr.remove_session(session_id).await;

        debug!("Connection {} closed", peer);
        Ok(())
    }

    /// Graceful shutdown
    pub async fn shutdown(self) -> Result<()> {
        info!("Shutting down 9P.e server...");

        // Close all sessions
        self.session_manager.close_all().await?;

        // Stop mesh if enabled
        if self.config.mesh_enabled {
            debug!("Stopping mesh networking");
        }

        // Stop metrics if enabled
        if self.config.metrics_enabled {
            debug!("Stopping metrics server");
        }

        info!("Server shutdown complete");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::TransportType;

    #[tokio::test]
    async fn test_server_builder() {
        let server = Server::builder()
            .network_config(NetworkConfig::default())
            .transport(TransportType::Tcp) // Use TCP for testing
            .root_directory(PathBuf::from("/tmp"))
            .build()
            .await;

        assert!(server.is_ok());
    }
}