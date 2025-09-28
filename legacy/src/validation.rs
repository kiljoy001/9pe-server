//! Input validation module for security and data integrity
//!
//! Provides comprehensive validation for all user inputs to prevent:
//! - Path traversal attacks
//! - Command injection
//! - Buffer overflows
//! - Invalid characters
//! - Resource exhaustion

use std::path::{Path, PathBuf};
use anyhow::{Result, bail};
use regex::Regex;
use once_cell::sync::Lazy;

/// Maximum allowed path length
const MAX_PATH_LENGTH: usize = 4096;

/// Maximum allowed username length
const MAX_USERNAME_LENGTH: usize = 256;

/// Maximum allowed file name length
const MAX_FILENAME_LENGTH: usize = 255;

/// Maximum allowed password length
const MAX_PASSWORD_LENGTH: usize = 1024;

/// Minimum password length
const MIN_PASSWORD_LENGTH: usize = 8;

/// Maximum command length for execution
const MAX_COMMAND_LENGTH: usize = 8192;

/// Valid username pattern (alphanumeric, underscore, dash, dot)
static USERNAME_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[a-zA-Z0-9_.-]+$").expect("Invalid regex")
});

/// Valid filename pattern (no directory separators or null bytes)
static FILENAME_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[^/\0]+$").expect("Invalid regex")
});

