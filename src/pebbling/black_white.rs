//! Black-White Pebbling Game
//!
//! Models space complexity using Black (computed) and White (assumption) pebbles.
//! Rules based on Cook & Sethi (1976).

use std::collections::{HashSet, HashMap};
use super::graph::ComputationGraph;
use anyhow::{Result, anyhow};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PebbleType {
    Black,
    White,
}

#[derive(Debug, Clone)]
pub enum BWMove {
    /// Place a black pebble on v. Requires all parents to have pebbles.
    PlaceBlack(usize),
    /// Remove a black pebble from v. Always allowed if present.
    RemoveBlack(usize),
    /// Place a white pebble on v. Always allowed (start of assumption).
    PlaceWhite(usize),
    /// Remove a white pebble from v. Requires all parents to have pebbles (end of assumption).
    RemoveWhite(usize),
}

pub struct BlackWhiteGame<'a> {
    graph: &'a ComputationGraph,
    /// Which nodes have which pebbles. A node can theoretically have both,
    /// but usually one is sufficient for the game state.
    /// We'll track set of pebbles on each node.
    pebbles: HashMap<usize, HashSet<PebbleType>>,
    pub max_pebbles: usize,
    pub history: Vec<BWMove>,
}

impl<'a> BlackWhiteGame<'a> {
    pub fn new(graph: &'a ComputationGraph) -> Self {
        Self {
            graph,
            pebbles: HashMap::new(),
            max_pebbles: 0,
            history: Vec::new(),
        }
    }

    pub fn current_pebbles(&self) -> usize {
        self.pebbles.values().map(|p| p.len()).sum()
    }

    pub fn has_pebble(&self, node_id: usize) -> bool {
        if let Some(p) = self.pebbles.get(&node_id) {
            !p.is_empty()
        } else {
            false
        }
    }

    pub fn apply_move(&mut self, mv: BWMove) -> Result<()> {
        match mv {
            BWMove::PlaceBlack(node) => {
                // Rule 1: Can place black if all parents have pebbles
                if let Some(parents) = self.graph.get_parents(node) {
                    for &parent in parents {
                        if !self.has_pebble(parent) {
                            return Err(anyhow!("Cannot place black on {}: parent {} has no pebble", node, parent));
                        }
                    }
                } else {
                    return Err(anyhow!("Node {} not in graph", node));
                }
                self.pebbles.entry(node).or_default().insert(PebbleType::Black);
            },
            BWMove::RemoveBlack(node) => {
                // Rule 2: Can remove black anytime
                if let Some(p) = self.pebbles.get_mut(&node) {
                    if !p.remove(&PebbleType::Black) {
                         return Err(anyhow!("No black pebble on {}", node));
                    }
                } else {
                    return Err(anyhow!("No pebbles on {}", node));
                }
            },
            BWMove::PlaceWhite(node) => {
                // Rule 3: Can place white anytime
                if !self.graph.nodes.contains_key(&node) {
                     return Err(anyhow!("Node {} not in graph", node));
                }
                self.pebbles.entry(node).or_default().insert(PebbleType::White);
            },
            BWMove::RemoveWhite(node) => {
                // Rule 4: Can remove white if all parents have pebbles
                if let Some(p) = self.pebbles.get_mut(&node) {
                    if !p.contains(&PebbleType::White) {
                        return Err(anyhow!("No white pebble on {}", node));
                    }
                } else {
                    return Err(anyhow!("No pebbles on {}", node));
                }

                if let Some(parents) = self.graph.get_parents(node) {
                    for &parent in parents {
                        if !self.has_pebble(parent) {
                            return Err(anyhow!("Cannot remove white from {}: parent {} has no pebble", node, parent));
                        }
                    }
                }
                self.pebbles.entry(node).or_default().remove(&PebbleType::White);
            },
        }

        self.max_pebbles = self.max_pebbles.max(self.current_pebbles());
        self.history.push(mv);
        Ok(())
    }
}
