//! Compute control via synthetic files
//!
//! Submit GPU/WASM compute jobs by writing to files in /srv/compute/

use crate::synth::{ControlHandler, SyntheticFilesystem};
use crate::sycl::ffi::{SyclDevice, SyclQueue, sycl_get_device, sycl_create_queue, sycl_discover_devices, SyclDeviceInfo};
use anyhow::Result;
use std::sync::Arc;
use std::path::PathBuf;
use tokio::sync::RwLock;
use std::collections::HashMap;
use uuid::Uuid;

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
    pub job_type: String,  // "sycl", "wasm", "opencl"
    pub input: Vec<u8>,
    pub status: JobStatus,
    pub submitted_at: std::time::SystemTime,
}

/// Compute job manager
pub struct ComputeManager {
    jobs: Arc<RwLock<HashMap<String, ComputeJob>>>,
    sycl_available: bool,
}

impl ComputeManager {
    pub fn new() -> Self {
        // Check if SYCL is available
        let sycl_available = unsafe {
            let mut devices = vec![SyclDeviceInfo {
                name: [0; 256],
                vendor: [0; 128],
                compute_units: 0,
                global_memory_size: 0,
                local_memory_size: 0,
                max_work_group_size: 0,
                is_gpu: false,
                is_cpu: false,
                supports_fp64: false,
                supports_fp16: false,
            }];
            let mut count: usize = 1;
            let err = sycl_discover_devices(devices.as_mut_ptr(), &mut count as *mut usize);
            err.is_ok() && count > 0
        };

        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
            sycl_available,
        }
    }

    pub async fn submit_job(&self, job_type: String, input: Vec<u8>) -> Result<String> {
        let job_id = Uuid::new_v4().to_string();
        let job = ComputeJob {
            id: job_id.clone(),
            job_type,
            input,
            status: JobStatus::Pending,
            submitted_at: std::time::SystemTime::now(),
        };

        self.jobs.write().await.insert(job_id.clone(), job);
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
}

impl Default for ComputeManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Register compute control files in the synthetic filesystem
pub async fn register_compute_control(
    synth: &SyntheticFilesystem,
    manager: Arc<ComputeManager>,
) -> Result<()> {
    // Create /srv/compute directory
    synth.create_directory(&PathBuf::from("/srv/compute")).await?;

    // /srv/compute/submit - Write job spec to submit
    synth.create_control_file(
        &PathBuf::from("/srv/compute/submit"),
        Arc::new(SubmitHandler { manager: manager.clone() })
    ).await?;

    // /srv/compute/jobs - Read list of jobs
    synth.create_control_file(
        &PathBuf::from("/srv/compute/jobs"),
        Arc::new(JobsHandler { manager: manager.clone() })
    ).await?;

    // /srv/compute/devices - Read available GPU devices
    synth.create_control_file(
        &PathBuf::from("/srv/compute/devices"),
        Arc::new(DevicesHandler { manager: manager.clone() })
    ).await?;

    // /srv/compute/status - Read compute system status
    synth.create_control_file(
        &PathBuf::from("/srv/compute/status"),
        Arc::new(StatusHandler { manager: manager.clone() })
    ).await?;

    Ok(())
}

/// Handler for /srv/compute/submit - submit compute job
struct SubmitHandler {
    manager: Arc<ComputeManager>,
}

impl ControlHandler for SubmitHandler {
    fn read(&self) -> Result<Vec<u8>> {
        Ok(b"Write job spec (JSON format):\n\
             {\n\
               \"type\": \"sycl\" | \"wasm\" | \"opencl\",\n\
               \"operation\": \"vector_add\" | \"matmul\" | \"custom\",\n\
               \"data\": \"base64-encoded-input\"\n\
             }\n".to_vec())
    }

    fn write(&self, data: &[u8]) -> Result<()> {
        let job_spec = String::from_utf8(data.to_vec())?;
        let spec: serde_json::Value = serde_json::from_str(&job_spec)?;

        let job_type = spec["type"].as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'type' field"))?
            .to_string();

        let input_data = spec["data"].as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'data' field"))?
            .as_bytes()
            .to_vec();

        let job_id = futures::executor::block_on(
            self.manager.submit_job(job_type, input_data)
        )?;

        println!("Job submitted: {}", job_id);
        Ok(())
    }
}

/// Handler for /srv/compute/jobs - list all jobs
struct JobsHandler {
    manager: Arc<ComputeManager>,
}

