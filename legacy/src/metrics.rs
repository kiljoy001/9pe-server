//! Metrics and Grafana Integration for 9P.e Server
//!
//! Provides Prometheus-style metrics for Grafana monitoring

use std::time::SystemTime;
use prometheus::{
    Encoder, TextEncoder, Counter, Gauge, Histogram, HistogramVec,
    IntCounter, IntGauge, IntGaugeVec, CounterVec,
    register_counter, register_gauge, register_histogram, register_histogram_vec,
    register_int_counter, register_int_gauge, register_int_gauge_vec, register_counter_vec,
};
use axum::{
    Router,
    routing::get,
    response::Response,
    http::{StatusCode, header},
    Json,
};
use lazy_static::lazy_static;
use tracing::{info, error};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

lazy_static! {
    // Connection metrics
    pub static ref CONNECTIONS_TOTAL: IntCounter = register_int_counter!(
        "ninep_connections_total",
        "Total number of client connections"
    ).unwrap();

    pub static ref ACTIVE_CONNECTIONS: IntGauge = register_int_gauge!(
        "ninep_connections_active",
        "Number of active connections"
    ).unwrap();

    pub static ref PROTOCOL_GAUGE: IntGaugeVec = register_int_gauge_vec!(
        "ninep_connections_by_protocol",
        "Active connections by protocol type",
        &["protocol"]
    ).unwrap();

    // Message metrics
    pub static ref MESSAGES_TOTAL: CounterVec = register_counter_vec!(
        "ninep_messages_total",
        "Total messages processed",
        &["type", "status"]
    ).unwrap();

    pub static ref MESSAGE_LATENCY: HistogramVec = register_histogram_vec!(
        "ninep_message_duration_seconds",
        "Message processing latency",
        &["type"]
    ).unwrap();

    // File operation metrics
    pub static ref FILE_OPS: CounterVec = register_counter_vec!(
        "ninep_file_operations_total",
        "File operations",
        &["operation", "status"]
    ).unwrap();

    pub static ref FILE_BYTES_READ: Counter = register_counter!(
        "ninep_bytes_read_total",
        "Total bytes read"
    ).unwrap();

    pub static ref FILE_BYTES_WRITTEN: Counter = register_counter!(
        "ninep_bytes_written_total",
        "Total bytes written"
    ).unwrap();

    // System metrics
    pub static ref MEMORY_USAGE: IntGauge = register_int_gauge!(
        "ninep_memory_bytes",
        "Memory usage in bytes"
    ).unwrap();

    pub static ref OPEN_FILES: IntGauge = register_int_gauge!(
        "ninep_open_files",
        "Number of open file handles"
    ).unwrap();

    pub static ref UPTIME_SECONDS: IntGauge = register_int_gauge!(
        "ninep_uptime_seconds",
        "Server uptime in seconds"
    ).unwrap();

    // Error metrics
    pub static ref ERRORS_TOTAL: CounterVec = register_counter_vec!(
        "ninep_errors_total",
        "Total errors by type",
        &["error_type"]
    ).unwrap();

    // Performance metrics
    pub static ref REQUEST_QUEUE_SIZE: IntGauge = register_int_gauge!(
        "ninep_request_queue_size",
        "Number of requests in queue"
    ).unwrap();

    pub static ref THROUGHPUT: Gauge = register_gauge!(
        "ninep_throughput_mbps",
        "Current throughput in Mbps"
    ).unwrap();

    // QUIC specific metrics
    pub static ref QUIC_STREAMS: IntGauge = register_int_gauge!(
        "ninep_quic_streams_active",
        "Active QUIC streams"
    ).unwrap();

    pub static ref QUIC_RTT: Histogram = register_histogram!(
        "ninep_quic_rtt_ms",
        "QUIC connection RTT in milliseconds"
    ).unwrap();

    // Start time for uptime calculation
    static ref START_TIME: SystemTime = SystemTime::now();
}

