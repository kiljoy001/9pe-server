//! Compute control via synthetic files
//!
//! Submit GPU/WASM compute jobs by writing to files in /srv/compute/

use crate::gpu::GpuRuntime;
use crate::sycl::ffi::{
    sycl_discover_devices, sycl_get_device, sycl_get_device_count, sycl_get_device_info,
    sycl_release_device,
};
use crate::synth::{ControlHandler, SyntheticFilesystem};
use crate::wasm::threadsafe::TranslatorBackend;
use crate::wasm::ThreadSafeTranslatorRegistry;
use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

const SYSTEM_SYCL_MOUNT: &str = "/system/sycl";
const SYCL_COMPUTE_SUBMIT: &str = "/gpu/compute/submit";

/// Compute job status
#[derive(Clone, Debug)]
pub enum JobStatus {
    Pending,
    Running,
    Completed(Vec<u8>),
    Failed(String),
}

/// Compute job
#[derive(Clone, Debug)]
pub struct ComputeJob {
    pub id: String,
    pub job_type: String, // "sycl" or "wasm"
    pub operation: String,
    pub input: Vec<u8>,
    pub status: JobStatus,
    pub submitted_at: std::time::SystemTime,
    pub device_id: Option<String>,
    pub requested_vram: u64,
    pub allocated_vram: u64,
}

#[derive(Clone, Debug)]
pub struct JobSubmission {
    pub job_type: String,
    pub operation: String,
    pub payload: Vec<u8>,
    pub requested_vram: u64,
    pub device_hint: Option<usize>,
}

#[derive(Clone, Debug)]
struct JobExecutionRequest {
    job_id: String,
    submission: JobSubmission,
}

/// Compute job manager
pub struct ComputeManager {
    jobs: Arc<RwLock<HashMap<String, ComputeJob>>>,
    sycl_available: bool,
    gpu_runtimes: Vec<Arc<GpuRuntime>>,
    job_tx: UnboundedSender<JobExecutionRequest>,
    job_rx: Mutex<Option<UnboundedReceiver<JobExecutionRequest>>>,
    started: AtomicBool,
    translator_registry: RwLock<Option<Arc<ThreadSafeTranslatorRegistry>>>,
    allocations: RwLock<HashMap<String, (Arc<GpuRuntime>, u64)>>,
}

impl ComputeManager {
    pub fn new() -> Self {
        Self::with_runtimes(Vec::new())
    }

