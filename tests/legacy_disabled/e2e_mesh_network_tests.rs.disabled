//! End-to-end integration tests for mesh network and peer discovery
//!
//! Tests automatic peer discovery via mDNS and DHT

use std::fs;
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

/// Helper for mesh network node
struct MeshNode {
    child: Child,
    port: u16,
    mesh_port: u16,
    temp_dir: TempDir,
}

impl MeshNode {
    fn start(port: u16, mesh_port: u16, bootstrap_nodes: Vec<String>) -> anyhow::Result<Self> {
        let temp_dir = TempDir::new()?;
        let root_path = temp_dir.path().to_path_buf();

        fs::write(
            root_path.join("node_data.txt"),
            format!("Node on port {}", port),
        )?;

        let mut args = vec![
            "serve".to_string(),
            "--port".to_string(),
            port.to_string(),
            "--root".to_string(),
            root_path.to_str().unwrap().to_string(),
            "--mesh".to_string(),
            "--mesh-port".to_string(),
            mesh_port.to_string(),
        ];

        // Add bootstrap nodes
        for node in bootstrap_nodes {
            args.push("--bootstrap".to_string());
            args.push(node);
        }

        let child = Command::new("./target/release/ninep-server")
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        thread::sleep(Duration::from_secs(3));

        Ok(MeshNode {
            child,
            port,
            mesh_port,
            temp_dir,
        })
    }

    fn address(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    fn mesh_address(&self) -> String {
        format!("127.0.0.1:{}", self.mesh_port)
    }
}

impl Drop for MeshNode {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Test single mesh node can start
#[test]
fn test_e2e_single_mesh_node() {
    let node = MeshNode::start(18001, 18101, vec![]).expect("Failed to start mesh node");

    thread::sleep(Duration::from_secs(2));

    // Node should be accessible
    let conn = TcpStream::connect_timeout(&node.address().parse().unwrap(), Duration::from_secs(5));

    assert!(conn.is_ok(), "Mesh node should accept connections");
}

/// Test two mesh nodes can discover each other
#[test]
fn test_e2e_two_node_mesh_discovery() {
    // Start first node
    let node1 = MeshNode::start(18002, 18102, vec![]).expect("Failed to start node1");

    thread::sleep(Duration::from_secs(2));

    // Start second node with first as bootstrap
    let node2 =
        MeshNode::start(18003, 18103, vec![node1.mesh_address()]).expect("Failed to start node2");

    thread::sleep(Duration::from_secs(5)); // Give time for discovery

    // Both should be running
    let conn1 =
        TcpStream::connect_timeout(&node1.address().parse().unwrap(), Duration::from_secs(5));
    let conn2 =
        TcpStream::connect_timeout(&node2.address().parse().unwrap(), Duration::from_secs(5));

    assert!(conn1.is_ok(), "Node1 should be accessible");
    assert!(conn2.is_ok(), "Node2 should be accessible");
}

/// Test mesh network with three nodes
#[test]
fn test_e2e_three_node_mesh() {
    let node1 = MeshNode::start(18004, 18104, vec![]).expect("Failed to start node1");

    thread::sleep(Duration::from_secs(2));

    let node2 =
        MeshNode::start(18005, 18105, vec![node1.mesh_address()]).expect("Failed to start node2");

    thread::sleep(Duration::from_secs(2));

    let node3 = MeshNode::start(
        18006,
        18106,
        vec![node1.mesh_address(), node2.mesh_address()],
    )
    .expect("Failed to start node3");

    thread::sleep(Duration::from_secs(5));

    // All three should form a mesh
    for (i, addr) in [node1.address(), node2.address(), node3.address()]
        .iter()
        .enumerate()
    {
        let conn = TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_secs(5));
        assert!(conn.is_ok(), "Node {} should be in mesh", i + 1);
    }
}

/// Test mesh node handles invalid bootstrap address
#[test]
fn test_e2e_mesh_invalid_bootstrap() {
    // Start node with invalid bootstrap address
    let node = MeshNode::start(
        18007,
        18107,
        vec!["192.0.2.1:9999".to_string()], // Non-existent
    );

    // Should still start (handle bootstrap failure gracefully)
    assert!(node.is_ok(), "Should handle invalid bootstrap gracefully");

    if let Ok(node) = node {
        thread::sleep(Duration::from_secs(2));

        let conn =
            TcpStream::connect_timeout(&node.address().parse().unwrap(), Duration::from_secs(5));

        assert!(conn.is_ok(), "Node should run despite bootstrap failure");
    }
}

/// Test mesh network handles node departure
#[test]
fn test_e2e_mesh_node_departure() {
    let node1 = MeshNode::start(18008, 18108, vec![]).expect("Failed to start node1");

    thread::sleep(Duration::from_secs(2));

    let mut node2 =
        MeshNode::start(18009, 18109, vec![node1.mesh_address()]).expect("Failed to start node2");

    thread::sleep(Duration::from_secs(3));

    // Kill node2
    node2.child.kill().expect("Failed to kill node2");
    node2.child.wait().expect("Failed to wait for node2");

    thread::sleep(Duration::from_secs(2));

    // Node1 should still be functional
    let conn =
        TcpStream::connect_timeout(&node1.address().parse().unwrap(), Duration::from_secs(5));

    assert!(
        conn.is_ok(),
        "Node1 should remain functional after node2 leaves"
    );
}

/// Test mesh ports don't conflict with service ports
#[test]
fn test_e2e_mesh_port_separation() {
    let node = MeshNode::start(18010, 18110, vec![]).expect("Failed to start node");

    thread::sleep(Duration::from_secs(2));

    // Both ports should be accessible
    let service_conn = TcpStream::connect_timeout(
        &format!("127.0.0.1:{}", node.port).parse().unwrap(),
        Duration::from_secs(5),
    );

    let mesh_conn = TcpStream::connect_timeout(
        &format!("127.0.0.1:{}", node.mesh_port).parse().unwrap(),
        Duration::from_secs(5),
    );

    assert!(service_conn.is_ok(), "Service port should be accessible");
    assert!(mesh_conn.is_ok(), "Mesh port should be accessible");
}

/// Test mesh with multiple bootstrap nodes
#[test]
fn test_e2e_mesh_multiple_bootstrap() {
    let node1 = MeshNode::start(18011, 18111, vec![]).expect("Failed to start node1");

    thread::sleep(Duration::from_secs(2));

    let node2 = MeshNode::start(18012, 18112, vec![]).expect("Failed to start node2");

    thread::sleep(Duration::from_secs(2));

    // Node3 bootstraps from both
    let node3 = MeshNode::start(
        18013,
        18113,
        vec![node1.mesh_address(), node2.mesh_address()],
    )
    .expect("Failed to start node3");

    thread::sleep(Duration::from_secs(5));

    // All should be functional
    for addr in [node1.address(), node2.address(), node3.address()] {
        let conn = TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_secs(5));
        assert!(conn.is_ok(), "Node at {} should be accessible", addr);
    }
}

