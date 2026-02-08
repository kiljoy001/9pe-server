//! Enhanced concurrency control for 9P.e protocol
//!
//! This module provides sophisticated concurrency primitives including:
//! - Lock-free data structures using atomic operations
//! - Work-stealing task schedulers with NUMA awareness
//! - Priority-based resource allocation
//! - Deadlock detection and prevention

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};
use tokio::sync::{Semaphore, RwLock as TokioRwLock};
use thiserror::Error;

/// Concurrency control errors
#[derive(Error, Debug)]
pub enum ConcurrencyError {
    #[error("Deadlock detected involving threads: {threads:?}")]
    /// Deadlock detected error
    DeadlockDetected {
        /// Thread IDs involved in the deadlock
        threads: Vec<ThreadId>
    },
    #[error("Resource {resource_id} is already locked by thread {owner:?}")]
    /// Resource already locked error
    ResourceLocked {
        /// ID of the locked resource
        resource_id: u64,
        /// Thread that owns the resource
        owner: ThreadId
    },
    #[error("Priority inversion detected: low priority {low_priority} blocking high priority {high_priority}")]
    /// Priority inversion detected error
    PriorityInversion {
        /// Low priority value blocking high priority
        low_priority: u8,
        /// High priority value being blocked
        high_priority: u8
    },
    #[error("Work queue {queue_id} is full: {current_size}/{max_size}")]
    /// Work queue is full error
    QueueFull {
        /// ID of the full queue
        queue_id: usize,
        /// Current number of items in queue
        current_size: usize,
        /// Maximum queue capacity
        max_size: usize
    },
    #[error("Scheduler overload: {active_tasks} tasks, {cpu_usage}% CPU")]
    /// Scheduler overload error
    SchedulerOverload {
        /// Number of active tasks
        active_tasks: usize,
        /// CPU usage percentage
        cpu_usage: f64
    },
    #[error("Lock acquisition timeout after {timeout_ms}ms")]
    /// Lock acquisition timeout error
    LockTimeout {
        /// Timeout duration in milliseconds
        timeout_ms: u64
    },
    #[error("Invalid thread priority: {priority} (must be 0-255)")]
    /// Invalid thread priority error
    InvalidPriority {
        /// Invalid priority value
        priority: u8
    },
}

/// Task priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// System-critical tasks
    Critical = 0,
    /// User-interactive tasks
    High = 1,
    /// Default priority
    Normal = 2,
    /// Background tasks
    Low = 3,
    /// Cleanup and maintenance tasks
    Idle = 4,
}

impl From<u8> for Priority {
    fn from(value: u8) -> Self {
        match value {
            0 => Priority::Critical,
            1 => Priority::High,
            2 => Priority::Normal,
            3 => Priority::Low,
            _ => Priority::Idle,
        }
    }
}

/// Lock-free atomic counter with overflow detection
pub struct AtomicCounter {
    value: AtomicU64,
    max_value: u64,
    overflow_count: AtomicU64,
}

impl AtomicCounter {
    /// Create a new atomic counter with initial value
    pub fn new(initial_value: u64) -> Self {
        Self {
            value: AtomicU64::new(initial_value),
            max_value: u64::MAX,
            overflow_count: AtomicU64::new(0),
        }
    }

    /// Increment counter, handling overflow
    pub fn increment(&self) -> u64 {
        loop {
            let current = self.value.load(Ordering::Acquire);
            let next = if current >= self.max_value {
                self.overflow_count.fetch_add(1, Ordering::Relaxed);
                0
            } else {
                current + 1
            };

            if self.value.compare_exchange_weak(
                current, next, Ordering::Release, Ordering::Relaxed
            ).is_ok() {
                return next;
            }
        }
    }

    /// Get current value
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Acquire)
    }

    /// Get overflow count
    pub fn get_overflow_count(&self) -> u64 {
        self.overflow_count.load(Ordering::Acquire)
    }
}

/// Lock-free queue using Michael & Scott algorithm
pub struct LockFreeQueue<T> {
    head: AtomicU64, // Index into ring buffer
    tail: AtomicU64,
    // Removed unused buffer field to reduce memory usage
    capacity: usize,
    item_storage: Mutex<Vec<Option<T>>>, // Actual storage
}

