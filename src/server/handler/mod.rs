//! Message handler module - split into focused submodules

mod basic_ops;
mod ninepee_extensions;
mod connection_state;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use tracing::{debug, info, warn, error};

use crate::wasm::ThreadSafeTranslatorRegistry;
use crate::synth::SyntheticFilesystem;
use crate::settrans::VirtualSettransSystem;
use crate::consensus::BoundedGhostdag;
use crate::protocol::{WireFormat, Message, Tversion, Rversion, Tattach, Rattach, Twalk, Rwalk, Topen, Ropen, Tread, Rread, Twrite, Rwrite, Tclunk, Rclunk, Tstat, Rstat, Tauth, Rauth, MessageType, NinePeeMessage};

use self::connection_state::ConnectionState;
use self::basic_ops::BasicOpsHandler;
use self::ninepee_extensions::NinePeeExtensionsHandler;

// Re-export for testing
pub use self::basic_ops::BasicOpsHandler as PublicBasicOpsHandler;
pub use self::connection_state::ConnectionState as PublicConnectionState;

/// Main message handler that coordinates protocol handling
pub struct MessageHandler {
    /// Root filesystem path
    root: PathBuf,

    /// Maximum message size
    max_message_size: u32,

    /// Connection state management
    connection_state: ConnectionState,

    /// Basic 9P operations handler
    basic_ops: BasicOpsHandler,

    /// 9P.e extensions handler
    ninepee_extensions: NinePeeExtensionsHandler,

    /// Bounded GHOSTDAG consensus for namespace operations
    consensus_dag: Arc<BoundedGhostdag>,
}

impl MessageHandler {
    /// Create a new message handler
    pub fn new(
        root_path: PathBuf,
        max_message_size: u32,
        translator_registry: Arc<ThreadSafeTranslatorRegistry>,
        settrans_system: Arc<VirtualSettransSystem>,
        synth_fs: Arc<SyntheticFilesystem>,
    ) -> Result<Self> {
        let node_id = format!("node-{}", std::process::id());
        let consensus_dag = Arc::new(BoundedGhostdag::new(node_id));
        let connection_state = ConnectionState::new();

        let mut basic_ops = BasicOpsHandler::new(
            root_path.clone(),
            connection_state.clone(),
        );
        basic_ops.set_consensus_dag(consensus_dag.clone());

        let ninepee_extensions = NinePeeExtensionsHandler::new(
            translator_registry,
            settrans_system,
            synth_fs,
            connection_state.clone(),
        );

        Ok(Self {
            root: root_path,
            max_message_size,
            connection_state,
            basic_ops,
            ninepee_extensions,
            consensus_dag,
        })
    }

    /// Deserialize a NinePeeMessage from bytes
    pub async fn deserialize_ninepee_message(&self, data: Vec<u8>) -> Result<NinePeeMessage> {
        bincode::deserialize(&data).map_err(|e| anyhow::anyhow!("Failed to deserialize message: {}", e))
    }

    /// Serialize a NinePeeMessage to bytes
    pub async fn serialize_ninepee_message(&self, message: &NinePeeMessage) -> Result<Vec<u8>> {
        bincode::serialize(message).map_err(|e| anyhow::anyhow!("Failed to serialize message: {}", e))
    }

    /// Handle an incoming 9P message
    pub async fn handle_message(&mut self, message: NinePeeMessage) -> Result<NinePeeMessage> {
        match message {
            // Basic 9P operations
            NinePeeMessage::Version { msize, version } =>
                self.handle_version(msize, version).await,
            NinePeeMessage::Attach { fid, afid, uname, aname } =>
                self.basic_ops.handle_attach(fid, afid, uname, aname).await,
            NinePeeMessage::Walk { fid, newfid, wnames } =>
                self.basic_ops.handle_walk(fid, newfid, wnames).await,
            NinePeeMessage::Open { fid, mode } =>
                self.basic_ops.handle_open(fid, mode).await,
            NinePeeMessage::Create { fid, name, perm, mode } =>
                self.basic_ops.handle_create(fid, name, perm, mode).await,
            NinePeeMessage::Read { fid, offset, count } =>
                self.basic_ops.handle_read(fid, offset, count).await,
            NinePeeMessage::Write { fid, offset, data } =>
                self.basic_ops.handle_write(fid, offset, data).await,
            NinePeeMessage::Clunk { fid } =>
                self.basic_ops.handle_clunk(fid).await,
            NinePeeMessage::Remove { fid } =>
                self.basic_ops.handle_remove(fid).await,
            NinePeeMessage::Stat { fid } =>
                self.basic_ops.handle_stat(fid).await,
            NinePeeMessage::Wstat { fid, stat } =>
                self.basic_ops.handle_wstat(fid, stat).await,

            // 9P.e extensions - use existing variants that map to our functionality
            NinePeeMessage::TranslatorSpawn { translator_id: _, code: _, config: _ } =>
                Ok(NinePeeMessage::Error {
                    ename: "Translator spawn not implemented".to_string(),
                    errno: 38, // ENOSYS
                }),
            NinePeeMessage::TranslatorMessage { translator_id: _, data } =>
                // Map to WASM invoke with dummy path
                self.ninepee_extensions.handle_wasm_invoke("".to_string(), "invoke".to_string(), data).await,
            NinePeeMessage::ConsensusPropose { block_hash: _, parent_hashes: _ } =>
                Ok(NinePeeMessage::Error {
                    ename: "Consensus not implemented".to_string(),
                    errno: 38, // ENOSYS
                }),

            // Unimplemented or deprecated
            _ => Ok(NinePeeMessage::Error {
                ename: "Operation not implemented".to_string(),
                errno: 38, // ENOSYS
            }),
        }
    }

    /// Handle version negotiation
    async fn handle_version(&mut self, msize: u32, version: String) -> Result<NinePeeMessage> {
        debug!("Version negotiation: msize={}, version={}", msize, version);

        // Negotiate message size
        let negotiated_msize = msize.min(self.max_message_size);

        // Check version compatibility
        if !version.starts_with("9P2000") && !version.starts_with("9P.e") {
            return Ok(NinePeeMessage::Error {
                ename: format!("Unsupported protocol version: {}", version),
                errno: 22, // EINVAL
            });
        }

        info!("Protocol negotiated: version={}, msize={}", version, negotiated_msize);

        Ok(NinePeeMessage::Version {
            msize: negotiated_msize,
            version: "9P.e".to_string(),
        })
    }
}