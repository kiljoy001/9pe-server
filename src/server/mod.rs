//! Server module with clean separation of concerns

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{debug, error, info, warn};

use crate::auto_mount::AutoMountDaemon;
use crate::dht::SovereignDht;
use crate::identity::{NodePermissions, SovereignIdentity};
use crate::network::NetworkConfig;
use crate::transport::{ConnectionListener, ServerTls, TransportFactory, TransportType};

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
pub mod http_gateway;

pub use builder::ServerBuilder;
use session::SessionManager;

/// The main 9P.e server struct - no more God Object!
pub struct Server {
    config: ServerConfig,
    listener: Box<dyn ConnectionListener>,
    session_manager: Arc<SessionManager>,
    wasm: Arc<dyn crate::traits::WasmProvider>,
    // translator_registry: Arc<ThreadSafeTranslatorRegistry>, -> Replaced by WasmProvider
    // settrans_system: Arc<VirtualSettransSystem>, -> Replaced by WasmProvider
    filesystem: Arc<dyn crate::traits::StorageProvider>,
    compute: Arc<dyn crate::traits::ComputeBackend>,
    gpu_infos: Vec<GpuInfo>,
    sovereign_identity: Arc<SovereignIdentity>,
    dht: Arc<SovereignDht>,
    #[allow(dead_code)]
    namespace_manager: Arc<crate::namespace_manager::NamespaceManager>,
    #[allow(dead_code)]
    auto_mount_daemon: Option<AutoMountDaemon>,
    #[allow(dead_code)]
    consensus_coordinator: Option<Arc<crate::consensus::ConsensusCoordinator>>,
    #[allow(dead_code)]
    mesh_network: Option<Arc<crate::mesh::MeshNetwork>>,
    /// Shared memory manager
    pub shm: Arc<crate::ipc::SharedMemoryManager>,
    /// HTTP Gateway for browser access
    pub http_gateway: Option<Arc<crate::server::http_gateway::HttpGateway>>,
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
    pub dht_port: u16,
    pub metrics_enabled: bool,
    pub metrics_port: u16,
    pub translator_directory: PathBuf,
    pub settrans_directory: PathBuf,
    pub dht_store_path: PathBuf,
    pub auto_mount_enabled: bool,
    pub consensus_config: Option<crate::config::ConsensusConfig>,
    pub node_id: String,
    pub node_name: Option<String>,
    pub dht_bootstrap_peers: Vec<String>,
    pub service_discovery: Vec<String>,
    pub wasm_modules: Vec<crate::config::WasmModuleConfig>,
}

impl Server {
    /// Create a new server using the builder pattern
    pub fn builder() -> ServerBuilder {
        ServerBuilder::new()
    }

