//! Server module with clean separation of concerns

use anyhow::{Result, Context};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, debug, error};

use crate::network::NetworkConfig;
use crate::transport::{TransportType, TransportFactory, ConnectionListener};
use crate::wasm::ThreadSafeTranslatorRegistry;
use crate::settrans::SettransSystem;

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
    settrans_system: Arc<SettransSystem>,
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

        // Initialize thread-safe translator registry
        let translator_registry = Arc::new(ThreadSafeTranslatorRegistry::new(config.translator_directory.clone()));

        // Note: ThreadSafeTranslatorRegistry doesn't have scan_and_load method yet
        // This will be handled by the settrans system
        info!("Thread-safe WASM translator registry initialized at {:?}", config.translator_directory);

        // Initialize settrans system for filesystem-based translator management
        let settrans_system = Arc::new(
            SettransSystem::new(
                config.settrans_directory.clone(),
                translator_registry.clone(),
            ).await.context("Failed to initialize settrans system")?
        );
        info!("Settrans system initialized at {:?}", config.settrans_directory);

        // Start mesh networking if enabled
        if config.mesh_enabled {
            info!("Starting mesh networking on port {}", config.mesh_port);
            // Mesh initialization here
        }

        // Start metrics server if enabled
        if config.metrics_enabled {
            info!("Starting metrics server on port {}", config.metrics_port);
            // Metrics initialization here
        }

        Ok(Self {
            config,
            listener,
            session_manager,
            translator_registry,
            settrans_system,
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

                    // Spawn handler task
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_connection(
                            connection,
                            root_path,
                            max_message_size,
                            session_mgr,
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
    ) -> Result<()> {
        let peer = connection.peer_addr()?;
        info!(
            "New {} connection from {}",
            connection.protocol(),
            peer
        );

        // Create session
        let session_id = session_mgr.create_session(peer).await?;

        // Create message handler for this connection
        let mut handler = crate::server::handler::MessageHandler::new(root_path, max_message_size)?;

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
            let message = match handler.deserialize_message(message_buf).await {
                Ok(msg) => msg,
                Err(e) => {
                    error!("Failed to deserialize message: {}", e);
                    continue;
                }
            };

            debug!("Received message: {:?}", message);

            // Process message and get response
            let response = match handler.handle_message(message).await {
                Ok(resp) => resp,
                Err(e) => {
                    error!("Failed to handle message: {}", e);
                    ninepee::protocol::NinePeeMessage::Error {
                        ename: format!("Internal error: {}", e),
                        errno: 5, // EIO
                    }
                }
            };

            debug!("Sending response: {:?}", response);

            // Serialize response
            let response_data = match handler.serialize_message(&response).await {
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