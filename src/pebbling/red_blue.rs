//! Red-Blue Pebbling Game (Hong & Kung)
//!
//! Models I/O complexity.
//! Red pebbles = Fast Memory (Cache/RAM)
//! Blue pebbles = Slow Memory (Disk)

use std::collections::HashSet;
use super::graph::ComputationGraph;
use anyhow::{Result, anyhow};

#[derive(Debug, Clone)]
pub enum RBMove {
    /// Load: Blue -> Red. Requires Blue pebble on v.
    Load(usize),
    /// Store: Red -> Blue. Requires Red pebble on v.
    Store(usize),
    /// Compute: Place Red on v. Requires Red pebbles on all parents.
    Compute(usize),
    /// Free: Remove Red from v.
    Free(usize),
    /// Delete: Remove Blue from v.
    Delete(usize),
}

pub struct RedBlueGame<'a> {
    graph: &'a ComputationGraph,
    red_pebbles: HashSet<usize>,
    blue_pebbles: HashSet<usize>,
    pub max_red: usize, // Cache size constraint
    pub io_ops: usize, // Count of Loads + Stores
}

impl<'a> RedBlueGame<'a> {
    pub fn new(graph: &'a ComputationGraph, cache_size: usize) -> Self {
        let mut blue_pebbles = HashSet::new();
        // Initially, inputs might be in blue memory (optional, depending on formulation)
        // Or we assume inputs are computed via Compute from nothing (source nodes).
        // Standard Hong-Kung: Input nodes have Blue pebbles initially?
        // Or we can Compute inputs from "nothing" into Red.
        // Let's assume standard Compute rule handles sources (0 dependencies).

        Self {
            graph,
            red_pebbles: HashSet::new(),
            blue_pebbles,
            max_red: cache_size,
            io_ops: 0,
        }
    }

    // Setup inputs in Blue memory if needed
    pub fn set_initial_blue(&mut self, nodes: Vec<usize>) {
        for n in nodes {
            self.blue_pebbles.insert(n);
        }
    }

    pub fn apply_move(&mut self, mv: RBMove) -> Result<()> {
        match mv {
            RBMove::Load(node) => {
                if !self.blue_pebbles.contains(&node) {
                    return Err(anyhow!("Cannot Load {}: not in Blue memory", node));
                }
                if self.red_pebbles.len() >= self.max_red && !self.red_pebbles.contains(&node) {
                     return Err(anyhow!("Cache full ({}): cannot Load {}", self.max_red, node));
                }
                self.red_pebbles.insert(node);
                self.io_ops += 1;
            },
            RBMove::Store(node) => {
                if !self.red_pebbles.contains(&node) {
                    return Err(anyhow!("Cannot Store {}: not in Red memory", node));
                }
                self.blue_pebbles.insert(node);
                self.io_ops += 1;
            },
            RBMove::Compute(node) => {
                if self.red_pebbles.len() >= self.max_red && !self.red_pebbles.contains(&node) {
                     return Err(anyhow!("Cache full ({}): cannot Compute {}", self.max_red, node));
                }

                if let Some(parents) = self.graph.get_parents(node) {
                    for &parent in parents {
                        if !self.red_pebbles.contains(&parent) {
                            return Err(anyhow!("Cannot Compute {}: parent {} not in Red", node, parent));
                        }
                    }
                } else {
                     return Err(anyhow!("Node {} not in graph", node));
                }
                self.red_pebbles.insert(node);
            },
            RBMove::Free(node) => {
                if !self.red_pebbles.remove(&node) {
                    return Err(anyhow!("Cannot Free {}: not in Red", node));
                }
            },
            RBMove::Delete(node) => {
                if !self.blue_pebbles.remove(&node) {
                    return Err(anyhow!("Cannot Delete {}: not in Blue", node));
                }
            }
        }
        Ok(())
    }
}
