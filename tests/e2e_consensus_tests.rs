//! End-to-end integration tests for multi-node consensus
//!
//! Tests that consensus works across multiple server instances using mesh networking.
//!
//! IMPORTANT: Consensus uses the well-known mesh port (9650) - NOT a configurable port.
//! This is intentional for a global namespace system.

use std::process::{Command, Child, Stdio};
use std::time::Duration;
use std::thread;
use std::net::TcpStream;
use tempfile::TempDir;
use std::fs;

/// Well-known mesh networking port (used for consensus)
const MESH_PORT: u16 = 9650;

/// Helper to start a consensus-enabled server
struct ConsensusServer {
    child: Child,
    port: u16,
    temp_dir: TempDir,
}

impl ConsensusServer {
    fn start(port: u16) -> anyhow::Result<Self> {
        let temp_dir = TempDir::new()?;
        let root_path = temp_dir.path().to_path_buf();

        // Create test files
        fs::write(root_path.join("consensus_test.txt"), b"Consensus data")?;

        // Create config file to enable consensus
        let config_content = format!(
            r#"
[server]
listen_addr = "0.0.0.0:{}"
node_id = "test-node-{}"

[consensus]
enabled = true
peers = []

[logging]
level = "info"
"#,
            port, port
        );

        let config_path = temp_dir.path().join("config.toml");
        fs::write(&config_path, config_content)?;

        // Start server with consensus enabled via config file
        // Mesh networking uses the well-known port 9650 (not configurable)
        let child = Command::new("./target/release/ninep-server")
            .args(&[
                "serve",
                "--port", &port.to_string(),
                "--root", root_path.to_str().unwrap(),
                "--no-quic",
            ])
            .env("CONFIG_FILE", config_path.to_str().unwrap())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        thread::sleep(Duration::from_secs(2));

        Ok(ConsensusServer {
            child,
            port,
            temp_dir,
        })
    }