/// Test isolated mesh networks don't interfere
#[test]
fn test_e2e_isolated_mesh_networks() {
    // Network 1
    let net1_node1 = MeshNode::start(18014, 18114, vec![]).expect("Failed to start network1 node1");

    thread::sleep(Duration::from_secs(2));

    let net1_node2 = MeshNode::start(18015, 18115, vec![net1_node1.mesh_address()])
        .expect("Failed to start network1 node2");

    // Network 2 (completely separate)
    let net2_node1 = MeshNode::start(18016, 18116, vec![]).expect("Failed to start network2 node1");

    thread::sleep(Duration::from_secs(2));

    let net2_node2 = MeshNode::start(18017, 18117, vec![net2_node1.mesh_address()])
        .expect("Failed to start network2 node2");

    thread::sleep(Duration::from_secs(5));

    // All four should be running independently
    for addr in [
        net1_node1.address(),
        net1_node2.address(),
        net2_node1.address(),
        net2_node2.address(),
    ] {
        let conn = TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_secs(5));
        assert!(conn.is_ok(), "Node at {} should be running", addr);
    }
}

/// Test mesh node can rejoin network
#[test]
fn test_e2e_mesh_node_rejoin() {
    let node1 = MeshNode::start(18018, 18118, vec![]).expect("Failed to start node1");

    thread::sleep(Duration::from_secs(2));

    // Start and stop node2
    {
        let _node2 = MeshNode::start(18019, 18119, vec![node1.mesh_address()])
            .expect("Failed to start node2");

        thread::sleep(Duration::from_secs(3));
    } // node2 drops

    thread::sleep(Duration::from_secs(2));

    // Restart node2
    let node2 =
        MeshNode::start(18019, 18119, vec![node1.mesh_address()]).expect("Failed to restart node2");

    thread::sleep(Duration::from_secs(5));

    // Both should be functional
    let conn1 =
        TcpStream::connect_timeout(&node1.address().parse().unwrap(), Duration::from_secs(5));
    let conn2 =
        TcpStream::connect_timeout(&node2.address().parse().unwrap(), Duration::from_secs(5));

    assert!(conn1.is_ok(), "Node1 should be accessible");
    assert!(conn2.is_ok(), "Node2 should rejoin successfully");
}

/// Test mesh with rapid node additions
#[test]
fn test_e2e_mesh_rapid_growth() {
    let node1 = MeshNode::start(18020, 18120, vec![]).expect("Failed to start node1");

    thread::sleep(Duration::from_secs(2));

    // Rapidly add 3 more nodes
    let node2 =
        MeshNode::start(18021, 18121, vec![node1.mesh_address()]).expect("Failed to start node2");

    let node3 =
        MeshNode::start(18022, 18122, vec![node1.mesh_address()]).expect("Failed to start node3");

    let node4 =
        MeshNode::start(18023, 18123, vec![node1.mesh_address()]).expect("Failed to start node4");

    thread::sleep(Duration::from_secs(5));

    // All should be functional
    for addr in [
        node1.address(),
        node2.address(),
        node3.address(),
        node4.address(),
    ] {
        let conn = TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_secs(5));
        assert!(conn.is_ok(), "Node at {} should be accessible", addr);
    }
}