    pub fn with_runtimes(runtimes: Vec<Arc<GpuRuntime>>) -> Self {
        let sycl_available = Self::detect_sycl_availability();
        let (job_tx, job_rx) = unbounded_channel();

        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
            sycl_available,
            gpu_runtimes: runtimes,
            job_tx,
            job_rx: Mutex::new(Some(job_rx)),
            started: AtomicBool::new(false),
            translator_registry: RwLock::new(None),
            allocations: RwLock::new(HashMap::new()),
        }
    }

    fn detect_sycl_availability() -> bool {
        unsafe {
            if sycl_discover_devices().is_err() {
                return false;
            }
            let mut count: u32 = 0;
            if sycl_get_device_count(&mut count).is_err() {
                return false;
            }
            count > 0
        }
    }

    pub fn start_workers(self: &Arc<Self>) {
        if self
            .started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        let mut rx_guard = self
            .job_rx
            .lock()
            .expect("job_rx mutex poisoned while starting workers");
        if let Some(rx) = rx_guard.take() {
            let manager = Arc::clone(self);
            tokio::spawn(async move {
                manager.run_job_loop(rx).await;
            });
        }
    }

    pub async fn set_translator_registry(
        &self,
        registry: Arc<ThreadSafeTranslatorRegistry>,
    ) -> Result<()> {
        let mut guard = self.translator_registry.write().await;
        if guard.is_some() {
            warn!("translator_registry already configured; ignoring duplicate assignment");
            return Ok(());
        }
        *guard = Some(registry);
        Ok(())
    }

    async fn track_allocation(&self, job_id: &str, runtime: Arc<GpuRuntime>, bytes: u64) {
        if bytes == 0 {
            return;
        }
        let mut guard = self.allocations.write().await;
        guard.insert(job_id.to_string(), (runtime, bytes));
    }

    async fn release_allocation(&self, job_id: &str) {
        let mut guard = self.allocations.write().await;
        if let Some((runtime, bytes)) = guard.remove(job_id) {
            if bytes > 0 && !runtime.release(bytes) {
                warn!(
                    "release for job {} clamped; VRAM accounting may be inconsistent",
                    job_id
                );
            }
        }
    }

    async fn run_job_loop(self: Arc<Self>, mut rx: UnboundedReceiver<JobExecutionRequest>) {
        while let Some(request) = rx.recv().await {
            if let Err(err) = self.process_job(request).await {
                error!("compute job failed: {err:?}");
            }
        }
    }

    async fn process_job(&self, request: JobExecutionRequest) -> Result<()> {
        let JobExecutionRequest { job_id, submission } = request;

        let required_vram_hint = submission.requested_vram;
        let mut selected: Option<Arc<GpuRuntime>> = None;

        // Try to select a GPU runtime if available
        if !self.gpu_runtimes.is_empty() {
            // Build runtime iterator prioritizing device hint.
            if let Some(idx) = submission.device_hint {
                if let Some(runtime) = self.gpu_runtimes.get(idx) {
                    if required_vram_hint == 0 || runtime.free_vram() >= required_vram_hint {
                        selected = Some(runtime.clone());
                    } else {
                        warn!(
                            "device hint gpu{} rejected job {} (insufficient VRAM)",
                            idx, job_id
                        );
                    }
                } else {
                    warn!("job {} requested unavailable device index {}", job_id, idx);
                }
            }

            if selected.is_none() {
                selected = self
                    .gpu_runtimes
                    .iter()
                    .find(|runtime| {
                        required_vram_hint == 0 || runtime.free_vram() >= required_vram_hint
                    })
                    .cloned();
            }
        }

        // If no GPU runtime available, use CPU fallback with device_id "cpu"
        let (device_id, runtime) = if let Some(runtime) = selected {
            (runtime.device_id().to_string(), Some(runtime))
        } else {
            info!("No GPU runtime available for job {}, will use CPU fallback", job_id);
            ("cpu".to_string(), None)
        };
        let prepared_job = match submission.job_type.as_str() {
            "sycl" => {
                match prepare_sycl_job(&submission.operation, &submission.payload, &device_id) {
                    Ok(job) => job,
                    Err(err) => {
                        self.mark_job_failed(&job_id, &format!("Invalid SYCL payload: {}", err))
                            .await;
                        return Ok(());
                    }
                }
            }
            other => {
                self.mark_job_failed(&job_id, &format!("Unsupported job type '{}'", other))
                    .await;
                return Ok(());
            }
        };

        let actual_vram = prepared_job.required_vram();

        // Only attempt VRAM allocation if we have a GPU runtime
        if let Some(ref runtime) = runtime {
            if actual_vram > 0 && !runtime.allocate(actual_vram) {
                self.mark_job_failed(&job_id, &format!("Insufficient VRAM on {}", device_id))
                    .await;
                return Ok(());
            }

            self.track_allocation(&job_id, runtime.clone(), actual_vram)
                .await;
        }

        self.mark_job_running(&job_id, &device_id, actual_vram)
            .await;

        let translator_registry = {
            let guard = self.translator_registry.read().await;
            guard.clone()
        };

        let Some(translator_registry) = translator_registry else {
            self.mark_job_failed(&job_id, "Translator registry not initialized")
                .await;
            return Ok(());
        };

        let translator = translator_registry
            .get_translator(&PathBuf::from(SYSTEM_SYCL_MOUNT))
            .await;

        let outcome = execute_sycl_job(&job_id, &device_id, prepared_job, translator).await;
        match outcome {
            Ok(result_bytes) => {
                self.mark_job_completed(&job_id, result_bytes).await;
                debug!("job {} completed on {}", job_id, device_id);
            }
            Err(err) => {
                warn!("job {} failed: {}", job_id, err);
                self.mark_job_failed(&job_id, &err.to_string()).await;
            }
        }

        self.release_allocation(&job_id).await;
        Ok(())
    }

    async fn mark_job_running(&self, job_id: &str, device_id: &str, requested_vram: u64) {
        let mut jobs = self.jobs.write().await;
        if let Some(job) = jobs.get_mut(job_id) {
            job.status = JobStatus::Running;
            job.device_id = Some(device_id.to_string());
            job.allocated_vram = requested_vram;
        }
    }

    async fn mark_job_completed(&self, job_id: &str, result: Vec<u8>) {
        let mut jobs = self.jobs.write().await;
        if let Some(job) = jobs.get_mut(job_id) {
            job.status = JobStatus::Completed(result);
            job.input.clear();
        }
    }

    async fn mark_job_failed(&self, job_id: &str, reason: &str) {
        let mut jobs = self.jobs.write().await;
        if let Some(job) = jobs.get_mut(job_id) {
            job.status = JobStatus::Failed(reason.to_string());
        }
    }

    pub async fn submit_job(&self, submission: JobSubmission) -> Result<String> {
        let job_id = Uuid::new_v4().to_string();
        let job = ComputeJob {
            id: job_id.clone(),
            job_type: submission.job_type.clone(),
            operation: submission.operation.clone(),
            input: submission.payload.clone(),
            status: JobStatus::Pending,
            submitted_at: std::time::SystemTime::now(),
            device_id: None,
            requested_vram: submission.requested_vram,
            allocated_vram: 0,
        };

        self.jobs.write().await.insert(job_id.clone(), job);

        self.job_tx
            .send(JobExecutionRequest {
                job_id: job_id.clone(),
                submission,
            })
            .map_err(|e| anyhow::anyhow!("Failed to queue compute job: {e}"))?;

        Ok(job_id)
    }

    pub async fn get_job_status(&self, job_id: &str) -> Option<JobStatus> {
        self.jobs.read().await.get(job_id).map(|j| j.status.clone())
    }

    pub async fn list_jobs(&self) -> Vec<ComputeJob> {
        self.jobs.read().await.values().cloned().collect()
    }

    pub fn is_sycl_available(&self) -> bool {
        self.sycl_available
    }

    pub async fn get_job(&self, job_id: &str) -> Option<ComputeJob> {
        self.jobs.read().await.get(job_id).cloned()
    }
}

impl Default for ComputeManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Serialize)]
struct TranslatorArgument {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    buffer_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<Value>,
}

#[derive(Serialize)]
struct TranslatorJobRequest {
    kernel: String,
    device_id: String,
    work_dims: Vec<usize>,
    arguments: Vec<TranslatorArgument>,
}

#[derive(Deserialize)]
struct TranslatorJobResponse {
    status: String,
    message: String,
    result: Option<Value>,
}

enum PreparedSyclData {
    VectorAdd {
        a: Vec<f32>,
        b: Vec<f32>,
    },
    MatrixMultiply {
        a: Vec<f32>,
        b: Vec<f32>,
        m: u32,
        n: u32,
        k: u32,
    },
}

struct PreparedSyclJob {
    request_bytes: Vec<u8>,
    required_vram: u64,
    data: PreparedSyclData,
}

impl PreparedSyclJob {
    fn required_vram(&self) -> u64 {
        self.required_vram
    }

    fn request_bytes(&self) -> &[u8] {
        &self.request_bytes
    }

    fn into_fallback_result(self) -> Result<Vec<u8>> {
        match self.data {
            PreparedSyclData::VectorAdd { a, b } => {
                if a.len() != b.len() {
                    anyhow::bail!("vector_add arguments must be same length");
                }
                let values = cpu_vector_add(&a, &b);
                Ok(serde_json::to_vec(&json!({ "values": values }))?)
            }
            PreparedSyclData::MatrixMultiply { a, b, m, n, k } => {
                let output = cpu_matrix_multiply(&a, &b, m as usize, n as usize, k as usize);
                Ok(serde_json::to_vec(&json!({
                    "values": output,
                    "m": m,
                    "n": n,
                    "k": k,
                }))?)
            }
        }
    }
}

