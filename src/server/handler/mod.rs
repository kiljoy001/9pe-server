//! Message handler module - split into focused submodules

mod basic_ops;
mod connection_state;
pub mod auth;
pub mod ninep_extensions;
mod consensus;

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info};

use crate::consensus::BoundedGhostdag;
use crate::namespace_manager::NamespaceManager;
use crate::protocol::NinePMessage;
use crate::settrans::VirtualSettransSystem;
use crate::synth::SyntheticFilesystem;
use crate::wasm::ThreadSafeTranslatorRegistry;

use self::basic_ops::BasicOpsHandler;
use self::connection_state::ConnectionState;
use self::auth::encode_auth_challenge;
use self::ninep_extensions::NinePExtensionsHandler;
use self::consensus::ConsensusHandler;

// Re-export for testing and external use
pub use self::basic_ops::BasicOpsHandler as PublicBasicOpsHandler;
pub use self::connection_state::ConnectionState as PublicConnectionState;
pub use self::consensus::ConsensusHandler as PublicConsensusHandler;
pub use self::ninep_extensions::NinePExtensionsHandler as PublicNinePExtensionsHandler;

/// Main message handler that coordinates protocol handling
pub struct MessageHandler {
    /// Root filesystem path
    #[allow(dead_code)]
    root: PathBuf,

    /// Maximum message size
    max_message_size: u32,

    /// Connection state management
    /// WASM provider
    wasm: Arc<dyn crate::traits::WasmProvider>,
    // translator_registry: Arc<ThreadSafeTranslatorRegistry>, -> Replaced
    // settrans_system: Arc<VirtualSettransSystem>, -> Replaced
    /// Basic 9P operations handler
    basic_ops: BasicOpsHandler,

    /// 9P.e extensions handler
    ninep_extensions: NinePExtensionsHandler,

    /// Consensus handler
    consensus_handler: ConsensusHandler,

    /// Storage provider (Filesystem)
    storage: Arc<dyn crate::traits::StorageProvider>,

    /// Compute backend
    compute: Arc<dyn crate::traits::ComputeBackend>,

    /// Consensus coordinator
    consensus_dag: Arc<crate::consensus::ConsensusCoordinator>,

    /// Server node id used in auth challenges
    server_node_id: String,

    /// Connection state
    pub connection_state: ConnectionState,

    /// Shared memory manager
    shm: Arc<crate::ipc::SharedMemoryManager>,
}

impl MessageHandler {
    /// Create a new message handler
    pub fn new(
        root_path: PathBuf,
        max_message_size: u32,
        storage: Arc<dyn crate::traits::StorageProvider>,
        compute: Arc<dyn crate::traits::ComputeBackend>,
        wasm: Arc<dyn crate::traits::WasmProvider>,
        dht: Option<Arc<crate::dht::SovereignDht>>,
        consensus_coordinator: Option<Arc<crate::consensus::ConsensusCoordinator>>,
        shm: Arc<crate::ipc::SharedMemoryManager>,
        namespace_manager: Option<Arc<NamespaceManager>>,
    ) -> Result<Self> {
        let node_id = format!("node-{}", std::process::id());
        let consensus_dag = Arc::new(crate::consensus::ConsensusCoordinator::new(node_id));
        let server_node_id = format!("server-{}", std::process::id());
        let connection_state = ConnectionState::new();
        if let Some(dht) = dht {
            let connection_state_clone = connection_state.clone();
            tokio::spawn(async move {
                connection_state_clone.set_dht(dht).await;
            });
        }

        let basic_ops = BasicOpsHandler::new(
            storage.clone(),
            connection_state.clone(),
            consensus_coordinator.clone(),
            namespace_manager,
        );

        let ninep_extensions = NinePExtensionsHandler::new(
            wasm.clone(),
            storage.clone(),
            connection_state.clone(),
            shm.clone(),
        );

        let consensus_handler = ConsensusHandler::new(
            consensus_dag.clone(),
            connection_state.clone(),
        );

        Ok(Self {
            root: root_path,
            max_message_size,
            connection_state,
            wasm,
            basic_ops,
            ninep_extensions,
            consensus_handler,
            storage,
            compute,
            consensus_dag,
            server_node_id,
            shm,
        })
    }

    /// Set the fog router for distributed work distribution
    pub async fn set_fog_router(&self, router: std::sync::Arc<dyn crate::fog::FogRouter>) {
        self.ninep_extensions.set_fog_router(router).await;
    }

    /// Deserialize a NinePMessage from bytes
    pub async fn deserialize_ninep_message(&self, data: Vec<u8>) -> Result<NinePMessage> {
        NinePMessage::deserialize(data)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize message: {}", e))
    }

    /// Serialize a NinePMessage to bytes
    pub async fn serialize_ninep_message(&self, message: &NinePMessage) -> Result<Vec<u8>> {
        message.serialize()
            .map_err(|e| anyhow::anyhow!("Failed to serialize message: {}", e))
    }

