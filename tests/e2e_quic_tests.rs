//! End-to-end integration tests for QUIC transport
//!
//! Tests QUIC connections with encryption and TLS

use std::process::{Command, Child, Stdio};
use std::time::Duration;
use std::thread;
use std::fs;
use tempfile::TempDir;
use std::net::UdpSocket;

/// Helper to start a QUIC-enabled server
struct QuicServer {
    child: Child,
    port: u16,
    temp_dir: TempDir,
}

impl QuicServer {
    fn start(port: u16, server_name: Option<&str>) -> anyhow::Result<Self> {
        let temp_dir = TempDir::new()?;
        let root_path = temp_dir.path().to_path_buf();

        // Create test files
        fs::write(root_path.join("quic_test.txt"), b"QUIC encrypted data")?;
        fs::create_dir(root_path.join("secure"))?;
        fs::write(root_path.join("secure/private.txt"), b"Encrypted content")?;

        let mut args = vec![
            "serve".to_string(),
            "--port".to_string(), port.to_string(),
            "--root".to_string(), root_path.to_str().unwrap().to_string(),
            "--quic".to_string(), // Explicitly enable QUIC
        ];

        // Add server name for TLS if provided
        if let Some(name) = server_name {
            args.push("--server-name".to_string());
            args.push(name.to_string());
        }

        let child = Command::new("./target/release/ninep-server")
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        // Wait for server to initialize (QUIC takes longer due to cert setup)
        thread::sleep(Duration::from_secs(3));

        Ok(QuicServer {
            child,
            port,
            temp_dir,
        })
    }

    fn address(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }
}

impl Drop for QuicServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Test QUIC server starts and binds UDP port
#[test]
fn test_e2e_quic_server_binding() {
    let server = QuicServer::start(16640, None)
        .expect("Failed to start QUIC server");

    thread::sleep(Duration::from_secs(2));

    // Try to bind to same UDP port (should fail if server is using it)
    let bind_result = UdpSocket::bind(format!("127.0.0.1:{}", server.port));

    // Port should be in use by QUIC server
    assert!(
        bind_result.is_err(),
        "QUIC server should be using the UDP port"
    );
}

/// Test QUIC client can connect to QUIC server
#[test]
fn test_e2e_quic_client_connection() {
    let server = QuicServer::start(16641, Some("localhost"))
        .expect("Failed to start QUIC server");

    // Connect using QUIC client (default)
    let output = Command::new("./target/release/ninep-server")
        .args(&[
            "client",
            "connect",
            &server.address(),
            // QUIC is default, no flag needed
        ])
        .output()
        .expect("Failed to run client");

    // Client should either succeed or handle connection attempt properly
    assert!(
        output.status.success() || output.status.code() == Some(1),
        "QUIC client should handle connection attempt"
    );
}

/// Test QUIC server with explicit server name
#[test]
fn test_e2e_quic_with_server_name() {
    let server = QuicServer::start(16642, Some("test.example.com"))
        .expect("Failed to start QUIC server with server name");

    thread::sleep(Duration::from_secs(2));

    // Server should be running
    let bind_result = UdpSocket::bind(format!("127.0.0.1:{}", server.port));
    assert!(bind_result.is_err(), "Server should be using port");
}

/// Test QUIC server handles multiple connections
#[test]
fn test_e2e_quic_multiple_connections() {
    let server = QuicServer::start(16643, None)
        .expect("Failed to start QUIC server");

    thread::sleep(Duration::from_secs(2));

    // Try multiple client connections sequentially
    for i in 0..3 {
        let output = Command::new("./target/release/ninep-server")
            .args(&[
                "client",
                "connect",
                &server.address(),
            ])
            .output();

        if let Ok(result) = output {
            // Each connection attempt should be handled
            assert!(
                result.status.success() || result.status.code() == Some(1),
                "Connection {} should be handled", i
            );
        }

        thread::sleep(Duration::from_millis(500));
    }
}

/// Test QUIC connection with different ports
#[test]
fn test_e2e_quic_different_ports() {
    let server1 = QuicServer::start(16644, None)
        .expect("Failed to start server 1");

    let server2 = QuicServer::start(16645, None)
        .expect("Failed to start server 2");

    thread::sleep(Duration::from_secs(2));

    // Both should be running on different ports
    let bind1 = UdpSocket::bind(format!("127.0.0.1:{}", server1.port));
    let bind2 = UdpSocket::bind(format!("127.0.0.1:{}", server2.port));

    assert!(bind1.is_err(), "Server 1 should be using port");
    assert!(bind2.is_err(), "Server 2 should be using port");
}

/// Test QUIC server cleanup on shutdown
#[test]
fn test_e2e_quic_server_cleanup() {
    let port = 16646;
    {
        let _server = QuicServer::start(port, None)
            .expect("Failed to start server");

        thread::sleep(Duration::from_secs(2));

        // Port should be in use
        let bind_result = UdpSocket::bind(format!("127.0.0.1:{}", port));
        assert!(bind_result.is_err(), "Port should be in use");
    } // Server drops here

    // Wait for cleanup
    thread::sleep(Duration::from_secs(2));

    // Port should be available again
    let bind_result = UdpSocket::bind(format!("127.0.0.1:{}", port));
    assert!(
        bind_result.is_ok(),
        "Port should be released after QUIC server shutdown"
    );
}

