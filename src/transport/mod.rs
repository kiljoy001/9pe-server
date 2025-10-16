//! Transport layer abstraction - QUIC by default, TCP as legacy

use anyhow::Result;
use async_trait::async_trait;
use std::net::SocketAddr;
use tokio::io::{AsyncRead, AsyncWrite};

pub mod quic;
pub mod tcp;

/// Transport type selection
#[derive(Debug, Clone)]
pub enum TransportType {
    /// Modern QUIC transport (default)
    Quic { server_name: Option<String> },
    /// Legacy TCP transport
    Tcp,
}

impl Default for TransportType {
    fn default() -> Self {
        // QUIC is the modern default!
        Self::Quic { server_name: None }
    }
}

/// Abstraction over different transport protocols
#[async_trait]
pub trait Transport: Send + Sync {
    /// Listen for incoming connections
    async fn listen(&self, addr: SocketAddr) -> Result<Box<dyn ConnectionListener>>;

    /// Connect to a remote server
    async fn connect(&self, addr: SocketAddr) -> Result<Box<dyn Connection>>;

    /// Get transport name for logging
    fn name(&self) -> &str;
}

/// Listener for incoming connections
#[async_trait]
pub trait ConnectionListener: Send + Sync {
    /// Accept a new connection
    async fn accept(&self) -> Result<Box<dyn Connection>>;

    /// Get the local address we're listening on
    fn local_addr(&self) -> Result<SocketAddr>;
}

/// Abstract connection
#[async_trait]
pub trait Connection: AsyncRead + AsyncWrite + Send + Unpin {
    /// Get the remote peer address
    fn peer_addr(&self) -> Result<SocketAddr>;

    /// Check if connection uses encryption
    fn is_encrypted(&self) -> bool;

    /// Get connection protocol name
    fn protocol(&self) -> &str;
}

/// Factory for creating transports
pub struct TransportFactory;

impl TransportFactory {
    /// Create a transport based on type
    pub fn create(transport_type: TransportType) -> Result<Box<dyn Transport>> {
        match transport_type {
            TransportType::Quic { server_name } => {
                Ok(Box::new(quic::QuicTransport::new(server_name)?))
            }
            TransportType::Tcp => Ok(Box::new(tcp::TcpTransport::new())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_quic() {
        let default = TransportType::default();
        assert!(matches!(default, TransportType::Quic { .. }));
    }

    #[test]
    fn test_transport_factory() {
        let quic = TransportFactory::create(TransportType::Quic { server_name: None });
        assert!(quic.is_ok());

        let tcp = TransportFactory::create(TransportType::Tcp);
        assert!(tcp.is_ok());
    }
}
