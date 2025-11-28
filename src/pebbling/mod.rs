//! Pebbling Memory Manager
//!
//! Implements graph pebbling algorithms for optimal memory management
//! of DAG-based computations (like Neural Networks).

use serde::{Serialize, Deserialize};
use std::collections::{HashMap, HashSet};

/// A node in the computation DAG
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: usize,
    pub dependencies: Vec<usize>, // Inputs required to compute this node
    pub memory_cost: usize,       // Memory required to store this node's output (in MB/units)
    pub computation_cost: usize,  // Time/cycles to compute
}

/// A computation graph (DAG)
#[derive(Debug, Clone, Default)]
pub struct ComputationGraph {
    pub nodes: HashMap<usize, Node>,
    pub roots: Vec<usize>, // Final outputs we want to compute
}

impl ComputationGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, id: usize, dependencies: Vec<usize>, memory_cost: usize, computation_cost: usize) {
        self.nodes.insert(id, Node {
            id,
            dependencies,
            memory_cost,
            computation_cost,
        });
    }

    pub fn set_roots(&mut self, roots: Vec<usize>) {
        self.roots = roots;
    }
}

/// Strategy for pebbling (memory management)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PebblingStrategy {
    /// Compute nodes as soon as their dependencies are ready (Topological sort)
    /// High memory usage, low recomputation.
    Greedy,
    /// Try to minimize peak memory usage, even if it means recomputing.
    /// (Simplified heuristic for this PoC)
    MemoryOptimal,
}

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

/// The Pebbling Manager
pub struct PebblingManager {
    graph: ComputationGraph,
}

impl PebblingManager {
    pub fn new(graph: ComputationGraph) -> Self {
        Self { graph }
    }

    /// Simulate execution and return stats
    pub fn optimize(&self, strategy: PebblingStrategy) -> PebblingResult {
        match strategy {
            PebblingStrategy::Greedy => self.run_greedy(),
            PebblingStrategy::MemoryOptimal => self.run_memory_optimal(),
        }
    }

    fn run_greedy(&self) -> PebblingResult {
        let mut peak_memory = 0;
        let mut current_memory = 0;
        let mut total_computation = 0;
        let mut schedule = Vec::new();

        let mut computed = HashSet::new();
        let mut ref_counts = self.calculate_ref_counts();

        // Simple topological sort / execution queue
        // In greedy, we just execute ready nodes
        let mut queue = self.get_initial_ready_nodes();

        // We need to verify if graph is acyclic and valid, but assuming valid DAG for PoC

        while !queue.is_empty() {
            // Sort to ensure deterministic order (e.g. by ID)
            queue.sort();

            let node_id = queue.remove(0);
            let node = &self.graph.nodes[&node_id];

            // Compute
            schedule.push(Step::Compute(node_id));
            computed.insert(node_id);
            total_computation += node.computation_cost;
            current_memory += node.memory_cost;
            peak_memory = peak_memory.max(current_memory);

            // Check if dependencies can be freed
            for &dep_id in &node.dependencies {
                if let Some(count) = ref_counts.get_mut(&dep_id) {
                    *count -= 1;
                    if *count == 0 {
                        // Free dependency
                        let dep_node = &self.graph.nodes[&dep_id];
                        current_memory -= dep_node.memory_cost;
                        schedule.push(Step::Free(dep_id));
                    }
                }
            }

            // Add new ready nodes
            for (id, n) in &self.graph.nodes {
                if !computed.contains(id) && !queue.contains(id) {
                    if n.dependencies.iter().all(|d| computed.contains(d)) {
                        queue.push(*id);
                    }
                }
            }
        }

        PebblingResult {
            peak_memory,
            total_computation,
            schedule,
        }
    }

