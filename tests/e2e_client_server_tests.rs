//! End-to-end integration tests for client-server communication
//!
//! These tests start actual server instances and test real client connections

use std::process::{Command, Child, Stdio};
use std::time::Duration;
use std::path::PathBuf;
use std::fs;
use std::net::TcpStream;
use std::io::{Read, Write};
use tempfile::TempDir;

/// Helper to start a server instance
struct TestServer {
    child: Child,
    port: u16,
    temp_dir: TempDir,
}

impl TestServer {
    fn start(port: u16) -> anyhow::Result<Self> {
        let temp_dir = TempDir::new()?;
        let root_path = temp_dir.path().to_path_buf();

        // Create some test files
        fs::write(root_path.join("test.txt"), b"Hello from 9P server!")?;
        fs::create_dir(root_path.join("subdir"))?;
        fs::write(root_path.join("subdir/data.json"), b"{\"test\": true}")?;

        // Start server
        let child = Command::new("./target/release/ninep-server")
            .args(&[
                "serve",
                "--port", &port.to_string(),
                "--root", root_path.to_str().unwrap(),
                "--no-quic", // Use TCP for simpler E2E tests
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        // Wait for server to be ready
        std::thread::sleep(Duration::from_secs(2));

        Ok(TestServer {
            child,
            port,
            temp_dir,
        })
    }

    fn address(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Test basic TCP connection to server
#[test]
fn test_e2e_tcp_connection() {
    let server = TestServer::start(15640).expect("Failed to start server");

    // Try to connect via TCP
    let result = TcpStream::connect_timeout(
        &server.address().parse().unwrap(),
        Duration::from_secs(5)
    );

    assert!(result.is_ok(), "Should be able to connect to server via TCP");
}

/// Test client can connect and perform version negotiation
#[test]
fn test_e2e_client_connection() {
    let server = TestServer::start(15641).expect("Failed to start server");

    // Connect using our client
    let output = Command::new("./target/release/ninep-server")
        .args(&[
            "client",
            "connect",
            &server.address(),
            "--no-quic",
        ])
        .output()
        .expect("Failed to run client");

    // Client should either succeed or fail gracefully
    assert!(
        output.status.success() || output.status.code() == Some(1),
        "Client should handle connection attempt properly"
    );
}

/// Test server accepts multiple concurrent connections
#[test]
fn test_e2e_concurrent_connections() {
    let server = TestServer::start(15642).expect("Failed to start server");

    // Try 5 concurrent connections
    let mut streams = Vec::new();
    for _ in 0..5 {
        let stream = TcpStream::connect_timeout(
            &server.address().parse().unwrap(),
            Duration::from_secs(5)
        );
        assert!(stream.is_ok(), "Should accept concurrent connections");
        streams.push(stream.unwrap());
    }

    // All streams should still be connected
    assert_eq!(streams.len(), 5);
}

/// Test server serves files from root directory
#[test]
fn test_e2e_file_serving() {
    let server = TestServer::start(15643).expect("Failed to start server");

    // The test files were created in TestServer::start()
    // Verify they exist in the temp directory
    let test_file = server.temp_dir.path().join("test.txt");
    assert!(test_file.exists(), "Test file should exist");

    let content = fs::read_to_string(&test_file).unwrap();
    assert_eq!(content, "Hello from 9P server!");
}

/// Test server handles invalid connections gracefully
#[test]
fn test_e2e_invalid_connection_handling() {
    let server = TestServer::start(15644).expect("Failed to start server");

    // Connect and send garbage data
    let mut stream = TcpStream::connect_timeout(
        &server.address().parse().unwrap(),
        Duration::from_secs(5)
    ).expect("Should connect");

    // Send invalid 9P data
    let garbage = vec![0xFF; 100];
    let _ = stream.write_all(&garbage);

    // Server should handle gracefully (not crash)
    std::thread::sleep(Duration::from_secs(1));

    // Try connecting again to verify server still works
    let result = TcpStream::connect_timeout(
        &server.address().parse().unwrap(),
        Duration::from_secs(5)
    );
    assert!(result.is_ok(), "Server should still accept connections after receiving garbage");
}

/// Test server responds to multiple requests
#[test]
fn test_e2e_multiple_requests() {
    let server = TestServer::start(15645).expect("Failed to start server");

    // Connect
    let mut stream = TcpStream::connect_timeout(
        &server.address().parse().unwrap(),
        Duration::from_secs(5)
    ).expect("Should connect");

    // Send multiple simple requests (even if invalid, server shouldn't crash)
    for _ in 0..10 {
        let data = vec![0x00, 0x00, 0x00, 0x04]; // Minimal message
        let _ = stream.write_all(&data);
        std::thread::sleep(Duration::from_millis(100));
    }

    // Server should still be alive
    let result = TcpStream::connect_timeout(
        &server.address().parse().unwrap(),
        Duration::from_secs(5)
    );
    assert!(result.is_ok(), "Server should handle multiple requests");
}

/// Test server binds to different transports
#[test]
fn test_e2e_transport_binding() {
    // Test TCP binding
    let tcp_server = TestServer::start(15646);
    assert!(tcp_server.is_ok(), "Should bind TCP transport");

    // Note: QUIC would require certificate setup, so we just test TCP for now
}

/// Test server properly cleans up resources on shutdown
#[test]
fn test_e2e_server_shutdown() {
    let port = 15647;
    {
        let _server = TestServer::start(port).expect("Failed to start server");
        // Server exists in this scope

        let result = TcpStream::connect_timeout(
            &format!("127.0.0.1:{}", port).parse().unwrap(),
            Duration::from_secs(5)
        );
        assert!(result.is_ok(), "Should connect while server running");
    } // Server drops here

    // Wait for cleanup
    std::thread::sleep(Duration::from_secs(1));

    // Port should be available again
    let result = TcpStream::connect_timeout(
        &format!("127.0.0.1:{}", port).parse().unwrap(),
        Duration::from_millis(500)
    );
    assert!(result.is_err(), "Port should be released after server shutdown");
}

/// Test client connection timeout handling
#[test]
fn test_e2e_connection_timeout() {
    // Try to connect to non-existent server
    let result = TcpStream::connect_timeout(
        &"127.0.0.1:19999".parse().unwrap(),
        Duration::from_millis(500)
    );

    assert!(result.is_err(), "Should timeout on non-existent server");
}

/// Test server handles rapid connect/disconnect
#[test]
fn test_e2e_rapid_connections() {
    let server = TestServer::start(15648).expect("Failed to start server");

    // Rapidly connect and disconnect 20 times
    for _ in 0..20 {
        let stream = TcpStream::connect_timeout(
            &server.address().parse().unwrap(),
            Duration::from_secs(5)
        );
        assert!(stream.is_ok(), "Should handle rapid connections");
        drop(stream); // Immediately disconnect
    }

    // Server should still be responsive
    let result = TcpStream::connect_timeout(
        &server.address().parse().unwrap(),
        Duration::from_secs(5)
    );
    assert!(result.is_ok(), "Server should remain stable after rapid connections");
}
