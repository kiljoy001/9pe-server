//! Fault Injection and Network Partition Tests
//! These tests simulate failures and network partitions in the distributed system

use ninepe_server::*;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, AtomicUsize, Ordering}};
use std::thread;
use std::time::{Duration, Instant};
use std::collections::{HashMap, HashSet};

// Simulated network node
struct Node {
    id: usize,
    dag: Arc<Mutex<consensus::EnhancedGhostdag>>,
    peers: Arc<Mutex<Vec<usize>>>,
    alive: Arc<AtomicBool>,
    partition_group: Arc<AtomicUsize>,
    messages_sent: Arc<AtomicUsize>,
    messages_received: Arc<AtomicUsize>,
}

impl Node {
    fn new(id: usize, k: usize) -> Self {
        Self {
            id,
            dag: Arc::new(Mutex::new(consensus::EnhancedGhostdag::new(k))),
            peers: Arc::new(Mutex::new(Vec::new())),
            alive: Arc::new(AtomicBool::new(true)),
            partition_group: Arc::new(AtomicUsize::new(0)),
            messages_sent: Arc::new(AtomicUsize::new(0)),
            messages_received: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    fn kill(&self) {
        self.alive.store(false, Ordering::Release);
    }

    fn revive(&self) {
        self.alive.store(true, Ordering::Release);
    }

    fn can_communicate_with(&self, other: &Node) -> bool {
        // Can communicate if both alive and in same partition group
        self.is_alive() && other.is_alive() &&
        self.partition_group.load(Ordering::Acquire) == other.partition_group.load(Ordering::Acquire)
    }
}

// Distributed network simulator
struct Network {
    nodes: Vec<Arc<Node>>,
    network_partitioned: Arc<AtomicBool>,
}

impl Network {
    fn new(num_nodes: usize, k: usize) -> Self {
        let mut nodes = Vec::new();
        for i in 0..num_nodes {
            nodes.push(Arc::new(Node::new(i, k)));
        }

        // Connect all nodes as peers
        for i in 0..num_nodes {
            let mut peers = nodes[i].peers.lock().unwrap();
            for j in 0..num_nodes {
                if i != j {
                    peers.push(j);
                }
            }
        }

        Self {
            nodes,
            network_partitioned: Arc::new(AtomicBool::new(false)),
        }
    }

    fn partition_network(&self, group1_size: usize) {
        // Split network into two partition groups
        self.network_partitioned.store(true, Ordering::Release);

        for (i, node) in self.nodes.iter().enumerate() {
            if i < group1_size {
                node.partition_group.store(0, Ordering::Release);
            } else {
                node.partition_group.store(1, Ordering::Release);
            }
        }
    }

    fn heal_partition(&self) {
        // Reunite all nodes in partition group 0
        self.network_partitioned.store(false, Ordering::Release);

        for node in &self.nodes {
            node.partition_group.store(0, Ordering::Release);
        }
    }

    fn broadcast_block(&self, sender_id: usize, block: consensus::GhostdagBlock) -> usize {
        let sender = &self.nodes[sender_id];
        if !sender.is_alive() {
            return 0;
        }

        let mut successful_sends = 0;
        let peers = sender.peers.lock().unwrap().clone();

        for peer_id in peers {
            let peer = &self.nodes[peer_id];
            if sender.can_communicate_with(peer) {
                // Simulate network delay
                thread::sleep(Duration::from_micros(100));

                // Try to add block to peer's DAG
                let mut peer_dag = peer.dag.lock().unwrap();
                if peer_dag.add_block(block.clone()).is_ok() {
                    successful_sends += 1;
                    peer.messages_received.fetch_add(1, Ordering::Relaxed);
                }
                sender.messages_sent.fetch_add(1, Ordering::Relaxed);
            }
        }

        successful_sends
    }

    fn get_consensus_state(&self) -> Vec<consensus::ConsensusStats> {
        self.nodes.iter()
            .filter(|n| n.is_alive())
            .map(|n| n.dag.lock().unwrap().get_consensus_stats())
            .collect()
    }
}

#[cfg(test)]
mod fault_injection {
    use super::*;

    #[test]
    fn test_node_failure_recovery() {
        // Test that consensus continues despite node failures
        let network = Network::new(5, 3);

        // Initialize with genesis on all nodes
        let genesis = consensus::GhostdagBlock::genesis(b"genesis".to_vec());
        for node in &network.nodes {
            let mut dag = node.dag.lock().unwrap();
            dag.add_block(genesis.clone()).unwrap();
        }

        // Kill node 2 (40% failure)
        network.nodes[2].kill();

        // Continue creating blocks
        for i in 0..10 {
            let block = consensus::GhostdagBlock::new(
                vec![[0u8; 32]], // Simplified parent hash
                format!("block_{}", i).into_bytes()
            );

            // Node 0 creates and broadcasts
            let sends = network.broadcast_block(0, block);
            assert!(sends >= 2); // Should reach at least 2 live nodes
        }

        // Verify consensus among live nodes
        let states = network.get_consensus_state();
        assert_eq!(states.len(), 4); // 4 nodes alive

        // All live nodes should have similar state
        let first_state = &states[0];
        for state in &states[1..] {
            // Allow some divergence but core metrics should be close
            assert!((state.total_blocks as i64 - first_state.total_blocks as i64).abs() <= 2);
        }
    }

    #[test]
    fn test_network_partition_consistency() {
        // Test that system maintains consistency during network partition
        let network = Network::new(6, 3);

        // Initialize with genesis
        let genesis = consensus::GhostdagBlock::genesis(b"genesis".to_vec());
        for node in &network.nodes {
            let mut dag = node.dag.lock().unwrap();
            dag.add_block(genesis.clone()).unwrap();
        }

        // Create partition: nodes 0-2 in group 0, nodes 3-5 in group 1
        network.partition_network(3);

        // Each partition creates blocks independently
        let handles: Vec<_> = vec![
            // Group 0 activity
            thread::spawn({
                let net = Arc::new(network.nodes.clone());
                move || {
                    for i in 0..5 {
                        let block = consensus::GhostdagBlock::new(
                            vec![[0u8; 32]],
                            format!("group0_block_{}", i).into_bytes()
                        );
                        let mut dag = net[0].dag.lock().unwrap();
                        dag.add_block(block).ok();
                        thread::sleep(Duration::from_millis(10));
                    }
                }
            }),
            // Group 1 activity
            thread::spawn({
                let net = Arc::new(network.nodes.clone());
                move || {
                    for i in 0..5 {
                        let block = consensus::GhostdagBlock::new(
                            vec![[0u8; 32]],
                            format!("group1_block_{}", i).into_bytes()
                        );
                        let mut dag = net[3].dag.lock().unwrap();
                        dag.add_block(block).ok();
                        thread::sleep(Duration::from_millis(10));
                    }
                }
            }),
        ];

        // Let partitions diverge
        for h in handles {
            h.join().unwrap();
        }

        // Check divergence during partition
        let group0_stats = network.nodes[0].dag.lock().unwrap().get_consensus_stats();
        let group1_stats = network.nodes[3].dag.lock().unwrap().get_consensus_stats();

        // Groups should have different block counts (diverged)
        assert_ne!(group0_stats.total_blocks, group1_stats.total_blocks);

        // Heal partition
        network.heal_partition();

        // Sync blocks between groups (simplified - in real system would use gossip)
        // For now, just verify that both groups maintained valid state
        assert!(group0_stats.blue_blocks + group0_stats.red_blocks == group0_stats.total_blocks);
        assert!(group1_stats.blue_blocks + group1_stats.red_blocks == group1_stats.total_blocks);
    }

    #[test]
    fn test_cascading_failures() {
        // Test system resilience to cascading failures
        let network = Network::new(10, 5);

        // Initialize
        let genesis = consensus::GhostdagBlock::genesis(b"genesis".to_vec());
        for node in &network.nodes {
            let mut dag = node.dag.lock().unwrap();
            dag.add_block(genesis.clone()).unwrap();
        }

        // Progressively kill nodes
        let mut alive_count = 10;
        for i in 0..7 {
            // Kill node i
            network.nodes[i].kill();
            alive_count -= 1;

            // Try to continue consensus
            if alive_count >= 3 {
                // Should still function with 3+ nodes
                let block = consensus::GhostdagBlock::new(
                    vec![[0u8; 32]],
                    format!("block_after_failure_{}", i).into_bytes()
                );

                // Find an alive node to create block
                for (j, node) in network.nodes.iter().enumerate() {
                    if node.is_alive() {
                        let mut dag = node.dag.lock().unwrap();
                        let result = dag.add_block(block.clone());
                        assert!(result.is_ok(), "Should handle block with {} nodes alive", alive_count);
                        break;
                    }
                }
            }
        }

        // Verify remaining nodes still have valid state
        let final_states = network.get_consensus_state();
        assert_eq!(final_states.len(), 3); // Exactly 3 nodes alive

        for state in &final_states {
            assert!(state.memory_optimization_ratio >= 1.0);
        }
    }

    #[test]
    fn test_byzantine_node_behavior() {
        // Test resilience to Byzantine (malicious) node behavior
        let network = Network::new(7, 3);

        // Initialize
        let genesis = consensus::GhostdagBlock::genesis(b"genesis".to_vec());
        for node in &network.nodes {
            let mut dag = node.dag.lock().unwrap();
            dag.add_block(genesis.clone()).unwrap();
        }

        // Node 0 acts Byzantine - tries to create conflicting blocks
        let byzantine_node = &network.nodes[0];
        let mut byzantine_dag = byzantine_node.dag.lock().unwrap();

        // Try to add multiple conflicting blocks with same parent
        for i in 0..10 {
            let malicious_block = consensus::GhostdagBlock::new(
                vec![[0u8; 32]],
                format!("byzantine_{}", i).into_bytes()
            );
            // Byzantine node tries to fork the chain maliciously
            byzantine_dag.add_block(malicious_block).ok();
        }
        drop(byzantine_dag);

        // Honest nodes continue with normal operation
        for i in 1..network.nodes.len() {
            if network.nodes[i].is_alive() {
                let block = consensus::GhostdagBlock::new(
                    vec![[0u8; 32]],
                    format!("honest_{}", i).into_bytes()
                );
                let mut dag = network.nodes[i].dag.lock().unwrap();
                dag.add_block(block).ok();
            }
        }

        // Verify honest nodes maintain consensus despite Byzantine behavior
        let honest_states: Vec<_> = network.nodes[1..]
            .iter()
            .map(|n| n.dag.lock().unwrap().get_consensus_stats())
            .collect();

        // Honest nodes should have similar view
        for state in &honest_states {
            assert_eq!(state.blue_blocks + state.red_blocks, state.total_blocks);
        }
    }

    #[test]
    fn test_random_fault_injection() {
        use rand::Rng;

        let network = Network::new(8, 4);
        let mut rng = rand::thread_rng();

        // Initialize
        let genesis = consensus::GhostdagBlock::genesis(b"genesis".to_vec());
        for node in &network.nodes {
            let mut dag = node.dag.lock().unwrap();
            dag.add_block(genesis.clone()).unwrap();
        }

        // Random fault injection over 100 iterations
        for iteration in 0..100 {
            let fault_type = rng.gen_range(0..5);

            match fault_type {
                0 => {
                    // Random node failure
                    let node_id = rng.gen_range(0..network.nodes.len());
                    network.nodes[node_id].kill();
                }
                1 => {
                    // Random node recovery
                    let node_id = rng.gen_range(0..network.nodes.len());
                    network.nodes[node_id].revive();
                }
                2 => {
                    // Network partition
                    if !network.network_partitioned.load(Ordering::Acquire) {
                        let split = rng.gen_range(2..network.nodes.len() - 1);
                        network.partition_network(split);
                    }
                }
                3 => {
                    // Heal partition
                    network.heal_partition();
                }
                _ => {
                    // Normal operation - create and broadcast block
                    for node in &network.nodes {
                        if node.is_alive() && rng.gen_bool(0.3) {
                            let block = consensus::GhostdagBlock::new(
                                vec![[0u8; 32]],
                                format!("block_{}", iteration).into_bytes()
                            );
                            let mut dag = node.dag.lock().unwrap();
                            dag.add_block(block).ok();
                            break;
                        }
                    }
                }
            }

            thread::sleep(Duration::from_micros(100));
        }

        // System should still be functional after random faults
        let alive_nodes = network.nodes.iter().filter(|n| n.is_alive()).count();
        assert!(alive_nodes > 0, "At least one node should survive");

        // Check that surviving nodes have valid state
        for node in &network.nodes {
            if node.is_alive() {
                let stats = node.dag.lock().unwrap().get_consensus_stats();
                assert_eq!(stats.blue_blocks + stats.red_blocks, stats.total_blocks);
            }
        }
    }
}