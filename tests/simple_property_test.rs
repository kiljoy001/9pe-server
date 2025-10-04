//! Simple property test to demonstrate property testing works
//! Tests basic NinePeeMessage serialization properties

use proptest::prelude::*;
use ninep_server::protocol::NinePeeMessage;

/// Test that NinePeeMessage serialization works
#[test]
fn prop_ninepee_message_serialization() {
    proptest!(|(msize in 1024u32..1048576u32, version in "[a-zA-Z0-9.]{1,20}")| {
        let message = NinePeeMessage::Version { msize, version: version.clone() };

        // Test serialization
        let serialized = bincode::serialize(&message).unwrap();
        assert!(!serialized.is_empty());

        // Test deserialization
        let deserialized: NinePeeMessage = bincode::deserialize(&serialized).unwrap();

        // Verify round-trip
        if let NinePeeMessage::Version { msize: m, version: v } = deserialized {
            assert_eq!(m, msize);
            assert_eq!(v, version);
        } else {
            panic!("Deserialized message type mismatch");
        }
    });
}

/// Test that error messages work correctly
#[test]
fn prop_error_message_creation() {
    proptest!(|(errno in 1u32..100u32)| {
        let error_msg = NinePeeMessage::Error {
            ename: format!("Test error {}", errno),
            errno,
        };

        // Test that it's identified as an error
        assert!(error_msg.is_error());
        assert!(!error_msg.is_extension());
        assert!(error_msg.fid().is_none());

        // Test serialization
        let serialized = bincode::serialize(&error_msg).unwrap();
        let deserialized: NinePeeMessage = bincode::deserialize(&serialized).unwrap();

        // Verify it's still an error after round-trip
        assert!(deserialized.is_error());
    });
}

/// Test that extension messages work correctly
#[test]
fn prop_extension_message_creation() {
    proptest!(|(translator_id in 1u32..1000u32, data in prop::collection::vec(any::<u8>(), 0..100))| {
        let ext_msg = NinePeeMessage::TranslatorMessage {
            translator_id,
            data: data.clone(),
        };

        // Test that it's identified as an extension
        assert!(!ext_msg.is_error());
        assert!(ext_msg.is_extension());
        assert!(ext_msg.fid().is_none());

        // Test serialization
        let serialized = bincode::serialize(&ext_msg).unwrap();
        let deserialized: NinePeeMessage = bincode::deserialize(&serialized).unwrap();

        // Verify it's still an extension after round-trip
        assert!(deserialized.is_extension());
    });
}