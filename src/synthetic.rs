//! Synthetic File System Implementation
//!
//! Provides dynamically generated files that don't exist on disk

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{RwLock, mpsc};
use anyhow::Result;
use async_trait::async_trait;

/// Metric types for chart generation
#[derive(Debug, Clone)]
pub enum MetricType {
    Cpu,
    Memory,
    Network,
    Process,
}

// Import from sibling module
#[cfg(not(test))]
use crate::modern_draw::{ModernDisplay, CanvasHtmlGenerator};
#[cfg(test)]
use super::modern_draw::{ModernDisplay, CanvasHtmlGenerator};

// Import Gaussian Splatting system
// use crate::gaussian_splat::{GaussianSplatGenerator, MetricType};

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
#[derive(Clone)]
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
#[derive(Clone)]
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

/// Simple chart generator for dashboard visualizations
pub struct GaussianSplatChartGenerator {
    metric_type: MetricType,
    chart_name: String,
}

impl GaussianSplatChartGenerator {
    pub fn new(metric_type: MetricType, chart_name: String) -> Self {
        Self {
            metric_type,
            chart_name,
        }
    }
}

#[async_trait]
impl SyntheticGenerator for GaussianSplatChartGenerator {
    async fn generate(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        // Generate mock data based on metric type
        let data = match self.metric_type {
            MetricType::Cpu => vec![45.0, 32.0, 67.0, 28.0, 55.0, 73.0, 41.0, 29.0],
            MetricType::Memory => vec![8.2, 6.1, 9.8, 7.3, 8.9, 7.6, 8.4, 9.1],
            MetricType::Network => vec![120.0, 85.0, 230.0, 67.0, 145.0, 98.0, 176.0, 134.0],
            MetricType::Process => vec![32.0, 28.0, 41.0, 35.0, 39.0, 33.0, 37.0, 31.0],
        };

        // Generate chart type identifier
        let chart_type = match self.metric_type {
            MetricType::Cpu => "cpu_usage",
            MetricType::Memory => "memory_usage",
            MetricType::Network => "network_activity",
            MetricType::Process => "process_count",
        };

        // Generate HTML content with embedded chart data
        let chart_data = format!("<div>Gaussian Splat Chart for {}</div>", self.chart_name);

        // Create HTML wrapper with embedded chart data
        let html_content = format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Gaussian Splat Chart - {}</title>
    <style>
        body {{
            margin: 0;
            padding: 20px;
            font-family: -apple-system, system-ui, sans-serif;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            color: white;
        }}
        .chart-container {{
            background: rgba(255, 255, 255, 0.95);
            border-radius: 16px;
            padding: 24px;
            box-shadow: 0 8px 32px rgba(0,0,0,0.1);
            backdrop-filter: blur(10px);
        }}
        .chart-title {{
            color: #2c3e50;
            margin-bottom: 16px;
            font-size: 1.2rem;
            font-weight: 600;
        }}
        .gaussian-chart {{
            width: 100%;
            height: 400px;
            border-radius: 8px;
            background: white;
            position: relative;
            overflow: hidden;
        }}
        .splat-point {{
            position: absolute;
            border-radius: 50%;
            opacity: 0.7;
            transition: all 0.3s ease;
        }}
        .splat-point:hover {{
            opacity: 1;
            transform: scale(1.2);
        }}
    </style>
</head>
<body>
    <div class="chart-container">
        <div class="chart-title">📊 {} - Gaussian Splat Visualization</div>
        <div class="gaussian-chart" id="chart">
            {}
        </div>
    </div>

    <script>
        // Auto-refresh every 5 seconds
        setTimeout(() => {{
            window.location.reload();
        }}, 5000);
    </script>
</body>
</html>"#, self.chart_name, self.chart_name, chart_data);

        let bytes = html_content.as_bytes();
        let start = offset.min(bytes.len() as u64) as usize;
        let end = (start + count as usize).min(bytes.len());
        Ok(bytes[start..end].to_vec())
    }

    async fn size(&self) -> u64 {
        16384 // Approximate HTML size
    }

    fn refresh_rate_ms(&self) -> u64 {
        5000 // Update every 5 seconds for live charts
    }
}

/// Service discovery synthetic file generator
/// Creates files in /srv/ that connect to discovered services
pub struct ServiceDiscoveryGenerator {
    /// Map of service name to connection info
    services: Arc<RwLock<HashMap<String, ServiceInfo>>>,
}

