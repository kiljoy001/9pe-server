//! Server builder with dependency injection

use anyhow::Result;
use std::path::PathBuf;

use super::{Server, ServerConfig};
use crate::network::NetworkConfig;
use crate::transport::TransportType;
use crate::wasm::ThreadSafeTranslatorRegistry;
use crate::settrans::VirtualSettransSystem;
use crate::synth::SyntheticFilesystem;
use std::sync::Arc;

/// Builder for creating a Server with dependency injection
pub struct ServerBuilder {
    network_config: Option<NetworkConfig>,
    transport: Option<TransportType>,
    root_directory: Option<PathBuf>,
    max_message_size: u32,
    worker_threads: Option<usize>,
    mesh_enabled: bool,
    mesh_port: u16,
    metrics_enabled: bool,
    metrics_port: u16,
    // translator_directory: Option<PathBuf>, // Removed
    // settrans_directory: Option<PathBuf>, // Removed
    auto_mount_enabled: bool,
    dht_bootstrap_peers: Vec<String>,
    service_discovery: Vec<String>,
    config: Option<crate::config::Config>,
    state_directory: Option<PathBuf>,
    // Dependency Injection fields
    storage_provider: Option<std::sync::Arc<dyn crate::traits::StorageProvider>>,
    compute_backend: Option<std::sync::Arc<dyn crate::traits::ComputeBackend>>,
    wasm_provider: Option<Arc<dyn crate::traits::WasmProvider>>, // Added
}

impl Default for ServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerBuilder {
    pub fn new() -> Self {
        Self {
            network_config: None,
            transport: None,
            root_directory: None,
            max_message_size: 8 * 1024 * 1024, // 8MB default
            worker_threads: None,
            mesh_enabled: true,
            mesh_port: 9650,
            metrics_enabled: true,
            metrics_port: 9090,
            // translator_directory: None, // Removed
            // settrans_directory: None, // Removed
            auto_mount_enabled: true, // Auto-mount enabled by default
            dht_bootstrap_peers: Vec::new(),
            service_discovery: Vec::new(),
            config: None,
            state_directory: None,
            storage_provider: None,
            compute_backend: None,
            wasm_provider: None, // Added
        }
    }

    pub fn state_directory(mut self, path: PathBuf) -> Self {
        self.state_directory = Some(path);
        self
    }

    pub fn with_config(mut self, config: crate::config::Config) -> Self {
        self.config = Some(config);
        self
    }

    pub fn network_config(mut self, config: NetworkConfig) -> Self {
        self.network_config = Some(config);
        self
    }

    pub fn transport(mut self, transport: TransportType) -> Self {
        self.transport = Some(transport);
        self
    }

    pub fn root_directory(mut self, path: PathBuf) -> Self {
        self.root_directory = Some(path);
        self
    }

    pub fn max_message_size(mut self, size: u32) -> Self {
        self.max_message_size = size;
        self
    }

    pub fn worker_threads(mut self, threads: Option<usize>) -> Self {
        self.worker_threads = threads;
        self
    }

    pub fn mesh_enabled(mut self, enabled: bool) -> Self {
        self.mesh_enabled = enabled;
        self
    }

    pub fn mesh_port(mut self, port: u16) -> Self {
        self.mesh_port = port;
        self
    }

    pub fn metrics_enabled(mut self, enabled: bool) -> Self {
        self.metrics_enabled = enabled;
        self
    }

    pub fn metrics_port(mut self, port: u16) -> Self {
        self.metrics_port = port;
        self
    }

    // pub fn translator_directory(mut self, path: PathBuf) -> Self { // Removed
    //     self.translator_directory = Some(path);
    //     self
    // }

    // pub fn settrans_directory(mut self, path: PathBuf) -> Self { // Removed
    //     self.settrans_directory = Some(path);
    //     self
    // }

    pub fn auto_mount_enabled(mut self, enabled: bool) -> Self {
        self.auto_mount_enabled = enabled;
        self
    }

    pub fn dht_bootstrap_peers(mut self, peers: Vec<String>) -> Self {
        self.dht_bootstrap_peers = peers;
        self
    }

    pub fn service_discovery(mut self, services: Vec<String>) -> Self {
        self.service_discovery = services;
        self
    }

    pub fn with_storage(mut self, provider: std::sync::Arc<dyn crate::traits::StorageProvider>) -> Self {
        self.storage_provider = Some(provider);
        self
    }

