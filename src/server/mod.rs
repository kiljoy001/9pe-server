//! Server module with clean separation of concerns

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use crate::auto_mount::AutoMountDaemon;
use crate::network::NetworkConfig;
use crate::transport::{ConnectionListener, TransportFactory, TransportType};

#[cfg(feature = "gpu")]
use crate::compute_control::{register_compute_control, ComputeManager};
#[cfg(feature = "gpu")]
use crate::gpu::{discover_gpus, synthetic::register_gpu_controls, GpuInfo, GpuRuntime};

#[cfg(feature = "translators")]
use crate::settrans::VirtualSettransSystem;
#[cfg(feature = "translators")]
use crate::wasm::ThreadSafeTranslatorRegistry;

#[cfg(feature = "synthetic")]
use crate::synth::SyntheticFilesystem;

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
    compute_manager: Arc<ComputeManager>,
    gpu_infos: Vec<GpuInfo>,
    #[allow(dead_code)]
    namespace_manager: Arc<crate::namespace_manager::NamespaceManager>,
    #[allow(dead_code)]
    auto_mount_daemon: Option<AutoMountDaemon>,
    #[allow(dead_code)]
    consensus_coordinator: Option<Arc<crate::consensus::ConsensusCoordinator>>,
    #[allow(dead_code)]
    mesh_network: Option<Arc<crate::mesh::MeshNetwork>>,
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
        // Log execution mode (user vs system)
        crate::util::log_execution_mode();

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
        let translator_registry = Arc::new(ThreadSafeTranslatorRegistry::new(
            config.translator_directory.clone(),
        ));
        info!(
            "Thread-safe WASM translator registry initialized at {:?}",
            config.translator_directory
        );

        // Scan and load existing WASM translators from disk
        if let Err(e) = translator_registry.scan_and_load().await {
            error!("Failed to load existing translators: {}", e);
        }

        // Discover GPUs and wire synthetic compute namespace
        let gpu_infos = match discover_gpus() {
            Ok(list) => {
                info!("Discovered {} GPU device(s)", list.len());
                list
            }
            Err(e) => {
                warn!("GPU discovery failed: {e}");
                Vec::new()
            }
        };

        let gpu_runtimes: Vec<Arc<GpuRuntime>> = gpu_infos
            .iter()
            .map(|gpu| {
                let device_id = format!("gpu{}", gpu.local_index);
                Arc::new(GpuRuntime::new(&device_id, gpu.total_vram_bytes))
            })
            .collect();

        let compute_manager = Arc::new(ComputeManager::with_runtimes(gpu_runtimes.clone()));

        if let Err(e) = register_gpu_controls(&synth_fs, &gpu_infos, &gpu_runtimes).await {
            warn!("Failed to register GPU synthetic controls: {e}");
        } else {
            info!("GPU synthetic controls mounted under /srv/compute");
        }

        if let Err(e) = register_compute_control(
            &synth_fs,
            Arc::clone(&compute_manager),
            Arc::clone(&translator_registry),
        )
        .await
        {
            warn!("Failed to register compute control namespace: {e}");
        }

        // Initialize consensus coordinator if enabled (needed by namespace manager)
        let consensus_coordinator = if let Some(ref consensus_cfg) = config.consensus_config {
            info!(
                "Initializing consensus coordinator for node: {}",
                config.node_id
            );

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

            if !consensus_cfg.trusted_nodes.is_empty() {
                for trusted in &consensus_cfg.trusted_nodes {
                    match crate::consensus::PublicKey::from_hex(
                        trusted.algorithm.clone(),
                        &trusted.public_key,
                    ) {
                        Ok(public_key) => {
                            coordinator
                                .trust_node(trusted.node_id.clone(), public_key)
                                .await;
                            debug!(
                                "Registered trusted consensus peer {} (algorithm {})",
                                trusted.node_id, trusted.algorithm
                            );
                        }
                        Err(e) => {
                            warn!(
                                "Failed to register trusted consensus peer {}: {}",
                                trusted.node_id, e
                            );
                        }
                    }
                }
            }

            Some(coordinator)
        } else {
            info!("Consensus disabled in config");
            None
        };

        // Start mesh networking if enabled
        let mesh_network = if config.mesh_enabled {
            info!("Starting mesh networking on port {}", config.mesh_port);

            // Get bootstrap peers from consensus config
            let bootstrap_peers = if let Some(ref consensus_cfg) = config.consensus_config {
                consensus_cfg.peers.clone()
            } else {
                Vec::new()
            };

            let mesh = Arc::new(crate::mesh::MeshNetwork::new(
                config.node_id.clone(),
                config.mesh_port,
                bootstrap_peers,
            ));

            // Start the mesh network
            let mesh_clone = Arc::clone(&mesh);
            if let Err(e) = mesh_clone.start().await {
                error!("Failed to start mesh network: {}", e);
                None
            } else {
                info!(
                    "Mesh network started successfully on port {}",
                    config.mesh_port
                );
                Some(mesh)
            }
        } else {
            info!("Mesh networking disabled");
            None
        };

        // Initialize namespace manager (system-level translator)
        let namespace_manager = {
            use crate::namespace_manager::NamespaceManager;

            let manager = if let Some(ref mesh) = mesh_network {
                NamespaceManager::new(synth_fs.clone())?.with_mesh_network(Arc::clone(mesh))
            } else {
                NamespaceManager::new(synth_fs.clone())?
            };

            // Add consensus if available
            if let Some(ref _consensus) = consensus_coordinator {
                // Get the bounded ghostdag from consensus coordinator
                // For now, we'll initialize without consensus integration
                // TODO: Add get_bounded_ghostdag() method to ConsensusCoordinator
                info!("Namespace manager created (consensus integration pending)");
            }

            // Initialize namespace manager synthetic filesystem
            manager
                .initialize()
                .await
                .context("Failed to initialize namespace manager")?;

            // Register system namespaces
            info!("Namespace manager initialized at /srv/namespace/");
            Arc::new(manager)
        };

        // Set up mesh network with namespace manager if both exist
        if let Some(ref mesh) = mesh_network {
            use crate::namespace_manager::MeshMessageHandler;
            mesh.set_namespace_manager(
                Arc::clone(&namespace_manager) as Arc<dyn MeshMessageHandler>
            )
            .await;
        }

        // Initialize virtual settrans system with synthetic filesystem and namespace manager
        let settrans_system = Arc::new(
            VirtualSettransSystem::new(synth_fs.clone(), translator_registry.clone())
                .await
                .context("Failed to initialize virtual settrans system")?,
        );
        let settrans_path = crate::util::get_settrans_directory();
        info!(
            "Virtual settrans system initialized at {:?} (virtual only, no physical directories)",
            settrans_path
        );

        // Metrics are now exposed as files in /srv/stats/ instead of HTTP server
        // This is more Plan 9-like: everything is a file, every file is a function
        info!("Metrics available at /srv/stats/* (see src/stats.rs)");

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
            compute_manager,
            gpu_infos,
            namespace_manager,
            auto_mount_daemon,
            consensus_coordinator,
            mesh_network,
        })
    }

    /// Get the server's listening address
    pub fn address(&self) -> String {
        self.config.network.display_address()
    }

    /// Access the compute manager used for GPU/WASM job orchestration.
    pub fn compute_manager(&self) -> Arc<ComputeManager> {
        Arc::clone(&self.compute_manager)
    }

    /// Access the cached GPU discovery results.
    pub fn gpu_infos(&self) -> &[GpuInfo] {
        &self.gpu_infos
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
        info!("New {} connection from {}", connection.protocol(), peer);

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
