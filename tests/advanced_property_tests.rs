//! Advanced property-based tests for 9P.e protocol
//! These tests use proptest to generate thousands of random inputs

use ninepe_server::*;
use proptest::prelude::*;
use proptest::collection::vec;

// Strategy to generate arbitrary 9P.e messages
fn arb_9pe_message() -> impl Strategy<Value = protocol::NinePMessage> {
    prop_oneof![
        // Version message
        (0u32..=16_777_216u32, "[0-9A-Za-z.]+").prop_map(|(msize, version)| {
            protocol::NinePMessage::Version {
                msize,
                version: version[0..version.len().min(10)].to_string()
            }
        }),

        // Auth message
        (any::<u32>(), "[a-z]{1,20}", "[a-z]{1,20}").prop_map(|(afid, uname, aname)| {
            protocol::NinePMessage::Auth { afid, uname, aname, password: None }
        }),

        // Attach message
        (any::<u32>(), any::<u32>(), "[a-z]{1,20}", "[a-z]{1,20}").prop_map(|(fid, afid, uname, aname)| {
            protocol::NinePMessage::Attach { fid, afid, uname, aname }
        }),

        // Walk message with variable path components
        (any::<u32>(), any::<u32>(), vec("[a-z]{1,20}", 0..10)).prop_map(|(fid, newfid, wnames)| {
            protocol::NinePMessage::Walk { fid, newfid, wnames }
        }),

        // Read message
        (any::<u32>(), any::<u64>(), 0u32..=1_048_576u32, vec(any::<u8>(), 0..65_536)).prop_map(|(fid, offset, count, mut data)| {
            if data.len() > count as usize {
                data.truncate(count as usize);
            }
            protocol::NinePMessage::Read { fid, offset, count, data }
        }),

        // Write message with bounded data size
        (any::<u32>(), any::<u64>(), vec(any::<u8>(), 0..1000)).prop_map(|(fid, offset, data)| {
            protocol::NinePMessage::Write { fid, offset, data }
        }),

        // Stream messages
        (any::<u32>(), any::<u32>(), vec(any::<u8>(), 0..65536)).prop_map(|(stream_id, chunk_id, data)| {
            protocol::NinePMessage::StreamData { stream_id, chunk_id, data }
        }),

        // Consensus messages
        (vec(any::<u8>(), 32..=32), vec(vec(any::<u8>(), 32..=32), 0..5)).prop_map(|(hash_vec, parent_vecs)| {
            let mut block_hash = [0u8; 32];
            block_hash.copy_from_slice(&hash_vec);
            let parent_hashes = parent_vecs.into_iter().map(|v| {
                let mut h = [0u8; 32];
                h.copy_from_slice(&v);
                h
            }).collect();
            protocol::NinePMessage::ConsensusPropose { block_hash, parent_hashes }
        }),
    ]
}

