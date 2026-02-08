//! Minimal 9P.e server example
//!
//! This is THE simplest possible 9P.e server that actually serves files.
//! Start here. Get this working. Then add features.

use ninepe_server::protocol::{NinePMessage, MAX_MESSAGE_SIZE};
use ninepe_server::transport::QuicServer;
use std::net::SocketAddr;

fn generate_cert() -> (rustls::Certificate, rustls::PrivateKey) {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
        .expect("Failed to generate cert");
    let cert_der = rustls::Certificate(cert.serialize_der().unwrap());
    let key_der = rustls::PrivateKey(cert.serialize_private_key_der());
    (cert_der, key_der)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Starting minimal 9P.e server on localhost:5640");

    let (cert, key) = generate_cert();
    let addr: SocketAddr = "127.0.0.1:5640".parse()?;

    let server = QuicServer::new(addr, cert, key)?;

    println!("✅ Server listening on {}", addr);
    println!("📝 Waiting for connections...");

    // TODO: This is what needs to be implemented
    // The server.run() method should:
    //
    // 1. Accept incoming QUIC connections
    // 2. For each connection, accept bidirectional streams
    // 3. For each stream, read 9P.e messages
    // 4. Process the message (Version, Attach, Walk, Read, Write, etc.)
    // 5. Send back the response
    // 6. Loop
    //
    // Example pseudo-code:
    //
    // loop {
    //     let incoming = endpoint.accept().await?;
    //     tokio::spawn(async move {
    //         let connection = incoming.await?;
    //         while let Ok((send, recv)) = connection.accept_bi().await {
    //             let mut session = Session { send, recv, ... };
    //             loop {
    //                 let msg = session.read_message().await?;
    //                 let response = handle_message(msg).await?;
    //                 session.write_message(&response).await?;
    //             }
    //         }
    //     });
    // }

    server.run().await?;

    Ok(())
}

// This is what handle_message() should look like:
#[allow(dead_code)]
async fn handle_message(msg: NinePMessage) -> Result<NinePMessage, Box<dyn std::error::Error>> {
    match msg {
        NinePMessage::Version { msize, version: _ } => {
            println!("📨 Version request, msize={}", msize);
            Ok(NinePMessage::Version {
                msize: msize.min(MAX_MESSAGE_SIZE),
                version: "9P.e-1.0".to_string(),
            })
        }

        NinePMessage::Attach { fid, afid, uname, aname } => {
            println!("📨 Attach request: fid={}, uname={}, aname={}", fid, uname, aname);
            // TODO: Validate authentication
            // TODO: Return root directory qid
            Ok(NinePMessage::Error {
                ename: "Not implemented yet".to_string(),
                errno: 38,
            })
        }

        NinePMessage::Walk { fid, newfid, wnames } => {
            println!("📨 Walk request: fid={}, newfid={}, path={:?}", fid, newfid, wnames);
            // TODO: Walk filesystem path
            // TODO: Return qids for each component
            Ok(NinePMessage::Error {
                ename: "Not implemented yet".to_string(),
                errno: 38,
            })
        }

        NinePMessage::Read { fid, offset, count, data: _ } => {
            println!("📨 Read request: fid={}, offset={}, count={}", fid, offset, count);
            // TODO: Read file data
            // TODO: Return file contents
            Ok(NinePMessage::Error {
                ename: "Not implemented yet".to_string(),
                errno: 38,
            })
        }

        NinePMessage::Write { fid, offset, data } => {
            println!("📨 Write request: fid={}, offset={}, len={}", fid, offset, data.len());
            // TODO: Write file data
            // TODO: Return bytes written
            Ok(NinePMessage::Error {
                ename: "Not implemented yet".to_string(),
                errno: 38,
            })
        }

        _ => {
            println!("📨 Unhandled message type");
            Ok(NinePMessage::Error {
                ename: "Message type not implemented".to_string(),
                errno: 38,
            })
        }
    }
}
