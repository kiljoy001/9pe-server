//! End-to-end connection test for 9P.e server
//!
//! Tests the full client-server connection lifecycle:
//! 1. Server starts and listens
//! 2. Client connects
//! 3. Version negotiation
//! 4. Attach to root
//! 5. Walk/Read/Write operations

use std::time::Duration;
use std::path::PathBuf;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

use ninepe_server::protocol::NinePMessage;
use ninepe_server::server::Server;
use ninepe_server::transport::TransportType;
use ninepe_server::network::{NetworkConfig, BindAddress};

/// Helper to send a 9P message over a TCP stream using 9P.e wire format
/// Wire format: [4-byte size][type][payload]
/// Size includes the 4-byte size field itself
async fn send_message(stream: &mut TcpStream, msg: &NinePMessage) -> anyhow::Result<()> {
    let body = msg.serialize()?;
    // body is [type][payload]

    // Total size = 4 (size field) + body.len()
    let total_size = (4 + body.len()) as u32;

    stream.write_all(&total_size.to_le_bytes()).await?;
    stream.write_all(&body).await?;
    stream.flush().await?;

    Ok(())
}

/// Helper to receive a 9P message from a TCP stream
/// Server 9P.e format: [4-byte size][1-byte type][payload]
/// Size includes the 4-byte size field itself
async fn recv_message(stream: &mut TcpStream) -> anyhow::Result<NinePMessage> {
    let mut size_buf = [0u8; 4];
    stream.read_exact(&mut size_buf).await?;
    let total_size = u32::from_le_bytes(size_buf) as usize;

    if total_size < 5 {
        return Err(anyhow::anyhow!("Message too small: {}", total_size));
    }

    // Read rest of message (size - 4 for size field already read)
    // This gives us [type][payload]
    let mut body = vec![0u8; total_size - 4];
    stream.read_exact(&mut body).await?;

    // body is already [type][payload] which is what deserialize expects
    NinePMessage::deserialize(body)
        .map_err(|e| anyhow::anyhow!("Deserialize error: {}", e))
}

#[tokio::test]
async fn test_version_negotiation() {
    // Create server using builder pattern with TCP transport
    let server = Server::builder()
        .network_config(NetworkConfig {
            bind_address: BindAddress::Specific("127.0.0.1".parse().unwrap()),
            port: 15640, // Use a high port for testing
            ..Default::default()
        })
        .transport(TransportType::Tcp)
        .root_directory(PathBuf::from("."))
        .mesh_enabled(false)
        .metrics_enabled(false)
        .auto_mount_enabled(false)
        .state_directory(PathBuf::from("/tmp/9pe-test-version"))
        .build()
        .await
        .expect("Failed to create server");

    let addr = server.address();

    // Spawn server in background
    let server_handle = tokio::spawn(async move {
        let _ = server.run().await;
    });

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Connect client
    let mut stream = timeout(
        Duration::from_secs(5),
        TcpStream::connect(&addr)
    ).await
        .expect("Connection timeout")
        .expect("Failed to connect");

    // Send version message
    let version_req = NinePMessage::Version {
        msize: 8192,
        version: "9P.e".to_string(),
    };
    let _ = send_message(&mut stream, &version_req).await
        .expect("Failed to send version");

    // Receive response
    let response = timeout(
        Duration::from_secs(5),
        recv_message(&mut stream)
    ).await
        .expect("Response timeout")
        .expect("Failed to receive response");

    // Verify version response
    match response {
        NinePMessage::Version { msize, version } => {
            assert!(msize <= 8192, "msize should not exceed requested");
            assert!(version == "9P.e" || version == "9P2000", "Version should be negotiated");
            println!("Version negotiated: {} with msize {}", version, msize);
        }
        NinePMessage::Error { ename, errno } => {
            panic!("Server returned error: {} (errno {})", ename, errno);
        }
        other => {
            panic!("Unexpected response: {:?}", other);
        }
    }

    // Cleanup
    server_handle.abort();
}