/// Dangerous shell characters that could lead to command injection
static SHELL_CHARS_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[;&|`$()<>\{\}\[\]*?!~]").expect("Invalid regex")
});

/// SQL injection patterns
static SQL_INJECTION_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(union|select|insert|update|delete|drop|create|alter|exec|script|javascript|onclick|onerror)").expect("Invalid regex")
});

/// Path traversal patterns
static PATH_TRAVERSAL_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(\.\./|\.\.\\/|/\.\.)").expect("Invalid regex")
});

/// Input validation errors
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("Path traversal attempt detected")]
    PathTraversal,

    #[error("Invalid username format")]
    InvalidUsername,

    #[error("Invalid password: {0}")]
    InvalidPassword(String),

    #[error("Input too long: max {max} chars, got {actual}")]
    InputTooLong { max: usize, actual: usize },

    #[error("Input too short: min {min} chars, got {actual}")]
    InputTooShort { min: usize, actual: usize },

    #[error("Invalid characters detected")]
    InvalidCharacters,

    #[error("Potential command injection detected")]
    CommandInjection,

    #[error("Potential SQL injection detected")]
    SqlInjection,

    #[error("Invalid file name")]
    InvalidFileName,

    #[error("Null byte detected in input")]
    NullByte,
}

/// Validates a file path to prevent path traversal attacks
pub fn validate_path(path: &str) -> Result<PathBuf> {
    // Check for null bytes
    if path.contains('\0') {
        bail!(ValidationError::NullByte);
    }

    // Check length
    if path.len() > MAX_PATH_LENGTH {
        bail!(ValidationError::InputTooLong {
            max: MAX_PATH_LENGTH,
            actual: path.len()
        });
    }

    // Check for path traversal patterns
    if PATH_TRAVERSAL_REGEX.is_match(path) {
        bail!(ValidationError::PathTraversal);
    }

    // Parse and canonicalize the path
    let path_buf = PathBuf::from(path);

    // Check for absolute paths trying to escape the sandbox
    if path_buf.is_absolute() && !is_safe_absolute_path(&path_buf) {
        bail!(ValidationError::PathTraversal);
    }

    // Additional check: ensure no component is ".."
    for component in path_buf.components() {
        if let std::path::Component::ParentDir = component {
            bail!(ValidationError::PathTraversal);
        }
    }

    Ok(path_buf)
}

/// Check if an absolute path is within allowed directories
fn is_safe_absolute_path(path: &Path) -> bool {
    // Define allowed root directories
    let allowed_roots = [
        Path::new("/tmp"),
        Path::new("/var/9pe"),
        Path::new("/home"),
    ];

    // Check if path starts with any allowed root
    allowed_roots.iter().any(|root| path.starts_with(root))
}

/// Validates a username
pub fn validate_username(username: &str) -> Result<String> {
    // Check for null bytes
    if username.contains('\0') {
        bail!(ValidationError::NullByte);
    }

    // Check length
    if username.is_empty() {
        bail!(ValidationError::InputTooShort {
            min: 1,
            actual: 0
        });
    }

    if username.len() > MAX_USERNAME_LENGTH {
        bail!(ValidationError::InputTooLong {
            max: MAX_USERNAME_LENGTH,
            actual: username.len()
        });
    }

    // Check format
    if !USERNAME_REGEX.is_match(username) {
        bail!(ValidationError::InvalidUsername);
    }

    // Check for SQL injection patterns
    if SQL_INJECTION_REGEX.is_match(username) {
        bail!(ValidationError::SqlInjection);
    }

    Ok(username.to_string())
}

/// Validates a password
pub fn validate_password(password: &str) -> Result<String> {
    // Check for null bytes
    if password.contains('\0') {
        bail!(ValidationError::NullByte);
    }

    // Check length
    if password.len() < MIN_PASSWORD_LENGTH {
        bail!(ValidationError::InvalidPassword(
            format!("Password must be at least {} characters", MIN_PASSWORD_LENGTH)
        ));
    }

    if password.len() > MAX_PASSWORD_LENGTH {
        bail!(ValidationError::InputTooLong {
            max: MAX_PASSWORD_LENGTH,
            actual: password.len()
        });
    }

    // Check complexity (at least one uppercase, one lowercase, one digit)
    let has_upper = password.chars().any(|c| c.is_ascii_uppercase());
    let has_lower = password.chars().any(|c| c.is_ascii_lowercase());
    let has_digit = password.chars().any(|c| c.is_ascii_digit());

    if !has_upper || !has_lower || !has_digit {
        bail!(ValidationError::InvalidPassword(
            "Password must contain uppercase, lowercase, and digits".to_string()
        ));
    }

    Ok(password.to_string())
}

/// Validates a file name (not a path)
pub fn validate_filename(filename: &str) -> Result<String> {
    // Check for null bytes
    if filename.contains('\0') {
        bail!(ValidationError::NullByte);
    }

    // Check length
    if filename.is_empty() {
        bail!(ValidationError::InputTooShort {
            min: 1,
            actual: 0
        });
    }

    if filename.len() > MAX_FILENAME_LENGTH {
        bail!(ValidationError::InputTooLong {
            max: MAX_FILENAME_LENGTH,
            actual: filename.len()
        });
    }

    // Check format (no directory separators)
    if !FILENAME_REGEX.is_match(filename) {
        bail!(ValidationError::InvalidFileName);
    }

    // Check for special names
    let lower = filename.to_lowercase();
    if lower == "." || lower == ".." || lower == "con" || lower == "prn"
        || lower == "aux" || lower == "nul" {
        bail!(ValidationError::InvalidFileName);
    }

    Ok(filename.to_string())
}

/// Validates a command for safe execution
pub fn validate_command(command: &str) -> Result<String> {
    // Check for null bytes
    if command.contains('\0') {
        bail!(ValidationError::NullByte);
    }

    // Check length
    if command.len() > MAX_COMMAND_LENGTH {
        bail!(ValidationError::InputTooLong {
            max: MAX_COMMAND_LENGTH,
            actual: command.len()
        });
    }

    // Check for shell metacharacters that could lead to injection
    if SHELL_CHARS_REGEX.is_match(command) {
        bail!(ValidationError::CommandInjection);
    }

    Ok(command.to_string())
}

/// Validates generic text input
pub fn validate_text_input(input: &str, max_length: usize) -> Result<String> {
    // Check for null bytes
    if input.contains('\0') {
        bail!(ValidationError::NullByte);
    }

    // Check length
    if input.len() > max_length {
        bail!(ValidationError::InputTooLong {
            max: max_length,
            actual: input.len()
        });
    }

    // Check for control characters (except newline and tab)
    for ch in input.chars() {
        if ch.is_control() && ch != '\n' && ch != '\t' && ch != '\r' {
            bail!(ValidationError::InvalidCharacters);
        }
    }

    Ok(input.to_string())
}

/// Sanitizes a string for safe logging (removes sensitive data patterns)
pub fn sanitize_for_logging(input: &str) -> String {
    // Regex for common sensitive patterns
    static SENSITIVE_REGEX: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"(?i)(password|token|key|secret|auth|api_key)=[^\s]+").expect("Invalid regex")
    });

    SENSITIVE_REGEX.replace_all(input, "[REDACTED]").to_string()
}

/// Validates a network address
pub fn validate_network_address(address: &str) -> Result<String> {
    // Check for null bytes
    if address.contains('\0') {
        bail!(ValidationError::NullByte);
    }

    // Try to parse as socket address
    address.parse::<std::net::SocketAddr>()
        .map_err(|_| anyhow::anyhow!("Invalid network address: {}", address))?;

    Ok(address.to_string())
}

/// Validates an integer within range
pub fn validate_integer_range(value: i64, min: i64, max: i64) -> Result<i64> {
    if value < min || value > max {
        bail!("Value {} out of range [{}, {}]", value, min, max);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_validation() {
        // Valid paths
        assert!(validate_path("file.txt").is_ok());
        assert!(validate_path("dir/file.txt").is_ok());
        assert!(validate_path("/tmp/file.txt").is_ok());

        // Invalid paths
        assert!(validate_path("../etc/passwd").is_err());
        assert!(validate_path("file\0.txt").is_err());
        assert!(validate_path("/etc/passwd").is_err());
        assert!(validate_path("../../root").is_err());
    }

    #[test]
    fn test_username_validation() {
        // Valid usernames
        assert!(validate_username("john_doe").is_ok());
        assert!(validate_username("user123").is_ok());
        assert!(validate_username("alice.smith").is_ok());

        // Invalid usernames
        assert!(validate_username("").is_err());
        assert!(validate_username("user\0name").is_err());
        assert!(validate_username("admin'; DROP TABLE users--").is_err());
        assert!(validate_username("../../etc/passwd").is_err());
    }

    #[test]
    fn test_password_validation() {
        // Valid passwords
        assert!(validate_password("SecureP@ss123").is_ok());
        assert!(validate_password("MyPassword1").is_ok());

        // Invalid passwords
        assert!(validate_password("short").is_err());
        assert!(validate_password("alllowercase").is_err());
        assert!(validate_password("ALLUPPERCASE").is_err());
        assert!(validate_password("NoNumbers!").is_err());
    }

    #[test]
    fn test_command_validation() {
        // Valid commands
        assert!(validate_command("ls -la").is_ok());
        assert!(validate_command("echo hello").is_ok());

        // Invalid commands (injection attempts)
        assert!(validate_command("rm -rf /; echo").is_err());
        assert!(validate_command("echo $(whoami)").is_err());
        assert!(validate_command("cat file | grep pass").is_err());
    }
}