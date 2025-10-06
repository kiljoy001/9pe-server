//! Network binding addresses with IPv6-first support

use anyhow::Result;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use tracing::warn;

/// Represents different binding address types
#[derive(Debug, Clone, PartialEq)]
pub enum BindAddress {
    /// IPv6 dual-stack (accepts both IPv6 and IPv4)
    Any,
    /// IPv6 only
    Ipv6Any,
    /// IPv4 only
    Ipv4Any,
    /// Specific IP address
    Specific(IpAddr),
}

impl Default for BindAddress {
    fn default() -> Self {
        // Default to IPv6 dual-stack - modern by default!
        Self::Any
    }
}

impl BindAddress {
    /// Parse from interface name or IP address string
    pub fn from_interface(interface: Option<&str>) -> Result<Self> {
        match interface {
            None => Ok(Self::Any),
            Some(iface) => {
                // Try parsing as IP address first
                if let Ok(addr) = iface.parse::<IpAddr>() {
                    return Ok(Self::Specific(addr));
                }

                // Handle named interfaces
                match iface {
                    "lo" | "localhost" => Ok(Self::Specific(IpAddr::V6(Ipv6Addr::LOCALHOST))),
                    "lo4" | "localhost4" => Ok(Self::Specific(IpAddr::V4(Ipv4Addr::LOCALHOST))),
                    "any" | "all" => Ok(Self::Any),
                    "any4" | "all4" => Ok(Self::Ipv4Any),
                    "any6" | "all6" => Ok(Self::Ipv6Any),
                    _ => {
                        warn!("Unknown interface '{}', using IPv6 dual-stack", iface);
                        Ok(Self::Any)
                    }
                }
            }
        }
    }

    /// Convert to socket address with port
    pub fn to_socket_addr(&self, port: u16) -> Result<SocketAddr> {
        let addr = match self {
            Self::Any | Self::Ipv6Any => {
                SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port)
            }
            Self::Ipv4Any => {
                SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port)
            }
            Self::Specific(ip) => SocketAddr::new(*ip, port),
        };
        Ok(addr)
    }

    /// Get bind string for display
    pub fn to_bind_string(&self, port: u16) -> String {
        match self {
            Self::Any => format!("[::]:{}", port),
            Self::Ipv6Any => format!("[::]:{}", port),
            Self::Ipv4Any => format!("0.0.0.0:{}", port),
            Self::Specific(IpAddr::V4(v4)) => format!("{}:{}", v4, port),
            Self::Specific(IpAddr::V6(v6)) => format!("[{}]:{}", v6, port),
        }
    }

    /// Check if this is an IPv6 address (including dual-stack)
    pub fn is_ipv6(&self) -> bool {
        matches!(self, Self::Any | Self::Ipv6Any | Self::Specific(IpAddr::V6(_)))
    }

    /// Check if this supports dual-stack
    pub fn is_dual_stack(&self) -> bool {
        matches!(self, Self::Any)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_dual_stack() {
        let addr = BindAddress::default();
        assert!(addr.is_dual_stack());
        assert!(addr.is_ipv6());
    }

    #[test]
    fn test_interface_parsing() {
        assert_eq!(BindAddress::from_interface(None).unwrap(), BindAddress::Any);
        assert_eq!(
            BindAddress::from_interface(Some("localhost")).unwrap(),
            BindAddress::Specific(IpAddr::V6(Ipv6Addr::LOCALHOST))
        );
        assert_eq!(
            BindAddress::from_interface(Some("localhost4")).unwrap(),
            BindAddress::Specific(IpAddr::V4(Ipv4Addr::LOCALHOST))
        );
    }

    #[test]
    fn test_bind_string() {
        assert_eq!(BindAddress::Any.to_bind_string(5640), "[::]:5640");
        assert_eq!(BindAddress::Ipv4Any.to_bind_string(5640), "0.0.0.0:5640");
        assert_eq!(
            BindAddress::Specific(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)))
                .to_bind_string(5640),
            "127.0.0.1:5640"
        );
    }

    #[test]
    fn test_ip_parsing() {
        let addr = BindAddress::from_interface(Some("::1")).unwrap();
        assert!(matches!(addr, BindAddress::Specific(IpAddr::V6(_))));

        let addr = BindAddress::from_interface(Some("127.0.0.1")).unwrap();
        assert!(matches!(addr, BindAddress::Specific(IpAddr::V4(_))));
    }
}