//! Advanced Rate Limiting and DDoS Protection
//!
//! Provides multi-layered protection against DoS attacks:
//! - Per-IP rate limiting
//! - Per-user rate limiting
//! - Exponential backoff for failed auth attempts
//! - Connection throttling
//! - Resource usage limiting

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tokio::time::sleep;
use anyhow::{Result, bail};
use tracing::{warn, info, error};

/// Configuration for rate limiting
#[derive(Clone)]
pub struct RateLimitConfig {
    /// Maximum requests per IP per window
    pub max_requests_per_ip: u32,
    /// Maximum requests per user per window
    pub max_requests_per_user: u32,
    /// Maximum failed auth attempts before lockout
    pub max_failed_auth_attempts: u32,
    /// Maximum concurrent connections per IP
    pub max_connections_per_ip: u32,
    /// Window duration in seconds
    pub window_secs: u64,
    /// Lockout duration for too many failed auth attempts
    pub lockout_duration_secs: u64,
    /// Enable exponential backoff
    pub enable_exponential_backoff: bool,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests_per_ip: 100,      // 100 requests per minute
            max_requests_per_user: 200,     // 200 requests per minute per authenticated user
            max_failed_auth_attempts: 5,    // Lock after 5 failed attempts
            max_connections_per_ip: 10,     // Max 10 concurrent connections per IP
            window_secs: 60,                // 1 minute window
            lockout_duration_secs: 900,      // 15 minute lockout
            enable_exponential_backoff: true,
        }
    }
}

/// Request tracking data
struct RequestData {
    count: u32,
    reset_time: u64,
    failed_auth_attempts: u32,
    lockout_until: Option<u64>,
    last_request_time: u64,
}

impl RequestData {
    fn new(window_secs: u64) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();

        Self {
            count: 1,
            reset_time: now + window_secs,
            failed_auth_attempts: 0,
            lockout_until: None,
            last_request_time: now,
        }
    }
}

/// Advanced rate limiter with DDoS protection
pub struct EnhancedRateLimiter {
    config: RateLimitConfig,
    ip_limits: Arc<RwLock<HashMap<IpAddr, RequestData>>>,
    user_limits: Arc<RwLock<HashMap<String, RequestData>>>,
    connection_counts: Arc<RwLock<HashMap<IpAddr, u32>>>,
    blocked_ips: Arc<RwLock<HashMap<IpAddr, u64>>>,  // IP -> unblock time
}

