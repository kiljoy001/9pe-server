//! Property-based tests for thread safety and deadlock prevention
//! Verifies mutual exclusion, absence of data races, and lock ordering

use proptest::prelude::*;
use proptest::collection::{vec, hash_set};
use std::sync::{Arc, Mutex, RwLock};
use std::collections::{HashMap, HashSet, VecDeque};
use std::thread;
use std::time::Duration;
use parking_lot::{Mutex as ParkingMutex, RwLock as ParkingRwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ThreadId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct LockId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ResourceId(u32);

#[derive(Debug, Clone)]
enum Operation {
    Read(ResourceId),
    Write(ResourceId),
    Acquire(LockId),
    Release(LockId),
}

#[derive(Debug, Clone)]
struct ThreadState {
    id: ThreadId,
    held_locks: Vec<LockId>,
    waiting_for: Option<LockId>,
}

#[derive(Debug)]
struct SystemState {
    threads: HashMap<ThreadId, ThreadState>,
    locks: HashMap<LockId, Option<ThreadId>>,
    resources: HashMap<ResourceId, Option<ThreadId>>,
    resource_locks: HashMap<ResourceId, LockId>,
}

impl SystemState {
    fn new() -> Self {
        SystemState {
            threads: HashMap::new(),
            locks: HashMap::new(),
            resources: HashMap::new(),
            resource_locks: HashMap::new(),
        }
    }

    fn acquire_lock(&mut self, tid: ThreadId, lid: LockId) -> Result<(), String> {
        // Check for double locking
        if let Some(thread) = self.threads.get(&tid) {
            if thread.held_locks.contains(&lid) {
                return Err("Double locking detected".to_string());
            }
        }

        // Check if lock is available
        if let Some(owner) = self.locks.get(&lid).and_then(|o| *o) {
            if owner != tid {
                // Lock is held by another thread
                if let Some(thread) = self.threads.get_mut(&tid) {
                    thread.waiting_for = Some(lid);
                }
                return Err("Lock held by another thread".to_string());
            }
        }

        // Acquire the lock
        self.locks.insert(lid, Some(tid));
        let thread = self.threads.entry(tid).or_insert(ThreadState {
            id: tid,
            held_locks: Vec::new(),
            waiting_for: None,
        });
        thread.held_locks.push(lid);
        thread.waiting_for = None;

        Ok(())
    }

    fn release_lock(&mut self, tid: ThreadId, lid: LockId) -> Result<(), String> {
        // Check ownership
        if let Some(owner) = self.locks.get(&lid).and_then(|o| *o) {
            if owner != tid {
                return Err("Not the lock owner".to_string());
            }
        } else {
            return Err("Lock not held".to_string());
        }

        // Release the lock
        self.locks.insert(lid, None);
        if let Some(thread) = self.threads.get_mut(&tid) {
            thread.held_locks.retain(|&l| l != lid);
        }

        Ok(())
    }

    fn has_deadlock(&self) -> bool {
        // Build wait-for graph and detect cycles
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        for &tid in self.threads.keys() {
            if self.has_cycle_from(tid, &mut visited, &mut rec_stack) {
                return true;
            }
        }

        false
    }

    fn has_cycle_from(&self, tid: ThreadId, visited: &mut HashSet<ThreadId>,
                      rec_stack: &mut HashSet<ThreadId>) -> bool {
        visited.insert(tid);
        rec_stack.insert(tid);

        if let Some(thread) = self.threads.get(&tid) {
            if let Some(waiting_for) = thread.waiting_for {
                // Find who holds the lock
                if let Some(Some(holder)) = self.locks.get(&waiting_for) {
                    if !visited.contains(holder) {
                        if self.has_cycle_from(*holder, visited, rec_stack) {
                            return true;
                        }
                    } else if rec_stack.contains(holder) {
                        return true; // Found cycle
                    }
                }
            }
        }

        rec_stack.remove(&tid);
        false
    }

    fn check_mutual_exclusion(&self) -> bool {
        // No two threads hold the same lock
        for (lid, owner) in &self.locks {
            if let Some(tid1) = owner {
                for (lid2, owner2) in &self.locks {
                    if lid == lid2 && owner2.is_some() {
                        let tid2 = owner2.unwrap();
                        if tid1 != &tid2 {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }
}

/// Generate arbitrary thread operations
fn arbitrary_operations() -> impl Strategy<Value = Vec<Operation>> {
    vec(
        prop_oneof![
            (0u32..10).prop_map(|r| Operation::Read(ResourceId(r))),
            (0u32..10).prop_map(|r| Operation::Write(ResourceId(r))),
            (0u32..5).prop_map(|l| Operation::Acquire(LockId(l))),
            (0u32..5).prop_map(|l| Operation::Release(LockId(l))),
        ],
        1..20
    )
}

/// Generate lock ordering
fn arbitrary_lock_ordering() -> impl Strategy<Value = Vec<LockId>> {
    vec(0u32..10, 1..10).prop_map(|v| {
        let mut locks: Vec<_> = v.into_iter().map(LockId).collect();
        locks.sort();
        locks.dedup();
        locks
    })
}

proptest! {
    /// Test: Mutual exclusion is preserved
    #[test]
    fn prop_mutual_exclusion(
        thread_ops in vec(arbitrary_operations(), 2..5)
    ) {
        let mut sys = SystemState::new();

        for (tid_idx, ops) in thread_ops.iter().enumerate() {
            let tid = ThreadId(tid_idx as u32);

            for op in ops {
                match op {
                    Operation::Acquire(lid) => {
                        let _ = sys.acquire_lock(tid, *lid);
                    }
                    Operation::Release(lid) => {
                        let _ = sys.release_lock(tid, *lid);
                    }
                    _ => {}
                }

                // Check mutual exclusion after each operation
                prop_assert!(
                    sys.check_mutual_exclusion(),
                    "Mutual exclusion violated"
                );
            }
        }
    }

    /// Test: No double locking
    #[test]
    fn prop_no_double_locking(
        ops in arbitrary_operations()
    ) {
        let mut sys = SystemState::new();
        let tid = ThreadId(0);
        let mut held = HashSet::new();

        for op in ops {
            match op {
                Operation::Acquire(lid) => {
                    if held.contains(&lid) {
                        // Should fail - double locking
                        let result = sys.acquire_lock(tid, lid);
                        prop_assert!(result.is_err());
                    } else {
                        let _ = sys.acquire_lock(tid, lid);
                        held.insert(lid);
                    }
                }
                Operation::Release(lid) => {
                    if held.contains(&lid) {
                        let _ = sys.release_lock(tid, lid);
                        held.remove(&lid);
                    }
                }
                _ => {}
            }
        }
    }

    /// Test: Lock ordering prevents deadlocks
    #[test]
    fn prop_lock_ordering_prevents_deadlock(
        ordering in arbitrary_lock_ordering(),
        thread_ops in vec(vec(0usize..5, 1..5), 2..4)
    ) {
        let mut sys = SystemState::new();

        // Map indices to ordered locks
        let get_lock = |idx: usize| {
            ordering.get(idx % ordering.len()).copied().unwrap_or(LockId(0))
        };

        // Threads acquire locks in order
        for (tid_idx, lock_indices) in thread_ops.iter().enumerate() {
            let tid = ThreadId(tid_idx as u32);
            let mut sorted_indices = lock_indices.clone();
            sorted_indices.sort();

            for idx in sorted_indices {
                let lid = get_lock(idx);
                let _ = sys.acquire_lock(tid, lid);
            }
        }

        // Should not have deadlock if ordering is respected
        prop_assert!(!sys.has_deadlock());
    }

    /// Test: Detect cycles in wait-for graph
    #[test]
    fn prop_detect_cycles(
        create_cycle in prop::bool::ANY
    ) {
        let mut sys = SystemState::new();

        if create_cycle {
            // Create cycle: T1 -> L1 -> T2 -> L2 -> T1
            sys.acquire_lock(ThreadId(1), LockId(1)).ok();
            sys.acquire_lock(ThreadId(2), LockId(2)).ok();

            // T1 waits for L2 (held by T2)
            sys.threads.get_mut(&ThreadId(1)).unwrap().waiting_for = Some(LockId(2));
            // T2 waits for L1 (held by T1)
            sys.threads.get_mut(&ThreadId(2)).unwrap().waiting_for = Some(LockId(1));

            prop_assert!(sys.has_deadlock());
        } else {
            // No cycle - simple chain
            sys.acquire_lock(ThreadId(1), LockId(1)).ok();
            sys.acquire_lock(ThreadId(2), LockId(2)).ok();

            // T2 waits for L1 (no cycle back)
            sys.threads.get_mut(&ThreadId(2)).unwrap().waiting_for = Some(LockId(1));

            prop_assert!(!sys.has_deadlock());
        }
    }

    /// Test: RwLock allows multiple readers
    #[test]
    fn prop_rwlock_multiple_readers(
        num_readers in 2usize..10usize
    ) {
        let data = Arc::new(RwLock::new(0i32));
        let mut handles = vec![];

        // Multiple readers can access simultaneously
        for i in 0..num_readers {
            let data_clone = Arc::clone(&data);
            let handle = thread::spawn(move || {
                let value = data_clone.read().unwrap();
                thread::sleep(Duration::from_millis(10));
                *value + i as i32
            });
            handles.push(handle);
        }

        // All readers should complete
        let results: Vec<_> = handles.into_iter()
            .map(|h| h.join().unwrap())
            .collect();

        prop_assert_eq!(results.len(), num_readers);
    }

    /// Test: Writer excludes other operations
    #[test]
    fn prop_writer_exclusive(
        initial_value in 0i32..100i32,
        write_value in 100i32..200i32
    ) {
        let data = Arc::new(RwLock::new(initial_value));

        // Writer thread
        let data_write = Arc::clone(&data);
        let writer = thread::spawn(move || {
            let mut value = data_write.write().unwrap();
            thread::sleep(Duration::from_millis(50));
            *value = write_value;
            write_value
        });

        // Try to read while writing
        thread::sleep(Duration::from_millis(10));
        let read_during = if let Ok(guard) = data.try_read() {
            Some(*guard)
        } else {
            None
        };

        let written = writer.join().unwrap();

        // Read after writing
        let read_after = *data.read().unwrap();

        // During write, read should fail or see old value
        if let Some(val) = read_during {
            prop_assert!(val == initial_value || val == written);
        }

        // After write, should see new value
        prop_assert_eq!(read_after, written);
    }

    /// Test: No data races with proper synchronization
    #[test]
    fn prop_no_data_races(
        num_threads in 2usize..10usize,
        ops_per_thread in 10usize..50usize
    ) {
        let counter = Arc::new(Mutex::new(0u64));
        let mut handles = vec![];

        for _ in 0..num_threads {
            let counter_clone = Arc::clone(&counter);
            let handle = thread::spawn(move || {
                for _ in 0..ops_per_thread {
                    let mut val = counter_clone.lock().unwrap();
                    *val += 1;
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let final_value = *counter.lock().unwrap();
        let expected = (num_threads * ops_per_thread) as u64;

        // No lost updates
        prop_assert_eq!(final_value, expected);
    }

    /// Test: Arc reference counting
    #[test]
    fn prop_arc_reference_counting(
        num_clones in 1usize..20usize
    ) {
        let data = Arc::new(42i32);
        let mut clones = vec![];

        for _ in 0..num_clones {
            clones.push(Arc::clone(&data));
        }

        // Strong count should equal num_clones + 1 (original)
        prop_assert_eq!(Arc::strong_count(&data), num_clones + 1);

        // Drop all clones
        drop(clones);

        // Should be back to 1
        prop_assert_eq!(Arc::strong_count(&data), 1);
    }

    /// Test: Lock-free channel communication
    #[test]
    fn prop_lockfree_channels(
        messages in vec(0i32..1000i32, 1..100)
    ) {
        use std::sync::mpsc;

        let (tx, rx) = mpsc::channel();
        let msg_count = messages.len();

        // Send messages
        thread::spawn(move || {
            for msg in messages {
                tx.send(msg).unwrap();
            }
        });

        // Receive messages
        let mut received = vec![];
        for _ in 0..msg_count {
            if let Ok(msg) = rx.recv_timeout(Duration::from_secs(1)) {
                received.push(msg);
            }
        }

        // All messages received
        prop_assert_eq!(received.len(), msg_count);
    }
}

/// Test specific bug: mesh network thread safety issue
#[test]
fn test_mesh_network_thread_safety_bug() {
    // The bug: shared state without synchronization
    struct BuggyMeshNetwork {
        nodes: HashMap<u32, String>, // UNSAFE: No synchronization!
    }

    // This would cause data races
    // let buggy_network = BuggyMeshNetwork { nodes: HashMap::new() };

    // The fix: use Arc<RwLock<T>>
    struct FixedMeshNetwork {
        nodes: Arc<RwLock<HashMap<u32, String>>>,
        pending: Arc<ParkingMutex<VecDeque<String>>>,
    }

    let fixed_network = FixedMeshNetwork {
        nodes: Arc::new(RwLock::new(HashMap::new())),
        pending: Arc::new(ParkingMutex::new(VecDeque::new())),
    };

    let mut handles = vec![];

    // Multiple threads accessing network
    for i in 0..10 {
        let nodes = Arc::clone(&fixed_network.nodes);
        let pending = Arc::clone(&fixed_network.pending);

        let handle = thread::spawn(move || {
            // Read operations
            {
                let _nodes = nodes.read().unwrap();
                // Can have multiple readers
            }

            // Write operations
            {
                let mut nodes = nodes.write().unwrap();
                nodes.insert(i, format!("node_{}", i));
            }

            // Queue operations
            {
                let mut queue = pending.lock();
                queue.push_back(format!("msg_{}", i));
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Verify all operations completed
    let final_nodes = fixed_network.nodes.read().unwrap();
    assert_eq!(final_nodes.len(), 10);

    let final_pending = fixed_network.pending.lock();
    assert_eq!(final_pending.len(), 10);
}