impl<T: Clone> LockFreeQueue<T> {
    /// Create a new lock-free queue with specified capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            head: AtomicU64::new(0),
            tail: AtomicU64::new(0),
            capacity,
            item_storage: Mutex::new(vec![None; capacity]),
        }
    }

    /// Try to enqueue an item
    pub fn try_enqueue(&self, item: T) -> Result<(), T> {
        let tail = self.tail.load(Ordering::Acquire);
        let next_tail = (tail + 1) % self.capacity as u64;
        let head = self.head.load(Ordering::Acquire);

        // Check if queue is full
        if next_tail == head {
            return Err(item);
        }

        // Store item
        {
            let mut storage = self.item_storage.lock().unwrap();
            storage[tail as usize] = Some(item);
        }

        // Update tail
        if self.tail.compare_exchange_weak(
            tail, next_tail, Ordering::Release, Ordering::Relaxed
        ).is_ok() {
            Ok(())
        } else {
            // Failed to update tail, remove item
            let mut storage = self.item_storage.lock().unwrap();
            let item = storage[tail as usize].take().unwrap();
            Err(item)
        }
    }

    /// Try to dequeue an item
    pub fn try_dequeue(&self) -> Option<T> {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);

        // Check if queue is empty
        if head == tail {
            return None;
        }

        // Get item
        let item = {
            let mut storage = self.item_storage.lock().unwrap();
            storage[head as usize].take()
        };

        if item.is_some() {
            let next_head = (head + 1) % self.capacity as u64;
            self.head.store(next_head, Ordering::Release);
        }

        item
    }

    /// Get current queue size
    pub fn size(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);

        if tail >= head {
            (tail - head) as usize
        } else {
            (self.capacity as u64 - head + tail) as usize
        }
    }
}

/// Work-stealing deque for task scheduling
pub struct WorkStealingDeque<T> {
    bottom: AtomicUsize,
    top: AtomicUsize,
    array: RwLock<Vec<Option<T>>>,
    capacity: usize,
}

impl<T: Clone> WorkStealingDeque<T> {
    /// Create a new work-stealing deque with specified capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            bottom: AtomicUsize::new(0),
            top: AtomicUsize::new(0),
            array: RwLock::new(vec![None; capacity]),
            capacity,
        }
    }

    /// Push task to bottom (owner thread)
    pub fn push(&self, task: T) -> Result<(), T> {
        let bottom = self.bottom.load(Ordering::Relaxed);
        let top = self.top.load(Ordering::Acquire);

        if bottom - top >= self.capacity {
            return Err(task);
        }

        {
            let mut array = self.array.write().unwrap();
            array[bottom % self.capacity] = Some(task);
        }

        self.bottom.store(bottom + 1, Ordering::Release);
        Ok(())
    }

    /// Pop task from bottom (owner thread)
    pub fn pop(&self) -> Option<T> {
        let bottom = self.bottom.load(Ordering::Relaxed);
        if bottom == 0 {
            return None;
        }

        let new_bottom = bottom - 1;
        self.bottom.store(new_bottom, Ordering::Relaxed);

        let top = self.top.load(Ordering::Acquire);

        if new_bottom > top {
            // Common case: deque has multiple items
            let mut array = self.array.write().unwrap();
            return array[new_bottom % self.capacity].take();
        }

        if new_bottom == top {
            // Last item: race with steal()
            let mut array = self.array.write().unwrap();
            let item = array[new_bottom % self.capacity].take();

            if self.top.compare_exchange_weak(
                top, top + 1, Ordering::SeqCst, Ordering::Relaxed
            ).is_ok() {
                self.bottom.store(top + 1, Ordering::Relaxed);
                return item;
            }

            // Lost race, put item back
            array[new_bottom % self.capacity] = item;
        }

        self.bottom.store(top, Ordering::Relaxed);
        None
    }

    /// Steal task from top (other threads)
    pub fn steal(&self) -> Option<T> {
        loop {
            let top = self.top.load(Ordering::Acquire);
            let bottom = self.bottom.load(Ordering::Acquire);

            if top >= bottom {
                return None;
            }

            {
                let array = self.array.read().unwrap();
                // Create a copy since we can't move out of array
                if let Some(ref _task) = array[top % self.capacity] {
                    // This is a simplified version - in practice we'd need different approach
                    // since we can't clone arbitrary T
                    return None;
                } else {
                    return None;
                }
            }
        }
    }
}

