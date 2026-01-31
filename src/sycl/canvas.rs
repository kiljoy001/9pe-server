//! SYCL Canvas Renderer
//!
//! Provides GPU-accelerated rendering to a framebuffer that can be
//! exposed via the V8 Remote DOM translator.

use anyhow::{Result, anyhow};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use super::backend_loader::{SyclBackendLib, BackendType};
use super::ffi::{SyclDevice, SyclQueue, SyclBuffer};
use crate::ipc::SharedMemoryManager;

/// RGBA pixel format
pub const BYTES_PER_PIXEL: usize = 4;

/// Canvas renderer with SYCL backend
pub struct SyclCanvas {
    width: u32,
    height: u32,
    backend: Arc<SyclBackendLib>,
    device: SyclDevice,
    queue: SyclQueue,
    framebuffer: SyclBuffer,
    shm_manager: Arc<SharedMemoryManager>,
    shm_id: String,
}

// SYCL handles are opaque pointers managed by the backend library
// They are safe to send between threads because the backend library handles synchronization
unsafe impl Send for SyclCanvas {}
unsafe impl Sync for SyclCanvas {}

impl SyclCanvas {
    /// Create a new SYCL canvas with given dimensions
    pub fn new(
        width: u32,
        height: u32,
        backend_type: BackendType,
        shm_manager: Arc<SharedMemoryManager>,
    ) -> Result<Self> {
        let backend = Arc::new(SyclBackendLib::load(backend_type)
            .map_err(|e| anyhow!("Failed to load SYCL backend: {}", e))?);

        // Discover devices
        unsafe {
            (backend.discover_devices)();
        }

        // Get device (use device 0, which is the primary/dGPU)
        let mut device_count = 0u32;
        unsafe {
            (backend.get_device_count)(&mut device_count);
        }

        if device_count == 0 {
            return Err(anyhow!("No SYCL devices found"));
        }

        let device_id = 0;
        let mut device: SyclDevice = std::ptr::null_mut();
        unsafe {
            (backend.get_device)(device_id, &mut device);
        }
        info!("SYCL Canvas using device {} of {}", device_id, device_count);

        // Create queue
        let mut queue: SyclQueue = std::ptr::null_mut();
        unsafe {
            (backend.create_queue)(device, &mut queue);
        }

        // Create framebuffer on GPU
        let buffer_size = (width * height * BYTES_PER_PIXEL as u32) as usize;
        let mut framebuffer: SyclBuffer = std::ptr::null_mut();
        unsafe {
            (backend.create_buffer)(queue, buffer_size, &mut framebuffer);
        }

        // Allocate shared memory for CPU-side access
        let shm_id = format!("canvas_{}x{}", width, height);
        shm_manager.allocate(shm_id.clone(), buffer_size)?;

        Ok(Self {
            width,
            height,
            backend,
            device,
            queue,
            framebuffer,
            shm_manager,
            shm_id,
        })
    }

    /// Get canvas dimensions
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Clear the canvas with a solid color (RGBA)
    pub fn clear(&self, r: u8, g: u8, b: u8, a: u8) -> Result<()> {
        let pixel_count = (self.width * self.height) as usize;
        let mut pixels = vec![0u8; pixel_count * BYTES_PER_PIXEL];

        for i in 0..pixel_count {
            pixels[i * 4] = r;
            pixels[i * 4 + 1] = g;
            pixels[i * 4 + 2] = b;
            pixels[i * 4 + 3] = a;
        }

        // Write to GPU buffer
        unsafe {
            (self.backend.buffer_write)(
                self.queue,
                self.framebuffer,
                pixels.as_ptr() as *const _,
                0,
                pixels.len(),
            );
            (self.backend.queue_wait)(self.queue);
        }

        Ok(())
    }

    /// Render a test pattern (checkerboard)
    pub fn render_test_pattern(&self) -> Result<()> {
        let pixel_count = (self.width * self.height) as usize;
        let mut pixels = vec![0u8; pixel_count * BYTES_PER_PIXEL];

        for y in 0..self.height {
            for x in 0..self.width {
                let idx = ((y * self.width + x) * BYTES_PER_PIXEL as u32) as usize;

                // Checkerboard pattern
                let checker = ((x / 32) + (y / 32)) % 2;
                let color = if checker == 0 { 100u8 } else { 200u8 };

                pixels[idx] = color;     // R
                pixels[idx + 1] = color; // G
                pixels[idx + 2] = color; // B
                pixels[idx + 3] = 255;   // A
            }
        }

        // Write to GPU buffer
        unsafe {
            (self.backend.buffer_write)(
                self.queue,
                self.framebuffer,
                pixels.as_ptr() as *const _,
                0,
                pixels.len(),
            );
            (self.backend.queue_wait)(self.queue);
        }

        Ok(())
    }

