//! Pebbling Memory Manager
//!
//! Implements graph pebbling algorithms including:
//! - Black-White Pebbling (Space Complexity)
//! - Red-Blue Pebbling (I/O Complexity)

pub mod graph;
pub mod black_white;
pub mod red_blue;

pub use graph::ComputationGraph;
pub use black_white::{BlackWhiteGame, BWMove};
pub use red_blue::{RedBlueGame, RBMove};

// Legacy manager for backward compatibility with previous test structure
// (Optional: we can rewrite tests to use new structure)

use crate::pebbling::graph::ComputationGraph as Graph;
use crate::pebbling::black_white::BlackWhiteGame as BWGame;

/// Result of a pebbling schedule simulation
#[derive(Debug, Clone)]
pub struct PebblingResult {
    pub peak_memory: usize,
    pub total_computation: usize,
    pub schedule: Vec<Step>,
}

#[derive(Debug, Clone)]
pub enum Step {
    Compute(usize),
    Free(usize),
}

/// Strategy for pebbling (memory management)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PebblingStrategy {
    Greedy,
    MemoryOptimal,
}

/// The Pebbling Manager (Wrapper around specific games for PoC)
pub struct PebblingManager {
    graph: Graph,
}

impl PebblingManager {
    pub fn new(graph: Graph) -> Self {
        Self { graph }
    }

    pub fn optimize(&self, strategy: PebblingStrategy) -> PebblingResult {
        // We use Black-White game to simulate standard execution.
        // Step::Compute -> PlaceBlack
        // Step::Free -> RemoveBlack
        // Note: Our previous Greedy/Optimal implementations were effectively Black pebbling.

        // Since we refactored, let's reimplement the simple strategies using the new Game struct
        // to verify it works, but output the simple Result format expected by existing tests.

        let mut game = BWGame::new(&self.graph);
        let mut schedule_steps = Vec::new();
        let mut computation_cost = 0;

        match strategy {
            PebblingStrategy::Greedy => {
                 // Re-implement greedy using game moves
                 let mut ref_counts = std::collections::HashMap::new();
                 for node in self.graph.nodes.values() {
                    for &dep in &node.dependencies {
                        *ref_counts.entry(dep).or_insert(0) += 1;
                    }
                    for &root in &self.graph.roots {
                        *ref_counts.entry(root).or_insert(0) += 1;
                    }
                 }

                 let mut queue: Vec<usize> = self.graph.nodes.values()
                    .filter(|n| n.dependencies.is_empty())
                    .map(|n| n.id)
                    .collect();
                 let mut computed = std::collections::HashSet::new();

                 while !queue.is_empty() {
                     queue.sort();
                     let node_id = queue.remove(0);
                     let node = &self.graph.nodes[&node_id];

                     if game.apply_move(BWMove::PlaceBlack(node_id)).is_ok() {
                         schedule_steps.push(Step::Compute(node_id));
                         computation_cost += node.computation_cost;
                         computed.insert(node_id);

                         // Check frees
                         for &dep in &node.dependencies {
                             if let Some(c) = ref_counts.get_mut(&dep) {
                                 *c -= 1;
                                 if *c == 0 {
                                     let _ = game.apply_move(BWMove::RemoveBlack(dep));
                                     schedule_steps.push(Step::Free(dep));
                                 }
                             }
                         }

                         // Add ready
                         for (id, n) in &self.graph.nodes {
                            if !computed.contains(id) && !queue.contains(id) {
                                if n.dependencies.iter().all(|d| computed.contains(d)) {
                                    queue.push(*id);
                                }
                            }
                        }
                     }
                 }
            },
            PebblingStrategy::MemoryOptimal => {
                // Simplified DFS for memory optimal
                let mut ref_counts = std::collections::HashMap::new();
                 for node in self.graph.nodes.values() {
                    for &dep in &node.dependencies {
                        *ref_counts.entry(dep).or_insert(0) += 1;
                    }
                    for &root in &self.graph.roots {
                        *ref_counts.entry(root).or_insert(0) += 1;
                    }
                 }

                let mut ready: Vec<usize> = self.graph.nodes.values()
                    .filter(|n| n.dependencies.is_empty())
                    .map(|n| n.id)
                    .collect();
                ready.sort_by(|a, b| b.cmp(a)); // Reverse for pop

                let mut computed = std::collections::HashSet::new();

                while !ready.is_empty() {
                    let node_id = ready.pop().unwrap();
                    let node = &self.graph.nodes[&node_id];

                    if game.apply_move(BWMove::PlaceBlack(node_id)).is_ok() {
                         schedule_steps.push(Step::Compute(node_id));
                         computation_cost += node.computation_cost;
                         computed.insert(node_id);

                         // Check frees
                         for &dep in &node.dependencies {
                             if let Some(c) = ref_counts.get_mut(&dep) {
                                 *c -= 1;
                                 if *c == 0 {
                                     let _ = game.apply_move(BWMove::RemoveBlack(dep));
                                     schedule_steps.push(Step::Free(dep));
                                 }
                             }
                         }

                         // Add ready children first
                         let mut children = Vec::new();
                         for (id, n) in &self.graph.nodes {
                            if !computed.contains(id) && !ready.contains(id) && n.dependencies.contains(&node_id) {
                                if n.dependencies.iter().all(|d| computed.contains(d)) {
                                    children.push(*id);
                                }
                            }
                         }
                         children.sort_by(|a, b| b.cmp(a));
                         ready.extend(children);

                         // Add other ready
                         for (id, n) in &self.graph.nodes {
                            if !computed.contains(id) && !ready.contains(id) {
                                if n.dependencies.iter().all(|d| computed.contains(d)) {
                                    ready.push(*id);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Scale peak_pebbles to memory size (assuming constant node size for now or we map it)
        // The game tracks pebble count, not size. But for PoC let's assume 10MB per node as in tests.
        // To be accurate, we should use game state history to find max memory.
        // But the game tracks "max_pebbles".
        // Let's just return the value derived from our schedule to match old tests,
        // effectively validating that the new structure supports the old logic.

        let mut current_mem = 0;
        let mut max_mem = 0;
        for step in &schedule_steps {
            match step {
                Step::Compute(id) => {
                    current_mem += self.graph.nodes[id].memory_cost;
                    max_mem = max_mem.max(current_mem);
                },
                Step::Free(id) => {
                    current_mem -= self.graph.nodes[id].memory_cost;
                }
            }
        }

        PebblingResult {
            peak_memory: max_mem,
            total_computation: computation_cost,
            schedule: schedule_steps,
        }
    }
}
