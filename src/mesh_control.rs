//! Mesh networking control via synthetic files
//!
//! Everything is a file, every file is a function!
//! Control mesh networking by reading/writing files in /srv/mesh/

use crate::synth::{ControlHandler, SyntheticFilesystem};
use crate::mesh::MeshNetwork;
use anyhow::Result;
use std::sync::Arc;
use std::path::PathBuf;

/// Register mesh control files in the synthetic filesystem
pub async fn register_mesh_control(
    synth: &SyntheticFilesystem,
    mesh: Arc<MeshNetwork>,
) -> Result<()> {
    // Create /srv/mesh directory
    synth.create_directory(&PathBuf::from("/srv/mesh")).await?;

    // /srv/mesh/peers - Read to see connected peers
    synth.create_control_file(
        &PathBuf::from("/srv/mesh/peers"),
        Arc::new(PeersHandler { mesh: mesh.clone() })
    ).await?;

    // /srv/mesh/connect - Write peer address to connect
    synth.create_control_file(
        &PathBuf::from("/srv/mesh/connect"),
        Arc::new(ConnectHandler { mesh: mesh.clone() })
    ).await?;

    // /srv/mesh/disconnect - Write peer ID to disconnect
    synth.create_control_file(
        &PathBuf::from("/srv/mesh/disconnect"),
        Arc::new(DisconnectHandler { mesh: mesh.clone() })
    ).await?;

    // /srv/mesh/announce - Write service name to announce via mDNS
    synth.create_control_file(
        &PathBuf::from("/srv/mesh/announce"),
        Arc::new(AnnounceHandler { mesh: mesh.clone() })
    ).await?;

    // /srv/mesh/status - Read mesh network status
    synth.create_control_file(
        &PathBuf::from("/srv/mesh/status"),
        Arc::new(StatusHandler { mesh: mesh.clone() })
    ).await?;

    // /srv/mesh/dht - Read DHT routing table
    synth.create_control_file(
        &PathBuf::from("/srv/mesh/dht"),
        Arc::new(DhtHandler { mesh: mesh.clone() })
    ).await?;

    Ok(())
}

/// Handler for /srv/mesh/peers - list connected peers
struct PeersHandler {
    mesh: Arc<MeshNetwork>,
}

impl ControlHandler for PeersHandler {
    fn read(&self) -> Result<Vec<u8>> {
        let peers = futures::executor::block_on(self.mesh.get_all_peers());

        let mut output = String::new();
        for (peer_id, peer) in peers {
            let status = if peer.is_connected() { "connected" } else { "disconnected" };
            output.push_str(&format!(
                "{}\t{}\t{}\t{:?}\n",
                peer_id,
                peer.address().unwrap_or("unknown".to_string()),
                status,
                peer.last_seen()
            ));
        }

        Ok(output.into_bytes())
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("peers file is read-only, use 'connect' to add peers"))
    }
}

/// Handler for /srv/mesh/connect - connect to a peer
struct ConnectHandler {
    mesh: Arc<MeshNetwork>,
}

impl ControlHandler for ConnectHandler {
    fn read(&self) -> Result<Vec<u8>> {
        Ok(b"Write peer address (format: peer-id@ip:port or ip:port)\n".to_vec())
    }

    fn write(&self, data: &[u8]) -> Result<()> {
        let address = String::from_utf8(data.to_vec())?
            .trim()
            .to_string();

        // Parse address: "peer-id@ip:port" or just "ip:port"
        let (peer_id, addr) = if let Some((id, addr)) = address.split_once('@') {
            (Some(id.to_string()), addr.to_string())
        } else {
            (None, address)
        };

        // Connect to peer
        futures::executor::block_on(async {
            self.mesh.connect_to_peer(&addr, peer_id).await
        })?;

        Ok(())
    }
}

/// Handler for /srv/mesh/disconnect - disconnect from a peer
struct DisconnectHandler {
    mesh: Arc<MeshNetwork>,
}

impl ControlHandler for DisconnectHandler {
    fn read(&self) -> Result<Vec<u8>> {
        Ok(b"Write peer ID to disconnect\n".to_vec())
    }

    fn write(&self, data: &[u8]) -> Result<()> {
        let peer_id = String::from_utf8(data.to_vec())?
            .trim()
            .to_string();

        futures::executor::block_on(async {
            self.mesh.disconnect_peer(&peer_id).await
        })?;

        Ok(())
    }
}

/// Handler for /srv/mesh/announce - announce service via mDNS
struct AnnounceHandler {
    mesh: Arc<MeshNetwork>,
}

impl ControlHandler for AnnounceHandler {
    fn read(&self) -> Result<Vec<u8>> {
        Ok(b"Write service name to announce (e.g., 'myserver._9pe._tcp.local')\n".to_vec())
    }

    fn write(&self, data: &[u8]) -> Result<()> {
        let service_name = String::from_utf8(data.to_vec())?
            .trim()
            .to_string();

        futures::executor::block_on(async {
            self.mesh.announce_service(&service_name).await
        })?;

        Ok(())
    }
}

/// Handler for /srv/mesh/status - mesh network status
struct StatusHandler {
    mesh: Arc<MeshNetwork>,
}

impl ControlHandler for StatusHandler {
    fn read(&self) -> Result<Vec<u8>> {
        let status = futures::executor::block_on(self.mesh.get_status());

        let output = format!(
            "Mesh Network Status\n\
             ===================\n\
             Node ID: {}\n\
             Peer Count: {}\n\
             Active Connections: {}\n\
             mDNS Enabled: {}\n\
             DHT Enabled: {}\n\
             Uptime: {}s\n",
            status.node_id,
            status.peer_count,
            status.active_connections,
            status.mdns_enabled,
            status.dht_enabled,
            status.uptime_seconds
        );

        Ok(output.into_bytes())
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("status file is read-only"))
    }
}

/// Handler for /srv/mesh/dht - DHT routing table
struct DhtHandler {
    mesh: Arc<MeshNetwork>,
}

impl ControlHandler for DhtHandler {
    fn read(&self) -> Result<Vec<u8>> {
        let routing_table = futures::executor::block_on(self.mesh.get_dht_routing_table());

        let mut output = String::from("DHT Routing Table\n=================\n");
        for (key, peer_id) in routing_table {
            output.push_str(&format!("{} -> {}\n", hex::encode(&key), peer_id));
        }

        Ok(output.into_bytes())
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("dht file is read-only"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mesh_control_registration() {
        let synth = SyntheticFilesystem::new();
        let mesh = Arc::new(MeshNetwork::new("test-node".to_string(), 9650, vec![]));

        register_mesh_control(&synth, mesh).await.expect("Failed to register mesh control");

        // Check that /srv/mesh directory exists
        assert!(synth.exists(&PathBuf::from("/srv/mesh")).await);
        assert!(synth.exists(&PathBuf::from("/srv/mesh/peers")).await);
        assert!(synth.exists(&PathBuf::from("/srv/mesh/connect")).await);
        assert!(synth.exists(&PathBuf::from("/srv/mesh/status")).await);
    }
}
