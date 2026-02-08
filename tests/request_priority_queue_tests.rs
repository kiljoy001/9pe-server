//! Tests for Request Priority Queue System (#5)

use std::collections::BinaryHeap;
use std::cmp::Ordering;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Critical = 0,
    High = 1, 
    Normal = 2,
    Low = 3,
}

#[derive(Debug, Clone)]
pub struct PriorityRequest {
    pub id: u64,
    pub priority: Priority,
    pub arrival: Instant,
}

impl PartialEq for PriorityRequest {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for PriorityRequest {}

impl PartialOrd for PriorityRequest {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PriorityRequest {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority.cmp(&other.priority)
            .then_with(|| self.arrival.cmp(&other.arrival))
            .reverse()
    }
}

pub struct RequestPriorityQueue {
    queue: BinaryHeap<PriorityRequest>,
    max_size: usize,
}

impl RequestPriorityQueue {
    pub fn new(max_size: usize) -> Self {
        Self {
            queue: BinaryHeap::new(),
            max_size,
        }
    }
    
    pub fn enqueue(&mut self, req: PriorityRequest) -> Result<(), String> {
        if self.queue.len() >= self.max_size {
            return Err("Queue full".to_string());
        }
        self.queue.push(req);
        Ok(())
    }
    
    pub fn dequeue(&mut self) -> Option<PriorityRequest> {
        self.queue.pop()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_priority_ordering() {
        let mut queue = RequestPriorityQueue::new(10);
        
        let req1 = PriorityRequest {
            id: 1,
            priority: Priority::Low,
            arrival: Instant::now(),
        };
        
        let req2 = PriorityRequest {
            id: 2,
            priority: Priority::Critical,
            arrival: Instant::now(),
        };
        
        queue.enqueue(req1).unwrap();
        queue.enqueue(req2).unwrap();
        
        let first = queue.dequeue().unwrap();
        assert_eq!(first.priority, Priority::Critical);
    }
    
    #[test]
    fn test_queue_overflow() {
        let mut queue = RequestPriorityQueue::new(2);
        
        for i in 0..3 {
            let req = PriorityRequest {
                id: i,
                priority: Priority::Normal,
                arrival: Instant::now(),
            };
            
            if i < 2 {
                assert!(queue.enqueue(req).is_ok());
            } else {
                assert!(queue.enqueue(req).is_err());
            }
        }
    }
}
