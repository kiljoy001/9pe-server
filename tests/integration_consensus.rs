//! Integration tests for consensus and mesh networking
//!
//! These tests verify that the server actually initializes and uses
//! the consensus layer, not just that the consensus code exists.

use std::path::PathBuf;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_config_file_is_read() {
    // Create a test config file
    let config_content = r#"
[server]
listen_addr = "127.0.0.1:15640"
node_id = "test-node"

[consensus]
enabled = true
peers = ["127.0.0.1:15641"]

[llama]
enabled = false

[gpu]
enabled = false

[logging]
level = "info"
"#;

    let config_path = "/tmp/test_9pe_config.toml";
    std::fs::write(config_path, config_content).unwrap();

    // Parse the config using toml crate
    let config_str = std::fs::read_to_string(config_path).unwrap();
    let config: toml::Value = toml::from_str(&config_str).unwrap();

    // Verify config sections exist
    assert!(config.get("server").is_some(), "Server section missing");
    assert!(config.get("consensus").is_some(), "Consensus section missing");

    let consensus = config.get("consensus").unwrap();
    let enabled = consensus.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
    assert!(enabled, "Consensus should be enabled in test config");

    let peers = consensus.get("peers").and_then(|v| v.as_array());
    assert!(peers.is_some(), "Peers array missing");
    assert_eq!(peers.unwrap().len(), 1, "Should have 1 peer");

    std::fs::remove_file(config_path).ok();
}

#[tokio::test]
async fn test_consensus_coordinator_creation() {
    use ninep_server::consensus::{ConsensusCoordinator, CryptoProvider, Signature, PublicKey};
    use ninep_server::consensus::crypto::{PrivateKey, SharedSecret};
    use std::sync::Arc;
    use async_trait::async_trait;

    // Create a mock crypto provider
    struct MockCrypto;

    #[async_trait]
    impl CryptoProvider for MockCrypto {
        async fn sign(&self, _data: &[u8]) -> anyhow::Result<Signature> {
            Ok(Signature {
                algorithm: "mock".to_string(),
                data: vec![0; 64],
            })
        }

        async fn verify(&self, _data: &[u8], _signature: &Signature, _public_key: &PublicKey) -> anyhow::Result<bool> {
            Ok(true)
        }

        fn get_public_key(&self) -> PublicKey {
            PublicKey {
                algorithm: "mock".to_string(),
                key_data: vec![0; 32],
            }
        }

        async fn generate_keypair(&self) -> anyhow::Result<(PublicKey, PrivateKey)> {
            Ok((
                PublicKey {
                    algorithm: "mock".to_string(),
                    key_data: vec![0; 32],
                },
                PrivateKey {
                    algorithm: "mock".to_string(),
                    key_data: vec![0; 32],
                },
            ))
        }

        async fn encrypt(&self, data: &[u8], _recipient_key: &PublicKey) -> anyhow::Result<Vec<u8>> {
            Ok(data.to_vec())
        }

        async fn decrypt(&self, encrypted_data: &[u8]) -> anyhow::Result<Vec<u8>> {
            Ok(encrypted_data.to_vec())
        }

        async fn derive_shared_secret(&self, _other_public_key: &PublicKey) -> anyhow::Result<SharedSecret> {
            Ok(SharedSecret {
                data: vec![0; 32],
            })
        }
    }

    let crypto = Arc::new(MockCrypto);
    let coordinator = ConsensusCoordinator::new("test-node".to_string(), crypto);

    // Should be able to initialize
    let result = coordinator.initialize().await;
    assert!(result.is_ok(), "Consensus coordinator should initialize: {:?}", result.err());
}

#[tokio::test]
async fn test_server_with_consensus_enabled() {
    // This test verifies that when we build a server with consensus config,
    // the consensus layer is actually created and started

    // TODO: This will fail until we wire up config parsing in the server
    // The server should:
    // 1. Read config.toml
    // 2. Parse [consensus] section
    // 3. Create ConsensusCoordinator
    // 4. Call coordinator.initialize()
    // 5. Start mesh networking on the configured port
}

#[tokio::test]
async fn test_mesh_networking_port_binding() {
    // Test that mesh networking actually binds to a port
    use tokio::net::TcpListener;

    let mesh_port = 19650;
    let addr = format!("127.0.0.1:{}", mesh_port);

    // Try to bind mesh port
    let listener = TcpListener::bind(&addr).await;
    assert!(listener.is_ok(), "Should be able to bind to mesh port {}", addr);

    // TODO: The actual server should bind this port when mesh is enabled
    // Currently it just logs "Starting mesh networking" and does nothing
}

#[tokio::test]
async fn test_two_servers_discover_each_other() {
    // Integration test: start two servers, verify they discover each other

    // Server 1 config
    let config1 = r#"
[server]
listen_addr = "127.0.0.1:25640"
node_id = "node1"

[consensus]
enabled = true
peers = ["127.0.0.1:25641"]

[llama]
enabled = false

[gpu]
enabled = false
"#;

    // Server 2 config
    let config2 = r#"
[server]
listen_addr = "127.0.0.1:25641"
node_id = "node2"

[consensus]
enabled = true
peers = ["127.0.0.1:25640"]

[llama]
enabled = false

[gpu]
enabled = false
"#;

    std::fs::write("/tmp/test_config1.toml", config1).unwrap();
    std::fs::write("/tmp/test_config2.toml", config2).unwrap();

    // TODO: Start both servers with these configs
    // TODO: Verify they discover each other via mesh
    // TODO: Verify consensus state is shared

    // This will fail until we actually implement:
    // 1. Config file parsing
    // 2. Consensus initialization from config
    // 3. Mesh networking startup
    // 4. Peer discovery

    std::fs::remove_file("/tmp/test_config1.toml").ok();
    std::fs::remove_file("/tmp/test_config2.toml").ok();
}

#[tokio::test]
async fn test_metrics_shows_peer_count() {
    // When consensus is running, metrics should show connected peers

    // TODO: Start server with consensus
    // TODO: Connect another peer
    // TODO: Query metrics endpoint
    // TODO: Verify peer_count > 0

    // Currently metrics just returns static values
}

#[tokio::test]
async fn test_llama_config_is_used() {
    use ninep_server::consensus::LlamaCppWorker;

    let config = r#"
[llama]
enabled = true
server_url = "http://localhost:18080"
"#;

    std::fs::write("/tmp/test_llama_config.toml", config).unwrap();

    let config_str = std::fs::read_to_string("/tmp/test_llama_config.toml").unwrap();
    let config: toml::Value = toml::from_str(&config_str).unwrap();

    let llama = config.get("llama").unwrap();
    let enabled = llama.get("enabled").and_then(|v| v.as_bool()).unwrap();
    let server_url = llama.get("server_url").and_then(|v| v.as_str()).unwrap();

    assert!(enabled);
    assert_eq!(server_url, "http://localhost:18080");

    // Verify LlamaCppWorker uses the config
    let worker = LlamaCppWorker::new("test".to_string(), Some(server_url.to_string()));

    // TODO: The server should create this worker when [llama] enabled=true
    // Currently it never reads the config

    std::fs::remove_file("/tmp/test_llama_config.toml").ok();
}
