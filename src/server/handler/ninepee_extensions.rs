//! 9P.e extension operations handler

use std::sync::Arc;
use anyhow::Result;
use tracing::{debug, info, warn};
use crate::protocol::NinePeeMessage;

use crate::wasm::ThreadSafeTranslatorRegistry;
use crate::synth::SyntheticFilesystem;
use crate::settrans::VirtualSettransSystem;
use crate::consensus::{BoundedGhostdag, ConsensusCoordinator, JobRequest};
use crate::consensus::work_distribution::JobRequirements;
use crate::mesh::MeshNetwork;

use super::connection_state::ConnectionState;

/// Handler for 9P.e extension operations
pub struct NinePeeExtensionsHandler {
    /// WASM translator registry
    translator_registry: Arc<ThreadSafeTranslatorRegistry>,

    /// Virtual settrans system
    settrans_system: Arc<VirtualSettransSystem>,

    /// Synthetic filesystem
    synth_fs: Arc<SyntheticFilesystem>,

    /// Connection state
    connection_state: ConnectionState,

    consensus: Option<Arc<BoundedGhostdag>>,
    
    consensus_coordinator: Option<Arc<ConsensusCoordinator>>,

    mesh_network: Option<Arc<MeshNetwork>>,
}

impl NinePeeExtensionsHandler {
    /// Create a new extensions handler
    pub fn new(
        translator_registry: Arc<ThreadSafeTranslatorRegistry>,
        settrans_system: Arc<VirtualSettransSystem>,
        synth_fs: Arc<SyntheticFilesystem>,
        connection_state: ConnectionState,
        consensus: Option<Arc<BoundedGhostdag>>,
        mesh_network: Option<Arc<MeshNetwork>>,
    ) -> Self {
        Self {
            translator_registry,
            settrans_system,
            synth_fs,
            connection_state,
            consensus,
            consensus_coordinator: None,
            mesh_network,
        }
    }
    
    pub fn set_consensus_coordinator(&mut self, coordinator: Arc<ConsensusCoordinator>) {
        self.consensus_coordinator = Some(coordinator);
    }

    /// Handle settrans request
    pub async fn handle_settrans(
        &self,
        path: String,
        translator_name: String,
        args: Vec<String>,
    ) -> Result<NinePeeMessage> {
        debug!("Settrans: path={}, translator={}, args={:?}", path, translator_name, args);

        match self.settrans_system.set_translator(&path, &translator_name, args.clone()).await {
            Ok(_) => {
                info!("Virtual translator {} set on path {}", translator_name, path);
                Ok(NinePeeMessage::SyntheticCreate {
                    fid: 0,
                    generator: format!("settrans:{}:{}", path, translator_name),
                    params: bincode::serialize(&args).unwrap_or_default(),
                })
            }
            Err(e) => {
                warn!("Failed to set translator: {}", e);
                Ok(NinePeeMessage::Error {
                    ename: format!("Failed to set translator: {}", e),
                    errno: 5,
                })
            }
        }
    }

    /// Handle WASM invoke
    pub async fn handle_wasm_invoke(
        &self,
        path: String,
        function: String,
        args: Vec<u8>,
    ) -> Result<NinePeeMessage> {
        debug!("WasmInvoke: path={}, function={}", path, function);

        // Try to find a translator for this path
        let path_buf = std::path::PathBuf::from(&path);

        match self.translator_registry.get_translator(&path_buf).await {
            Some(translator) => {
                match translator.invoke_function(&function, args.clone()).await {
                    Ok(result) => Ok(NinePeeMessage::TranslatorMessage {
                        translator_id: 0,
                        data: result,
                    }),
                    Err(e) => {
                        warn!("WASM function invocation failed: {}", e);
                        Ok(NinePeeMessage::Error {
                            ename: format!("WASM invocation failed: {}", e),
                            errno: 5,
                        })
                    }
                }
            }
            None => Ok(NinePeeMessage::Error {
                ename: format!("No translator found for path {}", path),
                errno: 2,
            })
        }
    }

    /// Handle compute invoke (placeholder)
    pub async fn handle_compute_invoke(
        &self,
        kernel_id: String,
        _work_data: Vec<u8>,
    ) -> Result<NinePeeMessage> {
        debug!("ComputeInvoke: kernel_id={}", kernel_id);

        // Placeholder - OpenCL compute not implemented
        warn!("OpenCL compute invocation not implemented");

        Ok(NinePeeMessage::Error {
            ename: "Compute kernels not implemented".to_string(),
            errno: 38, // ENOSYS
        })
    }

