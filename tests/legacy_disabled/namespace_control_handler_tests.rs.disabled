//! Tests for NamespaceManager control handlers signature verification
//!
//! Tests the signature verification in:
//! - RegisterNamespaceHandler
//! - DeleteNamespaceHandler

use anyhow::Result;
use ed25519_dalek::{Signer, SigningKey};
use ninep_server::{
    namespace_manager::NamespaceManager,
    synth::{ControlHandler, SyntheticFilesystem},
};
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn test_register_namespace_handler_signature_verification() -> Result<()> {
    let synth_fs = Arc::new(SyntheticFilesystem::new());
    let manager = NamespaceManager::new(synth_fs)?;

    // Create a keypair for the namespace owner
    let keypair = SigningKey::from_bytes(&rand::random());
    let pubkey_hex = hex::encode(keypair.verifying_key().as_bytes());

    // Create valid registration data
    let path = "/srv/test/register_handler";
    let created_at = chrono::Utc::now().timestamp();
    let sign_data = format!(
        "{}{}{}{}",
        path,
        pubkey_hex,
        created_at,
        "" // participant_requirements (empty for None)
    );
    let signature = keypair.sign(sign_data.as_bytes());
    let signature_hex = hex::encode(signature.to_bytes());

    // Create the registration request JSON
    let request_json = json!({
        "path": path,
        "description": "Test namespace for handler",
        "type": "test",
        "pubkey": pubkey_hex,
        "signature": signature_hex
    });

    // Get the register handler
    let register_handler = manager.create_register_handler();

    // Test valid signature
    let result = register_handler.write(serde_json::to_string(&request_json)?.as_bytes());
    assert!(result.is_ok(), "Valid signature should be accepted");

    Ok(())
}

#[tokio::test]
async fn test_register_namespace_handler_invalid_signature() -> Result<()> {
    let synth_fs = Arc::new(SyntheticFilesystem::new());
    let manager = NamespaceManager::new(synth_fs)?;

    // Create a keypair for the namespace owner
    let keypair = SigningKey::from_bytes(&rand::random());
    let pubkey_hex = hex::encode(keypair.verifying_key().as_bytes());

    // Create invalid registration data (wrong signature)
    let path = "/srv/test/register_handler_invalid";
    let wrong_keypair = SigningKey::from_bytes(&rand::random());
    let sign_data = format!("{}{}", path, pubkey_hex);
    let signature = wrong_keypair.sign(sign_data.as_bytes());
    let signature_hex = hex::encode(signature.to_bytes());

    // Create the registration request JSON with invalid signature
    let request_json = json!({
        "path": path,
        "description": "Test namespace for handler with invalid signature",
        "type": "test",
        "pubkey": pubkey_hex,
        "signature": signature_hex
    });

    // Get the register handler
    let register_handler = manager.create_register_handler();

    // Test invalid signature
    let result = register_handler.write(serde_json::to_string(&request_json)?.as_bytes());
    assert!(result.is_err(), "Invalid signature should be rejected");

    Ok(())
}

#[tokio::test]
async fn test_delete_namespace_handler_signature_verification() -> Result<()> {
    let synth_fs = Arc::new(SyntheticFilesystem::new());
    let manager = NamespaceManager::new(synth_fs)?;

    // First register a namespace
    let keypair = SigningKey::from_bytes(&rand::random());
    manager
        .register_namespace(
            "/srv/test/delete_handler",
            "Test namespace for delete handler",
            "test",
            None,
            None,
            &keypair,
        )
        .await?;

    // Create valid delete data
    let path = "/srv/test/delete_handler";
    let pubkey_hex = hex::encode(keypair.verifying_key().as_bytes());
    let sign_data = format!("DELETE:{}:{}", path, pubkey_hex);
    let signature = keypair.sign(sign_data.as_bytes());
    let signature_hex = hex::encode(signature.to_bytes());

    // Create the delete request JSON
    let request_json = json!({
        "path": path,
        "pubkey": pubkey_hex,
        "signature": signature_hex
    });

    // Get the delete handler
    let delete_handler = manager.create_delete_handler();

    // Test valid signature
    let result = delete_handler.write(serde_json::to_string(&request_json)?.as_bytes());
    assert!(result.is_ok(), "Valid signature should be accepted");

    Ok(())
}

#[tokio::test]
async fn test_delete_namespace_handler_invalid_signature() -> Result<()> {
    let synth_fs = Arc::new(SyntheticFilesystem::new());
    let manager = NamespaceManager::new(synth_fs)?;

    // First register a namespace
    let keypair = SigningKey::from_bytes(&rand::random());
    manager
        .register_namespace(
            "/srv/test/delete_handler_invalid",
            "Test namespace for delete handler with invalid signature",
            "test",
            None,
            None,
            &keypair,
        )
        .await?;

    // Create invalid delete data (wrong signature)
    let path = "/srv/test/delete_handler_invalid";
    let pubkey_hex = hex::encode(keypair.verifying_key().as_bytes());
    let wrong_keypair = SigningKey::from_bytes(&rand::random());
    let sign_data = format!("DELETE:{}:{}", path, pubkey_hex);
    let signature = wrong_keypair.sign(sign_data.as_bytes());
    let signature_hex = hex::encode(signature.to_bytes());

    // Create the delete request JSON with invalid signature
    let request_json = json!({
        "path": path,
        "pubkey": pubkey_hex,
        "signature": signature_hex
    });

    // Get the delete handler
    let delete_handler = manager.create_delete_handler();

    // Test invalid signature
    let result = delete_handler.write(serde_json::to_string(&request_json)?.as_bytes());
    assert!(result.is_err(), "Invalid signature should be rejected");

    Ok(())
}