/// CPU fallback for vector addition with SIMD optimization
///
/// This function automatically detects and uses the best available SIMD instruction set:
/// - x86_64: AVX2 (8 floats/cycle) > SSE4.1 (4 floats/cycle) > scalar
/// - aarch64: NEON (4 floats/cycle) > scalar
///
/// Performance on modern CPUs:
/// - AVX2: ~5-8 GFLOPS for vector operations
/// - SSE4.1: ~2-4 GFLOPS
/// - NEON: ~2-4 GFLOPS
fn cpu_vector_add(a: &[f32], b: &[f32]) -> Vec<f32> {
    let len = a.len();
    let mut result = vec![0f32; len];

    // Try AVX2 (8 floats at a time)
    #[cfg(target_arch = "x86_64")]
    {
        // Runtime detection for AVX2
        if is_x86_feature_detected!("avx2") {
            unsafe {
                cpu_vector_add_avx2(a, b, &mut result);
            }
            return result;
        }

        // Runtime detection for SSE4.1 (4 floats at a time)
        if is_x86_feature_detected!("sse4.1") {
            unsafe {
                cpu_vector_add_sse41(a, b, &mut result);
                return result;
            }
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        if std::arch::is_aarch64_feature_detected!("neon") {
            unsafe {
                cpu_vector_add_neon(a, b, &mut result);
                return result;
            }
        }
    }

    // Scalar fallback
    for i in 0..len {
        result[i] = a[i] + b[i];
    }
    result
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn cpu_vector_add_avx2(a: &[f32], b: &[f32], result: &mut [f32]) {
    use std::arch::x86_64::*;

    let len = a.len();
    let chunks = len / 8;
    let remainder = len % 8;

    for i in 0..chunks {
        let idx = i * 8;
        let va = _mm256_loadu_ps(a.as_ptr().add(idx));
        let vb = _mm256_loadu_ps(b.as_ptr().add(idx));
        let vr = _mm256_add_ps(va, vb);
        _mm256_storeu_ps(result.as_mut_ptr().add(idx), vr);
    }

    // Handle remainder
    let base = chunks * 8;
    for i in 0..remainder {
        result[base + i] = a[base + i] + b[base + i];
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse4.1")]
unsafe fn cpu_vector_add_sse41(a: &[f32], b: &[f32], result: &mut [f32]) {
    use std::arch::x86_64::*;

    let len = a.len();
    let chunks = len / 4;
    let remainder = len % 4;

    for i in 0..chunks {
        let idx = i * 4;
        let va = _mm_loadu_ps(a.as_ptr().add(idx));
        let vb = _mm_loadu_ps(b.as_ptr().add(idx));
        let vr = _mm_add_ps(va, vb);
        _mm_storeu_ps(result.as_mut_ptr().add(idx), vr);
    }

    let base = chunks * 4;
    for i in 0..remainder {
        result[base + i] = a[base + i] + b[base + i];
    }
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn cpu_vector_add_neon(a: &[f32], b: &[f32], result: &mut [f32]) {
    use std::arch::aarch64::*;

    let len = a.len();
    let chunks = len / 4;
    let remainder = len % 4;

    for i in 0..chunks {
        let idx = i * 4;
        let va = vld1q_f32(a.as_ptr().add(idx));
        let vb = vld1q_f32(b.as_ptr().add(idx));
        let vr = vaddq_f32(va, vb);
        vst1q_f32(result.as_mut_ptr().add(idx), vr);
    }

    let base = chunks * 4;
    for i in 0..remainder {
        result[base + i] = a[base + i] + b[base + i];
    }
}

/// CPU fallback for matrix multiplication with cache-blocking optimization
///
/// Uses a tiled/blocked algorithm to improve cache locality:
/// - For large matrices: 32x32 tiles to fit in L1 cache
/// - For small matrices: simple triple-loop algorithm
///
/// Cache blocking reduces cache misses by a factor of ~10x, improving
/// performance from ~0.1 GFLOPS to ~1-2 GFLOPS on modern CPUs.
///
/// Future optimizations could include:
/// - SIMD vectorization of inner loops (AVX2/NEON)
/// - Multi-threading with rayon for parallel tiles
/// - Use BLAS libraries (OpenBLAS, Intel MKL) for production workloads
fn cpu_matrix_multiply(a: &[f32], b: &[f32], m: usize, n: usize, k: usize) -> Vec<f32> {
    let mut output = vec![0f32; m * n];

    // Use cache-friendly blocking/tiling for larger matrices
    const TILE_SIZE: usize = 32;

    if m >= TILE_SIZE && n >= TILE_SIZE && k >= TILE_SIZE {
        // Tiled matrix multiply for better cache utilization
        for i_tile in (0..m).step_by(TILE_SIZE) {
            for j_tile in (0..n).step_by(TILE_SIZE) {
                for k_tile in (0..k).step_by(TILE_SIZE) {
                    let i_max = (i_tile + TILE_SIZE).min(m);
                    let j_max = (j_tile + TILE_SIZE).min(n);
                    let k_max = (k_tile + TILE_SIZE).min(k);

                    for i in i_tile..i_max {
                        for j in j_tile..j_max {
                            let mut acc = output[i * n + j];
                            for k_idx in k_tile..k_max {
                                acc += a[i * k + k_idx] * b[k_idx * n + j];
                            }
                            output[i * n + j] = acc;
                        }
                    }
                }
            }
        }
    } else {
        // Simple multiplication for small matrices
        for row in 0..m {
            for col in 0..n {
                let mut acc = 0f32;
                for inner in 0..k {
                    acc += a[row * k + inner] * b[inner * n + col];
                }
                output[row * n + col] = acc;
            }
        }
    }

    output
}

fn prepare_sycl_job(operation: &str, payload: &[u8], device_id: &str) -> Result<PreparedSyclJob> {
    let value: Value = serde_json::from_slice(payload).context("Decode SYCL payload JSON")?;

    match operation {
        "vector_add" => prepare_vector_add(&value, device_id),
        "matrix_multiply" => prepare_matrix_multiply(&value, device_id),
        other => anyhow::bail!("Unsupported SYCL kernel '{}'", other),
    }
}

fn prepare_vector_add(value: &Value, device_id: &str) -> Result<PreparedSyclJob> {
    let a_vals = value
        .get("a")
        .ok_or_else(|| anyhow::anyhow!("vector_add payload missing 'a' field"))?;
    let b_vals = value
        .get("b")
        .ok_or_else(|| anyhow::anyhow!("vector_add payload missing 'b' field"))?;

    let a = parse_f32_array(a_vals, "a")?;
    let b = parse_f32_array(b_vals, "b")?;

    if a.len() != b.len() {
        anyhow::bail!("vector_add arguments must be same length");
    }

    if a.is_empty() {
        anyhow::bail!("vector_add requires at least one element");
    }

    let work_dims = value
        .get("work_dims")
        .and_then(|dims| dims.as_array())
        .map(|dims| {
            dims.iter()
                .filter_map(|dim| dim.as_u64().map(|d| d as usize))
                .collect::<Vec<_>>()
        })
        .filter(|dims: &Vec<_>| !dims.is_empty())
        .unwrap_or_else(|| vec![a.len()]);

    let req = TranslatorJobRequest {
        kernel: "vector_add".to_string(),
        device_id: device_id.to_string(),
        work_dims,
        arguments: vec![
            TranslatorArgument {
                name: "a".to_string(),
                buffer_id: None,
                value: Some(serde_json::to_value(&a)?),
            },
            TranslatorArgument {
                name: "b".to_string(),
                buffer_id: None,
                value: Some(serde_json::to_value(&b)?),
            },
        ],
    };

    let request_bytes = serde_json::to_vec(&req)?;
    let bytes_per_vec = (a.len() * std::mem::size_of::<f32>()) as u64;
    let required_vram = bytes_per_vec * 3; // two inputs + output

    Ok(PreparedSyclJob {
        request_bytes,
        required_vram,
        data: PreparedSyclData::VectorAdd { a, b },
    })
}

fn parse_f32_array(value: &Value, field: &str) -> Result<Vec<f32>> {
    let arr = value
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("'{}' must be an array", field))?;
    let mut output = Vec::with_capacity(arr.len());
    for item in arr {
        let number = item
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("'{}' array must contain numbers", field))?;
        output.push(number as f32);
    }
    Ok(output)
}

fn prepare_matrix_multiply(value: &Value, device_id: &str) -> Result<PreparedSyclJob> {
    let a_vals = value
        .get("a")
        .ok_or_else(|| anyhow::anyhow!("matrix_multiply payload missing 'a' field"))?;
    let b_vals = value
        .get("b")
        .ok_or_else(|| anyhow::anyhow!("matrix_multiply payload missing 'b' field"))?;

    let a = parse_f32_array(a_vals, "a")?;
    let b = parse_f32_array(b_vals, "b")?;

    let dims = value
        .get("dims")
        .or_else(|| {
            if value.get("m").is_some() && value.get("n").is_some() && value.get("k").is_some() {
                Some(value)
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow::anyhow!("matrix_multiply payload missing 'dims' or m/n/k fields"))?;

    let m = dims
        .get("m")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow::anyhow!("matrix_multiply requires integer 'm'"))? as u32;
    let n = dims
        .get("n")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow::anyhow!("matrix_multiply requires integer 'n'"))? as u32;
    let k = dims
        .get("k")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow::anyhow!("matrix_multiply requires integer 'k'"))? as u32;

    if a.len() != (m * k) as usize {
        anyhow::bail!(
            "matrix_multiply expected {} elements in 'a', found {}",
            m * k,
            a.len()
        );
    }
    if b.len() != (k * n) as usize {
        anyhow::bail!(
            "matrix_multiply expected {} elements in 'b', found {}",
            k * n,
            b.len()
        );
    }

    let work_dims = vec![m as usize, n as usize, k as usize];
    let req = TranslatorJobRequest {
        kernel: "matrix_multiply".to_string(),
        device_id: device_id.to_string(),
        work_dims,
        arguments: vec![
            TranslatorArgument {
                name: "a".to_string(),
                buffer_id: None,
                value: Some(serde_json::to_value(&a)?),
            },
            TranslatorArgument {
                name: "b".to_string(),
                buffer_id: None,
                value: Some(serde_json::to_value(&b)?),
            },
            TranslatorArgument {
                name: "m".to_string(),
                buffer_id: None,
                value: Some(Value::from(m)),
            },
            TranslatorArgument {
                name: "n".to_string(),
                buffer_id: None,
                value: Some(Value::from(n)),
            },
            TranslatorArgument {
                name: "k".to_string(),
                buffer_id: None,
                value: Some(Value::from(k)),
            },
        ],
    };

    let request_bytes = serde_json::to_vec(&req)?;
    let bytes_a = (a.len() * std::mem::size_of::<f32>()) as u64;
    let bytes_b = (b.len() * std::mem::size_of::<f32>()) as u64;
    let bytes_c = (m as usize * n as usize * std::mem::size_of::<f32>()) as u64;
    let required_vram = bytes_a + bytes_b + bytes_c;

    Ok(PreparedSyclJob {
        request_bytes,
        required_vram,
        data: PreparedSyclData::MatrixMultiply { a, b, m, n, k },
    })
}

async fn execute_sycl_job(
    job_id: &str,
    device_id: &str,
    prepared: PreparedSyclJob,
    translator: Option<Arc<dyn TranslatorBackend>>,
) -> Result<Vec<u8>> {
    let request = prepared.request_bytes().to_vec();

    if let Some(translator) = translator {
        match translator.write_file(SYCL_COMPUTE_SUBMIT, 0, request).await {
            Ok(bytes) => {
                let response: TranslatorJobResponse =
                    serde_json::from_slice(&bytes).context("Parse translator job response")?;
                if response.status.to_lowercase() == "completed" {
                    if let Some(result) = response.result {
                        let result_bytes = serde_json::to_vec(&result)?;
                        return Ok(result_bytes);
                    }
                    return Ok(Vec::new());
                }

                warn!(
                    "System translator reported failure for job {} on {}: {}",
                    job_id, device_id, response.message
                );
            }
            Err(err) => {
                warn!(
                    "Failed to submit job {} to system translator: {}",
                    job_id, err
                );
            }
        }
    } else {
        info!(
            "No SYCL translator available for job {}, using CPU fallback",
            job_id
        );
    }

    // Translator path failed or unavailable, fall back to CPU execution.
    let fallback = prepared.into_fallback_result()?;
    info!(
        "Falling back to CPU execution for job {} on {}",
        job_id, device_id
    );
    Ok(fallback)
}

/// Register compute control files in the synthetic filesystem
pub async fn register_compute_control(
    synth: &Arc<SyntheticFilesystem>,
    manager: Arc<ComputeManager>,
    translators: Arc<ThreadSafeTranslatorRegistry>,
) -> Result<()> {
    manager.set_translator_registry(translators.clone()).await?;
    manager.start_workers();

    // Create /srv/compute directory
    synth
        .create_directory(&PathBuf::from("/srv/compute"))
        .await?;

    // /srv/compute/submit - Write job spec to submit
    synth
        .create_control_file(
            &PathBuf::from("/srv/compute/submit"),
            Arc::new(SubmitHandler {
                manager: manager.clone(),
                synth: Arc::clone(synth),
            }),
        )
        .await?;

    // /srv/compute/jobs - Read list of jobs
    synth
        .create_control_file(
            &PathBuf::from("/srv/compute/jobs"),
            Arc::new(JobsHandler {
                manager: manager.clone(),
                translators: translators.clone(),
            }),
        )
        .await?;

    // /srv/compute/devices - Read available GPU devices
    synth
        .create_control_file(
            &PathBuf::from("/srv/compute/devices"),
            Arc::new(DevicesHandler {
                manager: manager.clone(),
            }),
        )
        .await?;

    // /srv/compute/status - Read compute system status
    synth
        .create_control_file(
            &PathBuf::from("/srv/compute/status"),
            Arc::new(StatusHandler {
                manager: manager.clone(),
                translators: translators.clone(),
            }),
        )
        .await?;

    Ok(())
}

/// Handler for /srv/compute/submit - submit compute job
struct SubmitHandler {
    manager: Arc<ComputeManager>,
    synth: Arc<SyntheticFilesystem>,
}

impl ControlHandler for SubmitHandler {
    fn read(&self) -> Result<Vec<u8>> {
        Ok(b"Write job spec (JSON format):\n\
             {\n\
               \"type\": \"sycl\" | \"wasm\",\n\
               \"operation\": \"vector_add\" | \"matrix_multiply\" | \"custom\",\n\
               \"data\": \"base64-encoded-input\",\n\
               \"vram_bytes\": 1048576,        // optional hint\n\
               \"device\": 0                   // optional GPU index\n\
             }\n"
        .to_vec())
    }

    fn write(&self, data: &[u8]) -> Result<()> {
        let job_spec = String::from_utf8(data.to_vec())?;
        let spec: serde_json::Value = serde_json::from_str(&job_spec)?;

        let job_type = spec
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'type' field"))?
            .to_string();

        let operation = spec
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or("custom")
            .to_string();

        let data_b64 = spec
            .get("data")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'data' field"))?;
        let payload = BASE64
            .decode(data_b64)
            .context("Failed to decode base64 payload")?;

        let requested_vram = spec
            .get("vram_bytes")
            .and_then(|v| v.as_u64())
            .unwrap_or(payload.len() as u64);
        let device_hint = spec
            .get("device")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        let submission = JobSubmission {
            job_type,
            operation,
            payload,
            requested_vram,
            device_hint,
        };

        let job_id = futures::executor::block_on(self.manager.submit_job(submission))?;

        futures::executor::block_on(async {
            publish_job_files(Arc::clone(&self.synth), Arc::clone(&self.manager), &job_id).await
        })?;

        info!("compute job queued: {}", job_id);
        Ok(())
    }
}

enum JobFileKind {
    Status,
    Result,
}

struct JobFileHandler {
    manager: Arc<ComputeManager>,
    job_id: String,
    kind: JobFileKind,
}

impl JobFileHandler {
    fn new(manager: Arc<ComputeManager>, job_id: String, kind: JobFileKind) -> Self {
        Self {
            manager,
            job_id,
            kind,
        }
    }
}

impl ControlHandler for JobFileHandler {
    fn read(&self) -> Result<Vec<u8>> {
        let job = futures::executor::block_on(self.manager.get_job(&self.job_id))
            .ok_or_else(|| anyhow::anyhow!("Job {} not found", self.job_id))?;

        match self.kind {
            JobFileKind::Status => {
                let status_str = match &job.status {
                    JobStatus::Pending => "pending",
                    JobStatus::Running => "running",
                    JobStatus::Completed(_) => "completed",
                    JobStatus::Failed(_) => "failed",
                };

                let parsed_result = match &job.status {
                    JobStatus::Completed(bytes) => serde_json::from_slice::<Value>(bytes).ok(),
                    _ => None,
                };

                let failure_reason = match &job.status {
                    JobStatus::Failed(reason) => Some(reason.clone()),
                    _ => None,
                };

                let submitted_secs = job
                    .submitted_at
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);

                let body = json!({
                    "id": job.id,
                    "type": job.job_type,
                    "operation": job.operation,
                    "status": status_str,
                    "device": job.device_id,
                    "requested_vram": job.requested_vram,
                    "allocated_vram": job.allocated_vram,
                    "submitted_at": submitted_secs,
                    "failure_reason": failure_reason,
                    "result_preview": parsed_result,
                });

                Ok(serde_json::to_vec(&body)?)
            }
            JobFileKind::Result => match job.status {
                JobStatus::Completed(bytes) => Ok(bytes),
                JobStatus::Failed(reason) => {
                    Err(anyhow::anyhow!("Job {} failed: {}", self.job_id, reason))
                }
                JobStatus::Pending | JobStatus::Running => {
                    Err(anyhow::anyhow!("Job {} not completed yet", self.job_id))
                }
            },
        }
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("read-only job control file"))
    }
}