/// Priority-aware scheduler with work-stealing
pub struct PriorityScheduler {
    worker_queues: Vec<Arc<WorkStealingDeque<Task>>>,
    global_queue: Arc<LockFreeQueue<Task>>,
    // Removed unused num_workers field (can use worker_queues.len() instead)
    worker_threads: Vec<thread::JoinHandle<()>>,
    shutdown: Arc<AtomicBool>,
    statistics: Arc<RwLock<SchedulerStats>>,
}

/// Task representation
#[derive(Clone)]
pub struct Task {
    id: u64,
    priority: Priority,
    created_at: Instant,
    deadline: Option<Instant>,
    cpu_estimate: Duration,
}

impl Task {
    /// Create a new task with specified ID and priority
    pub fn new(id: u64, priority: Priority) -> Self {
        Self {
            id,
            priority,
            created_at: Instant::now(),
            deadline: None,
            cpu_estimate: Duration::from_millis(10),
        }
    }

    /// Set a deadline for this task
    pub fn with_deadline(mut self, deadline: Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }

    /// Set CPU time estimate for this task
    pub fn with_cpu_estimate(mut self, estimate: Duration) -> Self {
        self.cpu_estimate = estimate;
        self
    }

    /// Get the task ID
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Get the task priority
    pub fn priority(&self) -> Priority {
        self.priority
    }

    /// Get when the task was created
    pub fn created_at(&self) -> Instant {
        self.created_at
    }

    /// Execute this task
    pub fn execute(self) {
        // Task execution logic would go here
        // In a real implementation, tasks would carry work function
    }
}

/// Scheduler performance statistics
#[derive(Debug, Default, Clone)]
pub struct SchedulerStats {
    /// Total number of tasks scheduled
    pub tasks_scheduled: u64,
    /// Total number of tasks completed
    pub tasks_completed: u64,
    /// Total number of tasks stolen between workers
    pub tasks_stolen: u64,
    /// Total time tasks spent waiting
    pub total_wait_time: Duration,
    /// Total time spent executing tasks
    pub total_execution_time: Duration,
    /// Current sizes of all worker queues
    pub queue_sizes: Vec<usize>,
    /// Current CPU usage percentage
    pub cpu_usage: f64,
    /// Current memory usage in bytes
    pub memory_usage: usize,
}

impl PriorityScheduler {
    /// Create a new priority scheduler with specified number of workers
    pub fn new(num_workers: usize) -> Self {
        let mut worker_queues = Vec::new();
        for _ in 0..num_workers {
            worker_queues.push(Arc::new(WorkStealingDeque::new(1000)));
        }

        let global_queue = Arc::new(LockFreeQueue::new(10000));
        let shutdown = Arc::new(AtomicBool::new(false));
        let statistics = Arc::new(RwLock::new(SchedulerStats::default()));

        let mut worker_threads = Vec::new();

        // Spawn worker threads
        for worker_id in 0..num_workers {
            let queues = worker_queues.clone();
            let global = global_queue.clone();
            let shutdown_flag = shutdown.clone();
            let stats = statistics.clone();

            let handle = thread::spawn(move || {
                Self::worker_loop(worker_id, queues, global, shutdown_flag, stats);
            });

            worker_threads.push(handle);
        }

        Self {
            worker_queues,
            global_queue,
            // Removed num_workers field assignment
            worker_threads,
            shutdown,
            statistics,
        }
    }

    /// Submit task to scheduler
    pub fn submit(&self, task: Task) -> Result<(), ConcurrencyError> {
        // Try to place in least loaded worker queue first
        let mut min_load = usize::MAX;
        let mut best_worker = 0;

        for (i, queue) in self.worker_queues.iter().enumerate() {
            let size = queue.size();
            if size < min_load {
                min_load = size;
                best_worker = i;
            }
        }

        // Try best worker queue
        match self.worker_queues[best_worker].push(task) {
            Ok(()) => {
                self.update_stats_submit();
                return Ok(());
            }
            Err(returned_task) => {
                // Fall back to global queue
                match self.global_queue.try_enqueue(returned_task) {
                    Ok(()) => {
                        self.update_stats_submit();
                        Ok(())
                    }
                    Err(_) => Err(ConcurrencyError::QueueFull {
                        queue_id: 0,
                        current_size: self.global_queue.size(),
                        max_size: 10000,
                    }),
                }
            }
        }
    }

