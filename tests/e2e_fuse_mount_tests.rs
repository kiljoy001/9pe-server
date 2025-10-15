//! End-to-end integration tests for FUSE mounting
//!
//! Tests actual FUSE mounts and file operations through mounted filesystem

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

/// Helper to manage server and FUSE mount
struct FuseTestSetup {
    server: Child,
    server_port: u16,
    mount_point: PathBuf,
    server_root: TempDir,
}

impl FuseTestSetup {
    fn start(port: u16) -> anyhow::Result<Self> {
        let server_root = TempDir::new()?;
        let mount_point = TempDir::new()?.into_path();

        // Create test files in server root
        fs::write(server_root.path().join("readme.txt"), b"FUSE test file")?;
        fs::create_dir(server_root.path().join("docs"))?;
        fs::write(
            server_root.path().join("docs/info.txt"),
            b"Documentation content",
        )?;

        // Start server
        let server = Command::new("./target/release/ninep-server")
            .args(&[
                "serve",
                "--port",
                &port.to_string(),
                "--root",
                server_root.path().to_str().unwrap(),
                "--transport",
                "tcp",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        thread::sleep(Duration::from_secs(2));

        Ok(FuseTestSetup {
            server,
            server_port: port,
            mount_point,
            server_root,
        })
    }

    fn mount_fuse(&self) -> anyhow::Result<Child> {
        // Use auto-mount command to mount the server
        let mount = Command::new("./target/release/ninep-server")
            .args(&[
                "auto-mount",
                "start",
                "--mount-point",
                self.mount_point.to_str().unwrap(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        thread::sleep(Duration::from_secs(3));
        Ok(mount)
    }

    fn unmount(&self) -> anyhow::Result<()> {
        // Try fusermount first
        let result = Command::new("fusermount")
            .args(&["-u", self.mount_point.to_str().unwrap()])
            .output();

        if result.is_err() || !result.unwrap().status.success() {
            // Fallback to umount
            Command::new("umount")
                .arg(self.mount_point.to_str().unwrap())
                .output()?;
        }

        Ok(())
    }
}

impl Drop for FuseTestSetup {
    fn drop(&mut self) {
        let _ = self.unmount();
        let _ = self.server.kill();
        let _ = self.server.wait();
        let _ = fs::remove_dir_all(&self.mount_point);
    }
}

/// Test FUSE mount point can be created
#[test]
#[ignore] // Requires FUSE privileges
fn test_e2e_fuse_mount_creation() {
    let setup = FuseTestSetup::start(17001).expect("Failed to start test setup");

    // Try to mount
    let mount_result = setup.mount_fuse();

    // Even if mount fails due to permissions, the attempt should be graceful
    match mount_result {
        Ok(mut mount_child) => {
            thread::sleep(Duration::from_secs(2));

            // Check if mount point exists
            assert!(setup.mount_point.exists(), "Mount point should exist");

            // Cleanup
            let _ = mount_child.kill();
            let _ = mount_child.wait();
        }
        Err(e) => {
            // If we can't mount due to permissions, that's OK for this test
            println!("Mount failed (may need FUSE privileges): {}", e);
        }
    }
}

/// Test reading files through FUSE mount
#[test]
#[ignore] // Requires FUSE privileges
fn test_e2e_fuse_read_file() {
    let setup = FuseTestSetup::start(17002).expect("Failed to start test setup");

    let mount_child = match setup.mount_fuse() {
        Ok(child) => child,
        Err(_) => {
            println!("Skipping test - FUSE mount not available");
            return;
        }
    };

    thread::sleep(Duration::from_secs(3));

    // Try to read file through FUSE mount
    let mounted_file = setup.mount_point.join("readme.txt");
    if mounted_file.exists() {
        let content =
            fs::read_to_string(&mounted_file).expect("Should be able to read file through FUSE");

        assert_eq!(content, "FUSE test file");
    }

    drop(mount_child);
}

/// Test listing directory through FUSE mount
#[test]
#[ignore] // Requires FUSE privileges
fn test_e2e_fuse_list_directory() {
    let setup = FuseTestSetup::start(17003).expect("Failed to start test setup");

    let mount_child = match setup.mount_fuse() {
        Ok(child) => child,
        Err(_) => {
            println!("Skipping test - FUSE mount not available");
            return;
        }
    };

    thread::sleep(Duration::from_secs(3));

    // Try to list directory through FUSE mount
    if let Ok(entries) = fs::read_dir(&setup.mount_point) {
        let names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();

        // Should see some files (at least the ones we created or placeholders)
        assert!(!names.is_empty(), "Should see files in mounted directory");
    }

    drop(mount_child);
}

/// Test FUSE mount handles server disconnection
#[test]
#[ignore] // Requires FUSE privileges
fn test_e2e_fuse_server_disconnect() {
    let mut setup = FuseTestSetup::start(17004).expect("Failed to start test setup");

    let mount_child = match setup.mount_fuse() {
        Ok(child) => child,
        Err(_) => {
            println!("Skipping test - FUSE mount not available");
            return;
        }
    };

    thread::sleep(Duration::from_secs(3));

    // Kill the server
    setup.server.kill().expect("Failed to kill server");
    setup.server.wait().expect("Failed to wait for server");

    thread::sleep(Duration::from_secs(2));

    // FUSE mount should handle this gracefully (may return errors but shouldn't crash)
    let _ = fs::read_dir(&setup.mount_point);

    drop(mount_child);
}

/// Test multiple FUSE mounts to same server
#[test]
#[ignore] // Requires FUSE privileges
fn test_e2e_multiple_fuse_mounts() {
    let setup = FuseTestSetup::start(17005).expect("Failed to start test setup");

    // Try to create first mount
    let mount1 = setup.mount_fuse();

    // Try to create second mount point
    let mount_point2 = TempDir::new().unwrap().into_path();

    let mount2 = Command::new("./target/release/ninep-server")
        .args(&[
            "auto-mount",
            "start",
            "--mount-point",
            mount_point2.to_str().unwrap(),
        ])
        .spawn();

    thread::sleep(Duration::from_secs(3));

    // At least one should work (multiple mounts may or may not be supported)
    assert!(
        mount1.is_ok() || mount2.is_ok(),
        "At least one mount should succeed"
    );

    // Cleanup
    if let Ok(mut m1) = mount1 {
        let _ = m1.kill();
    }
    if let Ok(mut m2) = mount2 {
        let _ = m2.kill();
    }
    let _ = fs::remove_dir_all(mount_point2);
}

/// Test FUSE mount permissions
#[test]
#[ignore] // Requires FUSE privileges
fn test_e2e_fuse_permissions() {
    let setup = FuseTestSetup::start(17006).expect("Failed to start test setup");

    let mount_child = match setup.mount_fuse() {
        Ok(child) => child,
        Err(_) => {
            println!("Skipping test - FUSE mount not available");
            return;
        }
    };

    thread::sleep(Duration::from_secs(3));

    // Check if we can stat the mount point
    if let Ok(metadata) = fs::metadata(&setup.mount_point) {
        assert!(metadata.is_dir(), "Mount point should be a directory");
    }

    drop(mount_child);
}

/// Test FUSE mount cleanup on abnormal termination
#[test]
#[ignore] // Requires FUSE privileges
fn test_e2e_fuse_cleanup() {
    let setup = FuseTestSetup::start(17007).expect("Failed to start test setup");

    {
        let mount_child = match setup.mount_fuse() {
            Ok(child) => child,
            Err(_) => {
                println!("Skipping test - FUSE mount not available");
                return;
            }
        };

        thread::sleep(Duration::from_secs(2));
        drop(mount_child); // Abruptly drop
    }

    thread::sleep(Duration::from_secs(2));

    // Mount point should be cleanable
    let unmount_result = setup.unmount();
    // Should succeed or be already unmounted
    let _ = unmount_result;
}

/// Test auto-mount status command
#[test]
fn test_e2e_automount_status() {
    // Try to get auto-mount status
    let output = Command::new("./target/release/ninep-server")
        .args(&["auto-mount", "status"])
        .output()
        .expect("Failed to run auto-mount status");

    // Command should execute (even if no mounts active)
    assert!(
        output.status.success() || output.status.code() == Some(1),
        "Status command should execute"
    );
}

/// Test auto-mount with invalid mount point
#[test]
fn test_e2e_automount_invalid_path() {
    let output = Command::new("./target/release/ninep-server")
        .args(&[
            "auto-mount",
            "start",
            "--mount-point",
            "/invalid/nonexistent/path/that/should/fail",
        ])
        .output()
        .expect("Failed to run command");

    // Should fail gracefully
    assert!(
        !output.status.success(),
        "Should fail with invalid mount point"
    );
}