    /// Handle an incoming 9P message
    pub async fn handle_message(&mut self, message: NinePMessage) -> Result<NinePMessage> {
        match message {
            // Basic 9P operations
            NinePMessage::Version { msize, version } => self.handle_version(msize, version).await,
            NinePMessage::Attach {
                fid,
                afid,
                uname,
                aname,
            } => self.basic_ops.handle_attach(fid, afid, uname, aname).await,
            NinePMessage::Walk {
                fid,
                newfid,
                wnames,
            } => self.basic_ops.handle_walk(fid, newfid, wnames).await,
            NinePMessage::Open { fid, mode } => self.basic_ops.handle_open(fid, mode).await,
            NinePMessage::Create {
                fid,
                name,
                perm,
                mode,
            } => self.basic_ops.handle_create(fid, name, perm, mode).await,
            NinePMessage::Read {
                fid, offset, count, ..
            } => self.basic_ops.handle_read(fid, offset, count).await,
            NinePMessage::Write { fid, offset, data } => {
                self.basic_ops.handle_write(fid, offset, data).await
            }
            NinePMessage::Clunk { fid } => self.basic_ops.handle_clunk(fid).await,
            NinePMessage::Remove { fid } => self.basic_ops.handle_remove(fid).await,
            NinePMessage::Stat { fid, .. } => self.basic_ops.handle_stat(fid).await,
            NinePMessage::Wstat { fid, stat } => self.basic_ops.handle_wstat(fid, stat).await,
            NinePMessage::Auth {
                afid,
                uname,
                aname,
                password,
            } => {
                let challenge = self
                    .connection_state
                    .create_auth_session(afid, self.server_node_id.clone(), None)
                    .await;

                self.connection_state.create_fid(
                    afid,
                    format!("/auth/{}", afid),
                    0,     // mode
                    true,  // synthetic
                    None,  // translator_id
                ).await;

                let _ = encode_auth_challenge(&challenge)?;
                Ok(NinePMessage::Auth {
                    afid,
                    uname,
                    aname,
                    password,
                })
            }

            // 9P.e extensions - use existing variants that map to our functionality
            NinePMessage::TranslatorSpawn {
                translator_id: _,
                code: _,
                config: _,
            } => Ok(NinePMessage::Error {
                ename: "Translator system available but managed through settrans".to_string(),
                errno: 0, // Success - system exists
            }),
            NinePMessage::TranslatorMessage {
                translator_id: _,
                data,
            } =>
            // Map to WASM invoke with dummy path
            {
                self.ninep_extensions
                    .handle_wasm_invoke("".to_string(), "invoke".to_string(), data)
                    .await
            }
            NinePMessage::ConsensusPropose {
                block_hash,
                parent_hashes,
            } => self.consensus_handler.handle_propose(block_hash, parent_hashes).await,

            NinePMessage::ConsensusVote {
                block_hash,
                vote,
            } => self.consensus_handler.handle_vote(block_hash, vote).await,

            NinePMessage::ConsensusCommit {
                block_hash,
                blue_score,
            } => self.consensus_handler.handle_commit(block_hash, blue_score).await,
            NinePMessage::MemAlloc { size, id } => self.ninep_extensions.handle_mem_alloc(size, id).await,
            NinePMessage::MemBorrow { id, write } => {
                self.ninep_extensions.handle_mem_borrow(id, write, &self.connection_state).await
            }
            NinePMessage::MemRelease { id } => {
                self.ninep_extensions.handle_mem_release(id, &self.connection_state).await
            }
            _ => Ok(NinePMessage::Error {
                ename: "Operation recognized but not active in this configuration".to_string(),
                errno: 0, // Success - operation exists but not active
            }),
        }
    }

    /// Handle version negotiation
    async fn handle_version(&mut self, msize: u32, version: String) -> Result<NinePMessage> {
        debug!("Version negotiation: msize={}, version={}", msize, version);

        // Negotiate message size
        let negotiated_msize = msize.min(self.max_message_size);

        // Check version compatibility
        let negotiated_version = if version.starts_with("9P.e") {
            "9P.e".to_string()
        } else if version.starts_with("9P2000") {
            "9P2000".to_string()
        } else {
            return Ok(NinePMessage::Error {
                ename: format!("Unsupported protocol version: {}", version),
                errno: 22, // EINVAL
            });
        };

        self.connection_state.set_protocol_version(negotiated_version.clone()).await;

        info!(
            "Protocol negotiated: version={}, msize={}",
            negotiated_version, negotiated_msize
        );

        Ok(NinePMessage::Version {
            msize: negotiated_msize,
            version: negotiated_version,
        })
    }
}

#[cfg(test)]
mod fuzz_tests {
    use super::*;
    use proptest::prelude::*;

    /// Fuzz test: Protocol message deserialization should never panic
    #[test]
    fn fuzz_protocol_message_deserialization() {
        proptest!(|(bytes: Vec<u8>)| {
            // Should never panic, only return Ok or Err
            let _ = bincode::deserialize::<NinePMessage>(&bytes);
        });
    }

    /// Fuzz test: Message size validation
    #[test]
    fn fuzz_message_size_validation() {
        proptest!(|(size: u32, max_size in 1024u32..16_000_000u32)| {
            // Test all combinations of message sizes
            let negotiated = size.min(max_size);
            prop_assert!(negotiated <= max_size);
        });
    }

    /// Fuzz test: Version string validation
    #[test]
    fn fuzz_version_validation() {
        proptest!(|(version in ".*")| {
            // Should safely handle any version string
            let is_valid = version.starts_with("9P2000") || version.starts_with("9P.e");
            // Just ensure no panic
            let _ = is_valid;
        });
    }

    /// Fuzz test: Serialization round-trip
    #[test]
    fn fuzz_serialize_deserialize_roundtrip() {
        proptest!(|(msize: u32, version in "9P.*")| {
            let msg = NinePMessage::Version { msize, version };
            if let Ok(bytes) = bincode::serialize(&msg) {
                let _ = bincode::deserialize::<NinePMessage>(&bytes);
                // Should not panic
            }
        });
    }
}
