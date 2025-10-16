//! Tests for 9P.e Namespace Access extension messages
//!
//! Tests the new namespace access extension messages:
//! - NamespaceAccessRequest
//! - NamespaceAccessResponse

use ninep_server::protocol::NinePeeMessage;
use serde_json;

#[test]
fn test_namespace_access_request_message() {
    let mut requester_pubkey = [0u8; 32];
    requester_pubkey.copy_from_slice(&rand::random::<[u8; 32]>());

    let message = NinePeeMessage::NamespaceAccessRequest {
        namespace_path: "/srv/compute/pool".to_string(),
        requester_pubkey,
        requested_role: "participant".to_string(),
        message: "Requesting access to compute pool".to_string(),
    };

    // Test serialization
    let serialized = bincode::serialize(&message).unwrap();
    let deserialized: NinePeeMessage = bincode::deserialize(&serialized).unwrap();

    match deserialized {
        NinePeeMessage::NamespaceAccessRequest {
            namespace_path,
            requester_pubkey: pubkey,
            requested_role,
            message: msg,
        } => {
            assert_eq!(namespace_path, "/srv/compute/pool");
            assert_eq!(pubkey, requester_pubkey);
            assert_eq!(requested_role, "participant");
            assert_eq!(msg, "Requesting access to compute pool");
        }
        _ => panic!("Deserialized to wrong message type"),
    }

    // Test JSON serialization
    let json = serde_json::to_string(&message).unwrap();
    let from_json: NinePeeMessage = serde_json::from_str(&json).unwrap();

    match from_json {
        NinePeeMessage::NamespaceAccessRequest {
            namespace_path,
            requester_pubkey: pubkey,
            requested_role,
            message: msg,
        } => {
            assert_eq!(namespace_path, "/srv/compute/pool");
            assert_eq!(pubkey, requester_pubkey);
            assert_eq!(requested_role, "participant");
            assert_eq!(msg, "Requesting access to compute pool");
        }
        _ => panic!("JSON deserialized to wrong message type"),
    }
}

#[test]
fn test_namespace_access_response_message() {
    let mut requester_pubkey = [0u8; 32];
    requester_pubkey.copy_from_slice(&rand::random::<[u8; 32]>());

    let message = NinePeeMessage::NamespaceAccessResponse {
        namespace_path: "/srv/compute/pool".to_string(),
        requester_pubkey,
        approved: true,
        message: "Access granted".to_string(),
    };

    // Test serialization
    let serialized = bincode::serialize(&message).unwrap();
    let deserialized: NinePeeMessage = bincode::deserialize(&serialized).unwrap();

    match deserialized {
        NinePeeMessage::NamespaceAccessResponse {
            namespace_path,
            requester_pubkey: pubkey,
            approved,
            message: msg,
        } => {
            assert_eq!(namespace_path, "/srv/compute/pool");
            assert_eq!(pubkey, requester_pubkey);
            assert_eq!(approved, true);
            assert_eq!(msg, "Access granted");
        }
        _ => panic!("Deserialized to wrong message type"),
    }

    // Test JSON serialization
    let json = serde_json::to_string(&message).unwrap();
    let from_json: NinePeeMessage = serde_json::from_str(&json).unwrap();

    match from_json {
        NinePeeMessage::NamespaceAccessResponse {
            namespace_path,
            requester_pubkey: pubkey,
            approved,
            message: msg,
        } => {
            assert_eq!(namespace_path, "/srv/compute/pool");
            assert_eq!(pubkey, requester_pubkey);
            assert_eq!(approved, true);
            assert_eq!(msg, "Access granted");
        }
        _ => panic!("JSON deserialized to wrong message type"),
    }

    // Test rejection case
    let reject_message = NinePeeMessage::NamespaceAccessResponse {
        namespace_path: "/srv/compute/pool".to_string(),
        requester_pubkey,
        approved: false,
        message: "Access denied: insufficient permissions".to_string(),
    };

    let json = serde_json::to_string(&reject_message).unwrap();
    let from_json: NinePeeMessage = serde_json::from_str(&json).unwrap();

    match from_json {
        NinePeeMessage::NamespaceAccessResponse {
            namespace_path,
            requester_pubkey: pubkey,
            approved,
            message: msg,
        } => {
            assert_eq!(namespace_path, "/srv/compute/pool");
            assert_eq!(pubkey, requester_pubkey);
            assert_eq!(approved, false);
            assert_eq!(msg, "Access denied: insufficient permissions");
        }
        _ => panic!("JSON deserialized to wrong message type"),
    }
}

#[test]
fn test_namespace_extension_message_identification() {
    let mut requester_pubkey = [0u8; 32];
    requester_pubkey.copy_from_slice(&rand::random::<[u8; 32]>());

    // Test that namespace extension messages are properly identified
    let access_request = NinePeeMessage::NamespaceAccessRequest {
        namespace_path: "/test".to_string(),
        requester_pubkey,
        requested_role: "participant".to_string(),
        message: "test".to_string(),
    };

    let access_response = NinePeeMessage::NamespaceAccessResponse {
        namespace_path: "/test".to_string(),
        requester_pubkey,
        approved: true,
        message: "test".to_string(),
    };

    // These should NOT be identified as errors
    assert!(!access_request.is_error());
    assert!(!access_response.is_error());

    // These should NOT be identified as basic extensions
    // (They're namespace extensions, not the original translator/consensus extensions)
    assert!(!access_request.is_extension());
    assert!(!access_response.is_extension());
}

#[test]
fn test_namespace_extension_message_fid_handling() {
    let mut requester_pubkey = [0u8; 32];
    requester_pubkey.copy_from_slice(&rand::random::<[u8; 32]>());

    // Namespace extension messages don't have FIDs, so they should return None
    let access_request = NinePeeMessage::NamespaceAccessRequest {
        namespace_path: "/test".to_string(),
        requester_pubkey,
        requested_role: "participant".to_string(),
        message: "test".to_string(),
    };

    let access_response = NinePeeMessage::NamespaceAccessResponse {
        namespace_path: "/test".to_string(),
        requester_pubkey,
        approved: true,
        message: "test".to_string(),
    };

    assert_eq!(access_request.fid(), None);
    assert_eq!(access_response.fid(), None);
}

#[test]
fn test_namespace_access_request_with_different_roles() {
    let mut requester_pubkey = [0u8; 32];
    requester_pubkey.copy_from_slice(&rand::random::<[u8; 32]>());

    // Test different roles
    let roles = vec!["participant", "contributor", "admin"];

    for role in roles {
        let message = NinePeeMessage::NamespaceAccessRequest {
            namespace_path: "/test".to_string(),
            requester_pubkey,
            requested_role: role.to_string(),
            message: format!("Requesting {} role", role),
        };

        let json = serde_json::to_string(&message).unwrap();
        let from_json: NinePeeMessage = serde_json::from_str(&json).unwrap();

        match from_json {
            NinePeeMessage::NamespaceAccessRequest {
                requested_role: r, ..
            } => {
                assert_eq!(r, role);
            }
            _ => panic!("JSON deserialized to wrong message type"),
        }
    }
}
