//! Synthetic File System Implementation
//!
//! Provides dynamically generated files that don't exist on disk

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use anyhow::Result;
use async_trait::async_trait;

/// Trait for synthetic file generators
#[async_trait]
pub trait SyntheticGenerator: Send + Sync {
    /// Generate file content on demand
    async fn generate(&self, offset: u64, count: u32) -> Result<Vec<u8>>;

    /// Get file size (can be dynamic)
    async fn size(&self) -> u64;

    /// Check if file supports streaming
    fn supports_streaming(&self) -> bool { false }

    /// Get refresh rate in milliseconds (0 = no auto-refresh)
    fn refresh_rate_ms(&self) -> u64 { 0 }
}

/// CPU info synthetic file
pub struct CpuInfoGenerator;

#[async_trait]
impl SyntheticGenerator for CpuInfoGenerator {
    async fn generate(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        // Would use sysinfo::System here
        // let info = sysinfo::System::new_all();
        let content = format!(
            "processor\t: {}\n\
             model name\t: {}\n\
             cpu MHz\t\t: {:.2}\n\
             cache size\t: {} KB\n\
             cpu cores\t: {}\n\
             bogomips\t: {:.2}\n",
            4,  // Mock CPU count
            "Intel Core i7",  // Mock CPU brand
            3200.0,  // Mock frequency
            8192,  // Mock cache
            4,  // Mock core count
            6400.0  // Mock bogomips
        );

        let bytes = content.as_bytes();
        let start = offset.min(bytes.len() as u64) as usize;
        let end = (start + count as usize).min(bytes.len());
        Ok(bytes[start..end].to_vec())
    }

    async fn size(&self) -> u64 {
        256 // Approximate size
    }
}

/// Memory info synthetic file
pub struct MemInfoGenerator;

#[async_trait]
impl SyntheticGenerator for MemInfoGenerator {
    async fn generate(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        // Mock memory info
        let content = format!(
            "MemTotal:       {} kB\n\
             MemFree:        {} kB\n\
             MemAvailable:   {} kB\n\
             Buffers:        0 kB\n\
             Cached:         {} kB\n\
             SwapTotal:      {} kB\n\
             SwapFree:       {} kB\n",
            16777216,  // 16GB total
            8388608,   // 8GB free
            10485760,  // 10GB available
            2097152,   // 2GB cached
            8388608,   // 8GB swap
            4194304    // 4GB swap free
        );

        let bytes = content.as_bytes();
        let start = offset.min(bytes.len() as u64) as usize;
        let end = (start + count as usize).min(bytes.len());
        Ok(bytes[start..end].to_vec())
    }

    async fn size(&self) -> u64 {
        512
    }
}

/// Network statistics synthetic file
pub struct NetStatGenerator;

#[async_trait]
impl SyntheticGenerator for NetStatGenerator {
    async fn generate(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        let content = format!(
            "Active Internet connections (servers and established)\n\
             Proto Recv-Q Send-Q Local Address           Foreign Address         State\n\
             tcp        0      0 0.0.0.0:5641            0.0.0.0:*               LISTEN\n\
             tcp        0      0 0.0.0.0:9090            0.0.0.0:*               LISTEN\n"
        );

        let bytes = content.as_bytes();
        let start = offset.min(bytes.len() as u64) as usize;
        let end = (start + count as usize).min(bytes.len());
        Ok(bytes[start..end].to_vec())
    }

    async fn size(&self) -> u64 {
        1024
    }
}

/// Random data generator (like /dev/urandom)
pub struct RandomGenerator;

#[async_trait]
impl SyntheticGenerator for RandomGenerator {
    async fn generate(&self, _offset: u64, count: u32) -> Result<Vec<u8>> {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let data: Vec<u8> = (0..count).map(|_| rng.gen()).collect();
        Ok(data)
    }

    async fn size(&self) -> u64 {
        u64::MAX // Infinite
    }

    fn supports_streaming(&self) -> bool {
        true
    }
}

/// Zero data generator (like /dev/zero)
pub struct ZeroGenerator;

#[async_trait]
impl SyntheticGenerator for ZeroGenerator {
    async fn generate(&self, _offset: u64, count: u32) -> Result<Vec<u8>> {
        Ok(vec![0u8; count as usize])
    }

    async fn size(&self) -> u64 {
        u64::MAX // Infinite
    }

    fn supports_streaming(&self) -> bool {
        true
    }
}

/// Current timestamp generator
pub struct TimestampGenerator;