async fn publish_job_files(
    synth: Arc<SyntheticFilesystem>,
    manager: Arc<ComputeManager>,
    job_id: &str,
) -> Result<()> {
    let job_dir = PathBuf::from(format!("/srv/compute/jobs/{}", job_id));
    synth.create_directory(&job_dir).await?;

    synth
        .create_control_file(
            &job_dir.join("status"),
            Arc::new(JobFileHandler::new(
                Arc::clone(&manager),
                job_id.to_string(),
                JobFileKind::Status,
            )),
        )
        .await?;

    synth
        .create_control_file(
            &job_dir.join("result"),
            Arc::new(JobFileHandler::new(
                manager,
                job_id.to_string(),
                JobFileKind::Result,
            )),
        )
        .await?;

    Ok(())
}

/// Handler for /srv/compute/jobs - list all jobs
struct JobsHandler {
    manager: Arc<ComputeManager>,
    translators: Arc<ThreadSafeTranslatorRegistry>,
}

impl ControlHandler for JobsHandler {
    fn read(&self) -> Result<Vec<u8>> {
        let jobs = futures::executor::block_on(self.manager.list_jobs());
        let translator_jobs = collect_system_jobs(&self.translators).unwrap_or_default();

        let mut output = String::from("Compute Jobs\n============\n");
        for job in jobs {
            let status_str = match &job.status {
                JobStatus::Pending => "pending".to_string(),
                JobStatus::Running => "running".to_string(),
                JobStatus::Completed(result) => {
                    format!("completed ({} bytes)", result.len())
                }
                JobStatus::Failed(e) => format!("failed: {}", e),
            };

            output.push_str(&format!(
                "{}\t{}::{}\t{}\tdevice={}\tvram={}\t{:?}\n",
                job.id,
                job.job_type,
                job.operation,
                status_str,
                job.device_id.as_deref().unwrap_or("-"),
                job.allocated_vram,
                job.submitted_at
            ));
        }

        if !translator_jobs.is_empty() {
            output.push_str("\nSystem Translator Jobs\n----------------------\n");
            for job in translator_jobs {
                output.push_str(&format!(
                    "{}\tkernel={}\tstatus={}\tdevice={}\texec={} ns\n",
                    job.id,
                    job.kernel,
                    job.status,
                    job.device_id,
                    job.execution_time_ns.unwrap_or(0)
                ));
            }
        }

        Ok(output.into_bytes())
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!(
            "jobs file is read-only, use 'submit' to add jobs"
        ))
    }
}

