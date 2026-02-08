use crate::traits::{ComputeBackend, DeviceInfo, ComputeJob, JobStatus};
use crate::compute_control::{ComputeManager, FogJobOptions, JobSubmission, JobStatus as LegacyJobStatus};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

pub struct ComputeManagerAdapter {
    manager: Arc<ComputeManager>,
}

impl ComputeManagerAdapter {
    pub fn new(manager: Arc<ComputeManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl ComputeBackend for ComputeManagerAdapter {
    fn discover_devices(&self) -> Result<Vec<DeviceInfo>> {
        // In a real implementation mapping, we would query helper methods.
        // For now, we return empty as ComputeManager manages runtimes internally.
        // A full implementation would expose `manager.gpu_runtimes`.
        Ok(vec![])
    }

    async fn submit_job(&self, job: ComputeJob) -> Result<String> {
        let submission = JobSubmission {
            job_type: job.job_type,
            operation: job.operation,
            payload: job.params,
            requested_vram: 0,
            device_hint: None,
            shm_handle: job.shm_handle,
            priority: crate::compute_control::JobPriority::Normal,
            timeout_secs: 300,
            fog: FogJobOptions::default(),
        };

        self.manager.submit_job(submission).await
    }

    async fn get_job_status(&self, job_id: &str) -> Option<JobStatus> {
        self.manager.get_job_status(job_id).await.map(|status| match status {
            LegacyJobStatus::Pending => JobStatus::Pending,
            LegacyJobStatus::Running => JobStatus::Running,
            LegacyJobStatus::Completed(data) => JobStatus::Completed(data),
            LegacyJobStatus::Failed(err) => JobStatus::Failed(err),
        })
    }
}
