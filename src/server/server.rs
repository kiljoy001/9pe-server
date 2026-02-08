use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{debug, error, info, warn};

use super::ServerConfig;
use super::handler::MessageHandler;
use crate::consensus::{ConsensusCoordinator, synthetic::register_consensus_controls};
use crate::gpu::{GpuInfo, GpuRuntime, synthetic::register_gpu_controls};
use crate::compute_control::{register_compute_control, ComputeManager};
use crate::ipc::SharedMemoryManager;
use crate::namespace_manager::register_namespace_controls;
use crate::protocol::NinePMessage;
use crate::settrans::VirtualSettransSystem;
use crate::storage_adapter::SyntheticStorageAdapter;
use crate::synth::SyntheticFilesystem;
use crate::wasm::ThreadSafeTranslatorRegistry;
use crate::wasm_adapter::WasmRegistryAdapter;

/// Core server struct that holds the synthetic filesystem and compute manager.
pub struct Server {
    pub config: ServerConfig,
    /// The underlying synthetic filesystem (for direct access if needed)
    pub synth_fs: Arc<SyntheticFilesystem>,
    /// Storage provider wrapping the synthetic filesystem
    pub filesystem: Arc<dyn crate::traits::StorageProvider>,
    /// Compute backend
    pub compute: Arc<dyn crate::traits::ComputeBackend>,
    /// WASM provider for translators
    pub wasm: Arc<dyn crate::traits::WasmProvider>,
    /// Shared memory manager
    pub shm: Arc<SharedMemoryManager>,
    /// Discovered GPU devices
    pub gpu_infos: Vec<GpuInfo>,
    /// Server address string
    address: String,
}

impl Server {
    /// Create a new server instance with dependency injection support
    ///
    /// # Arguments
    /// * `config` - Server configuration
    /// * `storage` - Optional injected storage provider (defaults to synthetic filesystem)
    /// * `compute` - Optional injected compute backend (defaults to GPU compute manager)
    /// * `shm` - Shared memory manager for IPC
    pub async fn new(
        config: ServerConfig,
        storage: Option<Arc<dyn crate::traits::StorageProvider>>,
        compute: Option<Arc<dyn crate::traits::ComputeBackend>>,
        shm: Arc<crate::ipc::SharedMemoryManager>,
    ) -> Result<Self> {
        // 1️⃣ Synthetic filesystem (always create for internal use, even if external storage injected)
        let synth_fs = Arc::new(SyntheticFilesystem::new());

        // 2️⃣ Register namespace controls on the synthetic filesystem
        let _namespace_mgr = register_namespace_controls(&synth_fs).await?;

        // 3️⃣ Discover GPUs via SYCL wrapper
        let gpu_infos = discover_gpus()?;

        // 4️⃣ Create GPU runtimes for each discovered device
        let gpu_runtimes: Vec<std::sync::Arc<GpuRuntime>> = gpu_infos
            .iter()
            .map(|gpu_info| {
                let id = format!("gpu{}", gpu_info.local_index);
                Arc::new(GpuRuntime::new(&id, gpu_info.total_vram_bytes))
            })
            .collect();

        // 5️⃣ Create translator registry
        let translator_registry = Arc::new(ThreadSafeTranslatorRegistry::new(
            config.translator_directory.clone(),
        ));
        // Attempt to scan and load translators (non-fatal if missing)
        if let Err(e) = translator_registry.scan_and_load().await {
            warn!("Failed to load translators: {}", e);
        }

        // 6️⃣ Register GPU synthetic files (info, vram_* etc.)
        register_gpu_controls(&synth_fs, &gpu_infos, &gpu_runtimes).await?;

        // 7️⃣ Create compute manager and register compute control files
        let compute_mgr = Arc::new(ComputeManager::with_runtimes(gpu_runtimes.clone()));
        register_compute_control(&synth_fs, compute_mgr.clone(), translator_registry.clone()).await?;

        // 8️⃣ Register consensus synthetic files if consensus is enabled
        if config.consensus_config.is_some() {
            let node_id = config.node_id.clone();
            let coordinator = Arc::new(ConsensusCoordinator::new(node_id));
            if let Err(e) = register_consensus_controls(&synth_fs, coordinator).await {
                warn!("Failed to register consensus controls: {}", e);
            } else {
                info!("Consensus controls mounted at /srv/consensus");
            }
        }

        // 9️⃣ Create settrans system and WASM provider
        let settrans = Arc::new(VirtualSettransSystem::new(
            synth_fs.clone(),
            translator_registry.clone(),
            config.settrans_directory.clone(),
        ));
        let wasm_provider: Arc<dyn crate::traits::WasmProvider> = Arc::new(WasmRegistryAdapter::new(
            translator_registry,
            settrans,
        ));

        // Use injected compute backend or create default
        let compute_backend = compute.unwrap_or_else(|| {
            Arc::new(crate::compute_adapter::ComputeManagerAdapter::new(compute_mgr))
        });

        // Use injected storage provider or create default synthetic adapter
        let filesystem = storage.unwrap_or_else(|| {
            Arc::new(SyntheticStorageAdapter::new(synth_fs.clone()))
        });

        // Extract address from config for the address() method
        let address = format!("{}:{}", config.network.bind_address, config.network.port);

        Ok(Server {
            config,
            synth_fs,
            filesystem,
            compute: compute_backend,
            wasm: wasm_provider,
            shm,
            gpu_infos,
            address,
        })
    }
    