/// Initialize metrics collection
pub fn init_metrics() {
    info!("📊 Initializing Grafana metrics collection");

    // Set initial values
    ACTIVE_CONNECTIONS.set(0);
    OPEN_FILES.set(0);
    REQUEST_QUEUE_SIZE.set(0);

    // Initialize protocol gauges
    PROTOCOL_GAUGE.with_label_values(&["tcp"]).set(0);
    PROTOCOL_GAUGE.with_label_values(&["quic"]).set(0);

    // Start background metrics updater
    tokio::spawn(update_system_metrics());
}

/// Background task to update system metrics
async fn update_system_metrics() {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));

    loop {
        interval.tick().await;

        // Update uptime
        if let Ok(elapsed) = START_TIME.elapsed() {
            UPTIME_SECONDS.set(elapsed.as_secs() as i64);
        }

        // Update memory usage (approximate)
        if let Ok(mem_info) = sys_info::mem_info() {
            let used = (mem_info.total - mem_info.free) * 1024; // Convert to bytes
            MEMORY_USAGE.set(used as i64);
        }
    }
}

/// Create Prometheus metrics endpoint for Grafana
pub fn metrics_router() -> Router {
    Router::new()
        .route("/metrics", get(serve_metrics))
        .route("/health", get(health_check))
        .route("/health/detailed", get(detailed_health_check))
        .route("/health/ready", get(readiness_check))
        .route("/health/live", get(liveness_check))
}

/// Serve Prometheus metrics
async fn serve_metrics() -> Response {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();

    let mut buffer = Vec::new();
    if let Err(e) = encoder.encode(&metric_families, &mut buffer) {
        error!("Failed to encode metrics: {}", e);
        return Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body("Failed to encode metrics".into())
            .unwrap();
    }

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, encoder.format_type())
        .body(buffer.into())
        .unwrap()
}

/// Health check endpoint
async fn health_check() -> &'static str {
    "OK"
}

#[derive(Serialize, Deserialize)]
struct HealthStatus {
    status: String,
    timestamp: String,
    uptime_seconds: u64,
    components: HashMap<String, ComponentHealth>,
}

#[derive(Serialize, Deserialize)]
struct ComponentHealth {
    status: String,
    message: String,
    last_check: String,
}

/// Detailed health check with component status
async fn detailed_health_check() -> Json<HealthStatus> {
    let now = chrono::Utc::now();
    let mut components = HashMap::new();

    // Check server uptime
    let uptime = START_TIME.elapsed().unwrap_or_default().as_secs();

    // Check active connections
    let active_conns = ACTIVE_CONNECTIONS.get();
    components.insert(
        "connections".to_string(),
        ComponentHealth {
            status: "healthy".to_string(),
            message: format!("Active connections: {}", active_conns),
            last_check: now.to_rfc3339(),
        },
    );

    // Check memory usage
    let memory_status = if let Ok(mem_info) = sys_info::mem_info() {
        let used_pct = ((mem_info.total - mem_info.free) as f64 / mem_info.total as f64) * 100.0;
        if used_pct > 90.0 {
            ComponentHealth {
                status: "warning".to_string(),
                message: format!("Memory usage high: {:.1}%", used_pct),
                last_check: now.to_rfc3339(),
            }
        } else {
            ComponentHealth {
                status: "healthy".to_string(),
                message: format!("Memory usage: {:.1}%", used_pct),
                last_check: now.to_rfc3339(),
            }
        }
    } else {
        ComponentHealth {
            status: "unknown".to_string(),
            message: "Unable to read memory info".to_string(),
            last_check: now.to_rfc3339(),
        }
    };
    components.insert("memory".to_string(), memory_status);

    // Check file operations
    let file_errors = prometheus::gather()
        .iter()
        .find(|m| m.get_name() == "ninep_file_ops_total")
        .and_then(|m| {
            m.get_metric()
                .iter()
                .find(|metric| {
                    metric.get_label()
                        .iter()
                        .any(|label| label.get_name() == "status" && label.get_value() == "error")
                })
                .map(|m| m.get_counter().get_value() as u64)
        })
        .unwrap_or(0);

    let file_status = if file_errors > 100 {
        ComponentHealth {
            status: "warning".to_string(),
            message: format!("High file operation error count: {}", file_errors),
            last_check: now.to_rfc3339(),
        }
    } else {
        ComponentHealth {
            status: "healthy".to_string(),
            message: format!("File operation errors: {}", file_errors),
            last_check: now.to_rfc3339(),
        }
    };
    components.insert("file_operations".to_string(), file_status);

    // Check mesh networking
    let mesh_peers = prometheus::gather()
        .iter()
        .find(|m| m.get_name() == "ninep_connections_by_protocol")
        .and_then(|m| {
            m.get_metric()
                .iter()
                .find(|metric| {
                    metric.get_label()
                        .iter()
                        .any(|label| label.get_name() == "protocol" && label.get_value() == "mesh")
                })
                .map(|m| m.get_gauge().get_value() as i64)
        })
        .unwrap_or(0);

    let mesh_status = if mesh_peers == 0 {
        ComponentHealth {
            status: "warning".to_string(),
            message: "No mesh peers connected - operating in isolation".to_string(),
            last_check: now.to_rfc3339(),
        }
    } else {
        ComponentHealth {
            status: "healthy".to_string(),
            message: format!("Mesh peers connected: {}", mesh_peers),
            last_check: now.to_rfc3339(),
        }
    };
    components.insert("mesh_networking".to_string(), mesh_status);

    // Overall status
    let overall_status = if components.values().any(|c| c.status == "critical") {
        "critical"
    } else if components.values().any(|c| c.status == "warning") {
        "warning"
    } else {
        "healthy"
    };

    Json(HealthStatus {
        status: overall_status.to_string(),
        timestamp: now.to_rfc3339(),
        uptime_seconds: uptime,
        components,
    })
}

