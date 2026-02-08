//! Rate limiting and resource management for DoS protection
//! Defense #3 & #4: Connection limits and rate limiting

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::sync::atomic::{AtomicUsize, Ordering};
use crate::protocol::ProtocolError;

/// Maximum concurrent connections from a single IP
pub const MAX_CONNECTIONS_PER_IP: usize = 10;

/// Maximum total concurrent connections
pub const MAX_TOTAL_CONNECTIONS: usize = 1000;

/// Maximum allocations per connection
pub const MAX_ALLOCATIONS_PER_CONNECTION: usize = 100;

/// Maximum total memory per connection (10MB)
pub const MAX_MEMORY_PER_CONNECTION: usize = 10 * 1024 * 1024;

/// Connection creation rate limit (per IP)
pub const MAX_NEW_CONNECTIONS_PER_MINUTE: usize = 20;

/// Time window for rate limiting
pub const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);

/// Connection state tracking for resource limits
#[derive(Debug)]
pub struct ConnectionResources {
    /// Unique connection identifier
    pub id: usize,
    /// Socket address of the connection
    pub addr: SocketAddr,
    /// Timestamp when connection was created
    pub created_at: Instant,
    /// Number of active memory allocations
    pub allocations: AtomicUsize,
    /// Total memory used by this connection
    pub memory_used: AtomicUsize,
    /// Timestamp of last activity on this connection
    pub last_activity: Arc<Mutex<Instant>>,
}

impl ConnectionResources {
    /// Create a new connection resource tracker
    pub fn new(id: usize, addr: SocketAddr) -> Self {
        Self {
            id,
            addr,
            created_at: Instant::now(),
            allocations: AtomicUsize::new(0),
            memory_used: AtomicUsize::new(0),
            last_activity: Arc::new(Mutex::new(Instant::now())),
        }
    }

    /// Try to allocate memory for this connection
    pub fn try_allocate(&self, size: usize) -> Result<(), ProtocolError> {
        // Check allocation count limit
        let current_allocations = self.allocations.load(Ordering::Acquire);
        if current_allocations >= MAX_ALLOCATIONS_PER_CONNECTION {
            return Err(ProtocolError::ResourceLimitExceeded);
        }

        // Check memory limit
        let current_memory = self.memory_used.load(Ordering::Acquire);
        if current_memory + size > MAX_MEMORY_PER_CONNECTION {
            return Err(ProtocolError::ResourceLimitExceeded);
        }

        // Atomically update counters
        self.allocations.fetch_add(1, Ordering::AcqRel);
        self.memory_used.fetch_add(size, Ordering::AcqRel);

        // Update last activity
        *self.last_activity.lock().unwrap() = Instant::now();

        Ok(())
    }

    /// Release allocated memory
    pub fn release(&self, size: usize) {
        self.allocations.fetch_sub(1, Ordering::AcqRel);
        self.memory_used.fetch_sub(size, Ordering::AcqRel);
    }

    /// Get current resource usage
    pub fn get_usage(&self) -> (usize, usize) {
        (
            self.allocations.load(Ordering::Acquire),
            self.memory_used.load(Ordering::Acquire),
        )
    }
}

/// Rate limiter for connection creation
#[derive(Debug)]
pub struct RateLimiter {
    /// Connection attempts per IP
    attempts: Arc<Mutex<HashMap<SocketAddr, VecDeque<Instant>>>>,
    /// Active connections per IP
    connections: Arc<Mutex<HashMap<SocketAddr, Vec<Arc<ConnectionResources>>>>>,
    /// Total active connections
    total_connections: AtomicUsize,
    /// Connection ID counter
    next_conn_id: AtomicUsize,
}

impl RateLimiter {
    /// Create a new rate limiter
    pub fn new() -> Self {
        Self {
            attempts: Arc::new(Mutex::new(HashMap::new())),
            connections: Arc::new(Mutex::new(HashMap::new())),
            total_connections: AtomicUsize::new(0),
            next_conn_id: AtomicUsize::new(1),
        }
    }

    /// Check if a new connection is allowed from this IP
    pub fn allow_connection(&self, addr: SocketAddr) -> Result<Arc<ConnectionResources>, ProtocolError> {
        // Check total connection limit
        let total = self.total_connections.load(Ordering::Acquire);
        if total >= MAX_TOTAL_CONNECTIONS {
            return Err(ProtocolError::ResourceLimitExceeded);
        }

        let mut attempts = self.attempts.lock().unwrap();
        let mut connections = self.connections.lock().unwrap();

        // Clean up old attempts outside the time window
        let now = Instant::now();
        let ip_attempts = attempts.entry(addr).or_insert_with(VecDeque::new);
        while let Some(&front) = ip_attempts.front() {
            if now.duration_since(front) > RATE_LIMIT_WINDOW {
                ip_attempts.pop_front();
            } else {
                break;
            }
        }

        // Check rate limit
        if ip_attempts.len() >= MAX_NEW_CONNECTIONS_PER_MINUTE {
            return Err(ProtocolError::ResourceLimitExceeded);
        }

        // Check per-IP connection limit
        let ip_connections = connections.entry(addr).or_insert_with(Vec::new);
        if ip_connections.len() >= MAX_CONNECTIONS_PER_IP {
            return Err(ProtocolError::ResourceLimitExceeded);
        }

        // Connection allowed - record it
        ip_attempts.push_back(now);

        let conn_id = self.next_conn_id.fetch_add(1, Ordering::AcqRel);
        let conn_resources = Arc::new(ConnectionResources::new(conn_id, addr));
        ip_connections.push(conn_resources.clone());

        self.total_connections.fetch_add(1, Ordering::AcqRel);

        Ok(conn_resources)
    }

