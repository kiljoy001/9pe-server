//! Network module - Modern IPv6-first networking with IPv4 compatibility

use anyhow::Result;
use std::net::SocketAddr;

pub mod binding;
pub mod resolver;

pub use binding::BindAddress;
pub use resolver::NetworkResolver;

/// Network configuration for the server
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    /// Bind address (IPv6 dual-stack by default)
    pub bind_address: BindAddress,
    /// Port to listen on
    pub port: u16,
    /// Enable IPv6 dual-stack (default: true)
    pub ipv6_dual_stack: bool,
    /// Prefer IPv6 over IPv4 (default: true)
    pub prefer_ipv6: bool,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            bind_address: BindAddress::default(),
            port: 5640,
            ipv6_dual_stack: true,
            prefer_ipv6: true,
        }
    }
}

impl NetworkConfig {
    /// Create a new network configuration
    pub fn new(port: u16) -> Self {
        Self {
            port,
            ..Default::default()
        }
    }

    /// Set bind address from interface name or IP
    pub fn with_interface(mut self, interface: Option<&str>) -> Result<Self> {
        self.bind_address = BindAddress::from_interface(interface)?;
        Ok(self)
    }

    /// Get the socket address to bind to
    pub fn socket_addr(&self) -> Result<SocketAddr> {
        self.bind_address.to_socket_addr(self.port)
    }

    /// Get a display-friendly address string
    pub fn display_address(&self) -> String {
        match self.bind_address {
            BindAddress::Any => format!("[::]:{} (IPv6 dual-stack)", self.port),
            BindAddress::Ipv6Any => format!("[::]:{}", self.port),
            BindAddress::Ipv4Any => format!("0.0.0.0:{}", self.port),
            BindAddress::Specific(addr) => format!("{}:{}", addr, self.port),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_ipv6() {
        let config = NetworkConfig::default();
        assert!(config.ipv6_dual_stack);
        assert!(config.prefer_ipv6);
        assert_eq!(config.bind_address, BindAddress::Any);
    }

    #[test]
    fn test_display_address() {
        let config = NetworkConfig::default();
        let display = config.display_address();
        assert!(display.contains("IPv6 dual-stack"));
    }
}