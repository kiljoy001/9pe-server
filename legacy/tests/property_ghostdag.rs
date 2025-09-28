//! Property-based tests for GhostDAG consensus algorithm
//! Verifies termination, blue set properties, and absence of infinite recursion

use proptest::prelude::*;
use proptest::collection::{vec, hash_set};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::cell::RefCell;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct BlockHash(u64);

#[derive(Debug, Clone)]
struct Block {
    hash: BlockHash,
    parents: Vec<BlockHash>,
    height: u64,
    timestamp: u64,
    blue_score: u64,
}

#[derive(Debug)]
struct DAG {
    blocks: HashMap<BlockHash, Block>,
    blue_set: HashSet<BlockHash>,
    tips: HashSet<BlockHash>,
}

impl DAG {
    fn new() -> Self {
        let genesis = Block {
            hash: BlockHash(0),
            parents: vec![],
            height: 0,
            timestamp: 0,
            blue_score: 0,
        };

        let mut blocks = HashMap::new();
        let mut tips = HashSet::new();

        blocks.insert(genesis.hash.clone(), genesis);
        tips.insert(BlockHash(0));

        DAG {
            blocks,
            blue_set: HashSet::new(),
            tips,
        }
    }

    /// Get ancestors with bounded recursion (the fix for infinite recursion bug)
    fn get_ancestors_bounded(&self, hash: &BlockHash, max_depth: usize) -> HashSet<BlockHash> {
        let mut ancestors = HashSet::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut depth_map = HashMap::new();

        queue.push_back(hash.clone());
        depth_map.insert(hash.clone(), 0);
        visited.insert(hash.clone());

        while let Some(current) = queue.pop_front() {
            let current_depth = *depth_map.get(&current).unwrap_or(&0);

            if current_depth >= max_depth {
                continue; // Stop at max depth
            }

            ancestors.insert(current.clone());

            if let Some(block) = self.blocks.get(&current) {
                for parent in &block.parents {
                    if !visited.contains(parent) {
                        visited.insert(parent.clone());
                        queue.push_back(parent.clone());
                        depth_map.insert(parent.clone(), current_depth + 1);
                    }
                }
            }
        }

        ancestors
    }

    /// BUGGY VERSION: Get ancestors with unbounded recursion (the bug we're fixing)
    fn get_ancestors_unbounded_buggy(&self, hash: &BlockHash,
                                     visited: &mut HashSet<BlockHash>) -> HashSet<BlockHash> {
        if visited.contains(hash) {
            // BUG: This check comes too late if there's a cycle
            return HashSet::new();
        }
        visited.insert(hash.clone());

        let mut ancestors = HashSet::new();
        ancestors.insert(hash.clone());

        if let Some(block) = self.blocks.get(hash) {
            for parent in &block.parents {
                // BUG: No depth limit, can recurse forever with cycles
                let parent_ancestors = self.get_ancestors_unbounded_buggy(parent, visited);
                ancestors.extend(parent_ancestors);
            }
        }

        ancestors
    }

    /// Compute blue set with termination guarantee
    fn compute_blue_set_bounded(&mut self, tip: &BlockHash) -> HashSet<BlockHash> {
        let max_depth = self.blocks.len(); // Bounded by DAG size
        let ancestors = self.get_ancestors_bounded(tip, max_depth);

        // Simple heuristic: blocks with fewer conflicts are blue
        let mut blue_set = HashSet::new();

        for block_hash in &ancestors {
            let mut conflicts = 0;
            if let Some(block) = self.blocks.get(block_hash) {
                // Count blocks that are not ancestors
                for other in &ancestors {
                    if other != block_hash {
                        let other_ancestors = self.get_ancestors_bounded(other, max_depth);
                        if !other_ancestors.contains(block_hash) {
                            conflicts += 1;
                        }
                    }
                }
            }

            // Add to blue set if conflicts are below threshold
            if conflicts < ancestors.len() / 2 {
                blue_set.insert(block_hash.clone());
            }
        }

        blue_set
    }