#[derive(Debug, Clone)]
pub struct ServiceInfo {
    pub node_id: String,
    pub service_addr: String,  // e.g., "192.168.1.114:5641"
    pub capabilities: Vec<String>,
    pub discovered_at: SystemTime,
}

impl ServiceDiscoveryGenerator {
    pub fn new() -> Self {
        Self {
            services: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a discovered service
    pub async fn register_service(&self, name: String, info: ServiceInfo) {
        self.services.write().await.insert(name, info);
    }

    /// Remove a service
    pub async fn unregister_service(&self, name: &str) {
        self.services.write().await.remove(name);
    }

    /// Get all registered services
    pub async fn list_services(&self) -> Vec<(String, ServiceInfo)> {
        self.services.read().await
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

#[async_trait]
impl SyntheticGenerator for ServiceDiscoveryGenerator {
    async fn generate(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        let services = self.services.read().await;

        // Generate directory listing
        let mut content = String::new();
        content.push_str("# Discovered 9P.e Services\n\n");

        if services.is_empty() {
            content.push_str("No services discovered yet.\n");
            content.push_str("Services will appear here as they are discovered via mesh networking.\n");
        } else {
            for (name, info) in services.iter() {
                content.push_str(&format!("## {}\n", name));
                content.push_str(&format!("- Address: {}\n", info.service_addr));
                content.push_str(&format!("- Node ID: {}\n", info.node_id));
                content.push_str(&format!("- Capabilities: {}\n", info.capabilities.join(", ")));
                content.push_str(&format!("- Access: 9pe-client connect {}\n\n", info.service_addr));
            }
        }

        let bytes = content.as_bytes();
        let start = offset.min(bytes.len() as u64) as usize;
        let end = (start + count as usize).min(bytes.len());
        Ok(bytes[start..end].to_vec())
    }

    async fn size(&self) -> u64 {
        1024 // Directory listing size
    }

    fn refresh_rate_ms(&self) -> u64 {
        5000 // Update as services are discovered
    }
}

/// Manages all synthetic files in the system
pub struct SyntheticFileSystem {
    generators: Arc<RwLock<HashMap<String, Arc<dyn SyntheticGenerator>>>>,
    draw_display: Arc<ModernDisplay>,
    service_discovery: Arc<ServiceDiscoveryGenerator>,
}

impl SyntheticFileSystem {
    pub fn new() -> Self {
        let mut generators: HashMap<String, Arc<dyn SyntheticGenerator>> = HashMap::new();
        let draw_display = Arc::new(ModernDisplay::new());
        let service_discovery = Arc::new(ServiceDiscoveryGenerator::new());

        // Register default synthetic files
        generators.insert("/sys/cpuinfo".to_string(), Arc::new(CpuInfoGenerator));
        generators.insert("/sys/meminfo".to_string(), Arc::new(MemInfoGenerator));
        generators.insert("/sys/netstat".to_string(), Arc::new(NetStatGenerator));
        generators.insert("/dev/random".to_string(), Arc::new(RandomGenerator));
        generators.insert("/dev/zero".to_string(), Arc::new(ZeroGenerator));
        generators.insert("/sys/timestamp".to_string(), Arc::new(TimestampGenerator));
        generators.insert("/sys/metrics".to_string(), Arc::new(MetricsGenerator));
        generators.insert("/proc/list".to_string(), Arc::new(ProcessListGenerator));

        // Register graphics system files
        generators.insert("/draw/main/canvas.html".to_string(),
            Arc::new(CanvasHtmlGenerator::new(draw_display.clone(), "main".to_string())));

        // UI generators removed - less bloat

        // Chart generators removed - less bloat

        // Register service discovery
        generators.insert("/srv".to_string(), service_discovery.clone() as Arc<dyn SyntheticGenerator>);

        Self {
            generators: Arc::new(RwLock::new(generators)),
            draw_display,
            service_discovery,
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

    /// Register a discovered service
    pub async fn register_service(&self, name: String, info: ServiceInfo) {
        self.service_discovery.register_service(name.clone(), info).await;

        // Also create a synthetic file for direct access
        let service_file = format!("/srv/{}", name);
        // This would be a translator that connects to the remote service
        // For now, it's informational
    }

    /// Get the service discovery generator
    pub fn service_discovery(&self) -> Arc<ServiceDiscoveryGenerator> {
        self.service_discovery.clone()
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