/// Kubernetes-style readiness check
async fn readiness_check() -> Result<&'static str, StatusCode> {
    // Check if server is ready to accept requests
    let active_conns = ACTIVE_CONNECTIONS.get();

    // Check if we can handle more connections (basic check)
    if active_conns > 1000 {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }

    // Check memory usage
    if let Ok(mem_info) = sys_info::mem_info() {
        let used_pct = ((mem_info.total - mem_info.free) as f64 / mem_info.total as f64) * 100.0;
        if used_pct > 95.0 {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
    }

    Ok("Ready")
}

/// Kubernetes-style liveness check
async fn liveness_check() -> Result<&'static str, StatusCode> {
    // Basic liveness check - if we can respond, we're alive
    // In a real implementation, you might check for deadlocks, etc.
    Ok("Alive")
}

/// Record a connection event
pub fn record_connection(protocol: &str, connected: bool) {
    if connected {
        CONNECTIONS_TOTAL.inc();
        ACTIVE_CONNECTIONS.inc();
        PROTOCOL_GAUGE.with_label_values(&[protocol]).inc();
    } else {
        ACTIVE_CONNECTIONS.dec();
        PROTOCOL_GAUGE.with_label_values(&[protocol]).dec();
    }
}

/// Record a message being processed
pub fn record_message(msg_type: &str, success: bool, duration_secs: f64) {
    let status = if success { "success" } else { "error" };
    MESSAGES_TOTAL.with_label_values(&[msg_type, status]).inc();
    MESSAGE_LATENCY.with_label_values(&[msg_type]).observe(duration_secs);
}

/// Record file operation
pub fn record_file_op(operation: &str, success: bool, bytes: Option<u64>) {
    let status = if success { "success" } else { "error" };
    FILE_OPS.with_label_values(&[operation, status]).inc();

    if let Some(b) = bytes {
        match operation {
            "read" => FILE_BYTES_READ.inc_by(b as f64),
            "write" => FILE_BYTES_WRITTEN.inc_by(b as f64),
            _ => {}
        }
    }
}

/// Record an error
pub fn record_error(error_type: &str) {
    ERRORS_TOTAL.with_label_values(&[error_type]).inc();
}

