// 9P.e Server - Protocol Parser Fuzzer
// Copyright (C) 2024 9P.e Server Contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

#![no_main]

use libfuzzer_sys::fuzz_target;
use ninepe_server::*;

fuzz_target!(|data: &[u8]| {
    // Fuzz the 9P message parser
    let _ = parse_9p_message(data);

    // Try to parse as different message types
    let _ = parse_version_message(data);
    let _ = parse_auth_message(data);
    let _ = parse_attach_message(data);
    let _ = parse_walk_message(data);
    let _ = parse_open_message(data);
    let _ = parse_read_message(data);
    let _ = parse_write_message(data);
    let _ = parse_clunk_message(data);
    let _ = parse_remove_message(data);
    let _ = parse_stat_message(data);

    // If parsing succeeds, verify round-trip
    if let Ok(msg) = parse_9p_message(data) {
        let serialized = serialize_9p_message(&msg);
        let reparsed = parse_9p_message(&serialized);
        assert!(reparsed.is_ok(), "Round-trip failed");
    }
});

// Stub functions - replace with actual implementation
fn parse_9p_message(_data: &[u8]) -> Result<Message, Error> {
    Err(Error)
}

fn parse_version_message(_data: &[u8]) -> Result<Message, Error> {
    Err(Error)
}

fn parse_auth_message(_data: &[u8]) -> Result<Message, Error> {
    Err(Error)
}

fn parse_attach_message(_data: &[u8]) -> Result<Message, Error> {
    Err(Error)
}

fn parse_walk_message(_data: &[u8]) -> Result<Message, Error> {
    Err(Error)
}

fn parse_open_message(_data: &[u8]) -> Result<Message, Error> {
    Err(Error)
}

fn parse_read_message(_data: &[u8]) -> Result<Message, Error> {
    Err(Error)
}

fn parse_write_message(_data: &[u8]) -> Result<Message, Error> {
    Err(Error)
}

fn parse_clunk_message(_data: &[u8]) -> Result<Message, Error> {
    Err(Error)
}

fn parse_remove_message(_data: &[u8]) -> Result<Message, Error> {
    Err(Error)
}

fn parse_stat_message(_data: &[u8]) -> Result<Message, Error> {
    Err(Error)
}

fn serialize_9p_message(_msg: &Message) -> Vec<u8> {
    vec![]
}

struct Message;
struct Error;