    /// Internal constructor called by builder
    pub(crate) async fn new(
        config: ServerConfig,
        storage_provider: Option<Arc<dyn crate::traits::StorageProvider>>,
        compute_backend: Option<Arc<dyn crate::traits::ComputeBackend>>,
        shm: Arc<crate::ipc::SharedMemoryManager>,
    ) -> Result<Self> {
        // Log execution mode (user vs system)
        crate::util::log_execution_mode();

        // Create session manager
        let session_manager = Arc::new(SessionManager::new());

        // Sovereign identity + DHT storage
        let permissions = NodePermissions::owner_defaults();
        let sovereign_identity = Arc::new(SovereignIdentity::generate_with_permissions(permissions)?);
        let dht = Arc::new(
            SovereignDht::new_with_store(Arc::clone(&sovereign_identity), &config.dht_store_path)
                .await?,
        );

        // Create transport
        let transport = TransportFactory::create(config.transport.clone())?;

        // Get socket address
        let addr = config.network.socket_addr()?;

        let tls = match config.transport {
            TransportType::Quic { .. } => Some(ServerTls {
                cert: sovereign_identity.certificate.clone(),
                key: sovereign_identity.private_key_der.clone(),
            }),
            _ => None,
        };

        // Start listening
        let listener = transport
            .listen(addr, tls)
            .await
            .context("Failed to start listener")?;

        let dht_listen = std::net::SocketAddr::from(([0, 0, 0, 0], config.dht_port));
        // Convert bootstrap peer strings to Multiaddr
        let bootstrap_addrs: Vec<libp2p::Multiaddr> = config
            .dht_bootstrap_peers
            .iter()
            .filter_map(|peer_str| {
                peer_str.parse::<libp2p::Multiaddr>().ok().or_else(|| {
                    // Try parsing as SocketAddr and converting
                    peer_str.parse::<std::net::SocketAddr>().ok().map(|addr| {
                        match addr {
                            std::net::SocketAddr::V4(v4) => {
                                libp2p::Multiaddr::from(libp2p::multiaddr::Protocol::Ip4(*v4.ip()))
                                    .with(libp2p::multiaddr::Protocol::Tcp(v4.port()))
                            }
                            std::net::SocketAddr::V6(v6) => {
                                libp2p::Multiaddr::from(libp2p::multiaddr::Protocol::Ip6(*v6.ip()))
                                    .with(libp2p::multiaddr::Protocol::Tcp(v6.port()))
                            }
                        }
                    })
                })
            })
            .collect();

        if !bootstrap_addrs.is_empty() {
            info!("Starting DHT with {} bootstrap peers", bootstrap_addrs.len());
        }

        if let Err(e) = dht.start_networking(dht_listen, bootstrap_addrs).await {
            warn!("Failed to start DHT networking: {}", e);
        }
        dht.start_maintenance(Duration::from_secs(60));

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

        // Load explicitly configured WASM modules
        for module_config in &config.wasm_modules {
            info!("Loading configured WASM module: {} from {}", module_config.name, module_config.path);
            match tokio::fs::read(&module_config.path).await {
                Ok(bytes) => {
                    let mount_point = PathBuf::from(format!("/srv/{}", module_config.name));
                    if let Err(e) = translator_registry.load_translator(
                        module_config.name.clone(),
                        mount_point,
                        bytes
                    ).await {
                        error!("Failed to load WASM module {}: {}", module_config.name, e);
                    }
                }
                Err(e) => {
                    error!("Failed to read WASM module file {}: {}", module_config.path, e);
                }
            }
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
            ));

            // Initialize the coordinator
            coordinator.initialize().await?;
            info!("Consensus coordinator initialized successfully");

