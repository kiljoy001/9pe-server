//! Tests for NamespaceManager with new features including access requests and M-of-N requirements
//!
//! Tests added features:
//! - Access requests with approval/rejection workflows
//! - Participant management for M-of-N requirements
//! - Liveness tracking and garbage collection
//! - Public namespace automatic approval
//! - M-of-N signature requirements validation

use anyhow::Result;
use ninep_server::{
    namespace_manager::{NamespaceManager, NamespaceClaim, NamespaceMetadata},
    synth::SyntheticFilesystem,
};
use std::sync::Arc;
use tokio;
use ed25519_dalek::{SigningKey, Signer};
use chrono::{Utc, Duration};
use hex;

#[tokio::test]
async fn test_namespace_participant_management() -> Result<()> {
    let synth_fs = Arc::new(SyntheticFilesystem::new());
    let manager = NamespaceManager::new(synth_fs)?;
    
    let owner_keypair = SigningKey::from_bytes(&rand::random());
    let participant_keypair = SigningKey::from_bytes(&rand::random());
    
    // Register namespace with 2-of-3 requirements
    let claim = manager.register_namespace(
        "/srv/test/participant",
        "Test namespace with participants",
        "test",
        Some((2, 3)), // 2-of-3 signatures required
        None, // expires_at
        &owner_keypair,
    ).await?;
    
    assert_eq!(claim.metadata.participant_requirements, Some((2, 3)));
    assert_eq!(claim.metadata.participants.len(), 1); // Owner is first participant
    assert_eq!(claim.metadata.participants[0], hex::encode(owner_keypair.verifying_key().as_bytes()));
    
    // Add a participant
    let participant_pubkey = hex::encode(participant_keypair.verifying_key().as_bytes());
    manager.add_participant(
        "/srv/test/participant",
        &participant_pubkey,
        &owner_keypair,
    ).await?;
    
    let updated_claim = manager.get_claim("/srv/test/participant").await?;
    assert_eq!(updated_claim.metadata.participants.len(), 2);
    assert!(updated_claim.metadata.participants.contains(&participant_pubkey));
    
    // Remove participant
    manager.remove_participant(
        "/srv/test/participant",
        &participant_pubkey,
        &owner_keypair,
    ).await?;
    
    let final_claim = manager.get_claim("/srv/test/participant").await?;
    assert_eq!(final_claim.metadata.participants.len(), 1);
    assert!(!final_claim.metadata.participants.contains(&participant_pubkey));
    
    Ok(())
}

#[tokio::test]
async fn test_namespace_access_request_workflow() -> Result<()> {
    let synth_fs = Arc::new(SyntheticFilesystem::new());
    let manager = NamespaceManager::new(synth_fs)?;
    
    let owner_keypair = SigningKey::from_bytes(&rand::random());
    let requester_keypair = SigningKey::from_bytes(&rand::random());
    
    // Register namespace
    manager.register_namespace(
        "/srv/test/access",
        "Test namespace with access control",
        "test",
        None,
        None, // expires_at
        &owner_keypair,
    ).await?;
    
    let requester_pubkey = hex::encode(requester_keypair.verifying_key().as_bytes());
    
    // Submit access request
    manager.submit_access_request(
        "/srv/test/access",
        &requester_pubkey,
        "participant",
        "Requesting access for testing",
    ).await?;
    
    // Check pending requests
    let pending = manager.list_pending_requests("/srv/test/access").await?;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].requester_pubkey, requester_pubkey);
    assert_eq!(pending[0].requested_role, "participant");
    assert_eq!(pending[0].status, "pending");
    
    // Approve request
    manager.approve_access_request(
        "/srv/test/access",
        &requester_pubkey,
        &owner_keypair,
    ).await?;
    
    // Check that requester is now a participant
    let claim = manager.get_claim("/srv/test/access").await?;
    assert!(claim.metadata.participants.contains(&requester_pubkey));
    
    // Check that request is no longer pending
    let pending = manager.list_pending_requests("/srv/test/access").await?;
    assert_eq!(pending.len(), 0);
    
    Ok(())
}

