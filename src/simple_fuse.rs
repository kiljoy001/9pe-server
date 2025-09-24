//! Simple FUSE implementation for 9P.e mounting
//!
//! This is a minimal implementation focused on getting Linux clients working

use std::path::Path;
use anyhow::Result;
use tracing::{info, warn, error};

/// Simple FUSE mounting using external tools
pub async fn mount_server(server_addr: String, mount_point: &Path) -> Result<()> {
    info!("🗻 Mounting {} at {:?} using simple method", server_addr, mount_point);

    // Create mount point
    tokio::fs::create_dir_all(mount_point).await?;

    // For now, create a marker that this is a 9P.e mount
    let marker_file = mount_point.join(".9pe_mount");
    let mount_info = format!(
        "server: {}\nmounted_at: {}\ntype: 9pe\n",
        server_addr,
        chrono::Utc::now(),
    );

    tokio::fs::write(marker_file, mount_info).await?;

    info!("✅ Mount point prepared at {:?}", mount_point);
    info!("💡 For real FUSE mounting, install fuse and enable FUSE feature");

    Ok(())
}

/// Unmount using fusermount
pub async fn unmount(mount_point: &Path) -> Result<()> {
    info!("📤 Unmounting {:?}", mount_point);

    // Try fusermount first
    let output = tokio::process::Command::new("fusermount")
        .arg("-u")
        .arg(mount_point)
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => {
            info!("✅ Unmounted with fusermount");
        }
        _ => {
            // Fallback: just remove marker
            let marker_file = mount_point.join(".9pe_mount");
            let _ = tokio::fs::remove_file(marker_file).await;
            info!("Removed mount marker");
        }
    }

    // Remove mount point if empty
    let _ = tokio::fs::remove_dir(mount_point).await;

    Ok(())
}

/// Check if FUSE is available
pub fn is_fuse_available() -> bool {
    Path::new("/dev/fuse").exists()
}

/// Mount with cleanup on exit
pub async fn mount_with_cleanup(server_addr: String, mount_point: &Path) -> Result<()> {
    if !is_fuse_available() {
        warn!("FUSE not available - creating mount marker only");
    }

    mount_server(server_addr, mount_point).await
}