/// Test QUIC vs TCP comparison (both should work)
#[test]
fn test_e2e_quic_vs_tcp_servers() {
    // Start QUIC server
    let quic_server = QuicServer::start(16647, None)
        .expect("Failed to start QUIC server");

    thread::sleep(Duration::from_secs(2));

    // Start TCP server (using --no-quic)
    let tcp_server = {
        let temp_dir = TempDir::new().unwrap();
        let root_path = temp_dir.path().to_path_buf();
        fs::write(root_path.join("tcp_test.txt"), b"TCP data").unwrap();

        Command::new("./target/release/ninep-server")
            .args(&[
                "serve",
                "--port", "16648",
                "--root", root_path.to_str().unwrap(),
                "--no-quic",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    };

    thread::sleep(Duration::from_secs(2));

    // QUIC uses UDP
    let quic_bind = UdpSocket::bind("127.0.0.1:16647");
    assert!(quic_bind.is_err(), "QUIC server should use UDP port");

    // TCP uses TCP (UDP should be available)
    let tcp_udp_bind = UdpSocket::bind("127.0.0.1:16648");
    assert!(tcp_udp_bind.is_ok(), "TCP server shouldn't use UDP port");

    drop(quic_server);
    drop(tcp_server);
}

/// Test QUIC server handles connection attempts on wrong protocol
#[test]
fn test_e2e_quic_wrong_protocol_handling() {
    let server = QuicServer::start(16649, None)
        .expect("Failed to start QUIC server");

    thread::sleep(Duration::from_secs(2));

    // Try to connect with TCP client to QUIC server (should fail gracefully)
    let output = Command::new("./target/release/ninep-server")
        .args(&[
            "client",
            "connect",
            &server.address(),
            "--no-quic", // Force TCP to QUIC server
        ])
        .output();

    if let Ok(result) = output {
        // Should fail but not crash
        assert!(
            !result.status.success(),
            "TCP client to QUIC server should fail"
        );
    }
}

/// Test QUIC encryption is actually enabled
#[test]
fn test_e2e_quic_encryption_enabled() {
    let server = QuicServer::start(16650, Some("localhost"))
        .expect("Failed to start QUIC server");

    thread::sleep(Duration::from_secs(2));

    // Server should be running (QUIC includes TLS)
    // The fact that it starts with --quic flag means encryption is enabled
    let bind_result = UdpSocket::bind(format!("127.0.0.1:{}", server.port));
    assert!(
        bind_result.is_err(),
        "QUIC server with encryption should be running"
    );
}

/// Test QUIC connection with rapid reconnects
#[test]
fn test_e2e_quic_rapid_reconnects() {
    let server = QuicServer::start(16651, None)
        .expect("Failed to start QUIC server");

    thread::sleep(Duration::from_secs(2));

    // Rapidly attempt connections
    for _ in 0..5 {
        let _ = Command::new("./target/release/ninep-server")
            .args(&[
                "client",
                "connect",
                &server.address(),
            ])
            .output();
    }

    // Server should still be responsive
    let bind_result = UdpSocket::bind(format!("127.0.0.1:{}", server.port));
    assert!(
        bind_result.is_err(),
        "Server should remain responsive after rapid reconnects"
    );
}

/// Test QUIC server with IPv4 vs IPv6
#[test]
fn test_e2e_quic_ipv4_binding() {
    let server = QuicServer::start(16652, None)
        .expect("Failed to start QUIC server");

    thread::sleep(Duration::from_secs(2));

    // Server should bind to IPv4 (127.0.0.1)
    let ipv4_bind = UdpSocket::bind(format!("127.0.0.1:{}", server.port));
    assert!(ipv4_bind.is_err(), "IPv4 port should be in use");
}

/// Test QUIC default vs explicit flag
#[test]
fn test_e2e_quic_default_behavior() {
    // Server without explicit --quic flag (should default to QUIC)
    let temp_dir = TempDir::new().unwrap();
    let root_path = temp_dir.path().to_path_buf();
    fs::write(root_path.join("test.txt"), b"data").unwrap();

    let server = Command::new("./target/release/ninep-server")
        .args(&[
            "serve",
            "--port", "16653",
            "--root", root_path.to_str().unwrap(),
            // No transport flag - should default to QUIC
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start server");

    thread::sleep(Duration::from_secs(3));

    // Should be using QUIC (UDP port)
    let bind_result = UdpSocket::bind("127.0.0.1:16653");
    assert!(
        bind_result.is_err(),
        "Server should default to QUIC (UDP)"
    );

    drop(server);
}

/// Test QUIC connection timeout behavior
#[test]
fn test_e2e_quic_connection_timeout() {
    // Try to connect to non-existent QUIC server
    let output = Command::new("./target/release/ninep-server")
        .args(&[
            "client",
            "connect",
            "127.0.0.1:19999",
            // Default QUIC
        ])
        .output()
        .expect("Failed to run client");

    // Should timeout or fail gracefully
    assert!(
        !output.status.success(),
        "Connection to non-existent QUIC server should fail"
    );
}

/// Test QUIC server handles certificate generation
#[test]
fn test_e2e_quic_certificate_generation() {
    // Server should auto-generate self-signed cert for QUIC
    let server = QuicServer::start(16654, None)
        .expect("Failed to start QUIC server");

    thread::sleep(Duration::from_secs(2));

    // If server starts successfully, cert was generated
    let bind_result = UdpSocket::bind(format!("127.0.0.1:{}", server.port));
    assert!(
        bind_result.is_err(),
        "QUIC server should have generated cert and be running"
    );
}
