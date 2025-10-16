//! Clock abstraction for testable time operations

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Clock trait for abstracting time operations
#[async_trait]
pub trait Clock: Send + Sync {
    /// Get the current time
    fn now(&self) -> DateTime<Utc>;

    /// Sleep for a duration (for rate limiting, etc)
    async fn sleep(&self, duration: std::time::Duration);
}

/// Real clock implementation using system time
#[derive(Clone)]
pub struct RealClock;

#[async_trait]
impl Clock for RealClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }

    async fn sleep(&self, duration: std::time::Duration) {
        tokio::time::sleep(duration).await;
    }
}

/// Mock clock for testing
#[derive(Clone)]
pub struct MockClock {
    current_time: Arc<RwLock<DateTime<Utc>>>,
}

impl Default for MockClock {
    fn default() -> Self {
        Self::new()
    }
}

impl MockClock {
    /// Create a new mock clock starting at the current time
    pub fn new() -> Self {
        Self {
            current_time: Arc::new(RwLock::new(Utc::now())),
        }
    }

    /// Create a mock clock starting at a specific time
    pub fn new_at(time: DateTime<Utc>) -> Self {
        Self {
            current_time: Arc::new(RwLock::new(time)),
        }
    }

    /// Advance the clock by a duration
    pub async fn advance(&self, duration: Duration) {
        let mut time = self.current_time.write().await;
        *time += duration;
    }

    /// Set the clock to a specific time
    pub async fn set_time(&self, time: DateTime<Utc>) {
        let mut current = self.current_time.write().await;
        *current = time;
    }
}

#[async_trait]
impl Clock for MockClock {
    fn now(&self) -> DateTime<Utc> {
        // Note: This blocks briefly but is acceptable for testing
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async { *self.current_time.read().await })
        })
    }

    async fn sleep(&self, _duration: std::time::Duration) {
        // Mock sleep does nothing - time is controlled manually
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_mock_clock_advance() {
        let clock = MockClock::new();
        let start = clock.now();

        clock.advance(Duration::seconds(60)).await;

        let end = clock.now();
        assert_eq!(end - start, Duration::seconds(60));
    }

    #[tokio::test]
    async fn test_real_clock() {
        let clock = RealClock;
        let start = clock.now();

        clock.sleep(std::time::Duration::from_millis(10)).await;

        let end = clock.now();
        assert!(end > start);
    }
}