    /// Render a gradient pattern
    pub fn render_gradient(&self) -> Result<()> {
        let pixel_count = (self.width * self.height) as usize;
        let mut pixels = vec![0u8; pixel_count * BYTES_PER_PIXEL];

        for y in 0..self.height {
            for x in 0..self.width {
                let idx = ((y * self.width + x) * BYTES_PER_PIXEL as u32) as usize;

                let r = ((x as f32 / self.width as f32) * 255.0) as u8;
                let g = ((y as f32 / self.height as f32) * 255.0) as u8;
                let b = 128u8;

                pixels[idx] = r;
                pixels[idx + 1] = g;
                pixels[idx + 2] = b;
                pixels[idx + 3] = 255;
            }
        }

        // Write to GPU buffer
        unsafe {
            (self.backend.buffer_write)(
                self.queue,
                self.framebuffer,
                pixels.as_ptr() as *const _,
                0,
                pixels.len(),
            );
            (self.backend.queue_wait)(self.queue);
        }

        Ok(())
    }

    /// Read the framebuffer into shared memory for access
    pub fn read_to_shm(&self) -> Result<Vec<u8>> {
        let buffer_size = (self.width * self.height * BYTES_PER_PIXEL as u32) as usize;
        let mut pixels = vec![0u8; buffer_size];

        // Read from GPU buffer
        unsafe {
            (self.backend.buffer_read)(
                self.queue,
                self.framebuffer,
                pixels.as_mut_ptr() as *mut _,
                0,
                buffer_size,
            );
            (self.backend.queue_wait)(self.queue);
        }

        // Write to shared memory
        let mut handle = self.shm_manager.borrow_write(&self.shm_id)?;
        let slice = handle.as_mut_slice()?;
        slice.copy_from_slice(&pixels);

        Ok(pixels)
    }

    /// Get the framebuffer as PNG bytes
    pub fn to_png(&self) -> Result<Vec<u8>> {
        let pixels = self.read_to_shm()?;

        // Create PNG encoder
        let mut png_data = Vec::new();
        {
            let mut encoder = png::Encoder::new(
                &mut png_data,
                self.width,
                self.height,
            );
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);

            let mut writer = encoder.write_header()
                .map_err(|e| anyhow!("PNG header error: {}", e))?;
            writer.write_image_data(&pixels)
                .map_err(|e| anyhow!("PNG write error: {}", e))?;
        }

        Ok(png_data)
    }

    /// Get raw RGBA bytes
    pub fn to_rgba_bytes(&self) -> Result<Vec<u8>> {
        self.read_to_shm()
    }
}

impl Drop for SyclCanvas {
    fn drop(&mut self) {
        info!("Cleaning up SYCL Canvas ({}x{}), waiting for GPU operations to complete...", self.width, self.height);

        unsafe {
            // CRITICAL: Wait for all pending GPU operations to complete before releasing resources
            // This prevents leaving the GPU in a bad state that persists after process exit
            (self.backend.queue_wait)(self.queue);

            info!("GPU operations complete, releasing resources...");
            (self.backend.release_buffer)(self.framebuffer);
            (self.backend.release_queue)(self.queue);
            (self.backend.release_device)(self.device);

            info!("SYCL Canvas cleanup complete");
        }

        // Note: SharedMemoryManager holds Arc<MemoryManager> which will be cleaned up
        // when the last reference is dropped (proper RAII)
    }
}

/// Thread-safe canvas wrapper for use in translators
pub struct CanvasRenderer {
    canvas: Arc<RwLock<SyclCanvas>>,
}

impl CanvasRenderer {
    pub fn new(
        width: u32,
        height: u32,
        backend_type: BackendType,
        shm_manager: Arc<SharedMemoryManager>,
    ) -> Result<Self> {
        let canvas = SyclCanvas::new(width, height, backend_type, shm_manager)?;
        Ok(Self {
            canvas: Arc::new(RwLock::new(canvas)),
        })
    }

    pub async fn clear(&self, r: u8, g: u8, b: u8, a: u8) -> Result<()> {
        let canvas = self.canvas.clone();
        tokio::task::spawn_blocking(move || {
            let canvas = canvas.blocking_read();
            canvas.clear(r, g, b, a)
        }).await?
    }

    pub async fn render_test_pattern(&self) -> Result<()> {
        let canvas = self.canvas.clone();
        tokio::task::spawn_blocking(move || {
            let canvas = canvas.blocking_read();
            canvas.render_test_pattern()
        }).await?
    }

    pub async fn render_gradient(&self) -> Result<()> {
        let canvas = self.canvas.clone();
        tokio::task::spawn_blocking(move || {
            let canvas = canvas.blocking_read();
            canvas.render_gradient()
        }).await?
    }

    pub async fn to_png(&self) -> Result<Vec<u8>> {
        let canvas = self.canvas.clone();
        tokio::task::spawn_blocking(move || {
            let canvas = canvas.blocking_read();
            canvas.to_png()
        }).await?
    }

    pub async fn to_rgba_bytes(&self) -> Result<Vec<u8>> {
        let canvas = self.canvas.clone();
        tokio::task::spawn_blocking(move || {
            let canvas = canvas.blocking_read();
            canvas.to_rgba_bytes()
        }).await?
    }

    pub async fn dimensions(&self) -> (u32, u32) {
        let canvas = self.canvas.read().await;
        canvas.dimensions()
    }
}