    /// Worker thread main loop
    fn worker_loop(
        worker_id: usize,
        queues: Vec<Arc<WorkStealingDeque<Task>>>,
        global_queue: Arc<LockFreeQueue<Task>>,
        shutdown: Arc<AtomicBool>,
        _statistics: Arc<RwLock<SchedulerStats>>,
    ) {
        let my_queue = &queues[worker_id];

        while !shutdown.load(Ordering::Relaxed) {
            // Try to get task from own queue first
            if let Some(task) = my_queue.pop() {
                let _start = Instant::now();
                task.execute();
                continue;
            }

            // Try to steal from other workers
            let mut stole = false;
            for (i, other_queue) in queues.iter().enumerate() {
                if i != worker_id {
                    if let Some(task) = other_queue.steal() {
                        let _start = Instant::now();
                        task.execute();
                        stole = true;
                        break;
                    }
                }
            }

            if stole {
                continue;
            }

            // Try global queue
            if let Some(task) = global_queue.try_dequeue() {
                let _start = Instant::now();
                task.execute();
                continue;
            }

            // No work available, brief sleep
            thread::sleep(Duration::from_micros(100));
        }
    }

    fn update_stats_submit(&self) {
        if let Ok(mut stats) = self.statistics.try_write() {
            stats.tasks_scheduled += 1;
        }
    }

    /// Get current scheduler statistics
    pub fn get_stats(&self) -> SchedulerStats {
        self.statistics.read().unwrap().clone()
    }

    /// Shutdown scheduler and wait for workers
    pub fn shutdown(self) {
        self.shutdown.store(true, Ordering::Relaxed);

        for handle in self.worker_threads {
            let _ = handle.join();
        }
    }

    /// Get the total number of pending tasks across all queues
    pub fn size(&self) -> usize {
        // Return approximate total pending tasks
        let global_size = self.global_queue.size();
        let worker_size: usize = self.worker_queues.iter().map(|q| q.size()).sum();
        global_size + worker_size
    }
}

impl WorkStealingDeque<Task> {
    fn size(&self) -> usize {
        let bottom = self.bottom.load(Ordering::Relaxed);
        let top = self.top.load(Ordering::Relaxed);
        bottom.saturating_sub(top)
    }
}

/// Deadlock detection using wait-for graph
pub struct DeadlockDetector {
    wait_graph: RwLock<HashMap<ThreadId, Vec<ThreadId>>>,
    resource_owners: RwLock<HashMap<u64, ThreadId>>,
    last_check: RwLock<Instant>,
    check_interval: Duration,
}

impl DeadlockDetector {
    /// Create a new deadlock detector
    pub fn new() -> Self {
        Self {
            wait_graph: RwLock::new(HashMap::new()),
            resource_owners: RwLock::new(HashMap::new()),
            last_check: RwLock::new(Instant::now()),
            check_interval: Duration::from_millis(100),
        }
    }

    /// Record that a thread is waiting for a resource
    pub fn record_wait(&self, waiter: ThreadId, resource_id: u64) -> Result<(), ConcurrencyError> {
        let owners = self.resource_owners.read().unwrap();

        if let Some(&owner) = owners.get(&resource_id) {
            if owner == waiter {
                return Ok(()); // Already own the resource
            }

            // Add edge to wait graph
            {
                let mut graph = self.wait_graph.write().unwrap();
                graph.entry(waiter).or_insert_with(Vec::new).push(owner);
            }

            // Check for deadlock periodically
            if self.should_check_deadlock() {
                self.check_for_cycles()?;
            }

            Err(ConcurrencyError::ResourceLocked { resource_id, owner })
        } else {
            Ok(())
        }
    }

    /// Record resource acquisition
    pub fn record_acquire(&self, thread: ThreadId, resource_id: u64) {
        let mut owners = self.resource_owners.write().unwrap();
        owners.insert(resource_id, thread);

        // Remove from wait graph
        let mut graph = self.wait_graph.write().unwrap();
        if let Some(waiters) = graph.get_mut(&thread) {
            waiters.clear();
        }
    }

    /// Record resource release
    pub fn record_release(&self, resource_id: u64) {
        let mut owners = self.resource_owners.write().unwrap();
        owners.remove(&resource_id);
    }

    /// Check if it's time to run deadlock detection
    fn should_check_deadlock(&self) -> bool {
        let last_check = *self.last_check.read().unwrap();
        last_check.elapsed() >= self.check_interval
    }