/// Handler for /srv/compute/devices - list GPU devices
struct DevicesHandler {
    manager: Arc<ComputeManager>,
}

impl ControlHandler for DevicesHandler {
    fn read(&self) -> Result<Vec<u8>> {
        use std::ffi::CStr;

        if !self.manager.is_sycl_available() {
            return Ok(b"SYCL not available - no GPU devices detected\n\
                       Install AdaptiveCpp for GPU support\n"
                .to_vec());
        }

        unsafe {
            if sycl_discover_devices().is_err() {
                return Ok(b"Failed to discover SYCL devices\n".to_vec());
            }

            let mut count: u32 = 0;
            if sycl_get_device_count(&mut count).is_err() {
                return Ok(b"Failed to get device count\n".to_vec());
            }

            let mut output = String::from("SYCL Devices\n============\n");

            for i in 0..count {
                let mut device = std::ptr::null_mut();
                if sycl_get_device(i, &mut device).is_err() {
                    continue;
                }

                let mut name_buf = vec![0i8; 256];
                let mut backend: i32 = 0;

                if sycl_get_device_info(device, name_buf.as_mut_ptr(), name_buf.len(), &mut backend)
                    .is_ok()
                {
                    let name = CStr::from_ptr(name_buf.as_ptr())
                        .to_str()
                        .unwrap_or("Unknown");

                    let backend_str = match backend {
                        0 => "OpenCL",
                        1 => "CUDA",
                        2 => "HIP",
                        3 => "Level-Zero (Intel)",
                        4 => "CPU",
                        _ => "Unknown",
                    };

                    output.push_str(&format!(
                        "Device {}: {}\n  Backend: {}\n  Type: {}\n",
                        i,
                        name,
                        backend_str,
                        if backend == 4 { "CPU" } else { "GPU" }
                    ));
                }

                sycl_release_device(device);
            }

            Ok(output.into_bytes())
        }
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("devices file is read-only"))
    }
}

