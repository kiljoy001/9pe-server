//! Statistics exposed as synthetic files
//!
//! Everything is a file, every file is a function!
//! Stats are exposed at /srv/stats/* as readable files that compute values on-the-fly.

use crate::synth::{ControlHandler, SyntheticFilesystem};
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// Statistics tracking for the server
pub struct ServerStats {
    start_time: SystemTime,
    connection_count: Arc<RwLock<u64>>,
    total_bytes_read: Arc<RwLock<u64>>,
    total_bytes_written: Arc<RwLock<u64>>,
    total_messages: Arc<RwLock<u64>>,
}

impl ServerStats {
    pub fn new() -> Self {
        Self {
            start_time: SystemTime::now(),
            connection_count: Arc::new(RwLock::new(0)),
            total_bytes_read: Arc::new(RwLock::new(0)),
            total_bytes_written: Arc::new(RwLock::new(0)),
            total_messages: Arc::new(RwLock::new(0)),
        }
    }

    /// Register stats files in the synthetic filesystem
    pub async fn register(&self, synth: &SyntheticFilesystem) -> Result<()> {
        use std::path::PathBuf;

        // Create /srv/stats directory
        synth.create_directory(&PathBuf::from("/srv/stats")).await?;

        // Uptime: Computed on every read
        synth.create_control_file(
            &PathBuf::from("/srv/stats/uptime"),
            Arc::new(UptimeHandler { start_time: self.start_time })
        ).await?;

        // Connection count: Live counter
        synth.create_control_file(
            &PathBuf::from("/srv/stats/connections"),
            Arc::new(CounterHandler { counter: self.connection_count.clone() })
        ).await?;

        // Bytes read: Live counter
        synth.create_control_file(
            &PathBuf::from("/srv/stats/bytes_read"),
            Arc::new(CounterHandler { counter: self.total_bytes_read.clone() })
        ).await?;

        // Bytes written: Live counter
        synth.create_control_file(
            &PathBuf::from("/srv/stats/bytes_written"),
            Arc::new(CounterHandler { counter: self.total_bytes_written.clone() })
        ).await?;

        // Messages processed: Live counter
        synth.create_control_file(
            &PathBuf::from("/srv/stats/messages"),
            Arc::new(CounterHandler { counter: self.total_messages.clone() })
        ).await?;

        // Version: Static info
        synth.create_control_file(
            &PathBuf::from("/srv/stats/version"),
            Arc::new(VersionHandler)
        ).await?;

        // Protocol: Static info
        synth.create_control_file(
            &PathBuf::from("/srv/stats/protocol"),
            Arc::new(ProtocolHandler)
        ).await?;

        // All stats in one file (Prometheus-style format)
        synth.create_control_file(
            &PathBuf::from("/srv/stats/all"),
            Arc::new(AllStatsHandler {
                start_time: self.start_time,
                connection_count: self.connection_count.clone(),
                total_bytes_read: self.total_bytes_read.clone(),
                total_bytes_written: self.total_bytes_written.clone(),
                total_messages: self.total_messages.clone(),
            })
        ).await?;

        Ok(())
    }

    /// Increment connection counter
    pub async fn increment_connections(&self) {
        *self.connection_count.write().await += 1;
    }

    /// Track bytes read
    pub async fn add_bytes_read(&self, bytes: u64) {
        *self.total_bytes_read.write().await += bytes;
    }

    /// Track bytes written
    pub async fn add_bytes_written(&self, bytes: u64) {
        *self.total_bytes_written.write().await += bytes;
    }

    /// Track message processed
    pub async fn increment_messages(&self) {
        *self.total_messages.write().await += 1;
    }
}

impl Default for ServerStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Handler for /srv/stats/uptime - computes uptime on every read
struct UptimeHandler {
    start_time: SystemTime,
}

impl ControlHandler for UptimeHandler {
    fn read(&self) -> Result<Vec<u8>> {
        let uptime = self.start_time.elapsed()
            .unwrap_or_default()
            .as_secs();
        Ok(format!("{}\n", uptime).into_bytes())
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("uptime is read-only"))
    }
}

/// Handler for counter files - reads current value
struct CounterHandler {
    counter: Arc<RwLock<u64>>,
}

