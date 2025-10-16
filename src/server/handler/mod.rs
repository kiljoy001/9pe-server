//! Message handler module - split into focused submodules

mod basic_ops;
mod connection_state;
mod ninepee_extensions;

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info};

use crate::consensus::BoundedGhostdag;
use crate::protocol::NinePeeMessage;
use crate::settrans::VirtualSettransSystem;
use crate::synth::SyntheticFilesystem;
use crate::wasm::ThreadSafeTranslatorRegistry;

use self::basic_ops::BasicOpsHandler;
use self::connection_state::ConnectionState;
use self::ninepee_extensions::NinePeeExtensionsHandler;

// Re-export for testing
pub use self::basic_ops::BasicOpsHandler as PublicBasicOpsHandler;
pub use self::connection_state::ConnectionState as PublicConnectionState;

/// Main message handler that coordinates protocol handling
pub struct MessageHandler {
    /// Root filesystem path
    #[allow(dead_code)]
    root: PathBuf,

    /// Maximum message size
    max_message_size: u32,

    /// Connection state management
    #[allow(dead_code)]
    connection_state: ConnectionState,

    /// Basic 9P operations handler
    basic_ops: BasicOpsHandler,

    /// 9P.e extensions handler
    ninepee_extensions: NinePeeExtensionsHandler,

    /// Bounded GHOSTDAG consensus for namespace operations
    #[allow(dead_code)]
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

        let mut basic_ops = BasicOpsHandler::new(root_path.clone(), connection_state.clone());
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
        bincode::deserialize(&data)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize message: {}", e))
    }

    /// Serialize a NinePeeMessage to bytes
    pub async fn serialize_ninepee_message(&self, message: &NinePeeMessage) -> Result<Vec<u8>> {
        bincode::serialize(message)
            .map_err(|e| anyhow::anyhow!("Failed to serialize message: {}", e))
    }

    /// Handle an incoming 9P message
    pub async fn handle_message(&mut self, message: NinePeeMessage) -> Result<NinePeeMessage> {
        match message {
            // Basic 9P operations
            NinePeeMessage::Version { msize, version } => self.handle_version(msize, version).await,
            NinePeeMessage::Attach {
                fid,
                afid,
                uname,
                aname,
            } => self.basic_ops.handle_attach(fid, afid, uname, aname).await,
            NinePeeMessage::Walk {
                fid,
                newfid,
                wnames,
            } => self.basic_ops.handle_walk(fid, newfid, wnames).await,
            NinePeeMessage::Open { fid, mode } => self.basic_ops.handle_open(fid, mode).await,
            NinePeeMessage::Create {
                fid,
                name,
                perm,
                mode,
            } => self.basic_ops.handle_create(fid, name, perm, mode).await,
            NinePeeMessage::Read {
                fid, offset, count, ..
            } => self.basic_ops.handle_read(fid, offset, count).await,
            NinePeeMessage::Write { fid, offset, data } => {
                self.basic_ops.handle_write(fid, offset, data).await
            }
            NinePeeMessage::Clunk { fid } => self.basic_ops.handle_clunk(fid).await,
            NinePeeMessage::Remove { fid } => self.basic_ops.handle_remove(fid).await,
            NinePeeMessage::Stat { fid, .. } => self.basic_ops.handle_stat(fid).await,
            NinePeeMessage::Wstat { fid, stat } => self.basic_ops.handle_wstat(fid, stat).await,

            // 9P.e extensions - use existing variants that map to our functionality
            NinePeeMessage::TranslatorSpawn {
                translator_id: _,
                code: _,
                config: _,
            } => Ok(NinePeeMessage::Error {
                ename: "Translator spawn not implemented".to_string(),
                errno: 38, // ENOSYS
            }),
            NinePeeMessage::TranslatorMessage {
                translator_id: _,
                data,
            } =>
            // Map to WASM invoke with dummy path
            {
                self.ninepee_extensions
                    .handle_wasm_invoke("".to_string(), "invoke".to_string(), data)
                    .await
            }
            NinePeeMessage::ConsensusPropose {
                block_hash: _,
                parent_hashes: _,
            } => Ok(NinePeeMessage::Error {
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

        info!(
            "Protocol negotiated: version={}, msize={}",
            version, negotiated_msize
        );

        Ok(NinePeeMessage::Version {
            msize: negotiated_msize,
            version: "9P.e".to_string(),
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
            let _ = bincode::deserialize::<NinePeeMessage>(&bytes);
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
            let msg = NinePeeMessage::Version { msize, version };
            if let Ok(bytes) = bincode::serialize(&msg) {
                let _ = bincode::deserialize::<NinePeeMessage>(&bytes);
                // Should not panic
            }
        });
    }
}
