//! IPv6 functionality tests

use anyhow::Result;
use ninep_server::network::BindAddress;

#[cfg(test)]
mod test_ipv6 {
    use super::*;

    // Wrapper to match expected signature from legacy tests
    fn resolve_bind_address(interface: Option<&str>, port: u16) -> Result<String> {
        BindAddress::resolve(interface, port)
    }

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
        assert_eq!(addr, "[::1]:5640", "Direct IPv6 should pass through"); // Note: BindAddress normalizes to brackets for IPv6

        let addr = resolve_bind_address(Some("192.168.1.1"), 5640).unwrap();
        assert_eq!(addr, "192.168.1.1:5640", "Direct IPv4 should pass through");

        let addr = resolve_bind_address(Some("2001:db8::1"), 5640).unwrap();
        assert_eq!(addr, "[2001:db8::1]:5640", "Direct IPv6 should pass through");
    }

    #[test]
    fn test_unknown_interface_fallback() {
        // Test unknown interface names fall back to IPv6 dual-stack
        let addr = resolve_bind_address(Some("unknown"), 5640).unwrap();
        assert_eq!(addr, "[::]:5640", "Unknown interface should use IPv6 dual-stack");
    }
}