#[tokio::test]
async fn test_public_namespace_auto_approval() -> Result<()> {
    let synth_fs = Arc::new(SyntheticFilesystem::new());
    let manager = NamespaceManager::new(synth_fs)?;
    
    let owner_keypair = SigningKey::from_bytes(&rand::random());
    let requester_keypair = SigningKey::from_bytes(&rand::random());
    
    // Register public namespace with open participation (1,0) = open
    manager.register_namespace(
        "/srv/public/test",
        "Public test namespace",
        "public",
        Some((1, 0)), // Open participation
        None, // expires_at
        &owner_keypair,
    ).await?;
    
    let requester_pubkey = hex::encode(requester_keypair.verifying_key().as_bytes());
    
    // Submit access request - should be auto-approved
    manager.submit_access_request(
        "/srv/public/test",
        &requester_pubkey,
        "participant",
        "Requesting access to public namespace",
    ).await?;
    
    // Check that requester is immediately a participant
    let claim = manager.get_claim("/srv/public/test").await?;
    assert!(claim.metadata.participants.contains(&requester_pubkey));
    
    // Check that no pending requests were created
    let pending = manager.list_pending_requests("/srv/public/test").await?;
    assert_eq!(pending.len(), 0);
    
    Ok(())
}

#[tokio::test]
async fn test_namespace_liveness_tracking() -> Result<()> {
    let synth_fs = Arc::new(SyntheticFilesystem::new());
    let manager = NamespaceManager::new(synth_fs)?;
    
    let owner_keypair = SigningKey::from_bytes(&rand::random());
    let participant_keypair = SigningKey::from_bytes(&rand::random());
    
    // Register namespace
    manager.register_namespace(
        "/srv/test/liveness",
        "Test namespace with liveness tracking",
        "test",
        None,
        None, // expires_at
        &owner_keypair,
    ).await?;
    
    let participant_pubkey = hex::encode(participant_keypair.verifying_key().as_bytes());
    
    // Add participant
    manager.add_participant(
        "/srv/test/liveness",
        &participant_pubkey,
        &owner_keypair,
    ).await?;
    
    // Update liveness
    manager.update_liveness("/srv/test/liveness", &participant_pubkey).await?;
    
    // Check that last activity was updated
    let claim = manager.get_claim("/srv/test/liveness").await?;
    let now = Utc::now();
    let elapsed = now - claim.metadata.last_activity;
    assert!(elapsed.num_seconds() < 5); // Should be recent
    
    Ok(())
}

#[tokio::test]
async fn test_namespace_garbage_collection() -> Result<()> {
    let synth_fs = Arc::new(SyntheticFilesystem::new());
    let manager = NamespaceManager::new(synth_fs)?;
    
    let owner_keypair = SigningKey::from_bytes(&rand::random());
    
    // Register namespace that will expire immediately
    let expired_at = Utc::now() - Duration::seconds(1);
    manager.register_namespace(
        "/srv/test/expired",
        "Test expired namespace",
        "test",
        None,
        Some(expired_at),
        &owner_keypair,
    ).await?;
    
    // Register namespace that should not expire
    manager.register_namespace(
        "/srv/test/active",
        "Test active namespace",
        "test",
        None,
        None, // expires_at
        &owner_keypair,
    ).await?;
    
    // Run garbage collection
    let count = manager.garbage_collect().await?;
    assert_eq!(count, 1); // One namespace should be collected
    
    // Check that expired namespace is gone
    let claims = manager.list_namespaces().await;
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].path, "/srv/test/active");
    
    Ok(())
}