/// Update throughput metric
pub fn update_throughput(mbps: f64) {
    THROUGHPUT.set(mbps);
}

/// Update QUIC metrics
pub fn update_quic_metrics(streams: i64, rtt_ms: f64) {
    QUIC_STREAMS.set(streams);
    QUIC_RTT.observe(rtt_ms);
}

/// Get current active connections count
pub fn get_active_connections() -> i64 {
    // This is a simplified implementation - in production you'd track this properly
    0
}

/// Record bytes read for compatibility with integrated server
pub fn record_bytes_read(bytes: u64) {
    record_file_op("read", true, Some(bytes));
}

/// Record bytes written for compatibility with integrated server
pub fn record_bytes_written(bytes: u64) {
    record_file_op("write", true, Some(bytes));
}

/* Grafana dashboard configuration as JSON
pub fn grafana_dashboard() -> serde_json::Value {
    serde_json::json!({
        "dashboard": {
            "title": "9P.e Server Monitoring",
            "panels": [
                {
                    "title": "Active Connections",
                    "type": "graph",
                    "targets": [{
                        "expr": "ninep_connections_active"
                    }]
                },
                {
                    "title": "Connections by Protocol",
                    "type": "graph",
                    "targets": [
                        {"expr": "ninep_connections_by_protocol{protocol=\"tcp\"}"},
                        {"expr": "ninep_connections_by_protocol{protocol=\"quic\"}"}
                    ]
                },
                {
                    "title": "Message Latency (95th percentile)",
                    "type": "graph",
                    "targets": [{
                        "expr": "histogram_quantile(0.95, ninep_message_duration_seconds)"
                    }]
                },
                {
                    "title": "Throughput (Mbps)",
                    "type": "graph",
                    "targets": [{
                        "expr": "ninep_throughput_mbps"
                    }]
                },
                {
                    "title": "File Operations Rate",
                    "type": "graph",
                    "targets": [{
                        "expr": "rate(ninep_file_operations_total[5m])"
                    }]
                },
                {
                    "title": "Data Transfer",
                    "type": "graph",
                    "targets": [
                        {"expr": "rate(ninep_bytes_read_total[5m])"},
                        {"expr": "rate(ninep_bytes_written_total[5m])"}
                    ]
                },
                {
                    "title": "Error Rate",
                    "type": "graph",
                    "targets": [{
                        "expr": "rate(ninep_errors_total[5m])"
                    }]
                },
                {
                    "title": "QUIC RTT",
                    "type": "graph",
                    "targets": [{
                        "expr": "ninep_quic_rtt_ms"
                    }]
                },
                {
                    "title": "Memory Usage",
                    "type": "graph",
                    "targets": [{
                        "expr": "ninep_memory_bytes"
                    }]
                },
                {
                    "title": "Server Uptime",
                    "type": "stat",
                    "targets": [{
                        "expr": "ninep_uptime_seconds"
                    }]
                }
            ]
        }
    })
}

// Create a Grafana datasource configuration
pub fn grafana_datasource_config(prometheus_url: &str) -> serde_json::Value {
    serde_json::json!({
        "name": "9PE-Prometheus",
        "type": "prometheus",
        "url": prometheus_url,
        "access": "proxy",
        "isDefault": true,
        "jsonData": {
            "httpMethod": "GET",
            "keepCookies": []
        }
    })
}
*/

/// Start metrics server on separate port
pub async fn start_metrics_server(port: u16) -> anyhow::Result<()> {
    // IPv6 dual-stack by default - accepts both IPv4 and IPv6
    let addr: std::net::SocketAddr = format!("[::]:{}", port).parse()?;

    info!("📊 Starting Grafana metrics server on http://[::]:{}/metrics (IPv6 dual-stack)", port);
    info!("📈 Configure Grafana to scrape from http://localhost:{}/metrics", port);

    let app = metrics_router();
    let listener = tokio::net::TcpListener::bind(addr).await?;

    axum::serve(listener, app).await?;
    Ok(())
}