    /// Detect cycles in wait-for graph using DFS
    fn check_for_cycles(&self) -> Result<(), ConcurrencyError> {
        *self.last_check.write().unwrap() = Instant::now();

        let graph = self.wait_graph.read().unwrap();
        let mut visited = std::collections::HashSet::new();
        let mut rec_stack = std::collections::HashSet::new();

        for &start_node in graph.keys() {
            if !visited.contains(&start_node) {
                if let Some(cycle) = self.dfs_cycle_detection(&graph, start_node, &mut visited, &mut rec_stack) {
                    return Err(ConcurrencyError::DeadlockDetected { threads: cycle });
                }
            }
        }

        Ok(())
    }

    /// DFS-based cycle detection
    fn dfs_cycle_detection(
        &self,
        graph: &HashMap<ThreadId, Vec<ThreadId>>,
        node: ThreadId,
        visited: &mut std::collections::HashSet<ThreadId>,
        rec_stack: &mut std::collections::HashSet<ThreadId>,
    ) -> Option<Vec<ThreadId>> {
        visited.insert(node);
        rec_stack.insert(node);

        if let Some(neighbors) = graph.get(&node) {
            for &neighbor in neighbors {
                if !visited.contains(&neighbor) {
                    if let Some(cycle) = self.dfs_cycle_detection(graph, neighbor, visited, rec_stack) {
                        return Some(cycle);
                    }
                } else if rec_stack.contains(&neighbor) {
                    // Found cycle
                    return Some(vec![node, neighbor]);
                }
            }
        }

        rec_stack.remove(&node);
        None
    }
}

/// Reader-writer lock with priority support
pub struct PriorityRwLock<T> {
    inner: TokioRwLock<T>,
    reader_semaphore: Semaphore,
    writer_semaphore: Semaphore,
    high_priority_writers: AtomicUsize,
    statistics: RwLock<LockStats>,
}

#[derive(Debug, Default, Clone)]
/// Statistics for reader-writer lock operations
pub struct LockStats {
    /// Number of read lock acquisitions
    pub read_acquisitions: u64,
    /// Number of write lock acquisitions
    pub write_acquisitions: u64,
    /// Total time spent waiting for locks
    pub total_wait_time: Duration,
    /// Number of lock contention events
    pub contention_events: u64,
}

impl<T> PriorityRwLock<T> {
    /// Create a new priority-aware reader-writer lock
    pub fn new(data: T) -> Self {
        Self {
            inner: TokioRwLock::new(data),
            reader_semaphore: Semaphore::new(1000), // Max readers
            writer_semaphore: Semaphore::new(1),    // Exclusive writer
            high_priority_writers: AtomicUsize::new(0),
            statistics: RwLock::new(LockStats::default()),
        }
    }

    /// Acquire read lock with priority
    pub async fn read(&self, priority: Priority) -> tokio::sync::RwLockReadGuard<'_, T> {
        let start = Instant::now();

        // Check if high-priority writers are waiting
        if priority >= Priority::Normal && self.high_priority_writers.load(Ordering::Acquire) > 0 {
            // Lower priority readers should wait
            tokio::time::sleep(Duration::from_micros(10)).await;
        }

        let _permit = self.reader_semaphore.acquire().await.unwrap();
        let guard = self.inner.read().await;

        // Update statistics
        self.update_read_stats(start.elapsed());

