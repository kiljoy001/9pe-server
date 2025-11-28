//! Tests for QUIC default behavior and optional server_name

use clap::Parser;
use ninep_server::cli::ServeCommand;

#[cfg(test)]
mod test_quic_defaults {
    use super::*;

    // Wrapper to parse ServeCommand. Since ServeCommand is a subcommand args struct,
    // we need to wrap it in a struct that derives Parser or use `try_parse_from` appropriately.
    // However, `ServeCommand` derives `Args`, not `Parser`.
    // We can define a wrapper struct here to test it.

    #[derive(Parser, Debug)]
    struct CliWrapper {
        #[command(flatten)]
        cmd: ServeCommand,
    }

    #[test]
    fn test_quic_is_default() {
        // Test that QUIC is enabled by default
        let args = CliWrapper::parse_from(&["test"]);
        assert!(args.cmd.quic, "QUIC should be enabled by default");
        assert!(!args.cmd.no_quic, "No-QUIC should be false by default");
    }

    #[test]
    fn test_explicit_quic_flag() {
        // Test explicit --quic flag
        let args = CliWrapper::parse_from(&["test", "--quic"]);
        assert!(args.cmd.quic, "QUIC should be enabled with --quic flag");
    }

    #[test]
    fn test_no_quic_flag() {
        // Test --no-quic disables QUIC.
        // Note: In clap, if `no-quic` is present and conflicts with `quic`, one takes precedence or sets the other.
        // The implementation in `ServeCommand` has:
        // #[arg(long, default_value = "true")] pub quic: bool,
        // #[arg(long = "no-quic", conflicts_with = "quic")] pub no_quic: bool,

        // If we pass `--no-quic`, `no_quic` should be true. `quic` might still be true because of default value,
        // but `no_quic` flag is what the logic checks:
        // `if self.no_quic { ... TCP } else { ... QUIC }`

        let args = CliWrapper::parse_from(&["test", "--no-quic"]);
        assert!(args.cmd.no_quic, "no-quic should be true with --no-quic flag");

        // The logic uses `no_quic` to decide, so checking `no_quic` is enough.
    }

    #[test]
    fn test_server_name_optional() {
        // Test that server_name is optional
        let args = CliWrapper::parse_from(&["test"]);
        assert_eq!(args.cmd.server_name, None, "server_name should be None by default");
    }

    #[test]
    fn test_server_name_provided() {
        // Test server_name can be provided
        let args = CliWrapper::parse_from(&["test", "--server-name", "example.com"]);
        assert_eq!(args.cmd.server_name, Some("example.com".to_string()));

        let args = CliWrapper::parse_from(&["test", "-n", "test.local"]);
        assert_eq!(args.cmd.server_name, Some("test.local".to_string()));
    }

    #[test]
    fn test_server_without_name_works() {
        // Test that server can start without server_name (for server mode)
        let args = CliWrapper::parse_from(&["test", "--port", "5641"]);
        // quic is default true
        assert!(args.cmd.quic, "QUIC should be enabled");
        assert_eq!(args.cmd.server_name, None, "No server name required for server");
        assert_eq!(args.cmd.port, 5641);
    }

    #[test]
    fn test_client_with_name() {
        // Test client-like invocation with server name
        let args = CliWrapper::parse_from(&["test", "--server-name", "server.example"]);
        assert!(args.cmd.quic, "QUIC should be enabled for client");
        assert_eq!(args.cmd.server_name, Some("server.example".to_string()));
    }

    #[test]
    fn test_legacy_tcp_mode() {
        // Test falling back to legacy TCP
        let args = CliWrapper::parse_from(&["test", "--no-quic"]);
        assert!(args.cmd.no_quic, "Should have no_quic flag set");
        assert_eq!(args.cmd.server_name, None, "No server name needed for TCP");
    }
}