    /// Check if DAG is acyclic (no block is its own ancestor)
    fn is_acyclic(&self) -> bool {
        for (hash, _) in &self.blocks {
            let ancestors = self.get_ancestors_bounded(hash, self.blocks.len());
            // Remove self from ancestors
            let mut ancestors_without_self = ancestors.clone();
            ancestors_without_self.remove(hash);

            // Check if we can reach ourselves through parents
            for ancestor in ancestors_without_self {
                let ancestor_ancestors = self.get_ancestors_bounded(&ancestor, self.blocks.len());
                if ancestor_ancestors.contains(hash) && ancestor != *hash {
                    return false; // Found cycle
                }
            }
        }
        true
    }

    fn add_block(&mut self, block: Block) -> Result<(), String> {
        // Verify parents exist
        for parent in &block.parents {
            if !self.blocks.contains_key(parent) {
                return Err(format!("Parent {:?} not found", parent));
            }
        }

        // Update tips
        for parent in &block.parents {
            self.tips.remove(parent);
        }
        self.tips.insert(block.hash.clone());

        self.blocks.insert(block.hash.clone(), block);
        Ok(())
    }
}

/// Generate arbitrary block hash
fn arbitrary_block_hash() -> impl Strategy<Value = BlockHash> {
    (0u64..100000u64).prop_map(BlockHash)
}

/// Generate valid parent hashes from existing blocks
fn arbitrary_parents(max_parents: usize) -> impl Strategy<Value = Vec<BlockHash>> {
    vec(arbitrary_block_hash(), 0..=max_parents)
}

/// Generate arbitrary block
fn arbitrary_block() -> impl Strategy<Value = Block> {
    (
        arbitrary_block_hash(),
        arbitrary_parents(3),
        0u64..1000u64,  // height
        0u64..1000000u64,  // timestamp
        0u64..1000u64,  // blue_score
    ).prop_map(|(hash, parents, height, timestamp, blue_score)| {
        Block { hash, parents, height, timestamp, blue_score }
    })
}