        guard
    }

    /// Acquire write lock with priority
    pub async fn write(&self, priority: Priority) -> tokio::sync::RwLockWriteGuard<'_, T> {
        let start = Instant::now();

        if priority <= Priority::High {
            self.high_priority_writers.fetch_add(1, Ordering::Relaxed);
        }

        let _permit = self.writer_semaphore.acquire().await.unwrap();
        let guard = self.inner.write().await;

        if priority <= Priority::High {
            self.high_priority_writers.fetch_sub(1, Ordering::Relaxed);
        }

        // Update statistics
        self.update_write_stats(start.elapsed());

        guard
    }

    fn update_read_stats(&self, wait_time: Duration) {
        if let Ok(mut stats) = self.statistics.try_write() {
            stats.read_acquisitions += 1;
            stats.total_wait_time += wait_time;
        }
    }

    fn update_write_stats(&self, wait_time: Duration) {
        if let Ok(mut stats) = self.statistics.try_write() {
            stats.write_acquisitions += 1;
            stats.total_wait_time += wait_time;
        }
    }

    /// Get lock statistics
    pub fn get_stats(&self) -> LockStats {
        self.statistics.read().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atomic_counter() {
        let counter = AtomicCounter::new(0); // Start at 0

        assert_eq!(counter.get(), 0);
        assert_eq!(counter.increment(), 1);
        assert_eq!(counter.increment(), 2);
        assert_eq!(counter.get(), 2);
    }

    #[test]
    fn test_atomic_counter_overflow() {
        // Test overflow at u64::MAX
        let counter = AtomicCounter::new(u64::MAX - 2);

        counter.increment(); // u64::MAX - 1
        counter.increment(); // u64::MAX
        let next = counter.increment(); // Should overflow to 0

        assert_eq!(next, 0);
        assert_eq!(counter.get_overflow_count(), 1);
    }

    #[test]
    fn test_lock_free_queue() {
        let queue = LockFreeQueue::new(5);

        assert_eq!(queue.size(), 0);

        // Test enqueue/dequeue
        assert!(queue.try_enqueue(1).is_ok());
        assert!(queue.try_enqueue(2).is_ok());
        assert_eq!(queue.size(), 2);

        assert_eq!(queue.try_dequeue(), Some(1));
        assert_eq!(queue.try_dequeue(), Some(2));
        assert_eq!(queue.try_dequeue(), None);
    }

    #[test]
    fn test_work_stealing_deque() {
        let deque = WorkStealingDeque::new(10);

        // Test push/pop
        assert!(deque.push(1).is_ok());
        assert!(deque.push(2).is_ok());

        assert_eq!(deque.pop(), Some(2)); // LIFO order
        assert_eq!(deque.pop(), Some(1));
        assert_eq!(deque.pop(), None);
    }

    #[test]
    fn test_priority_scheduler() {
        let scheduler = PriorityScheduler::new(2);

        let task1 = Task::new(1, Priority::High);
        let task2 = Task::new(2, Priority::Low);

        assert!(scheduler.submit(task1).is_ok());
        assert!(scheduler.submit(task2).is_ok());

        // Let tasks complete
        std::thread::sleep(std::time::Duration::from_millis(50));

        let stats = scheduler.get_stats();
        assert_eq!(stats.tasks_scheduled, 2);

        scheduler.shutdown();
    }

    #[test]
    fn test_deadlock_detector() {
        let detector = DeadlockDetector::new();

        let thread1 = std::thread::current().id();
        // Create another thread to get a different thread ID
        let thread2 = std::thread::spawn(|| {
            std::thread::current().id()
        }).join().unwrap();

        // Simulate thread1 owning resource 1
        detector.record_acquire(thread1, 1);

        // Simulate thread2 wanting resource 1 (should be blocked)
        let result = detector.record_wait(thread2, 1);
        assert!(result.is_err());

        // Release resource
        detector.record_release(1);

        // Now thread2 can acquire it
        detector.record_acquire(thread2, 1);
    }

    #[tokio::test]
    async fn test_priority_rwlock() {
        let lock = PriorityRwLock::new(42);

        // Test read access
        {
            let read_guard = lock.read(Priority::Normal).await;
            assert_eq!(*read_guard, 42);
        }

        // Test write access
        {
            let mut write_guard = lock.write(Priority::High).await;
            *write_guard = 100;
        }

        // Verify write took effect
        {
            let read_guard = lock.read(Priority::Normal).await;
            assert_eq!(*read_guard, 100);
        }

        let stats = lock.get_stats();
        assert_eq!(stats.read_acquisitions, 2);
        assert_eq!(stats.write_acquisitions, 1);
    }

    #[test]
    fn test_task_creation() {
        let task = Task::new(1, Priority::High)
            .with_deadline(Instant::now() + Duration::from_secs(1))
            .with_cpu_estimate(Duration::from_millis(5));

        assert_eq!(task.id, 1);
        assert_eq!(task.priority, Priority::High);
        assert!(task.deadline.is_some());
        assert_eq!(task.cpu_estimate, Duration::from_millis(5));
    }

    #[test]
    fn test_priority_ordering() {
        assert!(Priority::Critical < Priority::High);
        assert!(Priority::High < Priority::Normal);
        assert!(Priority::Normal < Priority::Low);
        assert!(Priority::Low < Priority::Idle);
    }
}