            if !consensus_cfg.trusted_nodes.is_empty() {
                for trusted in &consensus_cfg.trusted_nodes {
                    let key_bytes = hex::decode(&trusted.public_key)
                        .map_err(|e| anyhow::anyhow!("Invalid hex key: {}", e))?;
                    let key_array: [u8; 32] = key_bytes.try_into()
                        .map_err(|_| anyhow::anyhow!("Invalid key length"))?;
                    match crate::consensus::PublicKey::from_bytes(&key_array) {
                        Ok(public_key) => {
                            coordinator
                                .trust_node(trusted.node_id.clone(), key_array);
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

            // Get bootstrap peers from config (prefer dht_bootstrap_peers, fall back to consensus)
            let bootstrap_peers = if !config.dht_bootstrap_peers.is_empty() {
                config.dht_bootstrap_peers.clone()
            } else if let Some(ref consensus_cfg) = config.consensus_config {
                consensus_cfg.peers.clone()
            } else {
                Vec::new()
            };

            let mesh = Arc::new(crate::mesh::MeshNetwork::new(
                Arc::clone(&sovereign_identity),
                Arc::clone(&dht),
                config.mesh_port,
                bootstrap_peers,
                config.service_discovery.clone(),
            ));

            // Start the mesh network
            let mesh_clone = Arc::clone(&mesh);
            if let Err(e) = mesh_clone.start().await {
                error!("Failed to start mesh network: {}", e);
                None
            } else {
                let listen_addr = std::net::SocketAddr::from(([0, 0, 0, 0], config.mesh_port));
                if let Err(e) = dht
                    .register_self_with_name(listen_addr, config.node_name.clone())
                    .await
                {
                    warn!("Failed to register node in DHT: {}", e);
                }
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

            let mut manager = if let Some(ref mesh) = mesh_network {
                NamespaceManager::new(synth_fs.clone())?.with_mesh_network(Arc::clone(mesh))
            } else {
                NamespaceManager::new(synth_fs.clone())?
            };

            // Add consensus if available
            if let Some(ref consensus) = consensus_coordinator {
                manager = manager.with_consensus(Arc::clone(consensus));
                info!("Namespace manager integrated with consensus");
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

            // Create and configure fog router for distributed job execution
            match mesh.create_fog_router(Arc::clone(&compute_manager)).await {
                Ok(fog_router) => {
                    // Also set fog router on compute manager for local jobs that can be distributed
                    compute_manager.set_fog_router(fog_router.clone()).await;
                    info!("Fog router created for distributed job execution");
                }
                Err(e) => {
                    warn!("Failed to create fog router: {}", e);
                }
            }
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
            info!("Auto-mount enabled - initializing daemon (will start in run loop)");
            Some(crate::auto_mount::create_auto_mount_daemon())
        } else {
            None
        };

        // Create Wasm provider adapter
        let wasm_provider = Arc::new(crate::wasm_adapter::WasmRegistryAdapter::new(
            translator_registry.clone(),
            settrans_system.clone(),
        ));
        
        let root_path = config.root_directory.clone();
        let http_gateway = Some(Arc::new(crate::server::http_gateway::HttpGateway::new(
            storage_provider.clone().unwrap_or_else(|| Arc::new(crate::storage_adapter::PhysicalStorageAdapter::new(root_path.clone())))
        )));

        Ok(Self {
            config,
            listener,
            session_manager,
            wasm: wasm_provider,


            // Use injected filesystem or default to physical filesystem rooted at config.root_directory
            filesystem: storage_provider.unwrap_or_else(||
                Arc::new(crate::storage_adapter::PhysicalStorageAdapter::new(root_path))
            ),

            // Use injected compute backend or wrap the default manager
            compute: compute_backend.unwrap_or_else(||
                Arc::new(crate::compute_adapter::ComputeManagerAdapter::new(compute_manager.clone()))
            ),

            gpu_infos,
            sovereign_identity,
            dht,
            namespace_manager,
            auto_mount_daemon,
            consensus_coordinator,
            mesh_network,
            shm,
            http_gateway,
        })
    }

    /// Get the server's listening address
    pub fn address(&self) -> String {
        self.config.network.display_address()
    }

    /// Access the compute manager used for GPU/WASM job orchestration.
    pub fn compute(&self) -> Arc<dyn crate::traits::ComputeBackend> {
        Arc::clone(&self.compute)
    }

    /// Access the cached GPU discovery results.
    pub fn gpu_infos(&self) -> &[GpuInfo] {
        &self.gpu_infos
    }

    /// Run the server
    pub async fn run(mut self) -> Result<()> {
        info!(
            "9P.e server running on {} with root {:?}",
            self.address(),
            self.config.root_directory
        );

        // Start HTTP Gateway in background if configured
        info!("DEBUG: Checking HTTP gateway (is_some: {})", self.http_gateway.is_some());
        if let Some(gateway) = self.http_gateway.clone() {
            info!("🌐 Starting HTTP Gateway on port 9090...");
            tokio::spawn(async move {
                info!("HTTP Gateway task spawned, attempting to bind port 9090");
                match gateway.run(9090).await {
                    Ok(_) => info!("HTTP Gateway stopped normally"),
                    Err(e) => error!("HTTP Gateway failed: {}", e),
                }
            });
        } else {
            warn!("HTTP Gateway not configured - web interface unavailable");
        }

        // Start Auto-Mount Daemon now that server is active
        if let Some(daemon) = &mut self.auto_mount_daemon {
            // Register self as a local server so the daemon knows how to connect (Protocol/Port)
            if let Ok(socket_addr) = self.config.network.socket_addr() {
                // Determine effective address (use localhost for self-connection if binding 0.0.0.0)
                let host = if socket_addr.ip().is_unspecified() {
                    "127.0.0.1".to_string()
                } else {
                    socket_addr.ip().to_string()
                };

                daemon.register_local_server(
                    host,
                    socket_addr.port(),
                    self.config.transport.clone()
                ).await;
            }

            info!("Starting auto-mount daemon...");
            if let Err(e) = daemon.start().await {
                error!("Failed to start auto-mount daemon: {}", e);
            }
        }

        // Accept loop with proper error handling
        loop {
            match self.listener.accept().await {
                Ok(connection) => {
                    let root_path = self.config.root_directory.clone();
                    let max_message_size = self.config.max_message_size;
                    let session_mgr = Arc::clone(&self.session_manager);

                    // Clone components for the handler
                    let storage = Arc::clone(&self.filesystem);
                    let compute = Arc::clone(&self.compute);
                    let wasm = Arc::clone(&self.wasm);
                    let dht = Arc::clone(&self.dht);
                    let consensus_coordinator = self.consensus_coordinator.clone();
                    let shm = Arc::clone(&self.shm);
                    let namespace_manager = Arc::clone(&self.namespace_manager);
                    let mesh_network = self.mesh_network.clone();

                    // Spawn handler task
                    let node_id = self.config.node_id.clone();
                    tokio::spawn(async move {
                        let peer_id_str = &node_id;
                        // Optional: Log node ID
                        // debug!("Handling connection for node: {}", node_id);
                        // Skip hex validation that causes connection drops for non-hex IDs
                        if false {
                             let peer_id_str = &node_id;
                             match hex::decode(peer_id_str) {
                                Ok(_) => {},
                                Err(e) => warn!("Invalid hex node_id (non-fatal): {}", e),
                             }
                        }

                        // Get fog router from mesh network if available
                        let fog_router: Option<Arc<dyn crate::fog::FogRouter>> = if let Some(ref mesh) = mesh_network {
                            mesh.fog_router().await.map(|r| r as Arc<dyn crate::fog::FogRouter>)
                        } else {
                            None
                        };

                        if let Err(e) = Self::handle_connection(
                            connection,
                            root_path,
                            max_message_size,
                            session_mgr,
                            storage,
                            compute,
                            wasm,
                            dht,
                            consensus_coordinator,
                            shm,
                            namespace_manager,
                            fog_router,
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
    pub(crate) async fn handle_connection(
        mut connection: Box<dyn crate::transport::Connection>,
        root_path: std::path::PathBuf,
        max_message_size: u32,
        session_mgr: Arc<SessionManager>,
        storage: Arc<dyn crate::traits::StorageProvider>,
        compute: Arc<dyn crate::traits::ComputeBackend>,
        wasm: Arc<dyn crate::traits::WasmProvider>,
        dht: Arc<SovereignDht>,
        consensus_coordinator: Option<Arc<crate::consensus::ConsensusCoordinator>>,
        shm: Arc<crate::ipc::SharedMemoryManager>,
        namespace_manager: Arc<crate::namespace_manager::NamespaceManager>,
        fog_router: Option<Arc<dyn crate::fog::FogRouter>>,
    ) -> Result<()> {
        let peer = connection.peer_addr()?;
        info!("New {} connection from {}", connection.protocol(), peer);

        // Create session
        let session_id = session_mgr.create_session(peer).await?;

        let handler = crate::server::handler::MessageHandler::new(
            root_path,
            max_message_size,
            storage,
            compute,
            wasm,
            Some(dht),
            consensus_coordinator,
            shm,
            Some(namespace_manager),
        )?;

        // Set up fog router for distributed work distribution if available
        if let Some(router) = fog_router {
            handler.set_fog_router(router).await;
        }

        let mut handler = handler;

        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        // Handle messages
        loop {
            // Read message size header (4 bytes)
            let mut size_header = [0u8; 4];
            match connection.read_exact(&mut size_header).await {
                Ok(_) => {}
                Err(e) => {
                    debug!("Connection closed or read error: {}", e);
                    break;
                }
            }

            let message_size = u32::from_le_bytes(size_header);
            if message_size > max_message_size || message_size < 7 {
                error!("Invalid message size: {}", message_size);
                break;
            }

            // Read the rest of the message (size includes header)
            let mut body_buf = vec![0u8; (message_size - 4) as usize];
            if let Err(e) = connection.read_exact(&mut body_buf).await {
                error!("Failed to read message body: {}", e);
                break;
            }

            // Prepare full message for translation if needed
            let mut full_message_data = size_header.to_vec();
            full_message_data.extend_from_slice(&body_buf);

            // Peek message type (byte 4)
            let msg_type = body_buf[0];
            let is_legacy = msg_type >= 100 && msg_type <= 127;

            // Special case for Tflush (108) which doesn't exist in 9P.e
            if msg_type == 108 && is_legacy {
                // Respond with Rflush (size=7, type=109, tag from message)
                let tag = [body_buf[1], body_buf[2]];
                let rflush = vec![7, 0, 0, 0, 109, tag[0], tag[1]];
                if let Err(e) = connection.write_all(&rflush).await {
                    error!("Failed to send Rflush: {}", e);
                    break;
                }
                let _ = connection.flush().await;
                continue;
            }

            // Deserialize 9P message
            let message = if is_legacy {
                let session = crate::compatibility::CompatibilitySession::new();
                let translator = crate::compatibility::MessageTranslator::new(session);
                match translator.translate_legacy_to_9pe(&full_message_data) {
                    Ok(msg) => msg,
                    Err(e) => {
                        error!("Failed to translate legacy message: {}", e);
                        continue;
                    }
                }
            } else {
                match handler.deserialize_ninep_message(body_buf).await {
                    Ok(msg) => msg,
                    Err(e) => {
                        error!("Failed to deserialize 9P.e message: {}", e);
                        continue;
                    }
                }
            };

            debug!("Received message (is_legacy={}): {:?}", is_legacy, message);

            // Process message and get response
            let response = match handler.handle_message(message).await {
                Ok(resp) => resp,
                Err(e) => {
                    error!("Failed to handle message: {}", e);
                    crate::protocol::NinePMessage::Error {
                        ename: format!("Internal error: {}", e),
                        errno: 5, // EIO
                    }
                }
            };

            debug!("Sending response: {:?}", response);

            // Serialize response based on negotiated protocol
            let protocol = handler.connection_state.protocol_version().await;
            if protocol == "9P2000" {
                let mut session = crate::compatibility::CompatibilitySession::new();
                session.is_legacy = true;
                let translator = crate::compatibility::MessageTranslator::new(session);
                match translator.translate_9pe_to_legacy(&response) {
                    Ok(response_data) => {
                        // Response data already includes size header in translate_9pe_to_legacy
                        if let Err(e) = connection.write_all(&response_data).await {
                            error!("Failed to send legacy response: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Failed to translate response to legacy format: {}", e);
                        continue;
                    }
                }
            } else {
                // Serialize response using 9P.e manual format
                let response_data = match response.serialize() {
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
    use crate::transport::{ServerTls, TransportFactory, TransportType};
    use quinn::Endpoint;
    use crate::protocol::{MAX_MESSAGE_SIZE, NINEP_VERSION};
    use tempfile::tempdir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn test_server_builder() {
        let temp_dir = tempdir().expect("temp dir");
        let server = Server::builder()
            .network_config(NetworkConfig::default())
            .transport(TransportType::Tcp) // Use TCP for testing
            .state_directory(temp_dir.path().to_path_buf())
            .root_directory(PathBuf::from("/tmp"))
            .build()
            .await;
        
        assert!(server.is_ok());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_quic_connection_uses_real_handler() {
        let identity = Arc::new(SovereignIdentity::generate().expect("identity"));
        let dht_dir = tempdir().expect("dht dir");
        let dht = Arc::new(
            SovereignDht::new_with_store(Arc::clone(&identity), dht_dir.path())
                .await
                .expect("dht"),
        );

        let synth_fs = Arc::new(SyntheticFilesystem::new());
        let translators_dir = tempdir().expect("translators");
        let translator_registry = Arc::new(ThreadSafeTranslatorRegistry::new(
            translators_dir.path().to_path_buf(),
        ));
        let settrans_system = Arc::new(
            VirtualSettransSystem::new(Arc::clone(&synth_fs), Arc::clone(&translator_registry))
                .await
                .expect("settrans"),
        );

        let storage = Arc::new(crate::storage_adapter::SyntheticStorageAdapter::new(Arc::clone(&synth_fs)));
        let memory_manager = Arc::new(crate::memory::MemoryManager::new());
        let default_pool = memory_manager.create_pool(
            crate::memory::PoolConfig::default(),
            crate::memory::AllocationStrategy::FirstFit,
        ).expect("pool");
        let shm = Arc::new(crate::ipc::SharedMemoryManager::new(memory_manager).expect("shm"));

        let compute_manager = Arc::new(crate::compute_control::ComputeManager::new());
        let compute = Arc::new(crate::compute_adapter::ComputeManagerAdapter::new(compute_manager));

        let wasm = Arc::new(crate::wasm_adapter::WasmRegistryAdapter::new(
            translator_registry.clone(),
            settrans_system.clone()
        ));

        let consensus_coordinator: Option<Arc<crate::consensus::ConsensusCoordinator>> = None;

        let namespace_manager = Arc::new(
            crate::namespace_manager::NamespaceManager::new(Arc::clone(&synth_fs))
                .expect("namespace manager")
        );

        let transport = TransportFactory::create(TransportType::Quic { server_name: None })
            .expect("transport");
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let tls = ServerTls {
            cert: identity.certificate.clone(),
            key: identity.private_key_der.clone(),
        };
        let listener = transport.listen(addr, Some(tls)).await.expect("listen");
        let listen_addr = listener.local_addr().expect("local addr");

        let session_mgr = Arc::new(SessionManager::new());
        let root_path = PathBuf::from(".");
        let max_message_size = MAX_MESSAGE_SIZE;

        let server_task = tokio::spawn(async move {
            let connection = listener.accept().await.expect("accept");
            Server::handle_connection(
                connection,
                root_path,
                max_message_size,
                session_mgr,
                storage,
                compute,
                wasm,
                dht,
                consensus_coordinator,
                shm,
                namespace_manager,
                None, // No fog router in test
            )
            .await
            .ok();
        });

        let client_config = crate::transport::configure_client_insecure().expect("client config");
        let mut endpoint = Endpoint::client("0.0.0.0:0".parse().unwrap()).expect("endpoint");
        endpoint.set_default_client_config(client_config);
        
        let connection = endpoint.connect(listen_addr, "localhost")
            .expect("connect call")
            .await
            .expect("connect");

        let (mut send, mut recv) = connection.open_bi().await.expect("open stream");

        let message = crate::protocol::NinePMessage::Version {
            msize: MAX_MESSAGE_SIZE,
            version: NINEP_VERSION.to_string(),
        };
        let data = message.serialize().expect("serialize");
        let size = (data.len() + 4) as u32;
        let mut framed = size.to_le_bytes().to_vec();
        framed.extend_from_slice(&data);
        send.write_all(&framed).await.expect("write");
        send.flush().await.expect("flush");

        let mut size_buf = [0u8; 4];
        recv.read_exact(&mut size_buf).await.expect("read header");
        let resp_size = u32::from_le_bytes(size_buf);
        let mut resp_buf = vec![0u8; (resp_size - 4) as usize];
        recv.read_exact(&mut resp_buf).await.expect("read response");
        let response = crate::protocol::NinePMessage::deserialize(resp_buf).expect("deserialize");

        assert!(matches!(response, crate::protocol::NinePMessage::Version { .. }));

        drop(send);
        drop(recv);
        drop(connection);
        let _ = tokio::time::timeout(Duration::from_secs(2), server_task).await;
    }
}
