//! Network address resolution with IPv6 preference

use anyhow::{Result, Context};
use std::net::{SocketAddr, ToSocketAddrs};
use tracing::debug;

/// Network resolver that prefers IPv6
pub struct NetworkResolver {
    prefer_ipv6: bool,
}

impl Default for NetworkResolver {
    fn default() -> Self {
        Self { prefer_ipv6: true }
    }
}

impl NetworkResolver {
    /// Create a new resolver
    pub fn new(prefer_ipv6: bool) -> Self {
        Self { prefer_ipv6 }
    }

    /// Resolve a hostname to socket addresses
    pub fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>> {
        let addr_str = format!("{}:{}", host, port);
        let addrs: Vec<SocketAddr> = addr_str
            .to_socket_addrs()
            .context("Failed to resolve address")?
            .collect();

        if addrs.is_empty() {
            anyhow::bail!("No addresses resolved for {}", host);
        }

        // Sort addresses based on preference
        let sorted = if self.prefer_ipv6 {
            self.sort_ipv6_first(addrs)
        } else {
            self.sort_ipv4_first(addrs)
        };

        debug!(
            "Resolved {} to {} addresses (preferred: {})",
            host,
            sorted.len(),
            sorted.first().unwrap()
        );

        Ok(sorted)
    }

    /// Sort addresses with IPv6 first
    fn sort_ipv6_first(&self, mut addrs: Vec<SocketAddr>) -> Vec<SocketAddr> {
        addrs.sort_by_key(|a| match a {
            SocketAddr::V6(_) => 0,
            SocketAddr::V4(_) => 1,
        });
        addrs
    }

    /// Sort addresses with IPv4 first
    fn sort_ipv4_first(&self, mut addrs: Vec<SocketAddr>) -> Vec<SocketAddr> {
        addrs.sort_by_key(|a| match a {
            SocketAddr::V4(_) => 0,
            SocketAddr::V6(_) => 1,
        });
        addrs
    }

    /// Resolve and return the preferred address
    pub fn resolve_preferred(&self, host: &str, port: u16) -> Result<SocketAddr> {
        let addrs = self.resolve(host, port)?;
        addrs
            .into_iter()
            .next()
            .context("No addresses available")
    }

    /// Check if an address is IPv6
    pub fn is_ipv6(addr: &SocketAddr) -> bool {
        matches!(addr, SocketAddr::V6(_))
    }

    /// Check if an address is IPv4
    pub fn is_ipv4(addr: &SocketAddr) -> bool {
        matches!(addr, SocketAddr::V4(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn test_ipv6_preference() {
        let resolver = NetworkResolver::new(true);
        let v4 = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 5640);
        let v6 = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 5640);

        let sorted = resolver.sort_ipv6_first(vec![v4, v6]);
        assert!(NetworkResolver::is_ipv6(&sorted[0]));
    }

    #[test]
    fn test_ipv4_preference() {
        let resolver = NetworkResolver::new(false);
        let v4 = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 5640);
        let v6 = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 5640);

        let sorted = resolver.sort_ipv4_first(vec![v6, v4]);
        assert!(NetworkResolver::is_ipv4(&sorted[0]));
    }
}
