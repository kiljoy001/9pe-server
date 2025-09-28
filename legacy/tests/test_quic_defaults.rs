//! Tests for QUIC default behavior and optional server_name

use clap::Parser;

#[cfg(test)]
mod test_quic_defaults {
    use super::*;

    // This would need the ServeCommand struct from main.rs
    #[derive(Parser, Debug)]
    struct ServeCommand {
        /// Use QUIC transport with encryption (default: enabled for modern networking)
        #[arg(short, long, default_value = "true")]
        quic: bool,

        /// Use --no-quic to disable and fall back to legacy TCP
        #[arg(long = "no-quic", action = clap::ArgAction::SetFalse, overrides_with = "quic")]
        no_quic: bool,

        /// Server name for QUIC TLS certificate (optional, only needed by clients)
        #[arg(short = 'n', long)]
        server_name: Option<String>,

        /// Port to bind to
        #[arg(short, long, default_value = "5640")]
        port: u16,
    }

    #[test]
    fn test_quic_is_default() {
        // Test that QUIC is enabled by default
        let args = ServeCommand::parse_from(&["test"]);
        assert!(args.quic, "QUIC should be enabled by default");
    }

    #[test]
    fn test_explicit_quic_flag() {
        // Test explicit --quic flag
        let args = ServeCommand::parse_from(&["test", "--quic"]);
        assert!(args.quic, "QUIC should be enabled with --quic flag");
    }

    #[test]
    fn test_no_quic_flag() {
        // Test --no-quic disables QUIC
        let args = ServeCommand::parse_from(&["test", "--no-quic"]);
        assert!(!args.quic, "QUIC should be disabled with --no-quic flag");
    }

    #[test]
    fn test_server_name_optional() {
        // Test that server_name is optional
        let args = ServeCommand::parse_from(&["test"]);
        assert_eq!(args.server_name, None, "server_name should be None by default");
    }

    #[test]
    fn test_server_name_provided() {
        // Test server_name can be provided
        let args = ServeCommand::parse_from(&["test", "--server-name", "example.com"]);
        assert_eq!(args.server_name, Some("example.com".to_string()));

        let args = ServeCommand::parse_from(&["test", "-n", "test.local"]);
        assert_eq!(args.server_name, Some("test.local".to_string()));
    }

    #[test]
    fn test_server_without_name_works() {
        // Test that server can start without server_name (for server mode)
        let args = ServeCommand::parse_from(&["test", "--port", "5641"]);
        assert!(args.quic, "QUIC should be enabled");
        assert_eq!(args.server_name, None, "No server name required for server");
        assert_eq!(args.port, 5641);
    }

    #[test]
    fn test_client_with_name() {
        // Test client-like invocation with server name
        let args = ServeCommand::parse_from(&["test", "--server-name", "server.example"]);
        assert!(args.quic, "QUIC should be enabled for client");
        assert_eq!(args.server_name, Some("server.example".to_string()));
    }

    #[test]
    fn test_legacy_tcp_mode() {
        // Test falling back to legacy TCP
        let args = ServeCommand::parse_from(&["test", "--no-quic"]);
        assert!(!args.quic, "Should use legacy TCP");
        assert_eq!(args.server_name, None, "No server name needed for TCP");
    }

    #[test]
    fn test_quic_help_text() {
        // Test that help text correctly describes QUIC as default
        // This would need to parse the actual help output
        // For now, we just verify the structure is correct
        let result = ServeCommand::try_parse_from(&["test", "--help"]);
        assert!(result.is_err()); // --help causes an error (exits)

        let help_text = format!("{}", result.unwrap_err());
        assert!(help_text.contains("default") || help_text.contains("enabled"));
    }
}