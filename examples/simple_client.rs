//! Simple 9P.e QUIC client
//!
//! Tests connection to simple_file_server
//!
//! Run: cargo run --example simple_client

use ninepe_server::protocol::NinePMessage;
use quinn::{Endpoint, ClientConfig};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use ninepe_server::transport::configure_client_insecure;

fn configure_client() -> ClientConfig {
    configure_client_insecure().expect("Failed to configure insecure client")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔌 Connecting to 9P.e server...");

    let addr: SocketAddr = "127.0.0.1:5640".parse()?;
    let client_config = configure_client();

    let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
    endpoint.set_default_client_config(client_config);

    println!("📡 Connecting to {}...", addr);
    let connection = endpoint.connect(addr, "localhost")?.await?;
    println!("✅ Connected via QUIC!");

    // Open bidirectional stream
    let (mut send, mut recv) = connection.open_bi().await?;
    println!("📨 Opened stream");

    // Send Version message
    let version_msg = NinePMessage::Version {
        msize: 8192,
        version: "9P.e-1.0".to_string(),
    };

    println!("📤 Sending Version message...");
    let msg_bytes = version_msg.serialize()?;
    let msg_len = msg_bytes.len() as u32;

    send.write_all(&msg_len.to_le_bytes()).await?;
    send.write_all(&msg_bytes).await?;

    // Read response
    println!("📥 Waiting for response...");
    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;

    let mut response_buf = vec![0u8; len];
    recv.read_exact(&mut response_buf).await?;

    let response = NinePMessage::deserialize(response_buf)?;

    match response {
        NinePMessage::Version { msize, version } => {
            println!("✅ Got Version response!");
            println!("   msize: {}", msize);
            println!("   version: {}", version);
        }
        NinePMessage::Error { ename, .. } => {
            println!("⚠️  Got error: {}", ename);
        }
        _ => {
            println!("❓ Unexpected response type");
        }
    }

    // Try Attach
    let attach_msg = NinePMessage::Attach {
        fid: 0,
        afid: u32::MAX, // NOFID
        uname: "test".to_string(),
        aname: "".to_string(),
    };

    println!();
    println!("📤 Sending Attach message...");
    let msg_bytes = attach_msg.serialize()?;
    let msg_len = msg_bytes.len() as u32;

    send.write_all(&msg_len.to_le_bytes()).await?;
    send.write_all(&msg_bytes).await?;

    // Read response
    println!("📥 Waiting for response...");
    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;

    let mut response_buf = vec![0u8; len];
    recv.read_exact(&mut response_buf).await?;

    let response = NinePMessage::deserialize(response_buf)?;

    match response {
        NinePMessage::Attach { .. } => {
            println!("✅ Got Attach response - server works!");
        }
        NinePMessage::Error { ename, .. } => {
            println!("⚠️  Got error (expected): {}", ename);
            println!();
            println!("🎉 SUCCESS! We proved:");
            println!("   ✅ QUIC connection works");
            println!("   ✅ TLS encryption works");
            println!("   ✅ 9P.e messages serialize/deserialize");
            println!("   ✅ Server receives and responds");
            println!();
            println!("Next step: Implement actual file operations!");
        }
        _ => {
            println!("❓ Unexpected response type");
        }
    }

    Ok(())
}