    pub fn with_wasm(mut self, wasm: Arc<dyn crate::traits::WasmProvider>) -> Self {
        self.wasm_provider = Some(wasm);
        self
    }

    pub fn with_compute(mut self, backend: std::sync::Arc<dyn crate::traits::ComputeBackend>) -> Self {
        self.compute_backend = Some(backend);
        self
    }

    pub async fn build(self) -> Result<Server> {
        // Determine state directory (Priority: Builder > HOME/.9pe > ./.9pe)
        let ninep_home = if let Some(path) = self.state_directory {
            path
        } else {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(&home).join(".9pe")
        };

        // Extract consensus config and node_id from file config if present
        let (consensus_config, node_id, node_name, dht_port, config_bootstrap, config_services, wasm_modules, file_server_cfg) =
            if let Some(ref file_config) = self.config {
            let consensus = if file_config.consensus.enabled {
                Some(file_config.consensus.clone())
            } else {
                None
            };
            (
                consensus,
                file_config.server.node_id.clone(),
                file_config.server.node_name.clone(),
                file_config.server.dht_port,
                file_config.server.dht_bootstrap_peers.clone(),
                file_config.server.service_discovery.clone(),
                file_config.services.wasm_modules.clone(),
                Some(&file_config.server),
            )
        } else {
            (
                None,
                format!("node-{}", uuid::Uuid::new_v4()),
                None,
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                None,
            )
        };

        // Network Config Priority: Config File > Builder > Default
        // If file config exists, it might override defaults.
        // Since we are not changing ServerBuilder struct to Option, we assume 'self' values are defaults if they match default behavior
        // But better: Use config value if present, otherwise use builder value.

        let max_message_size = file_server_cfg
            .and_then(|c| c.max_message_size)
            .unwrap_or(self.max_message_size);

        let worker_threads = file_server_cfg
            .and_then(|c| c.worker_threads)
            .or(self.worker_threads);

        let mesh_enabled = file_server_cfg
            .and_then(|c| c.mesh_enabled)
            .unwrap_or(self.mesh_enabled);

        let mesh_port = file_server_cfg
            .and_then(|c| c.mesh_port)
            .unwrap_or(self.mesh_port);

        let auto_mount_enabled = file_server_cfg
            .and_then(|c| c.auto_mount_enabled)
            .unwrap_or(self.auto_mount_enabled);

        let dht_bootstrap_peers = if self.dht_bootstrap_peers.is_empty() {
            config_bootstrap
        } else {
            self.dht_bootstrap_peers
        };

        let service_discovery = if self.service_discovery.is_empty() {
            config_services
        } else {
            self.service_discovery
        };

        // Initialize Shared Memory manager early (needed by V8 translator)
        let memory_manager = Arc::new(crate::memory::MemoryManager::new());
        let _default_pool = memory_manager.create_pool(
            crate::memory::PoolConfig::default(),
            crate::memory::AllocationStrategy::FirstFit,
        ).map_err(|e| anyhow::anyhow!("Failed to create default memory pool: {}", e))?;

        let shm = Arc::new(crate::ipc::SharedMemoryManager::new(Arc::clone(&memory_manager))?);

        let root_path_opt = file_server_cfg
            .and_then(|c| c.root.clone())
            .or(self.root_directory);

        // Determine storage provider and effective root path for config
        let (base_provider, effective_root) = if let Some(provider) = self.storage_provider {
             // Injected provider takes precedence
             (provider, root_path_opt.unwrap_or_else(|| PathBuf::from("<custom-storage>")))
        } else if let Some(path) = root_path_opt {
             // Physical root configured
             (
                 Arc::new(crate::storage_adapter::PhysicalStorageAdapter::new(path.clone())) as Arc<dyn crate::traits::StorageProvider>,
                 path
             )
        } else {
             // Default to Synthetic Filesystem
             (
                 Arc::new(crate::storage_adapter::SyntheticStorageAdapter::new(Arc::new(crate::synth::SyntheticFilesystem::new()))) as Arc<dyn crate::traits::StorageProvider>,
                 PathBuf::from("<synthetic>")
             )
        };

        // Wrap in Router for Auto-Mounts
        let storage_provider = if auto_mount_enabled {
             let router = crate::storage_adapter::RouterStorageAdapter::new(base_provider.clone());
             
             // Gemini Bridge
             let gemini = Arc::new(crate::translators::gemini::GeminiTranslator::new());
             let gemini_html = Arc::new(crate::translators::html_renderer::HtmlRenderer::new(gemini.clone()));
             
             // Hypercore Bridge
             let hyper_config = crate::translators::hypercore::HypercoreConfig {
                 storage_path: ninep_home.join("hypercore"),
             };
             let hypercore = Arc::new(crate::translators::hypercore::HypercoreBridge::new(hyper_config));
             let hypercore_html = Arc::new(crate::translators::html_renderer::HtmlRenderer::new(hypercore.clone()));

             // TLD Router at /n/web
             let tld_router = Arc::new(crate::translators::tld_router::TldRouter::new(
                 gemini_html.clone(),
                 hypercore_html.clone(),
                 base_provider.clone(),
             ));
             
             router.mount(PathBuf::from("n/web"), tld_router).await;
             
             // V8 Remote DOM Translator
             let v8 = Arc::new(crate::translators::v8::V8Translator::new(Arc::clone(&shm)));
             router.mount(PathBuf::from("n/v8"), v8.clone()).await;

             // Legacy mounts for compatibility
             router.mount(PathBuf::from("n/gemini"), gemini_html).await;
             router.mount(PathBuf::from("n/hyper"), hypercore_html).await;

             Arc::new(router) as Arc<dyn crate::traits::StorageProvider>
        } else {
             println!("DEBUG: Auto-mount disabled, using base provider");
             base_provider
        };

        // Initialize WASM system
        let wasm_provider = if let Some(wasm) = self.wasm_provider {
            // If a WasmProvider is explicitly provided, use it.
            wasm
        } else {
            // Otherwise, create the default components: registry, settrans, and adapter.
            let registry = Arc::new(ThreadSafeTranslatorRegistry::new(ninep_home.join("translators")));
            
            // The VirtualSettransSystem expects a SyntheticFilesystem.
            // If we are using a SyntheticStorageAdapter, we should ideally share the underlying SyntheticFilesystem
            // but for now, to keep it simple and safe (avoiding downcasting complexity), 
            // we will create a dedicated one for settrans or use a fresh one.
            // Ideally: if storage_provider IS SyntheticStorageAdapter, reuse its FS. 
            // But we don't have easy access to inspect it here without downcasting.
            // Let's create a fresh adapter for settrans for now.
             
            let synth_fs_for_settrans = Arc::new(crate::synth::SyntheticFilesystem::new());
            
            let settrans = Arc::new(VirtualSettransSystem::new(synth_fs_for_settrans, registry.clone()).await?);
            let adapter = Arc::new(crate::wasm_adapter::WasmRegistryAdapter::new(registry.clone(), settrans.clone()));
            adapter as Arc<dyn crate::traits::WasmProvider>
        };

        // Network Config Priority: Config File > Builder > Default
        let network = if let Some(cfg) = file_server_cfg {
            let mut n = self.network_config.unwrap_or_default();
            if let Ok(addr) = cfg.listen_addr.parse::<std::net::SocketAddr>() {
                n.bind_address = crate::network::BindAddress::Specific(addr.ip());
                n.port = addr.port();
            }
            n
        } else {
            self.network_config.unwrap_or_default()
        };

        let config = ServerConfig {
            network,
            transport: file_server_cfg
                .and_then(|c| c.transport.clone())
                .map(|tc| match tc {
                    crate::config::TransportConfig::Tcp => crate::transport::TransportType::Tcp,
                    crate::config::TransportConfig::Quic { server_name } => crate::transport::TransportType::Quic { server_name },
                })
                .unwrap_or(self.transport.unwrap_or_default()),
            root_directory: effective_root,
            max_message_size,
            worker_threads,
            mesh_enabled,
            mesh_port,
            dht_port: dht_port.unwrap_or(mesh_port.saturating_add(1)),
            metrics_enabled: self.metrics_enabled,
            metrics_port: self.metrics_port,
            dht_store_path: ninep_home.join("dht"),
            auto_mount_enabled,
            consensus_config,
            node_id,
            node_name,
            dht_bootstrap_peers,
            service_discovery,
            translator_directory: ninep_home.join("translators"),
            settrans_directory: ninep_home.join("settrans"),
            wasm_modules,
        };

        Server::new(
            config,
            Some(storage_provider), // Pass the determined provider
            self.compute_backend,
            shm,
        ).await
    }
}