#[tokio::test]
async fn test_namespace_access_request_rejection() -> Result<()> {
    let synth_fs = Arc::new(SyntheticFilesystem::new());
    let manager = NamespaceManager::new(synth_fs)?;
    
    let owner_keypair = SigningKey::from_bytes(&rand::random());
    let requester_keypair = SigningKey::from_bytes(&rand::random());
    
    // Register namespace
    manager.register_namespace(
        "/srv/test/reject",
        "Test namespace with rejection",
        "test",
        None,
        None, // expires_at
        &owner_keypair,
    ).await?;
    
    let requester_pubkey = hex::encode(requester_keypair.verifying_key().as_bytes());
    
    // Submit access request
    manager.submit_access_request(
        "/srv/test/reject",
        &requester_pubkey,
        "participant",
        "Requesting access for testing",
    ).await?;
    
    // Reject request
    manager.reject_access_request(
        "/srv/test/reject",
        &requester_pubkey,
        &owner_keypair,
    ).await?;
    
    // Check that requester is NOT a participant
    let claim = manager.get_claim("/srv/test/reject").await?;
    assert!(!claim.metadata.participants.contains(&requester_pubkey));
    
    // Check request status
    let pending = manager.list_pending_requests("/srv/test/reject").await?;
    assert_eq!(pending.len(), 0); // No pending requests
    
    Ok(())
}

#[tokio::test]
async fn test_namespace_m_of_n_requirements() -> Result<()> {
    let synth_fs = Arc::new(SyntheticFilesystem::new());
    let manager = NamespaceManager::new(synth_fs)?;

    let owner_keypair = SigningKey::from_bytes(&rand::random());
    
    // Register namespace with 2-of-3 requirements
    manager.register_namespace(
        "/srv/test/mofn",
        "Test namespace with M-of-N requirements",
        "test",
        Some((2, 3)), // 2-of-3 signatures required
        None, // expires_at
        &owner_keypair,
    ).await?;

    let claim = manager.get_claim("/srv/test/mofn").await?;
    assert_eq!(claim.metadata.participant_requirements, Some((2, 3)));
    
    // Test that owner can add participants
    let participant1_keypair = SigningKey::from_bytes(&rand::random());
    let participant2_keypair = SigningKey::from_bytes(&rand::random());
    
    let participant1_pubkey = hex::encode(participant1_keypair.verifying_key().as_bytes());
    let participant2_pubkey = hex::encode(participant2_keypair.verifying_key().as_bytes());
    
    manager.add_participant(
        "/srv/test/mofn",
        &participant1_pubkey,
        &owner_keypair,
    ).await?;

    manager.add_participant(
        "/srv/test/mofn",
        &participant2_pubkey,
        &owner_keypair,
    ).await?;

    let updated_claim = manager.get_claim("/srv/test/mofn").await?;
    assert_eq!(updated_claim.metadata.participants.len(), 3); // owner + 2 participants
    assert!(updated_claim.metadata.participants.contains(&participant1_pubkey));
    assert!(updated_claim.metadata.participants.contains(&participant2_pubkey));

    // Test that participants can update liveness
    manager.update_liveness("/srv/test/mofn", &participant1_pubkey).await?;
    manager.update_liveness("/srv/test/mofn", &participant2_pubkey).await?;

    // Test that unauthorized users cannot perform operations
    let unauthorized_keypair = SigningKey::from_bytes(&rand::random());
    let result = manager.add_participant(
        "/srv/test/mofn",
        &hex::encode(unauthorized_keypair.verifying_key().as_bytes()),
        &unauthorized_keypair,
    ).await;
    
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Unauthorized"));

    Ok(())
}

#[tokio::test]
async fn test_namespace_open_participation() -> Result<()> {
    let synth_fs = Arc::new(SyntheticFilesystem::new());
    let manager = NamespaceManager::new(synth_fs)?;

    let owner_keypair = SigningKey::from_bytes(&rand::random());
    
    // Register public namespace with open participation (1,0) = open
    manager.register_namespace(
        "/srv/public/open",
        "Public namespace with open participation",
        "public",
        Some((1, 0)), // Open participation
        None, // expires_at
        &owner_keypair,
    ).await?;

    let claim = manager.get_claim("/srv/public/open").await?;
    assert_eq!(claim.metadata.participant_requirements, Some((1, 0)));

    Ok(())
}