    /// Constructs a new server instance, sets up synthetic GPU files, compute control, and mounts the compute namespace.
    pub async fn new_with_gpu_support() -> Result<Self> {
        // Create a default server config
        let config = ServerConfig {
            listen_addr: "0.0.0.0:5640".to_string(),
            node_id: "default".to_string(),
            node_name: None,
            network: crate::network::NetworkConfig::default(),
            transport: crate::transport::TransportType::default(),
            root_directory: std::path::PathBuf::from("."),
            max_message_size: 8 * 1024 * 1024,
            worker_threads: None,
            mesh_enabled: true,
            mesh_port: 9650,
            dht_port: 9651,
            metrics_enabled: true,
            metrics_port: 9090,
            translator_directory: std::path::PathBuf::from(".9pe/translators"),
            settrans_directory: std::path::PathBuf::from(".9pe/settrans"),
            dht_store_path: std::path::PathBuf::from(".9pe/dht"),
            auto_mount_enabled: true,
            consensus_config: None,
            auth_config: crate::auth::AuthConfig::default(),
        };

        let shm = Arc::new(SharedMemoryManager::new());
        Self::new(config, None, None, shm).await
    }
    
    /// Get the server address
    pub fn address(&self) -> &str {
        &self.address
    }
    
    /// Run the server and listen for incoming connections
    pub async fn run(&self) -> Result<()> {
        info!("Starting 9P.e server on {}", self.address());

        // Create transport based on configuration
        let transport = crate::transport::TransportFactory::create(self.config.transport.clone())?;
        let addr: std::net::SocketAddr = self.address().parse()
            .context("Invalid server address")?;

        let tls = match self.config.transport {
            crate::transport::TransportType::Quic { .. } => {
                let identity = crate::identity::SovereignIdentity::generate()
                    .context("Failed to generate identity for QUIC")?;
                Some(crate::transport::ServerTls {
                    cert: identity.certificate,
                    key: identity.private_key_der,
                })
            }
            _ => None,
        };

        let listener = transport.listen(addr, tls).await
            .context("Failed to bind to server address")?;

        info!("Server listening on {}", self.address());

        // Accept connections in a loop
        loop {
            match listener.accept().await {
                Ok(connection) => {
                    let filesystem = Arc::clone(&self.filesystem);
                    let compute = Arc::clone(&self.compute);
                    let wasm = Arc::clone(&self.wasm);
                    let shm = Arc::clone(&self.shm);
                    let root_dir = self.config.root_directory.clone();
                    let max_msg_size = self.config.max_message_size;

                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(
                            connection,
                            filesystem,
                            compute,
                            wasm,
                            shm,
                            root_dir,
                            max_msg_size,
                        ).await {
                            error!("Connection handler error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                    // Continue accepting connections despite individual failures
                }
            }
        }
    }
}

/// Handle an individual client connection
async fn handle_connection(
    mut connection: Box<dyn crate::transport::Connection>,
    filesystem: Arc<dyn crate::traits::StorageProvider>,
    compute: Arc<dyn crate::traits::ComputeBackend>,
    wasm: Arc<dyn crate::traits::WasmProvider>,
    shm: Arc<SharedMemoryManager>,
    root_dir: PathBuf,
    max_msg_size: u32,
) -> Result<()> {
    let peer = connection.peer_addr().unwrap_or_else(|_| "unknown".parse().unwrap());
    info!("New connection from {}", peer);

    // Create message handler for this connection
    let mut handler = MessageHandler::new(
        root_dir,
        max_msg_size,
        filesystem,
        compute,
        wasm,
        None, // DHT - could be passed in if needed
        None, // Consensus coordinator - could be passed in if needed
        shm,
        None, // Namespace manager - could be passed in if needed
    )?;

    // Message processing loop
    loop {
        // Read message length (4 bytes, little-endian)
        let mut len_buf = [0u8; 4];
        match connection.read_exact(&mut len_buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                debug!("Connection closed by peer {}", peer);
                break;
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Failed to read message length: {}", e));
            }
        }

        let msg_len = u32::from_le_bytes(len_buf) as usize;

        // Sanity check message size
        if msg_len > max_msg_size as usize {
            error!("Message too large: {} bytes (max {})", msg_len, max_msg_size);
            let err_msg = NinePMessage::Error {
                ename: format!("Message too large: {} bytes", msg_len),
                errno: 27, // EFBIG
            };
            let response_bytes = err_msg.serialize()?;
            let response_len = (response_bytes.len() as u32).to_le_bytes();
            connection.write_all(&response_len).await?;
            connection.write_all(&response_bytes).await?;
            continue;
        }

        // Read message body
        let mut msg_buf = vec![0u8; msg_len];
        connection.read_exact(&mut msg_buf).await
            .context("Failed to read message body")?;

        // Deserialize and handle message
        let request = match NinePMessage::deserialize(msg_buf) {
            Ok(msg) => msg,
            Err(e) => {
                error!("Failed to deserialize message: {}", e);
                let err_msg = NinePMessage::Error {
                    ename: format!("Invalid message format: {}", e),
                    errno: 22, // EINVAL
                };
                let response_bytes = err_msg.serialize()?;
                let response_len = (response_bytes.len() as u32).to_le_bytes();
                connection.write_all(&response_len).await?;
                connection.write_all(&response_bytes).await?;
                continue;
            }
        };

        debug!("Received: {:?}", request);

        // Handle the message
        let response = match handler.handle_message(request).await {
            Ok(resp) => resp,
            Err(e) => {
                error!("Handler error: {}", e);
                NinePMessage::Error {
                    ename: e.to_string(),
                    errno: 5, // EIO
                }
            }
        };

        debug!("Sending: {:?}", response);

        // Serialize and send response
        let response_bytes = response.serialize()?;
        let response_len = (response_bytes.len() as u32).to_le_bytes();
        connection.write_all(&response_len).await?;
        connection.write_all(&response_bytes).await?;
    }

    info!("Connection closed: {}", peer);
    Ok(())
}

/// Helper function that discovers GPUs using the SYCL FFI and converts them into `GpuInfo` structs.
fn discover_gpus() -> Result<Vec<GpuInfo>> {
    use crate::sycl::ffi::{sycl_discover_devices, SyclDeviceInfo};
    // Prepare a buffer for a single device – the SYCL discovery routine will tell us how many exist.
    let mut devices: Vec<SyclDeviceInfo> = vec![SyclDeviceInfo {
        name: [0; 256],
        vendor: [0; 128],
        compute_units: 0,
        global_memory_size: 0,
        local_memory_size: 0,
        max_work_group_size: 0,
        is_gpu: false,
        is_cpu: false,
        supports_fp64: false,
        supports_fp16: false,
    }];
    let mut count: usize = devices.len();
    let err = unsafe { sycl_discover_devices(devices.as_mut_ptr(), &mut count as *mut usize) };
    err.to_result().map_err(|e| anyhow::anyhow!("SYCL discovery error: {:?}", e))?;

    // Resize vector to the actual count returned by SYCL.
    devices.resize(count, SyclDeviceInfo {
        name: [0; 256],
        vendor: [0; 128],
        compute_units: 0,
        global_memory_size: 0,
        local_memory_size: 0,
        max_work_group_size: 0,
        is_gpu: false,
        is_cpu: false,
        supports_fp64: false,
        supports_fp16: false,
    });

    // Convert each SyclDeviceInfo into our internal GpuInfo.
    let mut infos = Vec::new();
    for (idx, dev) in devices.iter().enumerate() {
        let name = unsafe {
            String::from_utf8_lossy(std::slice::from_raw_parts(dev.name.as_ptr() as *const u8, dev.name.len())).trim_end_matches('\0').to_string()
        };
        let vendor = unsafe {
            String::from_utf8_lossy(std::slice::from_raw_parts(dev.vendor.as_ptr() as *const u8, dev.vendor.len())).trim_end_matches('\0').to_string()
        };
        let info = GpuInfo {
            name,
            vendor,
            compute_units: dev.compute_units,
            global_memory_size: dev.global_memory_size,
            local_memory_size: dev.local_memory_size,
            max_work_group_size: dev.max_work_group_size,
            is_gpu: dev.is_gpu,
            is_cpu: dev.is_cpu,
            supports_fp64: dev.supports_fp64,
            supports_fp16: dev.supports_fp16,
            total_vram_bytes: dev.global_memory_size,
            backend: if dev.is_gpu { "gpu".to_string() } else { "cpu".to_string() },
            local_index: idx,
        };
        infos.push(info);
    }
    Ok(infos)
}

impl Server {
    /// Get information about discovered GPU devices
    pub fn gpu_info(&self) -> &[crate::gpu::GpuInfo] {
        &self.gpu_infos
    }
}
