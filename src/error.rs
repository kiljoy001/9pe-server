//! Centralized error handling for 9P.e server

use std::fmt;
use std::io;
use std::net::AddrParseError;

/// Main error type for 9P.e server operations
#[derive(Debug)]
pub enum ServerError {
    /// Network-related errors
    Network(NetworkError),
    /// Transport-related errors
    Transport(TransportError),
    /// 9P protocol errors
    Protocol(ProtocolError),
    /// File system errors
    FileSystem(FileSystemError),
    /// Configuration errors
    Config(ConfigError),
    /// General I/O errors
    Io(io::Error),
}

#[derive(Debug)]
pub enum NetworkError {
    BindFailed(String),
    InvalidAddress(String),
    ConnectionFailed(String),
    DnsResolution(String),
}

#[derive(Debug)]
pub enum TransportError {
    QuicSetupFailed(String),
    TcpBindFailed(String),
    TlsError(String),
    ConnectionLost(String),
}

#[derive(Debug)]
pub enum ProtocolError {
    InvalidMessage(String),
    UnsupportedVersion(u32),
    AuthenticationFailed(String),
    PermissionDenied(String),
}

#[derive(Debug)]
pub enum FileSystemError {
    PathNotFound(String),
    AccessDenied(String),
    InvalidPath(String),
    DirectoryNotEmpty(String),
}

#[derive(Debug)]
pub enum ConfigError {
    InvalidValue(String),
    MissingRequired(String),
    ParseError(String),
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServerError::Network(e) => write!(f, "Network error: {}", e),
            ServerError::Transport(e) => write!(f, "Transport error: {}", e),
            ServerError::Protocol(e) => write!(f, "Protocol error: {}", e),
            ServerError::FileSystem(e) => write!(f, "File system error: {}", e),
            ServerError::Config(e) => write!(f, "Configuration error: {}", e),
            ServerError::Io(e) => write!(f, "I/O error: {}", e),
        }
    }
}

impl fmt::Display for NetworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetworkError::BindFailed(msg) => write!(f, "Failed to bind: {}", msg),
            NetworkError::InvalidAddress(addr) => write!(f, "Invalid address: {}", addr),
            NetworkError::ConnectionFailed(msg) => write!(f, "Connection failed: {}", msg),
            NetworkError::DnsResolution(msg) => write!(f, "DNS resolution failed: {}", msg),
        }
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportError::QuicSetupFailed(msg) => write!(f, "QUIC setup failed: {}", msg),
            TransportError::TcpBindFailed(msg) => write!(f, "TCP bind failed: {}", msg),
            TransportError::TlsError(msg) => write!(f, "TLS error: {}", msg),
            TransportError::ConnectionLost(msg) => write!(f, "Connection lost: {}", msg),
        }
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolError::InvalidMessage(msg) => write!(f, "Invalid message: {}", msg),
            ProtocolError::UnsupportedVersion(v) => write!(f, "Unsupported version: {}", v),
            ProtocolError::AuthenticationFailed(msg) => write!(f, "Authentication failed: {}", msg),
            ProtocolError::PermissionDenied(msg) => write!(f, "Permission denied: {}", msg),
        }
    }
}

impl fmt::Display for FileSystemError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileSystemError::PathNotFound(path) => write!(f, "Path not found: {}", path),
            FileSystemError::AccessDenied(path) => write!(f, "Access denied: {}", path),
            FileSystemError::InvalidPath(path) => write!(f, "Invalid path: {}", path),
            FileSystemError::DirectoryNotEmpty(path) => write!(f, "Directory not empty: {}", path),
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::InvalidValue(msg) => write!(f, "Invalid value: {}", msg),
            ConfigError::MissingRequired(field) => write!(f, "Missing required field: {}", field),
            ConfigError::ParseError(msg) => write!(f, "Parse error: {}", msg),
        }
    }
}

impl std::error::Error for ServerError {}
impl std::error::Error for NetworkError {}
impl std::error::Error for TransportError {}
impl std::error::Error for ProtocolError {}
impl std::error::Error for FileSystemError {}
impl std::error::Error for ConfigError {}

// Conversions from standard library errors
impl From<io::Error> for ServerError {
    fn from(err: io::Error) -> Self {
        ServerError::Io(err)
    }
}

impl From<AddrParseError> for ServerError {
    fn from(err: AddrParseError) -> Self {
        ServerError::Network(NetworkError::InvalidAddress(err.to_string()))
    }
}

impl From<NetworkError> for ServerError {
    fn from(err: NetworkError) -> Self {
        ServerError::Network(err)
    }
}

impl From<TransportError> for ServerError {
    fn from(err: TransportError) -> Self {
        ServerError::Transport(err)
    }
}

impl From<ProtocolError> for ServerError {
    fn from(err: ProtocolError) -> Self {
        ServerError::Protocol(err)
    }
}

impl From<FileSystemError> for ServerError {
    fn from(err: FileSystemError) -> Self {
        ServerError::FileSystem(err)
    }
}

impl From<ConfigError> for ServerError {
    fn from(err: ConfigError) -> Self {
        ServerError::Config(err)
    }
}

/// Result type alias for convenience
pub type Result<T> = std::result::Result<T, ServerError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ServerError::FileSystem(FileSystemError::PathNotFound("/test".to_string()));
        assert!(err.to_string().contains("Path not found"));
    }

    #[test]
    fn test_network_error_conversion() {
        let net_err = NetworkError::BindFailed("test".to_string());
        let server_err: ServerError = net_err.into();
        assert!(matches!(server_err, ServerError::Network(_)));
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = io::Error::from(io::ErrorKind::NotFound);
        let server_err: ServerError = io_err.into();
        assert!(matches!(server_err, ServerError::Io(_)));
    }

    #[test]
    fn test_protocol_error() {
        let err = ProtocolError::UnsupportedVersion(42);
        assert_eq!(err.to_string(), "Unsupported version: 42");
    }

    #[test]
    fn test_config_error() {
        let err = ConfigError::MissingRequired("port".to_string());
        assert!(err.to_string().contains("Missing required"));
    }

    /// Fuzz test: Error types should handle any string
    #[test]
    fn fuzz_error_messages() {
        use proptest::prelude::*;

        proptest!(|(msg in ".*")| {
            let _ = NetworkError::BindFailed(msg.clone());
            let _ = FileSystemError::PathNotFound(msg.clone());
            let _ = ConfigError::InvalidValue(msg.clone());
        });
    }
}
