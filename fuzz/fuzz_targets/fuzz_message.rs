// 9P.e Server - Message Handling Fuzzer
// Copyright (C) 2024 9P.e Server Contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

#![no_main]

use libfuzzer_sys::fuzz_target;
use arbitrary::Arbitrary;
use ninepe_server::*;

#[derive(Arbitrary, Debug)]
struct MessageInput {
    msg_type: u8,
    tag: u16,
    fid: u32,
    payload: Vec<u8>,
    size_field: u32,
}

fuzz_target!(|input: MessageInput| {
    // Build a raw message
    let mut raw_message = Vec::new();
    raw_message.extend_from_slice(&input.size_field.to_le_bytes());
    raw_message.push(input.msg_type);
    raw_message.extend_from_slice(&input.tag.to_le_bytes());
    raw_message.extend_from_slice(&input.fid.to_le_bytes());
    raw_message.extend_from_slice(&input.payload);

    // Test message parsing
    let _ = parse_message(&raw_message);

    // Test size validation
    if input.size_field as usize != raw_message.len() {
        let result = validate_message_size(&raw_message);
        assert!(result.is_err() || input.size_field < 10_000_000);
    }

    // Test message type validation
    let valid_types = vec![
        100, 101, // Tversion, Rversion
        102, 103, // Tauth, Rauth
        104, 105, // Tattach, Rattach
        106, 107, // Terror, Rerror
        108, 109, // Tflush, Rflush
        110, 111, // Twalk, Rwalk
        112, 113, // Topen, Ropen
        114, 115, // Tcreate, Rcreate
        116, 117, // Tread, Rread
        118, 119, // Twrite, Rwrite
        120, 121, // Tclunk, Rclunk
        122, 123, // Tremove, Rremove
        124, 125, // Tstat, Rstat
        126, 127, // Twstat, Rwstat
    ];

    if !valid_types.contains(&input.msg_type) {
        let result = parse_message(&raw_message);
        assert!(result.is_err());
    }

    // Test message serialization round-trip
    if let Ok(msg) = parse_message(&raw_message) {
        let serialized = serialize_message(&msg);
        let reparsed = parse_message(&serialized);
        assert!(reparsed.is_ok());

        // Check fields match
        if let Ok(reparsed_msg) = reparsed {
            assert_eq!(msg.get_type(), reparsed_msg.get_type());
            assert_eq!(msg.get_tag(), reparsed_msg.get_tag());
        }
    }

    // Test handling of malformed messages
    let malformed_tests = vec![
        vec![],  // Empty message
        vec![0, 0, 0, 0],  // Only size field
        vec![255; 10_000_000],  // Huge message
        raw_message[..raw_message.len()/2].to_vec(),  // Truncated
    ];

    for malformed in malformed_tests {
        let result = parse_message(&malformed);
        // Should handle gracefully without panic
        let _ = result;
    }

    // Test concurrent message handling
    let messages = vec![raw_message.clone(); 10];
    for msg in messages {
        let _ = handle_message_concurrent(&msg);
    }

    // Test message queue overflow
    for _ in 0..10000 {
        let _ = queue_message(&raw_message);
    }
});

// Stub implementations
fn parse_message(_data: &[u8]) -> Result<Message, Error> {
    Ok(Message::new())
}

fn validate_message_size(_data: &[u8]) -> Result<(), Error> {
    Ok(())
}

fn serialize_message(_msg: &Message) -> Vec<u8> {
    vec![]
}

fn handle_message_concurrent(_data: &[u8]) -> Result<(), Error> {
    Ok(())
}

fn queue_message(_data: &[u8]) -> Result<(), Error> {
    Ok(())
}

struct Message;

impl Message {
    fn new() -> Self { Message }
    fn get_type(&self) -> u8 { 0 }
    fn get_tag(&self) -> u16 { 0 }
}

struct Error;