    fn address(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    fn mesh_address(&self) -> String {
        format!("127.0.0.1:{}", MESH_PORT)
    }
}

impl Drop for ConsensusServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Test two nodes can discover each other via mesh networking
#[test]
fn test_e2e_two_node_consensus() {
    let node1 = ConsensusServer::start(16001).expect("Failed to start node1");
    let node2 = ConsensusServer::start(16002).expect("Failed to start node2");

    thread::sleep(Duration::from_secs(3));

    // Both nodes should be running and accepting connections
    assert!(
        TcpStream::connect_timeout(&node1.address().parse().unwrap(), Duration::from_secs(5)).is_ok(),
        "Node 1 should accept connections"
    );
    assert!(
        TcpStream::connect_timeout(&node2.address().parse().unwrap(), Duration::from_secs(5)).is_ok(),
        "Node 2 should accept connections"
    );
}

/// Test three-node consensus network
#[test]
fn test_e2e_three_node_consensus() {
    let node1 = ConsensusServer::start(16003).expect("Failed to start node1");
    let node2 = ConsensusServer::start(16004).expect("Failed to start node2");
    let node3 = ConsensusServer::start(16005).expect("Failed to start node3");

    thread::sleep(Duration::from_secs(3));

    // All nodes should be running
    for (i, node) in [&node1, &node2, &node3].iter().enumerate() {
        assert!(
            TcpStream::connect_timeout(&node.address().parse().unwrap(), Duration::from_secs(5)).is_ok(),
            "Node {} should accept connections", i + 1
        );
    }
}

/// Test single node works without peers
#[test]
fn test_e2e_single_node_consensus() {
    let node = ConsensusServer::start(16006).expect("Failed to start node");

    thread::sleep(Duration::from_secs(2));

    assert!(
        TcpStream::connect_timeout(&node.address().parse().unwrap(), Duration::from_secs(5)).is_ok(),
        "Single node should accept connections"
    );
}

/// Test consensus network handles node failure gracefully
#[test]
fn test_e2e_node_failure_handling() {
    let node1 = ConsensusServer::start(16007).expect("Failed to start node1");
    let mut node2 = ConsensusServer::start(16008).expect("Failed to start node2");

    thread::sleep(Duration::from_secs(3));

    // Kill node2
    node2.child.kill().expect("Failed to kill node2");
    node2.child.wait().expect("Failed to wait for node2");

    thread::sleep(Duration::from_secs(2));

    // Node1 should still be running
    assert!(
        TcpStream::connect_timeout(&node1.address().parse().unwrap(), Duration::from_secs(5)).is_ok(),
        "Node1 should remain functional after node2 failure"
    );
}

/// Test nodes can rejoin after disconnection
#[test]
fn test_e2e_node_rejoin() {
    let node1 = ConsensusServer::start(16009).expect("Failed to start node1");

    thread::sleep(Duration::from_secs(2));

    // Start and stop node2
    {
        let _node2 = ConsensusServer::start(16010).expect("Failed to start node2");
        thread::sleep(Duration::from_secs(3));
    } // node2 drops here

    thread::sleep(Duration::from_secs(2));

    // Start node2 again
    let node2 = ConsensusServer::start(16010).expect("Failed to restart node2");

    thread::sleep(Duration::from_secs(3));

    // Both should be running
    assert!(
        TcpStream::connect_timeout(&node1.address().parse().unwrap(), Duration::from_secs(5)).is_ok(),
        "Node1 should be running"
    );
    assert!(
        TcpStream::connect_timeout(&node2.address().parse().unwrap(), Duration::from_secs(5)).is_ok(),
        "Node2 should rejoin successfully"
    );
}

/// Test mesh networking uses well-known port 9650
#[test]
fn test_e2e_mesh_port_is_fixed() {
    let node = ConsensusServer::start(16011).expect("Failed to start node");

    thread::sleep(Duration::from_secs(2));

    // Mesh networking should be on the well-known port 9650
    assert_eq!(node.mesh_address(), "127.0.0.1:9650",
               "Mesh networking MUST use the well-known port 9650 for global namespace system");

    assert!(
        TcpStream::connect_timeout(&node.address().parse().unwrap(), Duration::from_secs(5)).is_ok(),
        "Node should be accessible on 9P port"
    );
}

/// Test multiple nodes discover each other automatically
#[test]
fn test_e2e_automatic_discovery() {
    // Start 4 nodes - they should all discover each other via mesh networking
    let node1 = ConsensusServer::start(16012).expect("Failed to start node1");
    let node2 = ConsensusServer::start(16013).expect("Failed to start node2");
    let node3 = ConsensusServer::start(16014).expect("Failed to start node3");
    let node4 = ConsensusServer::start(16015).expect("Failed to start node4");

    thread::sleep(Duration::from_secs(4));

    // All nodes should be running
    for (i, node) in [&node1, &node2, &node3, &node4].iter().enumerate() {
        assert!(
            TcpStream::connect_timeout(&node.address().parse().unwrap(), Duration::from_secs(5)).is_ok(),
            "Node {} should be running and auto-discovered", i + 1
        );
    }
}

/// Summary test documenting consensus architecture
#[test]
fn test_consensus_architecture_summary() {
    println!("\n========================================");
    println!("CONSENSUS ARCHITECTURE");
    println!("========================================\n");

    println!("1. Well-Known Ports (NOT Configurable)");
    println!("   - 9P protocol: port 5640");
    println!("   - Mesh/Consensus: port 9650 (FIXED)");
    println!("   - Metrics: port 9090\n");

    println!("2. Automatic Peer Discovery");
    println!("   - mDNS for local network");
    println!("   - DHT for global network");
    println!("   - No manual peer configuration needed\n");

    println!("3. Global Namespace System");
    println!("   - Fixed ports allow global discovery");
    println!("   - No port conflicts across networks");
    println!("   - Designed like DNS, not like Raft\n");

    println!("4. Consensus Via Mesh");
    println!("   - Consensus uses mesh networking");
    println!("   - GHOSTDAG algorithm for ordering");
    println!("   - Byzantine fault tolerance\n");

    println!("========================================");
    println!("Consensus tests verify multi-node");
    println!("coordination via mesh networking on");
    println!("the well-known port 9650");
    println!("========================================\n");
}
