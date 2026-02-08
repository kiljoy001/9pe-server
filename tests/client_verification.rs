use std::fs;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tempfile::TempDir;
use anyhow::Result;

// Import our client and identity
use ninepe_server::client::{NinePClient, ClientIdentity};

/// Helper to start a server instance
struct TestServer {
    child: Child,
    port: u16,
    temp_dir: TempDir,
}

impl TestServer {
    fn start(port: u16) -> Result<Self> {
        let temp_dir = TempDir::new()?;
        let root_path = temp_dir.path().to_path_buf();
        let config_path = root_path.join("config.toml");

        // Create some test files
        fs::write(root_path.join("test.txt"), b"Hello from 9P server!")?;
        fs::create_dir(root_path.join("subdir"))?;
        fs::write(root_path.join("subdir/data.json"), b"{\"test\": true}")?;

        // Create config file - port, root, and disable QUIC (use TCP)
        let config_content = format!(
            r#"
[server]
listen_addr = "127.0.0.1:{}"
root = "{}"

[server.transport]
type = "tcp"
"#,
            port,
            root_path.display()
        );
        fs::write(&config_path, config_content)?;

        // Start server
        let child = Command::new("./target/debug/ninepe-server")
            .env("HOME", root_path.to_str().unwrap()) // Isolate .9pe folder
            .args(&[
                "serve",
                "--config",
                config_path.to_str().unwrap(),
            ])
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;

        // Wait for server to be ready
        std::thread::sleep(Duration::from_secs(2)); // Reduced wait time as we're not building

        Ok(TestServer {
            child,
            port,
            temp_dir,
        })
    }

    fn address(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[tokio::test]
async fn test_client_connect() -> Result<()> {
    let server = TestServer::start(15640)?;
    
    // Test basic connection
    let client = NinePClient::connect(&server.address()).await;
    assert!(client.is_ok(), "Failed to connect to server: {:?}", client.err());
    
    Ok(())
}

#[tokio::test]
async fn test_client_list_root() -> Result<()> {
    let server = TestServer::start(15641)?;

    // Authentication required for directory listing
    let identity = ClientIdentity::generate()?;
    let mut client = NinePClient::connect_authenticated(&server.address(), identity).await?;

    let files = client.list_directory("/").await?;
    assert!(files.contains(&"test.txt".to_string()), "Root should contain test.txt");
    assert!(files.contains(&"subdir".to_string()), "Root should contain subdir");

    Ok(())
}

#[tokio::test]
async fn test_client_read_file() -> Result<()> {
    let server = TestServer::start(15642)?;

    // Authentication required for file reads
    let identity = ClientIdentity::generate()?;
    let mut client = NinePClient::connect_authenticated(&server.address(), identity).await?;

    let content = client.read_file("test.txt").await?;
    assert_eq!(content, b"Hello from 9P server!", "File content mismatch");

    Ok(())
}

#[tokio::test]
async fn test_client_read_at() -> Result<()> {
    let server = TestServer::start(15643)?;

    // Authentication required for file reads
    let identity = ClientIdentity::generate()?;
    let mut client = NinePClient::connect_authenticated(&server.address(), identity).await?;

    // Read "9P server!" (offset 11, length 10)
    let content = client.read_at("test.txt", 11, 10).await?;
    assert_eq!(content, b"9P server!", "Read at offset mismatch");

    Ok(())
}

#[tokio::test]
async fn test_client_stat() -> Result<()> {
    let server = TestServer::start(15644)?;

    // Authentication required for stat
    let identity = ClientIdentity::generate()?;
    let mut client = NinePClient::connect_authenticated(&server.address(), identity).await?;

    // Stat should return something (protocol specific, but not error)
    let stat = client.stat("test.txt").await;
    assert!(stat.is_ok(), "Stat failed");

    Ok(())
}

#[tokio::test]
async fn test_client_authenticated_read() -> Result<()> {
    let server = TestServer::start(15645)?;

    // Generate a fresh identity for this test
    let identity = ClientIdentity::generate()?;

    // Connect with authentication
    let mut client = NinePClient::connect_authenticated(&server.address(), identity).await?;

    // Now reads should work (we're authenticated)
    let content = client.read_file("test.txt").await?;
    assert_eq!(content, b"Hello from 9P server!", "Authenticated read should work");

    Ok(())
}

#[tokio::test]
async fn test_client_authenticated_list() -> Result<()> {
    let server = TestServer::start(15646)?;

    // Generate a fresh identity
    let identity = ClientIdentity::generate()?;

    // Connect with authentication
    let mut client = NinePClient::connect_authenticated(&server.address(), identity).await?;

    // List should work
    let files = client.list_directory("/").await?;
    assert!(files.contains(&"test.txt".to_string()), "Authenticated list should show test.txt");

    Ok(())
}
