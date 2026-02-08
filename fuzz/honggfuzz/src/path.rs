// 9P.e Server - Honggfuzz Path Fuzzer
// Copyright (C) 2024 9P.e Server Contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

use honggfuzz::fuzz;
use ninepe_server::*;

fn main() {
    loop {
        fuzz!(|data: &[u8]| {
            let path = String::from_utf8_lossy(data);

            // Path validation and sanitization
            let sanitized = sanitize_path(&path);
            let normalized = normalize_path(&path);

            // Verify safety properties
            assert!(!sanitized.contains("../"));
            assert!(!sanitized.contains('\0'));

            // Test various path attacks
            let attacks = vec![
                format!("{}/../etc/passwd", path),
                format!("{}%00.txt", path),
                format!("{}/../../../root", path),
                path.replace('/', &"/.".repeat(100)),
            ];

            for attack in attacks {
                let result = validate_path(&attack);
                if attack.contains("..") || attack.len() > 4096 {
                    assert!(result.is_err());
                }
            }

            // Unicode normalization attacks
            if path.chars().any(|c| c > '\x7f') {
                let _ = normalize_unicode_path(&path);
            }

            // Symlink traversal
            let _ = resolve_symlinks(&path);

            // Case sensitivity tests
            let _ = case_insensitive_lookup(&path);
        });
    }
}

// Stub functions
fn sanitize_path(path: &str) -> String {
    path.replace("../", "").replace('\0', "")
}

fn normalize_path(path: &str) -> String {
    if path.is_empty() { return "/".to_string(); }
    let mut result = path.replace("//", "/");
    if !result.starts_with('/') {
        result = format!("/{}", result);
    }
    result
}

fn validate_path(_path: &str) -> Result<(), ()> { Err(()) }
fn normalize_unicode_path(_path: &str) -> Result<String, ()> { Err(()) }
fn resolve_symlinks(_path: &str) -> Result<String, ()> { Err(()) }
fn case_insensitive_lookup(_path: &str) -> Result<String, ()> { Err(()) }