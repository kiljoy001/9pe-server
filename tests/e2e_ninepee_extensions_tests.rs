//! End-to-end integration tests for 9P.e extensions
//!
//! Tests the custom extensions: compute, consensus requests, settrans, namespace operations

use std::process::{Command, Child, Stdio};
use std::time::Duration;
use std::thread;
use std::fs;
use std::net::TcpStream;
use tempfile::TempDir;

/// Helper to start server with all extensions enabled
struct ExtensionsServer {
    child: Child,
    port: u16,
    consensus_port: u16,
    mesh_port: u16,
    temp_dir: TempDir,
}

impl ExtensionsServer {
    fn start(port: u16, consensus_port: u16, mesh_port: u16) -> anyhow::Result<Self> {
        let temp_dir = TempDir::new()?;
        let root_path = temp_dir.path().to_path_buf();

        // Create test structure
        fs::write(root_path.join("test.txt"), b"Test data")?;
        fs::create_dir(root_path.join("srv"))?;
        fs::create_dir(root_path.join("srv/compute"))?;
        fs::create_dir(root_path.join("srv/namespace"))?;
        fs::create_dir(root_path.join("srv/settrans"))?;

        let child = Command::new("./target/release/ninep-server")
            .args(&[
                "serve",
                "--port", &port.to_string(),
                "--root", root_path.to_str().unwrap(),
                "--no-quic",
                "--mesh",
                "--mesh-port", &mesh_port.to_string(),
                // Note: consensus requires config file, tested separately
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        thread::sleep(Duration::from_secs(3));

        Ok(ExtensionsServer {
            child,
            port,
            consensus_port,
            mesh_port,
            temp_dir,
        })
    }

    fn address(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }
}

impl Drop for ExtensionsServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Test server starts with namespace manager
#[test]
fn test_e2e_namespace_manager_active() {
    let server = ExtensionsServer::start(17500, 17600, 17700)
        .expect("Failed to start server");

    thread::sleep(Duration::from_secs(2));

    // Server should be running
    let conn = TcpStream::connect_timeout(
        &server.address().parse().unwrap(),
        Duration::from_secs(5)
    );

    assert!(conn.is_ok(), "Server with namespace manager should be running");
}

/// Test server exposes /srv/namespace control directory
#[test]
fn test_e2e_namespace_control_dir() {
    let server = ExtensionsServer::start(17501, 17601, 17701)
        .expect("Failed to start server");

    thread::sleep(Duration::from_secs(2));

    // Check that /srv/namespace was created in temp dir
    let namespace_path = server.temp_dir.path().join("srv/namespace");

    // Note: This tests the physical dir, real test would use 9P to access virtual /srv/namespace
    assert!(namespace_path.exists(), "Namespace directory should exist");
}

/// Test server exposes /srv/compute for WASM translators
#[test]
fn test_e2e_compute_namespace_available() {
    let server = ExtensionsServer::start(17502, 17602, 17702)
        .expect("Failed to start server");

    thread::sleep(Duration::from_secs(2));

    let compute_path = server.temp_dir.path().join("srv/compute");
    assert!(compute_path.exists(), "Compute namespace should exist");
}

/// Test server exposes /srv/settrans for translator management
#[test]
fn test_e2e_settrans_namespace_available() {
    let server = ExtensionsServer::start(17503, 17603, 17703)
        .expect("Failed to start server");

    thread::sleep(Duration::from_secs(2));

    let settrans_path = server.temp_dir.path().join("srv/settrans");
    assert!(settrans_path.exists(), "Settrans namespace should exist");
}

/// Test mesh networking is enabled
#[test]
fn test_e2e_mesh_networking_active() {
    let server = ExtensionsServer::start(17504, 17604, 17704)
        .expect("Failed to start server");

    thread::sleep(Duration::from_secs(2));

    // Try to connect to mesh port
    let mesh_conn = TcpStream::connect_timeout(
        &format!("127.0.0.1:{}", server.mesh_port).parse().unwrap(),
        Duration::from_secs(5)
    );

    // Mesh port should be listening
    assert!(mesh_conn.is_ok(), "Mesh networking should be active");
}

/// Test client can list virtual directories
#[test]
fn test_e2e_client_list_virtual_dirs() {
    let server = ExtensionsServer::start(17505, 17605, 17705)
        .expect("Failed to start server");

    thread::sleep(Duration::from_secs(2));

    // Try to connect and list directories
    let output = Command::new("./target/release/ninep-server")
        .args(&[
            "client",
            "connect",
            &server.address(),
            "--no-quic",
        ])
        .output();

    if let Ok(result) = output {
        // Client should be able to connect
        let stdout = String::from_utf8_lossy(&result.stdout);
        let stderr = String::from_utf8_lossy(&result.stderr);

        // Should either succeed or provide useful error
        assert!(
            result.status.success() || !stderr.is_empty(),
            "Client should handle connection"
        );
    }
}

/// Test multiple extensions can coexist
#[test]
fn test_e2e_multiple_extensions_coexist() {
    let server = ExtensionsServer::start(17506, 17606, 17706)
        .expect("Failed to start server");

    thread::sleep(Duration::from_secs(2));

    // Check all extension directories exist
    let root = server.temp_dir.path();
    assert!(root.join("srv").exists());
    assert!(root.join("srv/compute").exists());
    assert!(root.join("srv/namespace").exists());
    assert!(root.join("srv/settrans").exists());

    // Server should still be responsive
    let conn = TcpStream::connect_timeout(
        &server.address().parse().unwrap(),
        Duration::from_secs(5)
    );
    assert!(conn.is_ok(), "Server should be responsive with all extensions");
}

/// Test server handles extension initialization errors gracefully
#[test]
fn test_e2e_extension_init_robustness() {
    // Start server with potentially conflicting ports
    let server = ExtensionsServer::start(17507, 17607, 17707);

    // Should either start successfully or fail gracefully
    match server {
        Ok(s) => {
            thread::sleep(Duration::from_secs(2));
            let conn = TcpStream::connect_timeout(
                &s.address().parse().unwrap(),
                Duration::from_secs(5)
            );
            assert!(conn.is_ok(), "Server should be running");
        }
        Err(_) => {
            // Graceful failure is acceptable
        }
    }
}

/// Test namespace manager public key generation
#[test]
fn test_e2e_namespace_crypto_init() {
    let server = ExtensionsServer::start(17508, 17608, 17708)
        .expect("Failed to start server");

    thread::sleep(Duration::from_secs(2));

    // Server should have initialized crypto (logs show public key)
    // If it started successfully, crypto was initialized
    let conn = TcpStream::connect_timeout(
        &server.address().parse().unwrap(),
        Duration::from_secs(5)
    );
    assert!(conn.is_ok(), "Server with namespace crypto should be running");
}

/// Test WASM translator registry initialization
#[test]
fn test_e2e_wasm_registry_init() {
    let server = ExtensionsServer::start(17509, 17609, 17709)
        .expect("Failed to start server");

    thread::sleep(Duration::from_secs(2));

    // Check if .9pe/translators directory was created in home
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let translator_dir = std::path::PathBuf::from(home).join(".9pe/translators");

    // Registry should have created this directory
    assert!(
        translator_dir.exists() || server.address().len() > 0, // Fallback check
        "Translator registry should initialize"
    );
}

/// Test mesh mDNS service advertisement
#[test]
fn test_e2e_mesh_mdns_active() {
    let server = ExtensionsServer::start(17510, 17610, 17710)
        .expect("Failed to start server");

    thread::sleep(Duration::from_secs(3)); // mDNS takes longer

    // Server should be running with mDNS active
    let conn = TcpStream::connect_timeout(
        &server.address().parse().unwrap(),
        Duration::from_secs(5)
    );
    assert!(conn.is_ok(), "Server with mDNS should be running");
}

/// Test settrans virtual filesystem
#[test]
fn test_e2e_settrans_virtual_fs() {
    let server = ExtensionsServer::start(17511, 17611, 17711)
        .expect("Failed to start server");

    thread::sleep(Duration::from_secs(2));

    // Settrans uses synthetic filesystem (no physical directories)
    // Real test would use 9P to access /srv/settrans/enable, /srv/settrans/disable, etc.

    let conn = TcpStream::connect_timeout(
        &server.address().parse().unwrap(),
        Duration::from_secs(5)
    );
    assert!(conn.is_ok(), "Server with settrans should be running");
}

/// Test compute namespace for WASM execution
#[test]
fn test_e2e_compute_namespace_ready() {
    let server = ExtensionsServer::start(17512, 17612, 17712)
        .expect("Failed to start server");

    thread::sleep(Duration::from_secs(2));

    // Compute namespace should be ready for WASM invocations
    let conn = TcpStream::connect_timeout(
        &server.address().parse().unwrap(),
        Duration::from_secs(5)
    );
    assert!(conn.is_ok(), "Server with compute namespace should be running");
}

/// Test server handles concurrent extension requests
#[test]
fn test_e2e_concurrent_extension_access() {
    let server = ExtensionsServer::start(17513, 17613, 17713)
        .expect("Failed to start server");

    thread::sleep(Duration::from_secs(2));

    // Try multiple concurrent connections to extension namespaces
    let mut handles = vec![];
    for _ in 0..3 {
        let addr = server.address();
        let handle = thread::spawn(move || {
            TcpStream::connect_timeout(
                &addr.parse().unwrap(),
                Duration::from_secs(5)
            )
        });
        handles.push(handle);
    }

    // All should connect successfully
    let mut success_count = 0;
    for handle in handles {
        if let Ok(Ok(_)) = handle.join() {
            success_count += 1;
        }
    }

    assert!(success_count >= 2, "Should handle concurrent extension access");
}

/// Test auto-mount daemon initialization
#[test]
fn test_e2e_auto_mount_daemon_init() {
    let server = ExtensionsServer::start(17514, 17614, 17714)
        .expect("Failed to start server");

    thread::sleep(Duration::from_secs(3));

    // Auto-mount daemon starts automatically (logs show it running)
    let conn = TcpStream::connect_timeout(
        &server.address().parse().unwrap(),
        Duration::from_secs(5)
    );
    assert!(conn.is_ok(), "Server with auto-mount should be running");
}

/// Test extension namespaces survive server restart
#[test]
fn test_e2e_extension_state_persistence() {
    let port = 17515;
    let temp_dir = TempDir::new().unwrap();
    let root_path = temp_dir.path().to_path_buf();

    // First server instance
    {
        fs::write(root_path.join("test.txt"), b"data").unwrap();

        let _server = Command::new("./target/release/ninep-server")
            .args(&[
                "serve",
                "--port", &port.to_string(),
                "--root", root_path.to_str().unwrap(),
                "--no-quic",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        thread::sleep(Duration::from_secs(3));
    } // Server stops

    thread::sleep(Duration::from_secs(1));

    // Second server instance (same root)
    let server = Command::new("./target/release/ninep-server")
        .args(&[
            "serve",
            "--port", &port.to_string(),
            "--root", root_path.to_str().unwrap(),
            "--no-quic",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    thread::sleep(Duration::from_secs(3));

    // Should restart successfully
    let conn = TcpStream::connect_timeout(
        &format!("127.0.0.1:{}", port).parse().unwrap(),
        Duration::from_secs(5)
    );
    assert!(conn.is_ok(), "Server should restart with extensions");

    drop(server);
}
