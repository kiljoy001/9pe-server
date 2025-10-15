use crate::synth::{ControlHandler, SyntheticFilesystem};
use crate::server::ServerConfig;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use futures::executor::block_on;

pub async fn register_config_controls(synth: &SyntheticFilesystem, cfg: &ServerConfig) -> Result<()> {
    let base = PathBuf::from("/srv/config");
    synth.create_directory(&base).await?;

    // listen address (as string)
    let listen = Arc::new(RwLock::new(cfg.network.socket_addr().map(|a| a.to_string()).unwrap_or_default()));
    synth.create_control_file(&base.join("listen_addr"), Arc::new(StringControl { value: listen }))
        .await?;

    // node id
    let node = Arc::new(RwLock::new(cfg.node_id.clone()));
    synth.create_control_file(&base.join("node_id"), Arc::new(StringControl { value: node }))
        .await?;

    // mesh enabled flag
    let mesh_enabled = Arc::new(RwLock::new(cfg.mesh_enabled.to_string()));
    synth.create_control_file(&base.join("mesh_enabled"), Arc::new(StringControl { value: mesh_enabled }))
        .await?;

    // mesh port
    let mesh_port = Arc::new(RwLock::new(cfg.mesh_port.to_string()));
    synth.create_control_file(&base.join("mesh_port"), Arc::new(StringControl { value: mesh_port }))
        .await?;

    Ok(())
}

struct StringControl {
    value: Arc<RwLock<String>>,
}

impl ControlHandler for StringControl {
    fn read(&self) -> Result<Vec<u8>> {
        let v = block_on(self.value.read());
        Ok(v.clone().into_bytes())
    }
    fn write(&self, data: &[u8]) -> Result<()> {
        let s = String::from_utf8(data.to_vec()).map_err(|e| anyhow::anyhow!("Invalid UTF-8: {}", e))?;
        let mut v = block_on(self.value.write());
        *v = s;
        Ok(())
    }
}