/// Handler for /srv/compute/status - compute system status
struct StatusHandler {
    manager: Arc<ComputeManager>,
    translators: Arc<ThreadSafeTranslatorRegistry>,
}

impl ControlHandler for StatusHandler {
    fn read(&self) -> Result<Vec<u8>> {
        let jobs = futures::executor::block_on(self.manager.list_jobs());
        let translator_jobs = collect_system_jobs(&self.translators).unwrap_or_default();

        let mut pending = jobs
            .iter()
            .filter(|j| matches!(j.status, JobStatus::Pending))
            .count();
        let mut running = jobs
            .iter()
            .filter(|j| matches!(j.status, JobStatus::Running))
            .count();
        let mut completed = jobs
            .iter()
            .filter(|j| matches!(j.status, JobStatus::Completed(_)))
            .count();
        let mut failed = jobs
            .iter()
            .filter(|j| matches!(j.status, JobStatus::Failed(_)))
            .count();

        for job in &translator_jobs {
            match job.status.as_str() {
                "Pending" => pending += 1,
                "Running" => running += 1,
                "Completed" => completed += 1,
                "Failed" => failed += 1,
                _ => {}
            }
        }

        let output = format!(
            "Compute System Status\n\
             =====================\n\
             SYCL Available: {}\n\
             Total Jobs: {}\n\
             Pending: {}\n\
             Running: {}\n\
             Completed: {}\n\
             Failed: {}\n\
             (System translator jobs: {})\n",
            self.manager.is_sycl_available(),
            jobs.len() + translator_jobs.len(),
            pending,
            running,
            completed,
            failed,
            translator_jobs.len()
        );

        Ok(output.into_bytes())
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("status file is read-only"))
    }
}

