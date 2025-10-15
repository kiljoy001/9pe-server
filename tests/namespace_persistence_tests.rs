//! Tests for NamespaceManager persistence functionality

use anyhow::Result;
use ed25519_dalek::SigningKey;
use ninep_server::{namespace_manager::NamespaceManager, synth::SyntheticFilesystem};
use std::sync::Arc;
use tempfile::TempDir;

#[tokio::test]
async fn test_namespace_persistence() -> Result<()> {
    // Create a temporary directory for the database
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("namespaces.db");

    // Create namespace manager with persistence
    let synth_fs = Arc::new(SyntheticFilesystem::new());
    let manager = NamespaceManager::with_db_path(synth_fs, Some(db_path.clone()))?;

    // Create a keypair for the namespace owner
    let owner_keypair = SigningKey::from_bytes(&rand::random());

    // Register a namespace
    let claim = manager
        .register_namespace(
            "/srv/test/persistence",
            "Test namespace for persistence",
            "test",
            None,
            None,
            &owner_keypair,
        )
        .await?;

    // Verify the namespace was registered
    assert_eq!(claim.path, "/srv/test/persistence");

    // Get the list of namespaces to confirm it's in memory
    let namespaces = manager.list_namespaces().await;
    assert_eq!(namespaces.len(), 1);
    assert_eq!(namespaces[0].path, "/srv/test/persistence");

    // Create a new manager with the same database path
    let synth_fs2 = Arc::new(SyntheticFilesystem::new());
    let manager2 = NamespaceManager::with_db_path(synth_fs2, Some(db_path))?;
    manager2.load_namespace_claims().await?;

    // Verify the namespace was loaded from persistence
    let namespaces2 = manager2.list_namespaces().await;
    assert_eq!(namespaces2.len(), 1);
    assert_eq!(namespaces2[0].path, "/srv/test/persistence");

    Ok(())
}

#[tokio::test]
async fn test_namespace_persistence_without_db_path() -> Result<()> {
    // Create namespace manager without persistence
    let synth_fs = Arc::new(SyntheticFilesystem::new());
    let manager = NamespaceManager::new(synth_fs)?;

    // This should work fine even without persistence
    let owner_keypair = SigningKey::from_bytes(&rand::random());

    let claim = manager
        .register_namespace(
            "/srv/test/no_persistence",
            "Test namespace without persistence",
            "test",
            None,
            None,
            &owner_keypair,
        )
        .await?;

    assert_eq!(claim.path, "/srv/test/no_persistence");

    Ok(())
}