    /// Remove a connection when it closes
    pub fn remove_connection(&self, conn: &ConnectionResources) {
        let mut connections = self.connections.lock().unwrap();

        if let Some(ip_connections) = connections.get_mut(&conn.addr) {
            ip_connections.retain(|c| c.id != conn.id);

            if ip_connections.is_empty() {
                connections.remove(&conn.addr);
            }
        }

        self.total_connections.fetch_sub(1, Ordering::AcqRel);
    }

    /// Clean up idle connections
    pub fn cleanup_idle_connections(&self, idle_timeout: Duration) {
        let mut connections = self.connections.lock().unwrap();
        let now = Instant::now();
        let mut to_remove = Vec::new();

        for (_addr, conn_list) in connections.iter_mut() {
            conn_list.retain(|conn| {
                let last_activity = *conn.last_activity.lock().unwrap();
                if now.duration_since(last_activity) > idle_timeout {
                    to_remove.push(conn.id);
                    self.total_connections.fetch_sub(1, Ordering::AcqRel);
                    false
                } else {
                    true
                }
            });
        }

        // Remove empty entries
        connections.retain(|_, v| !v.is_empty());
    }

    /// Get current statistics
    pub fn get_stats(&self) -> RateLimiterStats {
        let connections = self.connections.lock().unwrap();
        let attempts = self.attempts.lock().unwrap();

        let mut total_memory = 0;
        let mut total_allocations = 0;
        let mut max_memory_per_conn = 0;

        for conn_list in connections.values() {
            for conn in conn_list {
                let (allocs, mem) = conn.get_usage();
                total_allocations += allocs;
                total_memory += mem;
                max_memory_per_conn = max_memory_per_conn.max(mem);
            }
        }

        RateLimiterStats {
            total_connections: self.total_connections.load(Ordering::Acquire),
            unique_ips: connections.len(),
            recent_attempts: attempts.values().map(|v| v.len()).sum(),
            total_memory,
            total_allocations,
            max_memory_per_conn,
        }
    }
}

/// Statistics for monitoring
#[derive(Debug)]
pub struct RateLimiterStats {
    /// Total number of active connections
    pub total_connections: usize,
    /// Number of unique IP addresses connected
    pub unique_ips: usize,
    /// Number of recent connection attempts
    pub recent_attempts: usize,
    /// Total memory used by all connections
    pub total_memory: usize,
    /// Total number of allocations across all connections
    pub total_allocations: usize,
    /// Maximum memory used by any single connection
    pub max_memory_per_conn: usize,
}

/// Guard for automatic resource cleanup
pub struct AllocationGuard {
    conn: Arc<ConnectionResources>,
    size: usize,
}

impl AllocationGuard {
    /// Create a new allocation guard that automatically cleans up on drop
    pub fn new(conn: Arc<ConnectionResources>, size: usize) -> Result<Self, ProtocolError> {
        conn.try_allocate(size)?;
        Ok(Self { conn, size })
    }
}

impl Drop for AllocationGuard {
    fn drop(&mut self) {
        self.conn.release(self.size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_rate_limiting() {
        let limiter = RateLimiter::new();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);

        // Should allow initial connections
        for i in 0..5 {
            let conn = limiter.allow_connection(addr).unwrap();
            assert_eq!(conn.id, i + 1);
        }

        let stats = limiter.get_stats();
        assert_eq!(stats.total_connections, 5);
        assert_eq!(stats.unique_ips, 1);
    }

    #[test]
    fn test_per_ip_limit() {
        let limiter = RateLimiter::new();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);

        // Create maximum connections
        let mut conns = Vec::new();
        for _ in 0..MAX_CONNECTIONS_PER_IP {
            conns.push(limiter.allow_connection(addr).unwrap());
        }

        // Next one should fail
        assert!(limiter.allow_connection(addr).is_err());

        // Remove one and try again
        limiter.remove_connection(&conns[0]);
        assert!(limiter.allow_connection(addr).is_ok());
    }

    #[test]
    fn test_memory_limits() {
        let limiter = RateLimiter::new();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        let conn = limiter.allow_connection(addr).unwrap();

        // Allocate some memory
        conn.try_allocate(1000).unwrap();
        conn.try_allocate(2000).unwrap();

        let (allocs, mem) = conn.get_usage();
        assert_eq!(allocs, 2);
        assert_eq!(mem, 3000);

        // Try to exceed memory limit
        let huge_alloc = MAX_MEMORY_PER_CONNECTION;
        assert!(conn.try_allocate(huge_alloc).is_err());

        // Release and try again
        conn.release(1000);
        let (allocs, mem) = conn.get_usage();
        assert_eq!(allocs, 1);
        assert_eq!(mem, 2000);
    }

    #[test]
    fn test_allocation_guard() {
        let limiter = RateLimiter::new();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        let conn = limiter.allow_connection(addr).unwrap();

        {
            let _guard = AllocationGuard::new(conn.clone(), 5000).unwrap();
            let (allocs, mem) = conn.get_usage();
            assert_eq!(allocs, 1);
            assert_eq!(mem, 5000);
        }

        // Guard dropped, resources should be released
        let (allocs, mem) = conn.get_usage();
        assert_eq!(allocs, 0);
        assert_eq!(mem, 0);
    }
}