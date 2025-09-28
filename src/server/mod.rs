//! Server module with clean separation of concerns

use anyhow::{Result, Context};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, debug, error};

use crate::network::NetworkConfig;
use crate::transport::{TransportType, TransportFactory, ConnectionListener};

pub mod builder;
pub mod handler;
pub mod session;

pub use builder::ServerBuilder;
use handler::MessageHandler;
use session::SessionManager;

/// The main 9P.e server struct - no more God Object!
pub struct Server {
    config: ServerConfig,
    listener: Box<dyn ConnectionListener>,
    message_handler: Arc<MessageHandler>,
    session_manager: Arc<SessionManager>,
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

        // Create message handler (no more God Object!)
        let message_handler = Arc::new(MessageHandler::new(
            config.root_directory.clone(),
            config.max_message_size,
        )?);

        // Create session manager
        let session_manager = Arc::new(SessionManager::new());

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
            message_handler,
            session_manager,
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
                    let handler = Arc::clone(&self.message_handler);
                    let session_mgr = Arc::clone(&self.session_manager);

                    // Spawn handler task
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_connection(
                            connection,
                            handler,
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

    /// Handle a single connection
    async fn handle_connection(
        mut connection: Box<dyn crate::transport::Connection>,
        handler: Arc<MessageHandler>,
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

        // Handle messages
        loop {
            // In real implementation:
            // - Read 9P message from connection
            // - Pass to message handler
            // - Write response
            // - Update session state

            // Placeholder for now
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }

        // Clean up session
        session_mgr.remove_session(session_id).await;

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