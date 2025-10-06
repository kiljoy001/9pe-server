//! TCP transport implementation - legacy fallback

use anyhow::Result;
use async_trait::async_trait;
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tracing::{info, debug};

use super::{Transport, Connection, ConnectionListener};

/// TCP transport implementation
pub struct TcpTransport;

impl Default for TcpTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl TcpTransport {
    pub fn new() -> Self {
        info!("Initializing legacy TCP transport");
        Self
    }
}

#[async_trait]
impl Transport for TcpTransport {

    async fn listen(&self, addr: SocketAddr) -> Result<Box<dyn ConnectionListener>> {
        info!("TCP listening on {} (legacy mode)", addr);
        let listener = TcpListener::bind(addr).await?;
        Ok(Box::new(TcpListenerWrapper { listener }))
    }

    async fn connect(&self, addr: SocketAddr) -> Result<Box<dyn Connection>> {
        debug!("Connecting to {} via TCP", addr);
        let stream = TcpStream::connect(addr).await?;
        Ok(Box::new(TcpConnection { stream }))
    }

    fn name(&self) -> &str {
        "TCP"
    }
}

/// TCP listener wrapper
struct TcpListenerWrapper {
    listener: TcpListener,
}

#[async_trait]
impl ConnectionListener for TcpListenerWrapper {
    async fn accept(&self) -> Result<Box<dyn Connection>> {
        let (stream, _addr) = self.listener.accept().await?;
        Ok(Box::new(TcpConnection { stream }))
    }

    fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.listener.local_addr()?)
    }
}

/// TCP connection wrapper
pub struct TcpConnection {
    stream: TcpStream,
}

#[async_trait]
impl Connection for TcpConnection {
    fn peer_addr(&self) -> Result<SocketAddr> {
        Ok(self.stream.peer_addr()?)
    }

    fn is_encrypted(&self) -> bool {
        false // TCP is not encrypted by default
    }

    fn protocol(&self) -> &str {
        "TCP"
    }
}

// Forward AsyncRead/AsyncWrite to the underlying TcpStream
impl tokio::io::AsyncRead for TcpConnection {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for TcpConnection {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}