    /// Handle consensus request
    pub async fn handle_consensus_request(
        &self,
        block: Vec<u8>,
    ) -> Result<NinePeeMessage> {
        debug!("ConsensusRequest: block_size={}", block.len());

        match &self.consensus {
            Some(consensus) => {
                match bincode::deserialize::<crate::consensus::Block>(&block) {
                    Ok(block_data) => {
                        match consensus.add_block(block_data).await {
                            Ok(_) => {
                                info!("Block added to consensus successfully");
                                Ok(NinePeeMessage::SyntheticCreate {
                                    fid: 0,
                                    generator: "consensus".to_string(),
                                    params: b"Block accepted".to_vec(),
                                })
                            }
                            Err(e) => {
                                warn!("Failed to add block to consensus: {}", e);
                                Ok(NinePeeMessage::Error {
                                    ename: format!("Failed to add block: {}", e),
                                    errno: 5,
                                })
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to deserialize block: {}", e);
                        Ok(NinePeeMessage::Error {
                            ename: format!("Invalid block data: {}", e),
                            errno: 22,
                        })
                    }
                }
            }
            None => {
                warn!("Consensus system not configured");
                Ok(NinePeeMessage::Error {
                    ename: "Consensus system not configured".to_string(),
                    errno: 38,
                })
            }
        }
    }

    /// Handle mesh connect
    pub async fn handle_mesh_connect(
        &self,
        node_id: String,
        address: String,
    ) -> Result<NinePeeMessage> {
        debug!("MeshConnect: node_id={}, address={}", node_id, address);

        match &self.mesh_network {
            Some(mesh) => {
                match mesh.connect_to_peer(&address, Some(node_id.clone())).await {
                    Ok(_) => {
                        info!("Successfully connected to peer {} at {}", node_id, address);
                        Ok(NinePeeMessage::SyntheticCreate {
                            fid: 0,
                            generator: "mesh".to_string(),
                            params: format!("Connected to {}", node_id).into_bytes(),
                        })
                    }
                    Err(e) => {
                        warn!("Failed to connect to peer: {}", e);
                        Ok(NinePeeMessage::Error {
                            ename: format!("Failed to connect to peer: {}", e),
                            errno: 5,
                        })
                    }
                }
            }
            None => {
                warn!("Mesh networking not configured");
                Ok(NinePeeMessage::Error {
                    ename: "Mesh networking not configured".to_string(),
                    errno: 38,
                })
            }
        }
    }

    /// Handle work submit
    pub async fn handle_work_submit(
        &self,
        task_id: String,
        data: Vec<u8>,
    ) -> Result<NinePeeMessage> {
        debug!("WorkSubmit: task_id={}, data_size={}", task_id, data.len());

        match &self.consensus_coordinator {
            Some(coordinator) => {
                let job = JobRequest {
                    id: String::new(),
                    work_type: task_id.clone(),
                    input_data: data,
                    requirements: JobRequirements {
                        min_nodes: 1,
                        min_cpu_cores: None,
                        min_memory_gb: None,
                        requires_gpu: false,
                        required_capabilities: vec![],
                        geographic_constraints: None,
                    },
                    priority: 1,
                    timeout_seconds: 300,
                    submitted_at: 0,
                };

                match coordinator.submit_work(job).await {
                    Ok(job_id) => {
                        info!("Work submitted successfully: task_id={}, job_id={}", task_id, job_id);
                        Ok(NinePeeMessage::SyntheticCreate {
                            fid: 0,
                            generator: "work".to_string(),
                            params: format!("Work submitted: {}", job_id).into_bytes(),
                        })
                    }
                    Err(e) => {
                        warn!("Failed to submit work: {}", e);
                        Ok(NinePeeMessage::Error {
                            ename: format!("Failed to submit work: {}", e),
                            errno: 5,
                        })
                    }
                }
            }
            None => {
                warn!("Work distribution not configured");
                Ok(NinePeeMessage::Error {
                    ename: "Work distribution not configured".to_string(),
                    errno: 38,
                })
            }
        }
    }

    /// Handle work result
    pub async fn handle_work_result(
        &self,
        task_id: String,
        result: Vec<u8>,
    ) -> Result<NinePeeMessage> {
        debug!("WorkResult: task_id={}, result_size={}", task_id, result.len());

        match &self.consensus_coordinator {
            Some(coordinator) => {
                match coordinator.get_work_result(&task_id).await {
                    Ok(Some(work_result)) => {
                        info!("Work result retrieved for task: {}", task_id);
                        Ok(NinePeeMessage::SyntheticCreate {
                            fid: 0,
                            generator: "work".to_string(),
                            params: work_result.result_data,
                        })
                    }
                    Ok(None) => {
                        warn!("No result found for task: {}", task_id);
                        Ok(NinePeeMessage::Error {
                            ename: format!("No result found for task: {}", task_id),
                            errno: 2,
                        })
                    }
                    Err(e) => {
                        warn!("Failed to get work result: {}", e);
                        Ok(NinePeeMessage::Error {
                            ename: format!("Failed to get work result: {}", e),
                            errno: 5,
                        })
                    }
                }
            }
            None => {
                warn!("Work distribution not configured");
                Ok(NinePeeMessage::Error {
                    ename: "Work distribution not configured".to_string(),
                    errno: 38,
                })
            }
        }
    }
}
