//! Tests for Pebbling Memory Manager

use ninep_server::pebbling::{ComputationGraph, PebblingManager, PebblingStrategy};

#[test]
fn test_linear_chain_memory() {
    // A -> B -> C -> D
    // Each node 10MB
    // Traditional/Greedy: A(10) -> B(10+10) -> free A -> C(10+10) -> free B -> D(10+10)
    // Peak memory: 20MB

    let mut graph = ComputationGraph::new();
    graph.add_node(1, vec![], 10, 1);
    graph.add_node(2, vec![1], 10, 1);
    graph.add_node(3, vec![2], 10, 1);
    graph.add_node(4, vec![3], 10, 1);
    graph.set_roots(vec![4]);

    let manager = PebblingManager::new(graph);

    let result = manager.optimize(PebblingStrategy::Greedy);
    println!("Linear Chain Result: {:?}", result);

    assert_eq!(result.peak_memory, 20); // Stores output of N and N-1
}

#[test]
fn test_diamond_graph_memory() {
    //   A
    //  / \
    // B   C
    //  \ /
    //   D
    // All nodes 10MB.
    // Order A -> B -> C -> D
    // 1. A (10)
    // 2. B (10+10=20) (A needed for C)
    // 3. C (20+10=30) (A can be freed after this? Yes) -> 20 (A freed) -> 30 (C added)
    //    Actually: Mem = A(10) + B(10) + C(10) = 30 peak.
    // 4. D (B+C+D) -> B+C needed.

    let mut graph = ComputationGraph::new();
    graph.add_node(1, vec![], 10, 1);     // A
    graph.add_node(2, vec![1], 10, 1);    // B
    graph.add_node(3, vec![1], 10, 1);    // C
    graph.add_node(4, vec![2, 3], 10, 1); // D
    graph.set_roots(vec![4]);

    let manager = PebblingManager::new(graph);

    let result = manager.optimize(PebblingStrategy::Greedy);
    println!("Diamond Graph Result: {:?}", result);

    // Check reasonable bounds
    assert!(result.peak_memory <= 30);
}

#[test]
fn test_tree_reduction() {
    // Binary Tree structure to test DFS vs BFS (Greedy)
    //        R
    //      /   \
    //     A     B
    //    / \   / \
    //   1   2 3   4

    // Greedy (BFS-like): 1, 2, 3, 4, A, B, R
    // Mem at A: needs 1,2 outputs.
    // If BFS: computes 1,2,3,4 (40MB) then A (needs 1,2), then B (needs 3,4).
    // Peak might be high if it keeps all leaves.

    // Optimal (DFS-like): 1, 2, A (free 1,2), 3, 4, B (free 3,4), R (free A,B)
    // Peak: 1(10) -> 2(20) -> A(10+10+10=30) -> free 1,2 -> A(10)
    //       -> 3(20) -> 4(30) -> B(30) -> free 3,4 -> A(10)+B(10)=20
    //       -> R(30) -> free A,B -> R(10).
    // Peak 30.

    let mut graph = ComputationGraph::new();
    // Leaves
    graph.add_node(1, vec![], 10, 1);
    graph.add_node(2, vec![], 10, 1);
    graph.add_node(3, vec![], 10, 1);
    graph.add_node(4, vec![], 10, 1);

    // Intermediates
    graph.add_node(5, vec![1, 2], 10, 1); // A
    graph.add_node(6, vec![3, 4], 10, 1); // B

    // Root
    graph.add_node(7, vec![5, 6], 10, 1); // R
    graph.set_roots(vec![7]);

    let manager = PebblingManager::new(graph);

    let greedy = manager.optimize(PebblingStrategy::Greedy);
    let optimal = manager.optimize(PebblingStrategy::MemoryOptimal);

    println!("Tree Greedy Peak: {}", greedy.peak_memory);
    println!("Tree Optimal Peak: {}", optimal.peak_memory);

    // In this specific small case, they might be similar depending on tie-breaking,
    // but Optimal should be <= Greedy.
    assert!(optimal.peak_memory <= greedy.peak_memory);
}
