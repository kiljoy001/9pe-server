use serde::{Serialize, Deserialize};
use std::collections::HashMap;

/// A node in the computation DAG
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: usize,
    pub dependencies: Vec<usize>, // Inputs required to compute this node
    pub memory_cost: usize,       // Memory required to store this node's output
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

    pub fn get_parents(&self, node_id: usize) -> Option<&Vec<usize>> {
        self.nodes.get(&node_id).map(|n| &n.dependencies)
    }
}