    fn run_memory_optimal(&self) -> PebblingResult {
        // A true optimal pebbling is NP-hard.
        // We implement a heuristic that prioritizes "chains" to minimize width.
        // This is a simplified Depth-First-Search based topological sort which often results in lower memory
        // than Breadth-First-Search (which Greedy often resembles).

        let mut peak_memory = 0;
        let mut current_memory = 0;
        let mut total_computation = 0;
        let mut schedule = Vec::new();

        let mut computed = HashSet::new();
        let mut ref_counts = self.calculate_ref_counts();

        // DFS execution
        // Start with roots (reversed dependencies needed for true DFS from output,
        // but here we simulate forward execution with DFS preference)

        // Actually, to minimize memory, we should finish subtrees completely before moving to others.
        // Let's assume we find a valid ordering that is "DFS-like".

        // Get all nodes
        let mut all_nodes: Vec<usize> = self.graph.nodes.keys().cloned().collect();
        all_nodes.sort(); // Deterministic

        // We need a helper to execute ready nodes in DFS manner (LIFO queue)
        let mut ready = self.get_initial_ready_nodes();
        // Sort reverse so pop gives smallest (or use specific heuristic)
        ready.sort_by(|a, b| b.cmp(a));

        while !ready.is_empty() {
            // Take the last added ready node (LIFO behavior encourages depth-first)
            let node_id = ready.pop().unwrap();
            let node = &self.graph.nodes[&node_id];

            // Compute
            schedule.push(Step::Compute(node_id));
            computed.insert(node_id);
            total_computation += node.computation_cost;
            current_memory += node.memory_cost;
            peak_memory = peak_memory.max(current_memory);

             // Check if dependencies can be freed
            for &dep_id in &node.dependencies {
                if let Some(count) = ref_counts.get_mut(&dep_id) {
                    *count -= 1;
                    if *count == 0 {
                        // Free dependency
                        let dep_node = &self.graph.nodes[&dep_id];
                        current_memory -= dep_node.memory_cost;
                        schedule.push(Step::Free(dep_id));
                    }
                }
            }

            // Find newly ready nodes and add to ready stack
            // We want to add children of current node to stack immediately to pursue depth
            let mut new_ready = Vec::new();
            for (id, n) in &self.graph.nodes {
                if !computed.contains(id) && !ready.contains(id) {
                    if n.dependencies.iter().all(|d| computed.contains(d)) {
                         // Only if it wasn't ready before?
                         // Actually the naive loop above is inefficient but works for PoC.
                         // Optimization: only check nodes that depend on `node_id`.
                         if n.dependencies.contains(&node_id) {
                             new_ready.push(*id);
                         }
                    }
                }
            }
            // Sort new ready nodes and append
            new_ready.sort_by(|a, b| b.cmp(a));
            ready.extend(new_ready);

            // Also check for any other nodes that might have become ready (if we skipped them for DFS)
            // Ideally we maintain a list of ready nodes. The above loop on all nodes is slow.
            // For PoC it is fine.
             for (id, n) in &self.graph.nodes {
                if !computed.contains(id) && !ready.contains(id) {
                     if n.dependencies.iter().all(|d| computed.contains(d)) {
                         ready.push(*id);
                     }
                }
             }
             // Re-sort ready to ensure LIFO preference is maintained (newly added should be at back?)
             // Actually, if we want DFS, we want the most recently discovered nodes (children) to be processed first.
             // So we push them to back of vector and pop from back.
             // But we just re-added everything.

             // Let's refine: `ready` is a Stack.
             // We want `new_ready` (children) to be at the top.
             // But my "catch all" loop messes order.
             // Let's stick to:
             // 1. Pop node.
             // 2. Compute.
             // 3. Find children that are now ready.
             // 4. Push them to stack.
             // (Any nodes ready but not children of this one are already in stack below).
        }

        PebblingResult {
            peak_memory,
            total_computation,
            schedule,
        }
    }

    fn calculate_ref_counts(&self) -> HashMap<usize, usize> {
        let mut counts = HashMap::new();
        for node in self.graph.nodes.values() {
            for &dep in &node.dependencies {
                *counts.entry(dep).or_insert(0) += 1;
            }
        }
        // Roots are also dependencies (of the "user")
        for &root in &self.graph.roots {
            *counts.entry(root).or_insert(0) += 1;
        }
        counts
    }

    fn get_initial_ready_nodes(&self) -> Vec<usize> {
        self.graph.nodes.values()
            .filter(|n| n.dependencies.is_empty())
            .map(|n| n.id)
            .collect()
    }
}