impl ControlHandler for JobsHandler {
    fn read(&self) -> Result<Vec<u8>> {
        let jobs = futures::executor::block_on(self.manager.list_jobs());

        let mut output = String::from("Compute Jobs\n============\n");
        for job in jobs {
            let status_str = match job.status {
                JobStatus::Pending => "pending".to_string(),
                JobStatus::Running => "running".to_string(),
                JobStatus::Completed(_) => "completed".to_string(),
                JobStatus::Failed(ref e) => format!("failed: {}", e),
            };

            output.push_str(&format!(
                "{}\t{}\t{}\t{:?}\n",
                job.id,
                job.job_type,
                status_str,
                job.submitted_at
            ));
        }

        Ok(output.into_bytes())
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("jobs file is read-only, use 'submit' to add jobs"))
    }
}

/// Handler for /srv/compute/devices - list GPU devices
struct DevicesHandler {
    manager: Arc<ComputeManager>,
}

impl ControlHandler for DevicesHandler {
    fn read(&self) -> Result<Vec<u8>> {
        if !self.manager.is_sycl_available() {
            return Ok(b"SYCL not available - no GPU devices detected\n\
                       Install AdaptiveCpp for GPU support\n".to_vec());
        }

        let mut devices = vec![SyclDeviceInfo {
            name: [0; 256],
            vendor: [0; 128],
            compute_units: 0,
            global_memory_size: 0,
            local_memory_size: 0,
            max_work_group_size: 0,
            is_gpu: false,
            is_cpu: false,
            supports_fp64: false,
            supports_fp16: false,
        }; 16];

        let mut count: usize = 16;

        unsafe {
            let err = sycl_discover_devices(devices.as_mut_ptr(), &mut count as *mut usize);
            if err.is_err() {
                return Ok(b"Failed to discover SYCL devices\n".to_vec());
            }
        }

        let mut output = String::from("SYCL Devices\n============\n");
        for i in 0..count {
            let dev = &devices[i];
            output.push_str(&format!(
                "Device {}: {} ({})\n  Compute Units: {}\n  Global Memory: {} GB\n  Type: {}\n",
                i,
                dev.name_str(),
                dev.vendor_str(),
                dev.compute_units,
                dev.global_memory_size / (1024 * 1024 * 1024),
                if dev.is_gpu { "GPU" } else { "CPU" }
            ));
        }

        Ok(output.into_bytes())
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("devices file is read-only"))
    }
}

/// Handler for /srv/compute/status - compute system status
struct StatusHandler {
    manager: Arc<ComputeManager>,
}

impl ControlHandler for StatusHandler {
    fn read(&self) -> Result<Vec<u8>> {
        let jobs = futures::executor::block_on(self.manager.list_jobs());
        let pending = jobs.iter().filter(|j| matches!(j.status, JobStatus::Pending)).count();
        let running = jobs.iter().filter(|j| matches!(j.status, JobStatus::Running)).count();
        let completed = jobs.iter().filter(|j| matches!(j.status, JobStatus::Completed(_))).count();
        let failed = jobs.iter().filter(|j| matches!(j.status, JobStatus::Failed(_))).count();

        let output = format!(
            "Compute System Status\n\
             =====================\n\
             SYCL Available: {}\n\
             Total Jobs: {}\n\
             Pending: {}\n\
             Running: {}\n\
             Completed: {}\n\
             Failed: {}\n",
            self.manager.is_sycl_available(),
            jobs.len(),
            pending,
            running,
            completed,
            failed
        );

        Ok(output.into_bytes())
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("status file is read-only"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_compute_manager() {
        let manager = ComputeManager::new();
        let job_id = manager.submit_job("sycl".to_string(), b"test data".to_vec())
            .await
            .expect("Failed to submit job");

        assert!(!job_id.is_empty());

        let status = manager.get_job_status(&job_id).await;
        assert!(status.is_some());
        assert!(matches!(status.unwrap(), JobStatus::Pending));
    }

    #[tokio::test]
    async fn test_compute_control_registration() {
        let synth = SyntheticFilesystem::new();
        let manager = Arc::new(ComputeManager::new());

        register_compute_control(&synth, manager).await
            .expect("Failed to register compute control");

        assert!(synth.exists(&PathBuf::from("/srv/compute")).await);
        assert!(synth.exists(&PathBuf::from("/srv/compute/submit")).await);
        assert!(synth.exists(&PathBuf::from("/srv/compute/jobs")).await);
        assert!(synth.exists(&PathBuf::from("/srv/compute/devices")).await);
    }
}
