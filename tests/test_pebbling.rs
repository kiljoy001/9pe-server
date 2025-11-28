//! Tests for Pebbling Memory Manager

use ninep_server::pebbling::{ComputationGraph, PebblingManager, PebblingStrategy};
use ninep_server::pebbling::{BlackWhiteGame, BWMove};
use ninep_server::pebbling::{RedBlueGame, RBMove};

#[test]
fn test_linear_chain_memory() {
    let mut graph = ComputationGraph::new();
    graph.add_node(1, vec![], 10, 1);
    graph.add_node(2, vec![1], 10, 1);
    graph.add_node(3, vec![2], 10, 1);
    graph.add_node(4, vec![3], 10, 1);
    graph.set_roots(vec![4]);

    let manager = PebblingManager::new(graph);

    let result = manager.optimize(PebblingStrategy::Greedy);
    assert_eq!(result.peak_memory, 20);
}

#[test]
fn test_diamond_graph_memory() {
    let mut graph = ComputationGraph::new();
    graph.add_node(1, vec![], 10, 1);
    graph.add_node(2, vec![1], 10, 1);
    graph.add_node(3, vec![1], 10, 1);
    graph.add_node(4, vec![2, 3], 10, 1);
    graph.set_roots(vec![4]);

    let manager = PebblingManager::new(graph);

    let result = manager.optimize(PebblingStrategy::Greedy);
    assert!(result.peak_memory <= 30);
}

#[test]
fn test_black_white_rules() {
    let mut graph = ComputationGraph::new();
    graph.add_node(1, vec![], 10, 1);
    graph.add_node(2, vec![1], 10, 1);

    let mut game = BlackWhiteGame::new(&graph);

    // Valid: Place White on 2
    assert!(game.apply_move(BWMove::PlaceWhite(2)).is_ok());

    // Valid: Place Black on 1 (no dependencies)
    assert!(game.apply_move(BWMove::PlaceBlack(1)).is_ok());

    // Valid: Place Black on 2 (1 needs to be pebble, but 1 has pebble now, so should be valid)
    // Wait, 1 has black pebble. 2 depends on 1. So placing black on 2 is valid.
    assert!(game.apply_move(BWMove::PlaceBlack(2)).is_ok());

    // Valid: Remove White on 2 (2 has black, parents have black)
    // Rule 4: Remove white if all parents have pebbles.
    // Parent of 2 is 1. 1 has black. So valid.
    assert!(game.apply_move(BWMove::RemoveWhite(2)).is_ok());

    // Invalid: Remove White on 1 (none there)
    assert!(game.apply_move(BWMove::RemoveWhite(1)).is_err());

    // Valid: Remove Black on 2
    assert!(game.apply_move(BWMove::RemoveBlack(2)).is_ok());
}

#[test]
fn test_red_blue_rules() {
    let mut graph = ComputationGraph::new();
    graph.add_node(1, vec![], 10, 1); // Source
    graph.add_node(2, vec![1], 10, 1); // Dependent

    // Cache size 1
    let mut game = RedBlueGame::new(&graph, 1);

    // Compute 1 (valid, no parents)
    assert!(game.apply_move(RBMove::Compute(1)).is_ok());

    // Store 1 (move to Blue)
    assert!(game.apply_move(RBMove::Store(1)).is_ok());

    // Compute 2 (Invalid, parent 1 not in Red, it is in Blue)
    // Need to free Red 1 first to make space? No, Store copies Red to Blue, Red stays.
    // But cache size is 1. Compute 1 took 1 slot.
    // Store 1 did not remove Red.

    // Try Compute 2 -> Fail (Cache full)
    assert!(game.apply_move(RBMove::Compute(2)).is_err());

    // Free 1 (Remove from Red)
    assert!(game.apply_move(RBMove::Free(1)).is_ok());

    // Try Compute 2 -> Fail (Parent 1 not in Red)
    assert!(game.apply_move(RBMove::Compute(2)).is_err());

    // Load 1 (Blue -> Red)
    assert!(game.apply_move(RBMove::Load(1)).is_ok());

    // Compute 2 -> Fail (Cache full, 1 is in Red)
    assert!(game.apply_move(RBMove::Compute(2)).is_err());
}