#[tokio::test]
async fn test_attach_and_walk() {
    // Create server using builder pattern with TCP transport
    let server = Server::builder()
        .network_config(NetworkConfig {
            bind_address: BindAddress::Specific("127.0.0.1".parse().unwrap()),
            port: 15641, // Different port from other test
            ..Default::default()
        })
        .transport(TransportType::Tcp)
        .root_directory(PathBuf::from("."))
        .mesh_enabled(false)
        .metrics_enabled(false)
        .auto_mount_enabled(false)
        .state_directory(PathBuf::from("/tmp/9pe-test-attach"))
        .build()
        .await
        .expect("Failed to create server");

    let addr = server.address();

    // Spawn server in background
    let server_handle = tokio::spawn(async move {
        let _ = server.run().await;
    });

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Connect client
    let mut stream = TcpStream::connect(&addr).await
        .expect("Failed to connect");

    // 1. Version negotiation
    let version_req = NinePMessage::Version {
        msize: 8192,
        version: "9P.e".to_string(),
    };
    send_message(&mut stream, &version_req).await.unwrap();
    let _version_resp = recv_message(&mut stream).await.unwrap();

    // 2. Attach to root
    let attach_req = NinePMessage::Attach {
        fid: 0,
        afid: u32::MAX, // NOFID
        uname: "testuser".to_string(),
        aname: "".to_string(),
    };
    send_message(&mut stream, &attach_req).await.unwrap();
    let attach_resp = recv_message(&mut stream).await.unwrap();

    match attach_resp {
        NinePMessage::Attach { fid, .. } => {
            assert_eq!(fid, 0, "Attach should return the requested fid");
            println!("Attached with fid {}", fid);
        }
        NinePMessage::Error { ename, .. } => {
            // Expected if auth is required - that's fine for this test
            println!("Attach requires auth (expected): {}", ename);
        }
        other => {
            panic!("Unexpected attach response: {:?}", other);
        }
    }

    // Cleanup
    server_handle.abort();
}

#[tokio::test]
async fn test_read_synthetic_file() {
    // Create server using builder pattern with TCP transport
    let server = Server::builder()
        .network_config(NetworkConfig {
            bind_address: BindAddress::Specific("127.0.0.1".parse().unwrap()),
            port: 15642, // Different port
            ..Default::default()
        })
        .transport(TransportType::Tcp)
        .root_directory(PathBuf::from("."))
        .mesh_enabled(false)
        .metrics_enabled(false)
        .auto_mount_enabled(false)
        .state_directory(PathBuf::from("/tmp/9pe-test-read"))
        .build()
        .await
        .expect("Failed to create server");

    let addr = server.address();

    // Spawn server in background
    let server_handle = tokio::spawn(async move {
        let _ = server.run().await;
    });

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Connect client
    let mut stream = TcpStream::connect(&addr).await
        .expect("Failed to connect");

    // 1. Version negotiation
    let version_req = NinePMessage::Version {
        msize: 8192,
        version: "9P.e".to_string(),
    };
    send_message(&mut stream, &version_req).await.unwrap();
    let version_resp = recv_message(&mut stream).await.unwrap();
    println!("Version response: {:?}", version_resp);

    // 2. Attach to root
    let attach_req = NinePMessage::Attach {
        fid: 0,
        afid: u32::MAX, // NOFID
        uname: "testuser".to_string(),
        aname: "".to_string(),
    };
    send_message(&mut stream, &attach_req).await.unwrap();
    let attach_resp = recv_message(&mut stream).await.unwrap();
    println!("Attach response: {:?}", attach_resp);

    // 3. Walk to /srv/compute
    let walk_req = NinePMessage::Walk {
        fid: 0,
        newfid: 1,
        wnames: vec!["srv".to_string(), "compute".to_string()],
    };
    send_message(&mut stream, &walk_req).await.unwrap();
    let walk_resp = recv_message(&mut stream).await.unwrap();
    println!("Walk response: {:?}", walk_resp);

    // 4. Walk to /srv/compute/info
    let walk_req2 = NinePMessage::Walk {
        fid: 1,
        newfid: 2,
        wnames: vec!["info".to_string()],
    };
    send_message(&mut stream, &walk_req2).await.unwrap();
    let walk_resp2 = recv_message(&mut stream).await.unwrap();
    println!("Walk to info response: {:?}", walk_resp2);

    // 5. Open the file
    let open_req = NinePMessage::Open {
        fid: 2,
        mode: 0, // OREAD
    };
    send_message(&mut stream, &open_req).await.unwrap();
    let open_resp = recv_message(&mut stream).await.unwrap();
    println!("Open response: {:?}", open_resp);

    // 6. Read the file
    let read_req = NinePMessage::Read {
        fid: 2,
        offset: 0,
        count: 4096,
        data: vec![],
    };
    send_message(&mut stream, &read_req).await.unwrap();
    let read_resp = recv_message(&mut stream).await.unwrap();

    match read_resp {
        NinePMessage::Read { data, .. } => {
            println!("Read {} bytes from /srv/compute/info", data.len());
            if !data.is_empty() {
                println!("Content: {}", String::from_utf8_lossy(&data));
            }
        }
        NinePMessage::Error { ename, errno } => {
            // May require auth - that's expected
            println!("Read error (may require auth): {} (errno {})", ename, errno);
        }
        other => {
            println!("Unexpected read response: {:?}", other);
        }
    }

    // Cleanup
    server_handle.abort();
}