impl ControlHandler for CounterHandler {
    fn read(&self) -> Result<Vec<u8>> {
        // Can't await in sync trait, so we use blocking
        let value = *futures::executor::block_on(self.counter.read());
        Ok(format!("{}\n", value).into_bytes())
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("counter is read-only"))
    }
}

/// Handler for /srv/stats/version
struct VersionHandler;

impl ControlHandler for VersionHandler {
    fn read(&self) -> Result<Vec<u8>> {
        Ok(format!("{}\n", crate::VERSION).into_bytes())
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("version is read-only"))
    }
}

/// Handler for /srv/stats/protocol
struct ProtocolHandler;

impl ControlHandler for ProtocolHandler {
    fn read(&self) -> Result<Vec<u8>> {
        Ok(b"9P.e/1.0\n".to_vec())
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("protocol is read-only"))
    }
}

/// Handler for /srv/stats/all - returns all stats in Prometheus format
struct AllStatsHandler {
    start_time: SystemTime,
    connection_count: Arc<RwLock<u64>>,
    total_bytes_read: Arc<RwLock<u64>>,
    total_bytes_written: Arc<RwLock<u64>>,
    total_messages: Arc<RwLock<u64>>,
}

impl ControlHandler for AllStatsHandler {
    fn read(&self) -> Result<Vec<u8>> {
        let uptime = self.start_time.elapsed()
            .unwrap_or_default()
            .as_secs();

        let connections = *futures::executor::block_on(self.connection_count.read());
        let bytes_read = *futures::executor::block_on(self.total_bytes_read.read());
        let bytes_written = *futures::executor::block_on(self.total_bytes_written.read());
        let messages = *futures::executor::block_on(self.total_messages.read());

        let output = format!(
            "# 9P.e Server Statistics\n\
             # HELP ninep_uptime_seconds Server uptime in seconds\n\
             # TYPE ninep_uptime_seconds gauge\n\
             ninep_uptime_seconds {}\n\
             \n\
             # HELP ninep_connections_total Total number of connections\n\
             # TYPE ninep_connections_total counter\n\
             ninep_connections_total {}\n\
             \n\
             # HELP ninep_bytes_read_total Total bytes read\n\
             # TYPE ninep_bytes_read_total counter\n\
             ninep_bytes_read_total {}\n\
             \n\
             # HELP ninep_bytes_written_total Total bytes written\n\
             # TYPE ninep_bytes_written_total counter\n\
             ninep_bytes_written_total {}\n\
             \n\
             # HELP ninep_messages_total Total 9P messages processed\n\
             # TYPE ninep_messages_total counter\n\
             ninep_messages_total {}\n\
             \n\
             # HELP ninep_version Server version\n\
             # TYPE ninep_version gauge\n\
             ninep_version{{version=\"{}\"}} 1\n\
             ",
            uptime, connections, bytes_read, bytes_written, messages, crate::VERSION
        );

        Ok(output.into_bytes())
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("all stats file is read-only"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_stats_registration() {
        let stats = ServerStats::new();
        let synth = SyntheticFilesystem::new();

        stats.register(&synth).await.expect("Failed to register stats");

        // Check that /srv/stats directory exists
        assert!(synth.exists(&std::path::PathBuf::from("/srv/stats")).await);
    }

    #[tokio::test]
    async fn test_connection_counter() {
        let stats = ServerStats::new();

        stats.increment_connections().await;
        stats.increment_connections().await;
        stats.increment_connections().await;

        let count = *stats.connection_count.read().await;
        assert_eq!(count, 3);
    }

    #[tokio::test]
    async fn test_uptime_increases() {
        let stats = ServerStats::new();
        let synth = SyntheticFilesystem::new();

        stats.register(&synth).await.expect("Failed to register");

        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        // Read uptime file
        let uptime_path = std::path::PathBuf::from("/srv/stats/uptime");
        let content = synth.read_file(&uptime_path).await.expect("Failed to read uptime");
        let uptime_str = String::from_utf8(content).expect("Invalid UTF-8");
        let uptime: u64 = uptime_str.trim().parse().expect("Invalid number");

        assert!(uptime >= 1, "Uptime should be at least 1 second");
    }
}
