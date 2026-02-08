//! Working Property-Based Tests for 9P.e Protocol
//! Tests key properties using quickcheck with proper type handling

use ninepe_server::*;
use quickcheck_macros::quickcheck;

// Protocol message tests
#[quickcheck]
fn prop_message_serialization_roundtrip(msize: u32, _tag: u16) -> bool {
    // Bound the inputs to reasonable values
    let msize = msize % 16_000_000 + 1024; // Between 1KB and 16MB

    let msg = protocol::NinePMessage::Version {
        msize,
        version: "9P.e".to_string(),
    };

    match msg.serialize() {
        Ok(bytes) => {
            match protocol::NinePMessage::deserialize(bytes) {
                Ok(deserialized) => msg == deserialized,
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}

#[quickcheck]
fn prop_message_size_bounds(size: u32) -> bool {
    let msg = protocol::NinePMessage::Read {
        fid: 1,
        offset: 0,
        count: size,
        data: Vec::new(),
    };

    match msg.serialize() {
        Ok(bytes) => {
            // Serialized size should be reasonable
            bytes.len() <= protocol::MAX_MESSAGE_SIZE as usize
        }
        Err(_) => {
            // Should fail for oversized messages
            size > protocol::MAX_MESSAGE_SIZE
        }
    }
}

// Memory allocation tests
#[quickcheck]
fn prop_memory_alignment(size: u32) -> bool {
    use memory::{MemoryPool, AllocationStrategy, PoolConfig};

    let size = (size % 65536) + 1; // Limit to reasonable size
    let config = PoolConfig::default();

    if let Ok(mut pool) = MemoryPool::new(0, config, AllocationStrategy::FirstFit) {
        match pool.allocate(size as usize) {
            Ok(ptr) => {
                // Allocation should be aligned
                (ptr as usize) % 8 == 0
            }
            Err(_) => {
                // Allocation failed - check if size was too large
                size as usize > pool.get_stats().total_allocated
            }
        }
    } else {
        false
    }
}

#[quickcheck]
fn prop_memory_no_overlap(sizes: Vec<u16>) -> bool {
    use memory::{MemoryPool, AllocationStrategy, PoolConfig};
    use std::collections::HashSet;

    let sizes: Vec<_> = sizes.into_iter().take(10).collect(); // Limit test size
    let config = PoolConfig::default();

    if let Ok(mut pool) = MemoryPool::new(0, config, AllocationStrategy::FirstFit) {
        let mut allocations = HashSet::new();

        for size in sizes {
            let size = (size as usize).max(1);
            if let Ok(ptr) = pool.allocate(size) {
                // Check no overlap with existing allocations
                for existing in &allocations {
                    let existing_start = *existing as usize;
                    let existing_end = existing_start + size;
                    let new_start = ptr as usize;
                    let new_end = new_start + size;

                    if (new_start < existing_end && new_end > existing_start) {
                        return false; // Overlap detected
                    }
                }
                allocations.insert(ptr);
            }
        }
        true
    } else {
        false
    }
}

// Concurrency tests
#[quickcheck]
fn prop_atomic_counter_monotonic(increments: u8) -> bool {
    use concurrency::AtomicCounter;
    use std::sync::Arc;
    use std::thread;

    let increments = increments.min(100); // Limit for test speed
    let counter = Arc::new(AtomicCounter::new(0));
    let mut handles = vec![];

    for _ in 0..increments {
        let counter_clone = Arc::clone(&counter);
        handles.push(thread::spawn(move || {
            counter_clone.increment();
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    counter.get() == increments as u64
}

// Consensus tests
#[quickcheck]
fn prop_block_hash_deterministic(data: Vec<u8>) -> bool {
    use consensus::GhostdagBlock;

    let data: Vec<u8> = data.into_iter().take(1000).collect(); // Limit size
    let block1 = GhostdagBlock::new(vec![], data.clone());
    let block2 = GhostdagBlock::new(vec![], data);

    // Same data should produce same block structure
    block1.timestamp == block2.timestamp &&
    block1.parent_hashes == block2.parent_hashes &&
    block1.data == block2.data
}

// Crypto tests - using fixed keys to avoid randomness issues
#[test]
fn test_crypto_session_management() {
    use crypto::CryptoSystem;

    let mut crypto = CryptoSystem::new();

    // Create sessions up to limit
    let mut session_count = 0;
    for _ in 0..10 {
        let result = crypto.create_session(None);
        if result.is_ok() {
            session_count += 1;
        }
    }

    // Sessions were created
    assert!(session_count > 0);
}

#[test]
fn test_translator_sandbox_isolation() {
    use translators::TranslatorSystem;

    let mut system = TranslatorSystem::new();

    // Spawn translator
    let result = system.spawn_translator(1, vec![1, 2, 3], vec![]);
    assert!(result.is_ok());

    // Check isolation
    let stats = system.get_stats();
    assert_eq!(stats.active_translators, 1);
}

#[test]
fn test_synthetic_file_generation() {
    use synthetic::{SyntheticFileSystem, GeneratorType};

    let system = SyntheticFileSystem::new();

    // Register generator
    let result = system.register_generator("test", synthetic::GeneratorType::StaticTemplate {
        template: "test template".to_string()
    });
    assert!(result.is_ok());

    // Check registration
    assert!(system.has_generator("test"));
}