#[derive(Debug)]
struct SystemJobEntry {
    id: String,
    kernel: String,
    status: String,
    device_id: String,
    execution_time_ns: Option<u64>,
}

fn collect_system_jobs(
    registry: &Arc<ThreadSafeTranslatorRegistry>,
) -> Result<Vec<SystemJobEntry>> {
    use anyhow::Context;
    futures::executor::block_on(async {
        let mount_point = PathBuf::from("/system/sycl");
        let translator = match registry.get_translator(&mount_point).await {
            Some(t) => t,
            None => return Ok(Vec::new()),
        };

        let entries = translator
            .list_files("/gpu/jobs")
            .await
            .context("list system jobs")?;
        let job_ids: HashSet<String> = entries.into_iter().filter(|e| !e.contains('/')).collect();
        let mut jobs = Vec::new();

        for job_id in job_ids {
            let status_path = format!("/gpu/jobs/{}/status", job_id);
            let data = translator
                .read_file(&status_path, 0, 16 * 1024)
                .await
                .context("read job status")?;
            if data.is_empty() {
                continue;
            }

            let value: Value = serde_json::from_slice(&data).context("parse job status json")?;
            let kernel = value
                .get("kernel")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let device_id = value
                .get("device_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let execution_time_ns = value.get("execution_time_ns").and_then(|v| v.as_u64());
            let status = value
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            jobs.push(SystemJobEntry {
                id: job_id,
                kernel,
                status,
                device_id,
                execution_time_ns,
            });
        }

        Ok(jobs)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wasm::ThreadSafeTranslatorRegistry;
    use serde_json::json;
    use tempfile::TempDir;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_compute_manager() {
        let manager = ComputeManager::new();
        let payload = serde_json::to_vec(&json!({
            "a": [1.0_f32, 2.0],
            "b": [3.0_f32, 4.0],
        }))
        .unwrap();
        let submission = JobSubmission {
            job_type: "sycl".to_string(),
            operation: "vector_add".to_string(),
            payload,
            requested_vram: 0,
            device_hint: None,
        };
        let job_id = manager
            .submit_job(submission)
            .await
            .expect("Failed to submit job");

        assert!(!job_id.is_empty());

        let status = manager.get_job_status(&job_id).await;
        assert!(status.is_some());
        assert!(matches!(status.unwrap(), JobStatus::Pending));
    }

    #[tokio::test]
    async fn test_compute_control_registration() {
        let synth = Arc::new(SyntheticFilesystem::new());
        let manager = Arc::new(ComputeManager::new());

        let temp_dir = TempDir::new().unwrap();
        let registry = Arc::new(ThreadSafeTranslatorRegistry::new(
            temp_dir.path().to_path_buf(),
        ));

        register_compute_control(&synth, manager, registry)
            .await
            .expect("Failed to register compute control");

        assert!(synth.exists(&PathBuf::from("/srv/compute")).await);
        assert!(synth.exists(&PathBuf::from("/srv/compute/submit")).await);
        assert!(synth.exists(&PathBuf::from("/srv/compute/jobs")).await);
        assert!(synth.exists(&PathBuf::from("/srv/compute/devices")).await);
    }

    #[tokio::test]
    async fn test_job_execution_with_vram_release() {
        let runtime = Arc::new(GpuRuntime::new("gpu0", 64 * 1024 * 1024));
        let manager = Arc::new(ComputeManager::with_runtimes(vec![runtime.clone()]));
        let temp_dir = TempDir::new().unwrap();
        let translator_registry = Arc::new(ThreadSafeTranslatorRegistry::new(
            temp_dir.path().to_path_buf(),
        ));
        translator_registry.scan_and_load().await.ok();
        manager
            .set_translator_registry(translator_registry)
            .await
            .unwrap();
        manager.start_workers();

        let payload = serde_json::to_vec(&json!({
            "a": [0.0_f32, 1.0, 2.0, 3.0],
            "b": [1.0_f32, 1.0, 1.0, 1.0],
        }))
        .unwrap();
        let submission = JobSubmission {
            job_type: "sycl".to_string(),
            operation: "vector_add".to_string(),
            payload,
            requested_vram: 0,
            device_hint: Some(0),
        };

        let job_id = manager
            .submit_job(submission)
            .await
            .expect("job submission should succeed");

        let mut completed = false;
        for _ in 0..40 {
            if let Some(JobStatus::Completed(result_bytes)) = manager.get_job_status(&job_id).await
            {
                let value: Value = serde_json::from_slice(&result_bytes).unwrap();
                let values = value.get("values").and_then(|v| v.as_array()).unwrap();
                assert_eq!(values.len(), 4);
                // Verify the computation is correct: [0,1,2,3] + [1,1,1,1] = [1,2,3,4]
                let floats: Vec<f32> = values.iter().map(|v| v.as_f64().unwrap() as f32).collect();
                assert_eq!(floats, vec![1.0, 2.0, 3.0, 4.0]);
                completed = true;
                break;
            }
            sleep(Duration::from_millis(25)).await;
        }

        assert!(
            completed,
            "vector_add job did not complete in allotted time"
        );
        assert_eq!(runtime.free_vram(), runtime.total_vram());
    }

    #[test]
    fn bench_cpu_vector_add() {
        use std::time::Instant;

        // Check what features are available
        #[cfg(target_arch = "x86_64")]
        {
            eprintln!("CPU Features:");
            eprintln!("  AVX2: {}", is_x86_feature_detected!("avx2"));
            eprintln!("  AVX: {}", is_x86_feature_detected!("avx"));
            eprintln!("  FMA: {}", is_x86_feature_detected!("fma"));
            eprintln!("  SSE4.1: {}", is_x86_feature_detected!("sse4.1"));
        }

        let size = 1_000_000;
        let a: Vec<f32> = (0..size).map(|i| i as f32).collect();
        let b: Vec<f32> = (0..size).map(|i| (i * 2) as f32).collect();

        let start = Instant::now();
        let result = super::cpu_vector_add(&a, &b);
        let elapsed = start.elapsed();

        // Verify correctness
        assert_eq!(result[0], 0.0);
        assert_eq!(result[1], 3.0);  // 1 + 2
        assert_eq!(result[100], 300.0);  // 100 + 200

        eprintln!("CPU vector add ({} elements): {:?}", size, elapsed);
        eprintln!("Throughput: {:.2} GFLOPS", (size as f64 / elapsed.as_secs_f64()) / 1e9);
    }

    #[test]
    fn bench_cpu_matrix_multiply() {
        use std::time::Instant;

        let m = 128;
        let n = 128;
        let k = 128;

        let a: Vec<f32> = (0..(m * k)).map(|i| (i % 10) as f32).collect();
        let b: Vec<f32> = (0..(k * n)).map(|i| (i % 10) as f32).collect();

        let start = Instant::now();
        let result = super::cpu_matrix_multiply(&a, &b, m, n, k);
        let elapsed = start.elapsed();

        // Verify size
        assert_eq!(result.len(), m * n);

        // 2 * M * N * K FLOPS for matrix multiply
        let flops = 2.0 * (m as f64) * (n as f64) * (k as f64);
        eprintln!("CPU matrix multiply ({}x{}x{}): {:?}", m, n, k, elapsed);
        eprintln!("Throughput: {:.2} GFLOPS", flops / elapsed.as_secs_f64() / 1e9);
    }

    #[tokio::test]
    async fn test_matrix_multiply_gpu() {
        let runtime = Arc::new(GpuRuntime::new("gpu0", 64 * 1024 * 1024));
        let manager = Arc::new(ComputeManager::with_runtimes(vec![runtime]));
        let temp_dir = TempDir::new().unwrap();
        let translator_registry = Arc::new(ThreadSafeTranslatorRegistry::new(
            temp_dir.path().to_path_buf(),
        ));
        // Call scan_and_load() to register system translator (GPU path)
        translator_registry.scan_and_load().await.ok();
        manager
            .set_translator_registry(translator_registry)
            .await
            .unwrap();
        manager.start_workers();

        let payload = serde_json::to_vec(&json!({
            "a": [1.0_f32, 0.0, 0.0, 1.0],
            "b": [1.0_f32, 2.0, 3.0, 4.0],
            "m": 2,
            "n": 2,
            "k": 2,
        }))
        .unwrap();

        let submission = JobSubmission {
            job_type: "sycl".to_string(),
            operation: "matrix_multiply".to_string(),
            payload,
            requested_vram: 0,
            device_hint: Some(0),
        };

        let job_id = manager
            .submit_job(submission)
            .await
            .expect("job submission should succeed");

        let mut completed = false;
        for _ in 0..40 {
            match manager.get_job_status(&job_id).await {
                Some(JobStatus::Completed(result_bytes)) => {
                    let value: Value = serde_json::from_slice(&result_bytes).unwrap();
                    let values = value.get("values").and_then(|v| v.as_array()).unwrap();
                    let floats: Vec<f32> = values.iter().map(|v| v.as_f64().unwrap() as f32).collect();
                    assert_eq!(floats, vec![1.0, 2.0, 3.0, 4.0]);
                    completed = true;
                    break;
                }
                Some(JobStatus::Failed(reason)) => {
                    panic!("Job failed: {}", reason);
                }
                _ => {
                    // Still pending or running
                }
            }
            sleep(Duration::from_millis(25)).await;
        }

        assert!(
            completed,
            "matrix multiply job did not complete in allotted time"
        );
    }

    #[tokio::test]
    async fn test_matrix_multiply_fallback() {
        let runtime = Arc::new(GpuRuntime::new("gpu0", 64 * 1024 * 1024));
        let manager = Arc::new(ComputeManager::with_runtimes(vec![runtime]));
        let temp_dir = TempDir::new().unwrap();
        let translator_registry = Arc::new(ThreadSafeTranslatorRegistry::new(
            temp_dir.path().to_path_buf(),
        ));
        // Don't call scan_and_load() to test CPU fallback without translator
        // translator_registry.scan_and_load().await.ok();
        manager
            .set_translator_registry(translator_registry)
            .await
            .unwrap();
        manager.start_workers();

        let payload = serde_json::to_vec(&json!({
            "a": [1.0_f32, 0.0, 0.0, 1.0],
            "b": [1.0_f32, 2.0, 3.0, 4.0],
            "m": 2,
            "n": 2,
            "k": 2,
        }))
        .unwrap();

        let submission = JobSubmission {
            job_type: "sycl".to_string(),
            operation: "matrix_multiply".to_string(),
            payload,
            requested_vram: 0,
            device_hint: Some(0),
        };

        let job_id = manager
            .submit_job(submission)
            .await
            .expect("job submission should succeed");

        let mut completed = false;
        for _ in 0..40 {
            match manager.get_job_status(&job_id).await {
                Some(JobStatus::Completed(result_bytes)) => {
                    let value: Value = serde_json::from_slice(&result_bytes).unwrap();
                    let values = value.get("values").and_then(|v| v.as_array()).unwrap();
                    let floats: Vec<f32> = values.iter().map(|v| v.as_f64().unwrap() as f32).collect();
                    assert_eq!(floats, vec![1.0, 2.0, 3.0, 4.0]);
                    completed = true;
                    break;
                }
                Some(JobStatus::Failed(reason)) => {
                    panic!("Job failed: {}", reason);
                }
                _ => {
                    // Still pending or running
                }
            }
            sleep(Duration::from_millis(25)).await;
        }

        assert!(
            completed,
            "matrix multiply job did not complete in allotted time"
        );
    }
}
