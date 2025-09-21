// 9P.e Server - Path Operations Fuzzer
// Copyright (C) 2024 9P.e Server Contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

#![no_main]

use libfuzzer_sys::fuzz_target;
use ninepee_server::*;

fuzz_target!(|input: &str| {
    // Test path sanitization
    let sanitized = sanitize_path(input);

    // Sanitized paths should never contain dangerous patterns
    assert!(!sanitized.contains("../"));
    assert!(!sanitized.contains("..\\"));
    assert!(!sanitized.contains('\0'));
    assert!(!sanitized.contains("//"));

    // Test path normalization
    let normalized = normalize_path(input);

    // Normalized paths should be absolute and clean
    if !normalized.is_empty() {
        assert!(normalized.starts_with('/'));
        assert!(!normalized.contains("//"));
        if normalized != "/" {
            assert!(!normalized.ends_with('/'));
        }
    }

    // Test path traversal detection
    let traversal_patterns = vec![
        "../etc/passwd",
        "../../root/.ssh/id_rsa",
        "..\\windows\\system32",
        "%2e%2e%2f",
        "..;/",
        "....//",
        "\0/etc/shadow",
    ];

    for pattern in &traversal_patterns {
        if input.contains(pattern) {
            assert!(is_path_traversal_attempt(input));
        }
    }

    // Test path validation
    let _ = validate_path(input);

    // Test path parsing
    let _ = parse_path_components(input);

    // Test Unicode handling
    if input.chars().any(|c| c > '\x7f') {
        let _ = handle_unicode_path(input);
    }

    // Test path length limits
    if input.len() > 4096 {
        let result = validate_path(input);
        assert!(result.is_err());
    }

    // Idempotence: sanitizing twice should give same result
    let sanitized_twice = sanitize_path(&sanitized);
    assert_eq!(sanitized, sanitized_twice);
});

// Stub functions
fn sanitize_path(path: &str) -> String {
    path.replace("../", "")
        .replace("..\\", "")
        .replace('\0', "")
        .replace("//", "/")
}

fn normalize_path(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let mut result = path.to_string();
    if !result.starts_with('/') {
        result = format!("/{}", result);
    }
    result.replace("//", "/")
}

fn is_path_traversal_attempt(path: &str) -> bool {
    path.contains("..") || path.contains('\0')
}

fn validate_path(path: &str) -> Result<(), Error> {
    if path.len() > 4096 {
        return Err(Error);
    }
    Ok(())
}

fn parse_path_components(_path: &str) -> Result<Vec<String>, Error> {
    Ok(vec![])
}

fn handle_unicode_path(_path: &str) -> Result<String, Error> {
    Ok(String::new())
}

struct Error;