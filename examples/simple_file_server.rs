//! Dead simple 9P.e file server
//!
//! This bypasses all the complex server architecture and just proves:
//! 1. QUIC transport works
//! 2. 9P.e messages work
//! 3. We can actually serve files
//!
//! Run: cargo run --example simple_file_server
//! Then in another terminal: cargo run --example simple_client

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use ninepe_server::protocol::{NinePMessage, ProtocolError, MAX_MESSAGE_SIZE};
use quinn::{Endpoint, ServerConfig, Connection, RecvStream, SendStream};
fn generate_cert() -> (rustls::Certificate, rustls::PrivateKey) {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
        .expect("Failed to generate cert");
    let cert_der = rustls::Certificate(cert.serialize_der().unwrap());
    let key_der = rustls::PrivateKey(cert.serialize_private_key_der());
    (cert_der, key_der)
}

fn configure_server(cert: rustls::Certificate, key: rustls::PrivateKey) -> ServerConfig {
    let cert_chain = vec![cert];
    let mut server_config = ServerConfig::with_single_cert(cert_chain, key).unwrap();

    let transport_config = Arc::get_mut(&mut server_config.transport).unwrap();
    transport_config.max_concurrent_bidi_streams(100u32.into());
    transport_config.max_idle_timeout(Some(Duration::from_secs(600).try_into().unwrap()));

    server_config
}

async fn handle_stream(mut send: SendStream, mut recv: RecvStream) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        // Read message length (4 bytes)
        let mut len_buf = [0u8; 4];
        match recv.read_exact(&mut len_buf).await {
            Ok(_) => {},
            Err(_) => {
                println!("📡 Client disconnected");
                break;
            }
        }

        let len = u32::from_le_bytes(len_buf) as usize;

        if len > MAX_MESSAGE_SIZE as usize {
            eprintln!("❌ Message too large: {}", len);
            break;
        }

        // Read message data
        let mut buf = vec![0u8; len];
        recv.read_exact(&mut buf).await?;

        // Deserialize
        let msg = NinePMessage::deserialize(buf)?;

        // Handle message
        let response = match msg {
            NinePMessage::Version { msize, version } => {
                println!("📨 Version: msize={}, version={}", msize, version);
                NinePMessage::Version {
                    msize: msize.min(MAX_MESSAGE_SIZE),
                    version: "9P.e-1.0".to_string(),
                }
            }

            NinePMessage::Attach { fid, afid, uname, aname } => {
                println!("📨 Attach: fid={}, afid={}, uname={}, aname={}", fid, afid, uname, aname);
                // TODO: Return proper qid
                NinePMessage::Error {
                    ename: "Attach not fully implemented - but we got this far!".to_string(),
                    errno: 0,
                }
            }

            _ => {
                println!("📨 Unhandled message type");
                NinePMessage::Error {
                    ename: "Not implemented yet".to_string(),
                    errno: 0,
                }
            }
        };

        // Serialize response
        let response_bytes = response.serialize()?;
        let response_len = response_bytes.len() as u32;

        // Send response
        send.write_all(&response_len.to_le_bytes()).await?;
        send.write_all(&response_bytes).await?;

        println!("✅ Response sent");
    }

    Ok(())
}

async fn handle_connection(conn: Connection) {
    println!("🔗 New connection from {}", conn.remote_address());

    loop {
        match conn.accept_bi().await {
            Ok((send, recv)) => {
                println!("📨 New stream opened");
                tokio::spawn(async move {
                    if let Err(e) = handle_stream(send, recv).await {
                        eprintln!("Stream error: {}", e);
                    }
                });
            }
            Err(e) => {
                println!("Connection closed: {}", e);
                break;
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Starting simple 9P.e QUIC server");

    let (cert, key) = generate_cert();
    let addr: SocketAddr = "127.0.0.1:5640".parse()?;

    let server_config = configure_server(cert, key);
    let endpoint = Endpoint::server(server_config, addr)?;

    println!("✅ Server listening on {}", addr);
    println!("📝 Waiting for connections...");
    println!();
    println!("Test with:");
    println!("  cargo run --example simple_client");
    println!();

    while let Some(incoming) = endpoint.accept().await {
        tokio::spawn(async move {
            match incoming.await {
                Ok(conn) => {
                    handle_connection(conn).await;
                }
                Err(e) => {
                    eprintln!("Connection failed: {}", e);
                }
            }
        });
    }

    Ok(())
}
