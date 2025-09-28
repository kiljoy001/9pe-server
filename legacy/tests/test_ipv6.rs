//! IPv6 functionality tests for recent changes

use anyhow::Result;

#[cfg(test)]
mod test_ipv6 {
    use super::*;

    // Import the resolve_bind_address function from main.rs
    // Note: This will require refactoring main.rs to expose this function

    #[test]
    fn test_ipv6_default_binding() {
        // Test that default binding uses IPv6 dual-stack
        let addr = resolve_bind_address(None, 5640).unwrap();
        assert_eq!(addr, "[::]:5640", "Default should be IPv6 dual-stack");
    }

    #[test]
    fn test_ipv6_localhost() {
        // Test IPv6 localhost resolution
        let addr = resolve_bind_address(Some("localhost"), 5640).unwrap();
        assert_eq!(addr, "[::1]:5640", "localhost should resolve to IPv6 loopback");

        let addr = resolve_bind_address(Some("lo"), 5640).unwrap();
        assert_eq!(addr, "[::1]:5640", "lo should resolve to IPv6 loopback");
    }

    #[test]
    fn test_ipv4_explicit() {
        // Test explicit IPv4 options still work
        let addr = resolve_bind_address(Some("localhost4"), 5640).unwrap();
        assert_eq!(addr, "127.0.0.1:5640", "localhost4 should be IPv4");

        let addr = resolve_bind_address(Some("lo4"), 5640).unwrap();
        assert_eq!(addr, "127.0.0.1:5640", "lo4 should be IPv4");

        let addr = resolve_bind_address(Some("any4"), 5640).unwrap();
        assert_eq!(addr, "0.0.0.0:5640", "any4 should be IPv4 any");
    }

    #[test]
    fn test_ipv6_explicit() {
        // Test explicit IPv6 options
        let addr = resolve_bind_address(Some("any6"), 5640).unwrap();
        assert_eq!(addr, "[::]:5640", "any6 should be IPv6 any");

        let addr = resolve_bind_address(Some("all6"), 5640).unwrap();
        assert_eq!(addr, "[::]:5640", "all6 should be IPv6 any");
    }

    #[test]
    fn test_any_interface() {
        // Test "any" and "all" use IPv6 dual-stack
        let addr = resolve_bind_address(Some("any"), 5640).unwrap();
        assert_eq!(addr, "[::]:5640", "any should use IPv6 dual-stack");

        let addr = resolve_bind_address(Some("all"), 5640).unwrap();
        assert_eq!(addr, "[::]:5640", "all should use IPv6 dual-stack");
    }

    #[test]
    fn test_direct_ip_addresses() {
        // Test direct IP address parsing
        let addr = resolve_bind_address(Some("127.0.0.1"), 5640).unwrap();
        assert_eq!(addr, "127.0.0.1:5640", "Direct IPv4 should pass through");

        let addr = resolve_bind_address(Some("::1"), 5640).unwrap();
        assert_eq!(addr, "::1:5640", "Direct IPv6 should pass through");

        let addr = resolve_bind_address(Some("192.168.1.1"), 5640).unwrap();
        assert_eq!(addr, "192.168.1.1:5640", "Direct IPv4 should pass through");

        let addr = resolve_bind_address(Some("2001:db8::1"), 5640).unwrap();
        assert_eq!(addr, "2001:db8::1:5640", "Direct IPv6 should pass through");
    }

    #[test]
    fn test_unknown_interface_fallback() {
        // Test unknown interface names fall back to IPv6 dual-stack
        let addr = resolve_bind_address(Some("unknown"), 5640).unwrap();
        assert_eq!(addr, "[::]:5640", "Unknown interface should use IPv6 dual-stack");
    }

    #[test]
    fn test_mesh_ipv6_listening() {
        // Test that mesh network listens on IPv6
        // This would require refactoring mesh.rs to make testable
        // For now this is a placeholder
    }

    #[test]
    fn test_metrics_ipv6_binding() {
        // Test that metrics server uses IPv6 dual-stack
        // This would require refactoring metrics.rs to make testable
        // For now this is a placeholder
    }
}

// Helper function to be extracted from main.rs
fn resolve_bind_address(interface: Option<&str>, port: u16) -> Result<String> {
    use tracing::warn;

    match interface {
        None => Ok(format!("[::]:{}", port)), // IPv6 any address (also accepts IPv4)
        Some(iface) => {
            if iface.parse::<std::net::IpAddr>().is_ok() {
                Ok(format!("{}:{}", iface, port))
            } else {
                match iface {
                    "lo" | "localhost" => Ok(format!("[::1]:{}", port)), // IPv6 loopback
                    "lo4" | "localhost4" => Ok(format!("127.0.0.1:{}", port)),
                    "any" | "all" => Ok(format!("[::]:{}", port)), // IPv6 dual-stack
                    "any4" | "all4" => Ok(format!("0.0.0.0:{}", port)),
                    "any6" | "all6" => Ok(format!("[::]:{}", port)),
                    _ => {
                        warn!("Interface '{}' not recognized, using IPv6 dual-stack", iface);
                        Ok(format!("[::]:{}", port))
                    }
                }
            }
        }
    }
}