impl EnhancedRateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            ip_limits: Arc::new(RwLock::new(HashMap::new())),
            user_limits: Arc::new(RwLock::new(HashMap::new())),
            connection_counts: Arc::new(RwLock::new(HashMap::new())),
            blocked_ips: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Check if an IP is rate limited
    pub async fn check_ip(&self, ip: IpAddr) -> Result<bool> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| anyhow::anyhow!("System clock error: {}", e))?
            .as_secs();

        // Check if IP is blocked
        {
            let blocked = self.blocked_ips.read().await;
            if let Some(&unblock_time) = blocked.get(&ip) {
                if now < unblock_time {
                    warn!("Blocked IP {} attempted access", ip);
                    return Ok(false);
                }
            }
        }

        // Clean up blocked IPs that have expired
        {
            let mut blocked = self.blocked_ips.write().await;
            blocked.retain(|_, &mut unblock_time| now < unblock_time);
        }

        // Check and update IP rate limits
        let mut limits = self.ip_limits.write().await;
        let data = limits.entry(ip).or_insert_with(|| RequestData::new(self.config.window_secs));

        // Check if locked out due to failed auth
        if let Some(lockout_until) = data.lockout_until {
            if now < lockout_until {
                warn!("IP {} is locked out until {}", ip, lockout_until);
                return Ok(false);
            } else {
                // Lockout expired, reset failed attempts
                data.lockout_until = None;
                data.failed_auth_attempts = 0;
            }
        }

        // Reset if window expired
        if now > data.reset_time {
            data.count = 1;
            data.reset_time = now + self.config.window_secs;
            data.last_request_time = now;
            Ok(true)
        } else if data.count >= self.config.max_requests_per_ip {
            warn!("IP {} exceeded rate limit: {} requests", ip, data.count);
            Ok(false)
        } else {
            // Apply exponential backoff if enabled
            if self.config.enable_exponential_backoff && data.failed_auth_attempts > 0 {
                let backoff_ms = 100 * (2_u64.pow(data.failed_auth_attempts.min(10)));
                let time_since_last = now - data.last_request_time;

                if time_since_last < (backoff_ms / 1000) {
                    warn!("IP {} needs to wait {} ms (exponential backoff)", ip, backoff_ms);
                    return Ok(false);
                }
            }

            data.count += 1;
            data.last_request_time = now;
            Ok(true)
        }
    }

    /// Check if a user is rate limited
    pub async fn check_user(&self, username: &str) -> Result<bool> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| anyhow::anyhow!("System clock error: {}", e))?
            .as_secs();

        let mut limits = self.user_limits.write().await;
        let data = limits.entry(username.to_string())
            .or_insert_with(|| RequestData::new(self.config.window_secs));

        if now > data.reset_time {
            data.count = 1;
            data.reset_time = now + self.config.window_secs;
            Ok(true)
        } else if data.count >= self.config.max_requests_per_user {
            warn!("User {} exceeded rate limit: {} requests", username, data.count);
            Ok(false)
        } else {
            data.count += 1;
            Ok(true)
        }
    }

    /// Record a failed authentication attempt
    pub async fn record_failed_auth(&self, ip: IpAddr) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| anyhow::anyhow!("System clock error: {}", e))?
            .as_secs();

        let mut limits = self.ip_limits.write().await;
        let data = limits.entry(ip).or_insert_with(|| RequestData::new(self.config.window_secs));

        data.failed_auth_attempts += 1;

        if data.failed_auth_attempts >= self.config.max_failed_auth_attempts {
            data.lockout_until = Some(now + self.config.lockout_duration_secs);
            error!("IP {} locked out for {} seconds due to {} failed auth attempts",
                   ip, self.config.lockout_duration_secs, data.failed_auth_attempts);
        } else {
            warn!("Failed auth attempt {} from IP {}", data.failed_auth_attempts, ip);
        }

        Ok(())
    }

    /// Reset failed auth attempts on successful login
    pub async fn reset_failed_auth(&self, ip: IpAddr) -> Result<()> {
        let mut limits = self.ip_limits.write().await;
        if let Some(data) = limits.get_mut(&ip) {
            data.failed_auth_attempts = 0;
            data.lockout_until = None;
            info!("Reset failed auth attempts for IP {}", ip);
        }
        Ok(())
    }

    /// Register a new connection
    pub async fn register_connection(&self, ip: IpAddr) -> Result<bool> {
        let mut counts = self.connection_counts.write().await;
        let count = counts.entry(ip).or_insert(0);

        if *count >= self.config.max_connections_per_ip {
            warn!("IP {} exceeded max connections: {}", ip, count);
            Ok(false)
        } else {
            *count += 1;
            info!("IP {} now has {} connections", ip, count);
            Ok(true)
        }
    }

    /// Unregister a connection
    pub async fn unregister_connection(&self, ip: IpAddr) -> Result<()> {
        let mut counts = self.connection_counts.write().await;
        if let Some(count) = counts.get_mut(&ip) {
            if *count > 0 {
                *count -= 1;
                info!("IP {} now has {} connections", ip, count);
                if *count == 0 {
                    counts.remove(&ip);
                }
            }
        }
        Ok(())
    }

    /// Block an IP address for a specified duration
    pub async fn block_ip(&self, ip: IpAddr, duration_secs: u64) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| anyhow::anyhow!("System clock error: {}", e))?
            .as_secs();

        let mut blocked = self.blocked_ips.write().await;
        blocked.insert(ip, now + duration_secs);
        error!("Blocked IP {} for {} seconds", ip, duration_secs);
        Ok(())
    }

    /// Unblock an IP address
    pub async fn unblock_ip(&self, ip: IpAddr) -> Result<()> {
        let mut blocked = self.blocked_ips.write().await;
        if blocked.remove(&ip).is_some() {
            info!("Unblocked IP {}", ip);
        }
        Ok(())
    }

    /// Get current statistics
    pub async fn get_stats(&self) -> RateLimitStats {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();

        let ip_limits = self.ip_limits.read().await;
        let user_limits = self.user_limits.read().await;
        let connection_counts = self.connection_counts.read().await;
        let blocked_ips = self.blocked_ips.read().await;

        let active_ips = ip_limits.len();
        let active_users = user_limits.len();
        let total_connections: u32 = connection_counts.values().sum();
        let blocked_count = blocked_ips.len();

        let locked_out_ips = ip_limits.values()
            .filter(|d| d.lockout_until.map_or(false, |t| now < t))
            .count();

        RateLimitStats {
            active_ips,
            active_users,
            total_connections,
            blocked_ips: blocked_count,
            locked_out_ips,
        }
    }

    /// Clean up old entries
    pub async fn cleanup(&self) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| anyhow::anyhow!("System clock error: {}", e))?
            .as_secs();

        // Clean up expired IP limits
        {
            let mut limits = self.ip_limits.write().await;
            limits.retain(|_, data| {
                now < data.reset_time + 300 // Keep for 5 minutes after expiry
            });
        }

        // Clean up expired user limits
        {
            let mut limits = self.user_limits.write().await;
            limits.retain(|_, data| {
                now < data.reset_time + 300
            });
        }

        // Clean up expired blocked IPs
        {
            let mut blocked = self.blocked_ips.write().await;
            blocked.retain(|_, &mut unblock_time| now < unblock_time);
        }

        info!("Rate limiter cleanup completed");
        Ok(())
    }
}