#[async_trait]
impl SyntheticGenerator for TimestampGenerator {
    async fn generate(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let content = format!("{}\n", timestamp);
        let bytes = content.as_bytes();
        let start = offset.min(bytes.len() as u64) as usize;
        let end = (start + count as usize).min(bytes.len());
        Ok(bytes[start..end].to_vec())
    }

    async fn size(&self) -> u64 {
        20 // Enough for timestamp + newline
    }

    fn refresh_rate_ms(&self) -> u64 {
        1000 // Update every second
    }
}

/// Server metrics generator
pub struct MetricsGenerator;

#[async_trait]
impl SyntheticGenerator for MetricsGenerator {
    async fn generate(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        // Fetch current metrics from Prometheus endpoint
        let response = reqwest::get("http://localhost:9090/metrics").await?;
        let content = response.text().await?;

        let bytes = content.as_bytes();
        let start = offset.min(bytes.len() as u64) as usize;
        let end = (start + count as usize).min(bytes.len());
        Ok(bytes[start..end].to_vec())
    }

    async fn size(&self) -> u64 {
        8192 // Approximate
    }

    fn refresh_rate_ms(&self) -> u64 {
        5000 // Update every 5 seconds
    }
}

/// Process list generator
pub struct ProcessListGenerator;

#[async_trait]
impl SyntheticGenerator for ProcessListGenerator {
    async fn generate(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        // use sysinfo::{System, SystemExt, ProcessExt};

        // Mock process list
        let mut content = String::from("PID\tNAME\tCPU%\tMEM_MB\n");
        content.push_str("1\tsystemd\t0.1\t5.2\n");
        content.push_str("100\tbash\t0.0\t2.1\n");
        content.push_str("5641\t9pe-server\t1.5\t45.3\n");

        let bytes = content.as_bytes();
        let start = offset.min(bytes.len() as u64) as usize;
        let end = (start + count as usize).min(bytes.len());
        Ok(bytes[start..end].to_vec())
    }

    async fn size(&self) -> u64 {
        65536 // Variable, but bounded
    }

    fn refresh_rate_ms(&self) -> u64 {
        2000
    }
}

/// Manages all synthetic files in the system
pub struct SyntheticFileSystem {
    generators: Arc<RwLock<HashMap<String, Arc<dyn SyntheticGenerator>>>>,
}

impl SyntheticFileSystem {
    pub fn new() -> Self {
        let mut generators: HashMap<String, Arc<dyn SyntheticGenerator>> = HashMap::new();

        // Register default synthetic files
        generators.insert("/sys/cpuinfo".to_string(), Arc::new(CpuInfoGenerator));
        generators.insert("/sys/meminfo".to_string(), Arc::new(MemInfoGenerator));
        generators.insert("/sys/netstat".to_string(), Arc::new(NetStatGenerator));
        generators.insert("/dev/random".to_string(), Arc::new(RandomGenerator));
        generators.insert("/dev/zero".to_string(), Arc::new(ZeroGenerator));
        generators.insert("/sys/timestamp".to_string(), Arc::new(TimestampGenerator));
        generators.insert("/sys/metrics".to_string(), Arc::new(MetricsGenerator));
        generators.insert("/proc/list".to_string(), Arc::new(ProcessListGenerator));

        Self {
            generators: Arc::new(RwLock::new(generators)),
        }
    }

    /// Check if a path is a synthetic file
    pub async fn is_synthetic(&self, path: &str) -> bool {
        self.generators.read().await.contains_key(path)
    }

    /// Get generator for a path
    pub async fn get_generator(&self, path: &str) -> Option<Arc<dyn SyntheticGenerator>> {
        self.generators.read().await.get(path).cloned()
    }

    /// Register a new synthetic file
    pub async fn register(&self, path: String, generator: Arc<dyn SyntheticGenerator>) {
        self.generators.write().await.insert(path, generator);
    }

    /// List all synthetic files
    pub async fn list(&self) -> Vec<String> {
        self.generators.read().await.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cpu_info() {
        let gen = CpuInfoGenerator;
        let data = gen.generate(0, 100).await.unwrap();
        assert!(!data.is_empty());
        let content = String::from_utf8_lossy(&data);
        assert!(content.contains("processor"));
    }

    #[tokio::test]
    async fn test_random_generator() {
        let gen = RandomGenerator;
        let data1 = gen.generate(0, 16).await.unwrap();
        let data2 = gen.generate(0, 16).await.unwrap();
        assert_eq!(data1.len(), 16);
        assert_ne!(data1, data2); // Should be random
    }

    #[tokio::test]
    async fn test_zero_generator() {
        let gen = ZeroGenerator;
        let data = gen.generate(0, 32).await.unwrap();
        assert_eq!(data, vec![0u8; 32]);
    }
}