proptest! {
    #[test]
    fn prop_message_serialization_roundtrip(msg in arb_9pe_message()) {
        // Property: deserialize(serialize(msg)) == msg
        if let Ok(serialized) = msg.serialize() {
            let deserialized = protocol::NinePMessage::deserialize(serialized)
                .expect("Failed to deserialize valid message");
            prop_assert_eq!(msg, deserialized);
        }
    }

    #[test]
    fn prop_message_size_bounds(msg in arb_9pe_message()) {
        // Property: All serialized messages respect MAX_MESSAGE_SIZE
        if let Ok(serialized) = msg.serialize() {
            prop_assert!(serialized.len() <= protocol::MAX_MESSAGE_SIZE as usize);
        }
    }

    #[test]
    fn prop_write_message_safe_constructor(
        fid in any::<u32>(),
        offset in any::<u64>(),
        size in 0usize..200_000_000usize
    ) {
        // Property: new_write_safe correctly validates size bounds
        match protocol::NinePMessage::new_write_safe(fid, offset, size) {
            Ok(msg) => {
                // If accepted, must be within bounds
                prop_assert!(size <= protocol::MAX_MESSAGE_SIZE as usize - 32);

                // And must serialize successfully
                let serialized = msg.serialize().expect("Valid message should serialize");
                prop_assert!(serialized.len() <= protocol::MAX_MESSAGE_SIZE as usize);
            }
            Err(_) => {
                // If rejected, must be too large
                prop_assert!(size > protocol::MAX_MESSAGE_SIZE as usize - 32);
            }
        }
    }

    #[test]
    fn prop_consensus_block_invariants(
        parent_count in 0usize..10usize,
        data_size in 0usize..10000usize
    ) {
        use ninepe_server::consensus::{GhostdagBlock, EnhancedGhostdag, ConsensusResult};

        let mut dag = EnhancedGhostdag::new(10);

        // Create genesis
        let genesis = GhostdagBlock::genesis(vec![0u8; data_size], [0u8; 32], [0u8; 64], 0, 0, 0);
        let genesis_hash = match dag.add_block(genesis) {
            Ok(ConsensusResult::BlockAccepted(h)) => h,
            _ => panic!("Genesis must be accepted"),
        };

        // Create blocks with varying parent counts
        let mut parent_hashes = vec![genesis_hash];
        for i in 0..parent_count {
            let block = GhostdagBlock::new(
                parent_hashes.clone(),
                format!("block_{}", i).into_bytes(),
                [0u8; 32], [0u8; 64], 0, 0, 0
            );

            if let Ok(ConsensusResult::BlockAccepted(hash)) = dag.add_block(block) {
                parent_hashes.push(hash);
                if parent_hashes.len() > 3 {
                    parent_hashes.remove(0);
                }
            }
        }

        // Property: Blue + Red blocks = Total blocks
        let stats = dag.get_consensus_stats();
        prop_assert_eq!(stats.blue_blocks + stats.red_blocks, stats.total_blocks);

        // Property: Memory usage stays bounded
        let usage = dag.get_memory_usage();
        prop_assert!(usage.total_blocks <= parent_count + 1);
    }

    #[test]
    fn prop_atomic_counter_monotonic(
        initial in 0u64..1000u64,
        increments in 1usize..1000usize
    ) {
        use concurrency::AtomicCounter;

        let counter = AtomicCounter::new(initial);
        let mut expected = initial;

        for _ in 0..increments {
            let prev = counter.get();
            counter.increment();
            let curr = counter.get();

            // Property: Counter is monotonically increasing (or wraps to 0)
            prop_assert!(curr == prev + 1 || (prev == u64::MAX && curr == 0));

            expected = if expected == u64::MAX { 0 } else { expected + 1 };
        }

        prop_assert_eq!(counter.get(), expected);
    }
}

// Strategies for generating complex message sequences
fn arb_message_sequence() -> impl Strategy<Value = Vec<protocol::NinePMessage>> {
    vec(arb_9pe_message(), 1..50)
}

proptest! {
    #[test]
    fn prop_message_sequence_processing(messages in arb_message_sequence()) {
        use protocol::ConnectionState;

        let mut state = ConnectionState::new(0, "9P.e", protocol::MAX_MESSAGE_SIZE);

        for msg in messages {
            // Process each message
            match msg {
                protocol::NinePMessage::Version { msize, version } => {
                    state.protocol_version = version;
                    state.max_message_size = msize.min(protocol::MAX_MESSAGE_SIZE);
                }
                protocol::NinePMessage::Auth { .. } => {
                    state.authenticated = true;
                }
                _ => {}
            }

            // Property: State remains consistent
            prop_assert!(state.max_message_size <= protocol::MAX_MESSAGE_SIZE);
            prop_assert!(state.max_message_size >= protocol::MIN_MESSAGE_SIZE);
        }
    }
}

#[cfg(test)]
mod shrinking_tests {
    use super::*;

    proptest! {
        #[test]
        fn prop_minimal_failing_case(
            data_size in 0usize..100_000_000usize
        ) {
            // This test will shrink to find the minimal size that causes failure
            if data_size > protocol::MAX_MESSAGE_SIZE as usize - 32 {
                // Expected to fail for large sizes
                let result = protocol::NinePMessage::new_write_safe(0, 0, data_size);
                prop_assert!(result.is_err());
            } else {
                // Should succeed for small sizes
                let result = protocol::NinePMessage::new_write_safe(0, 0, data_size);
                prop_assert!(result.is_ok());
            }
        }
    }
}
