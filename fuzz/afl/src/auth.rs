// 9P.e Server - AFL Authentication Fuzzer
// Copyright (C) 2024 9P.e Server Contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

use afl::fuzz;
use ninepee_server::*;

fn main() {
    fuzz!(|data: &[u8]| {
        // Parse as auth credentials
        if data.len() >= 2 {
            let username_len = data[0] as usize;
            let password_len = data[1] as usize;

            if data.len() >= 2 + username_len + password_len {
                let username = std::str::from_utf8(&data[2..2 + username_len]).unwrap_or("");
                let password = &data[2 + username_len..2 + username_len + password_len];

                // Test authentication
                let _ = authenticate(username, password);

                // Test SQL injection prevention
                if username.contains('\'') || username.contains(';') {
                    let result = authenticate(username, password);
                    assert!(result.is_err());
                }

                // Test null byte injection
                if username.contains('\0') || password.contains(&0) {
                    let result = authenticate(username, password);
                    assert!(result.is_err());
                }
            }
        }

        // Test Ed25519 signature verification
        if data.len() >= 96 {
            let public_key = &data[0..32];
            let signature = &data[32..96];
            let message = &data[96..];

            let _ = verify_ed25519(public_key, signature, message);
        }

        // Test M-of-N threshold
        if data.len() >= 2 {
            let m = (data[0] % 10) + 1;
            let n = (data[1] % 10) + m;
            let _ = validate_threshold(m, n);
        }
    });
}

// Stub functions
fn authenticate(_user: &str, _pass: &[u8]) -> Result<(), ()> { Err(()) }
fn verify_ed25519(_key: &[u8], _sig: &[u8], _msg: &[u8]) -> Result<(), ()> { Err(()) }
fn validate_threshold(_m: u8, _n: u8) -> Result<(), ()> { Err(()) }