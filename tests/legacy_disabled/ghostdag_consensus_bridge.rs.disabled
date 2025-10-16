//! Property-based bridge between the local bounded GHOSTDAG implementation and the
//! upstream 9PE consensus library. The goal is to ensure both stacks agree on
//! block acceptance decisions for the same randomly generated DAG growth
//! sequence.

use std::collections::{HashMap, HashSet};

use ninep_server::consensus::{
    bounded_ghostdag::{Block, BlockState, NamespaceOp},
    BoundedGhostdag,
};
use ninepee::consensus::{ConsensusResult, EnhancedGhostdag, GhostdagBlock};
use proptest::prelude::*;

const PROPTEST_CASES: u32 = 32;

#[derive(Debug, Clone)]
struct BlockSpec {
    parent_seeds: Vec<u32>,
    operations: Vec<NamespaceOp>,
    creator: String,
}

fn namespace_op_strategy() -> impl Strategy<Value = NamespaceOp> {
    prop_oneof![
        (any::<String>(), any::<u32>(), any::<bool>()).prop_map(|(path, mode, is_dir)| {
            NamespaceOp::Create {
                path: format!("/{}", sanitize(&path)),
                mode: mode & 0o777,
                is_dir,
            }
        }),
        (any::<String>(), any::<u64>(), any::<[u8; 32]>()).prop_map(|(path, offset, hash)| {
            NamespaceOp::Write {
                path: format!("/{}", sanitize(&path)),
                offset: offset % 1024,
                hash,
            }
        }),
        any::<String>().prop_map(|path| NamespaceOp::Delete {
            path: format!("/{}", sanitize(&path)),
        }),
    ]
}

fn block_spec_strategy() -> impl Strategy<Value = BlockSpec> {
    (
        prop::collection::vec(0u32..32, 0..3),
        prop::collection::vec(namespace_op_strategy(), 0..1),
        any::<String>(),
    )
        .prop_map(|(parent_seeds, operations, creator)| BlockSpec {
            parent_seeds,
            operations,
            creator,
        })
}

fn sanitize(input: &str) -> String {
    let cleaned: String = input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(12)
        .collect();
    if cleaned.is_empty() {
        "root".to_string()
    } else {
        cleaned
    }
}

fn parent_hashes(parent_ids: &[String], hash_map: &HashMap<String, [u8; 32]>) -> Vec<[u8; 32]> {
    parent_ids
        .iter()
        .filter_map(|id| hash_map.get(id).cloned())
        .collect()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(PROPTEST_CASES))]

    #[test]
    fn bounded_matches_upstream(blocks in prop::collection::vec(block_spec_strategy(), 1..32)) {
        let bounded = BoundedGhostdag::new("bridge".to_string());
        let mut upstream = EnhancedGhostdag::new(10);

        // Insert genesis block for both implementations
        let genesis_block = Block {
            id: "genesis".to_string(),
            parents: vec![],
            operations: vec![],
            timestamp: now_secs(),
            creator: "root".to_string(),
            signature: vec![0; 32],
            state: BlockState::Pending,
            ghost_weight: 1,
            height: 0,
        };

        futures::executor::block_on(bounded.add_block(genesis_block)).expect("bounded genesis");

        let genesis_upstream = GhostdagBlock::genesis(b"genesis".to_vec());
        let genesis_result = upstream
            .add_block(genesis_upstream)
            .expect("upstream genesis");

        let mut id_to_hash: HashMap<String, [u8; 32]> = HashMap::new();
        let genesis_hash = match genesis_result {
            ConsensusResult::BlockAccepted(hash) => hash,
            other => panic!("unexpected genesis result: {:?}", other),
        };
        id_to_hash.insert("genesis".to_string(), genesis_hash);

        let mut accepted_ids: Vec<String> = vec!["genesis".to_string()];

        for (idx, spec) in blocks.into_iter().enumerate() {
            let block_id = format!("block_{}", idx);
            let timestamp = now_secs();
            let creator = sanitize(&spec.creator);

            // Map seed indices to actual parent IDs drawn from already accepted blocks
            let mut parents: Vec<String> = spec
                .parent_seeds
                .iter()
                .filter_map(|seed| {
                    if accepted_ids.is_empty() {
                        None
                    } else {
                        let idx = (*seed as usize) % accepted_ids.len();
                        Some(accepted_ids[idx].clone())
                    }
                })
                .collect();

            if parents.is_empty() {
                parents.push("genesis".to_string());
            }

            // Deduplicate parents while preserving order preference
            let mut seen = HashSet::new();
            parents.retain(|p| seen.insert(p.clone()));

            let bounded_block = Block {
                id: block_id.clone(),
                parents: parents.clone(),
                operations: spec.operations.clone(),
                timestamp,
                creator: creator.clone(),
                signature: vec![0; 32],
                state: BlockState::Pending,
                ghost_weight: 1,
                height: 0,
            };

            let bounded_result = futures::executor::block_on(bounded.add_block(bounded_block));

            let upstream_block = GhostdagBlock::new(parent_hashes(&parents, &id_to_hash), block_id.as_bytes().to_vec());
            let upstream_result = upstream.add_block(upstream_block);

            let bounded_ok = bounded_result.is_ok();
            let upstream_ok = matches!(upstream_result, Ok(ConsensusResult::BlockAccepted(_)));

            prop_assert_eq!(bounded_ok, upstream_ok, "bounded/upstream divergence for {block_id}");

            if bounded_ok {
                accepted_ids.push(block_id.clone());
            }

            if let Ok(ConsensusResult::BlockAccepted(hash)) = upstream_result {
                id_to_hash.insert(block_id, hash);
            }
        }

        let stats = futures::executor::block_on(bounded.get_stats());
        prop_assert!(stats.total_blocks >= accepted_ids.len() as u64 - 1); // subtract genesis counted once
    }
}