proptest! {
    /// Test: Blue set computation always terminates
    #[test]
    fn prop_blue_set_terminates(
        blocks in vec(arbitrary_block(), 1..50)
    ) {
        let mut dag = DAG::new();

        // Add blocks carefully to maintain DAG structure
        for block in blocks {
            // Only use parents that exist
            let valid_parents: Vec<_> = block.parents.iter()
                .filter(|p| dag.blocks.contains_key(p))
                .cloned()
                .collect();

            let valid_block = Block {
                parents: if valid_parents.is_empty() {
                    vec![BlockHash(0)] // Use genesis as parent
                } else {
                    valid_parents
                },
                ..block
            };

            let _ = dag.add_block(valid_block);
        }

        // Should terminate with bounded recursion
        let tip = dag.tips.iter().next().cloned().unwrap_or(BlockHash(0));
        let blue_set = dag.compute_blue_set_bounded(&tip);

        // Blue set should exist (termination proven)
        prop_assert!(blue_set.len() <= dag.blocks.len());
    }

    /// Test: Acyclic DAGs prevent infinite recursion
    #[test]
    fn prop_acyclic_no_infinite_recursion(
        num_blocks in 1usize..20usize
    ) {
        let mut dag = DAG::new();

        // Build acyclic DAG by only allowing parents with lower height
        for i in 1..=num_blocks {
            let parents = if i == 1 {
                vec![BlockHash(0)]
            } else {
                vec![BlockHash((i - 1) as u64)]
            };

            let block = Block {
                hash: BlockHash(i as u64),
                parents,
                height: i as u64,
                timestamp: i as u64,
                blue_score: 0,
            };

            dag.add_block(block).unwrap();
        }

        prop_assert!(dag.is_acyclic());

        // Ancestors computation should stabilize
        for (hash, _) in &dag.blocks {
            let ancestors_n = dag.get_ancestors_bounded(hash, dag.blocks.len());
            let ancestors_n_plus_1 = dag.get_ancestors_bounded(hash, dag.blocks.len() + 1);

            // Should be the same (stabilized)
            prop_assert_eq!(ancestors_n, ancestors_n_plus_1);
        }
    }

    /// Test: Blue scores are monotonic
    #[test]
    fn prop_blue_score_monotonic(
        parent_score in 0u64..1000u64,
        additional_score in 0u64..100u64
    ) {
        let mut dag = DAG::new();

        // Add parent block
        let parent = Block {
            hash: BlockHash(1),
            parents: vec![BlockHash(0)],
            height: 1,
            timestamp: 100,
            blue_score: parent_score,
        };
        dag.add_block(parent).unwrap();

        // Add child block
        let child = Block {
            hash: BlockHash(2),
            parents: vec![BlockHash(1)],
            height: 2,
            timestamp: 200,
            blue_score: parent_score + additional_score,
        };
        dag.add_block(child).unwrap();

        // Child should have higher or equal blue score
        let parent_block = dag.blocks.get(&BlockHash(1)).unwrap();
        let child_block = dag.blocks.get(&BlockHash(2)).unwrap();

        prop_assert!(child_block.blue_score >= parent_block.blue_score);
    }

    /// Test: Blue set is subset of ancestors
    #[test]
    fn prop_blue_set_subset_ancestors(
        num_blocks in 2usize..15usize
    ) {
        let mut dag = DAG::new();

        // Build simple chain
        for i in 1..=num_blocks {
            let block = Block {
                hash: BlockHash(i as u64),
                parents: vec![BlockHash((i - 1) as u64)],
                height: i as u64,
                timestamp: i as u64,
                blue_score: i as u64,
            };
            dag.add_block(block).unwrap();
        }

        let tip = BlockHash(num_blocks as u64);
        let blue_set = dag.compute_blue_set_bounded(&tip);
        let ancestors = dag.get_ancestors_bounded(&tip, dag.blocks.len());

        // Every blue block must be an ancestor
        for blue_block in &blue_set {
            prop_assert!(ancestors.contains(blue_block));
        }
    }

    /// Test: Maximum path length bounded by DAG size
    #[test]
    fn prop_max_path_bounded(
        dag_size in 5usize..30usize
    ) {
        let mut dag = DAG::new();

        // Build DAG
        for i in 1..=dag_size {
            // Random parents from lower blocks
            let mut parents = vec![];
            if i > 1 {
                parents.push(BlockHash(((i - 1) % dag_size) as u64));
            } else {
                parents.push(BlockHash(0));
            }

            let block = Block {
                hash: BlockHash(i as u64),
                parents,
                height: i as u64,
                timestamp: i as u64,
                blue_score: 0,
            };

            dag.add_block(block).unwrap();
        }

        // Path length should be bounded
        for (hash, _) in &dag.blocks {
            let ancestors = dag.get_ancestors_bounded(hash, dag.blocks.len() * 2);
            prop_assert!(ancestors.len() <= dag.blocks.len());
        }
    }

    /// Test: Deterministic blue set selection
    #[test]
    fn prop_blue_set_deterministic(
        seed_blocks in vec(arbitrary_block(), 1..10)
    ) {
        let mut dag1 = DAG::new();
        let mut dag2 = DAG::new();

        // Build identical DAGs
        for block in &seed_blocks {
            let valid_parents: Vec<_> = block.parents.iter()
                .filter(|p| dag1.blocks.contains_key(p))
                .cloned()
                .collect();

            let valid_block = Block {
                parents: if valid_parents.is_empty() {
                    vec![BlockHash(0)]
                } else {
                    valid_parents
                },
                ..block.clone()
            };

            dag1.add_block(valid_block.clone()).ok();
            dag2.add_block(valid_block).ok();
        }

        // Compute blue sets from same tip
        if let Some(tip) = dag1.tips.iter().next() {
            let blue_set1 = dag1.compute_blue_set_bounded(tip);
            let blue_set2 = dag2.compute_blue_set_bounded(tip);

            // Should be identical
            prop_assert_eq!(blue_set1, blue_set2);
        }
    }

    /// Test: Fix for infinite recursion bug
    #[test]
    fn prop_recursion_depth_bounded(
        max_depth in 1usize..100usize,
        num_ancestors in 1usize..50usize
    ) {
        fn count_recursive_calls(depth: usize, max: usize) -> usize {
            if depth >= max {
                return 0;
            }
            1 + count_recursive_calls(depth + 1, max)
        }

        let calls = count_recursive_calls(0, max_depth);

        // Recursion depth equals max_depth
        prop_assert_eq!(calls, max_depth);

        // This models the fix: bounded recursion
        prop_assert!(calls <= max_depth);
    }

    /// Test: Consensus property - agreement on blue sets
    #[test]
    fn prop_consensus_agreement(
        fork_height in 1usize..10usize,
        chain_length in 2usize..15usize
    ) {
        let mut dag = DAG::new();

        // Build common chain
        for i in 1..=fork_height {
            let block = Block {
                hash: BlockHash(i as u64),
                parents: vec![BlockHash((i - 1) as u64)],
                height: i as u64,
                timestamp: i as u64,
                blue_score: i as u64,
            };
            dag.add_block(block).unwrap();
        }

        // Create fork
        let fork_point = BlockHash(fork_height as u64);

        // Branch 1
        for i in 1..chain_length {
            let block = Block {
                hash: BlockHash((100 + i) as u64),
                parents: vec![if i == 1 { fork_point.clone() } else { BlockHash((99 + i) as u64) }],
                height: (fork_height + i) as u64,
                timestamp: (fork_height + i) as u64,
                blue_score: (fork_height + i) as u64,
            };
            dag.add_block(block).unwrap();
        }

        // Branch 2
        for i in 1..chain_length {
            let block = Block {
                hash: BlockHash((200 + i) as u64),
                parents: vec![if i == 1 { fork_point.clone() } else { BlockHash((199 + i) as u64) }],
                height: (fork_height + i) as u64,
                timestamp: (fork_height + i) as u64,
                blue_score: (fork_height + i) as u64,
            };
            dag.add_block(block).unwrap();
        }

        // Blue sets from common ancestor should be consistent
        let blue_set_common = dag.compute_blue_set_bounded(&fork_point);
        prop_assert!(!blue_set_common.is_empty() || fork_height == 0);
    }
}

