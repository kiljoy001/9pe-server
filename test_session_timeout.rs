use ninep_server::auth::{AuthService, AuthConfig};
use tempfile::tempdir;
use std::time::Instant;

#[tokio::main]
async fn main() {
    println!("Testing session timeout behavior...");

    let dir = tempdir().unwrap();
    let config = AuthConfig {
        db_path: dir.path().join("test.db"),
        session_timeout: 2, // 2 seconds
        ..Default::default()
    };

    let auth = AuthService::new(config).await.unwrap();

    // Create user
    auth.create_user("testuser", "password123", 1000, 1000, vec![]).await.unwrap();

    // Authenticate
    let token = auth.authenticate("testuser", "password123", None).await.unwrap();
    println!("Token created");

    // Validate immediately
    let start = Instant::now();
    let result = auth.validate_session(&token).await;
    println!("First validation (should succeed): {:?} - took {:?}", result.is_ok(), start.elapsed());
    assert!(result.is_ok());

    // Wait for expiration
    println!("Waiting 3 seconds for session to expire...");
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // Validate after expiration
    let start = Instant::now();
    let result = auth.validate_session(&token).await;
    println!("Second validation (should fail): {:?} - took {:?}", result.is_ok(), start.elapsed());
    assert!(result.is_err());

    println!("Test completed successfully!");
}