/// Statistics about current rate limiting state
#[derive(Debug, Clone)]
pub struct RateLimitStats {
    pub active_ips: usize,
    pub active_users: usize,
    pub total_connections: u32,
    pub blocked_ips: usize,
    pub locked_out_ips: usize,
}

/// Middleware helper for automatic rate limiting
pub async fn apply_rate_limit_delay(failed_attempts: u32) {
    if failed_attempts > 0 {
        let delay_ms = 100 * (2_u64.pow(failed_attempts.min(10)));
        sleep(Duration::from_millis(delay_ms)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ip_rate_limiting() {
        let config = RateLimitConfig {
            max_requests_per_ip: 3,
            window_secs: 1,
            ..Default::default()
        };

        let limiter = EnhancedRateLimiter::new(config);
        let ip: IpAddr = "127.0.0.1".parse().unwrap();

        // First 3 requests should pass
        assert!(limiter.check_ip(ip).await.unwrap());
        assert!(limiter.check_ip(ip).await.unwrap());
        assert!(limiter.check_ip(ip).await.unwrap());

        // 4th request should fail
        assert!(!limiter.check_ip(ip).await.unwrap());

        // Wait for window to reset
        sleep(Duration::from_secs(2)).await;

        // Should pass again
        assert!(limiter.check_ip(ip).await.unwrap());
    }

    #[tokio::test]
    async fn test_failed_auth_lockout() {
        let config = RateLimitConfig {
            max_failed_auth_attempts: 3,
            lockout_duration_secs: 2,
            ..Default::default()
        };

        let limiter = EnhancedRateLimiter::new(config);
        let ip: IpAddr = "192.168.1.1".parse().unwrap();

        // Record failures
        for _ in 0..3 {
            limiter.record_failed_auth(ip).await.unwrap();
        }

        // Should be locked out
        assert!(!limiter.check_ip(ip).await.unwrap());

        // Wait for lockout to expire
        sleep(Duration::from_secs(3)).await;

        // Should pass again
        assert!(limiter.check_ip(ip).await.unwrap());
    }

    #[tokio::test]
    async fn test_connection_limiting() {
        let config = RateLimitConfig {
            max_connections_per_ip: 2,
            ..Default::default()
        };

        let limiter = EnhancedRateLimiter::new(config);
        let ip: IpAddr = "10.0.0.1".parse().unwrap();

        // Register 2 connections
        assert!(limiter.register_connection(ip).await.unwrap());
        assert!(limiter.register_connection(ip).await.unwrap());

        // 3rd should fail
        assert!(!limiter.register_connection(ip).await.unwrap());

        // Unregister one
        limiter.unregister_connection(ip).await.unwrap();

        // Should be able to register again
        assert!(limiter.register_connection(ip).await.unwrap());
    }
}