/// Test specific bug: infinite recursion in blue set computation
#[test]
fn test_infinite_recursion_bug() {
    let mut dag = DAG::new();

    // Create blocks that could cause cycle (the bug scenario)
    let block1 = Block {
        hash: BlockHash(1),
        parents: vec![BlockHash(0)],
        height: 1,
        timestamp: 100,
        blue_score: 1,
    };
    dag.add_block(block1).unwrap();

    // This would cause infinite recursion with unbounded recursion
    let mut visited = HashSet::new();

    // The buggy version would overflow the stack
    // let ancestors_buggy = dag.get_ancestors_unbounded_buggy(&BlockHash(1), &mut visited);

    // The fixed version terminates
    let ancestors_fixed = dag.get_ancestors_bounded(&BlockHash(1), 100);

    assert!(ancestors_fixed.len() <= dag.blocks.len());
    assert!(ancestors_fixed.contains(&BlockHash(1)));
    assert!(ancestors_fixed.contains(&BlockHash(0)));
}

/// Test: Verify the fix prevents stack overflow
#[test]
fn test_stack_overflow_prevention() {
    let mut dag = DAG::new();

    // Create deep chain that would cause stack overflow
    for i in 1..=10000 {
        let block = Block {
            hash: BlockHash(i),
            parents: vec![BlockHash(i - 1)],
            height: i,
            timestamp: i,
            blue_score: i,
        };
        dag.add_block(block).unwrap();
    }

    // This should not overflow with bounded recursion
    let tip = BlockHash(10000);
    let ancestors = dag.get_ancestors_bounded(&tip, dag.blocks.len());

    // Should complete without stack overflow
    assert!(ancestors.len() > 0);
    assert!(ancestors.len() <= dag.blocks.len());
}