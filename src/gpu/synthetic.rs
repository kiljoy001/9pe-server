// Synthetic GPU registration
use crate::gpu::GpuInfo;
use crate::gpu::GpuRuntime;
use crate::synth::{ControlHandler, SyntheticFilesystem};
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

/// Register synthetic GPU files under /srv/compute
pub async fn register_gpu_controls(
    synth: &Arc<SyntheticFilesystem>,
    devices: &[GpuInfo],
    runtimes: &[std::sync::Arc<GpuRuntime>],
) -> Result<()> {
    let base = PathBuf::from("/srv/compute");
    synth.create_directory(&base).await?;

    for (idx, gpu) in devices.iter().enumerate() {
        let device_id = format!("gpu{}", gpu.local_index);
        let gpu_dir = base.join(format!("gpu{}", gpu.local_index));
        synth.create_directory(&gpu_dir).await?;

        // static info file (JSON)
        let info_json = serde_json::json!({
            "name": gpu.name,
            "vendor": gpu.vendor,
            "compute_units": gpu.compute_units,
            "total_vram_mb": gpu.total_vram_bytes / (1024 * 1024),
            "backend": gpu.backend,
        })
        .to_string()
        .into_bytes();
        synth
            .create_control_file(
                &gpu_dir.join("info"),
                Arc::new(StaticHandler { data: info_json }),
            )
            .await?;

        // Get the corresponding runtime for this GPU
        let runtime = runtimes
            .get(idx)
            .cloned()
            .unwrap_or_else(|| Arc::new(GpuRuntime::new(&device_id, gpu.total_vram_bytes)));

        // vram_free – dynamic reading of free VRAM (bytes)
        struct VramFreeHandler {
            runtime: std::sync::Arc<GpuRuntime>,
        }
        impl ControlHandler for VramFreeHandler {
            fn read(&self) -> Result<Vec<u8>> {
                Ok(self.runtime.free_vram().to_string().into_bytes())
            }
            fn write(&self, _data: &[u8]) -> Result<()> {
                Err(anyhow::anyhow!("Read‑only file"))
            }
        }
        synth
            .create_control_file(
                &gpu_dir.join("vram_free"),
                Arc::new(VramFreeHandler {
                    runtime: runtime.clone(),
                }),
            )
            .await?;

        // Allocation control file – allocate VRAM blocks
        struct VramAllocateHandler {
            runtime: std::sync::Arc<GpuRuntime>,
        }
        impl ControlHandler for VramAllocateHandler {
            fn read(&self) -> Result<Vec<u8>> {
                Ok(Vec::new())
            }
            fn write(&self, data: &[u8]) -> Result<()> {
                let size: u64 = std::str::from_utf8(data)
                    .map_err(|e| anyhow::anyhow!("Invalid UTF-8: {}", e))?
                    .trim()
                    .parse()
                    .map_err(|e| anyhow::anyhow!("Invalid number: {}", e))?;

                if self.runtime.allocate(size) {
                    Ok(())
                } else {
                    Err(anyhow::anyhow!("Insufficient VRAM"))
                }
            }
        }
        synth
            .create_control_file(
                &gpu_dir.join("vram_allocate"),
                Arc::new(VramAllocateHandler {
                    runtime: runtime.clone(),
                }),
            )
            .await?;

        // Release control file – return VRAM to the pool
        struct VramReleaseHandler {
            runtime: std::sync::Arc<GpuRuntime>,
        }
        impl ControlHandler for VramReleaseHandler {
            fn read(&self) -> Result<Vec<u8>> {
                Ok(Vec::new())
            }
            fn write(&self, data: &[u8]) -> Result<()> {
                let size: u64 = std::str::from_utf8(data)
                    .map_err(|e| anyhow::anyhow!("Invalid UTF-8: {}", e))?
                    .trim()
                    .parse()
                    .map_err(|e| anyhow::anyhow!("Invalid number: {}", e))?;

                let total = self.runtime.total_vram();
                let free = self.runtime.free_vram();
                let used = total.saturating_sub(free);
                if size > used {
                    return Err(anyhow::anyhow!(
                        "Cannot release {} bytes; only {} bytes currently allocated",
                        size,
                        used
                    ));
                }

                if self.runtime.release(size) {
                    Ok(())
                } else {
                    Err(anyhow::anyhow!(
                        "VRAM release clamped; allocation tracking out of sync"
                    ))
                }
            }
        }
        synth
            .create_control_file(
                &gpu_dir.join("vram_release"),
                Arc::new(VramReleaseHandler {
                    runtime: runtime.clone(),
                }),
            )
            .await?;

        // Status control file - overall VRAM status
        struct VramStatusHandler {
            runtime: std::sync::Arc<GpuRuntime>,
        }
        impl ControlHandler for VramStatusHandler {
            fn read(&self) -> Result<Vec<u8>> {
                let free = self.runtime.free_vram();
                let total = self.runtime.total_vram();
                let used = total - free;
                let status = format!(
                    "Total: {} MB\nUsed: {} MB\nFree: {} MB\nUtilization: {:.1}%",
                    total / (1024 * 1024),
                    used / (1024 * 1024),
                    free / (1024 * 1024),
                    (used as f64 / total as f64) * 100.0
                );
                Ok(status.into_bytes())
            }
            fn write(&self, _data: &[u8]) -> Result<()> {
                Err(anyhow::anyhow!("Read‑only file"))
            }
        }
        synth
            .create_control_file(
                &gpu_dir.join("vram_status"),
                Arc::new(VramStatusHandler {
                    runtime: runtime.clone(),
                }),
            )
            .await?;
    }
    Ok(())
}

/// Simple read‑only control handler that returns static data.
struct StaticHandler {
    data: Vec<u8>,
}

impl ControlHandler for StaticHandler {
    fn read(&self) -> Result<Vec<u8>> {
        Ok(self.data.clone())
    }
    fn write(&self, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("Read‑only file"))
    }
}
