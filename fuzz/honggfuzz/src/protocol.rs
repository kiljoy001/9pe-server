// 9P.e Server - Honggfuzz Protocol Fuzzer
// Copyright (C) 2024 9P.e Server Contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

use honggfuzz::fuzz;
use ninepe_server::*;

fn main() {
    loop {
        fuzz!(|data: &[u8]| {
            // Main protocol parsing
            let _ = parse_9p_message(data);

            // Test message boundaries
            if data.len() >= 4 {
                let size = u32::from_le_bytes([
                    data[0],
                    data[1],
                    data[2],
                    data[3]
                ]);

                // Check size consistency
                if size as usize != data.len() && size < 10_000_000 {
                    let _ = handle_size_mismatch(data, size);
                }
            }

            // Test concurrent handling
            let _ = handle_concurrent_message(data);

            // Test state machine transitions
            if data.len() > 5 {
                let msg_type = data[4];
                let _ = validate_state_transition(msg_type);
            }

            // Test error recovery
            if data.len() > 1000 {
                let _ = test_error_recovery(data);
            }
        });
    }
}

// Stub functions
fn parse_9p_message(_data: &[u8]) -> Result<(), ()> { Err(()) }
fn handle_size_mismatch(_data: &[u8], _size: u32) -> Result<(), ()> { Err(()) }
fn handle_concurrent_message(_data: &[u8]) -> Result<(), ()> { Err(()) }
fn validate_state_transition(_msg_type: u8) -> Result<(), ()> { Err(()) }
fn test_error_recovery(_data: &[u8]) -> Result<(), ()> { Err(()) }