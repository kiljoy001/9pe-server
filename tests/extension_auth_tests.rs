//! Tests for 9P.e extension authentication requirements
//!
//! Verifies that all WASM and shared memory operations require authentication.

use proptest::prelude::*;
use std::path::Path;
use std::sync::Arc;
use tokio::runtime::Runtime;

use ninepe_server::server::handler::PublicConnectionState as ConnectionState;
use ninepe_server::server::handler::PublicNinePExtensionsHandler as NinePExtensionsHandler;
use ninepe_server::ipc::SharedMemoryManager;
use ninepe_server::memory::MemoryManager;
use ninepe_server::protocol::NinePMessage;
use ninepe_server::traits::{
    WasmProvider, WasmMetadata, Translator,
    StorageProvider, FileAttr, DirEntry,
};

/// Helper to create a test SharedMemoryManager
fn create_test_shm() -> Arc<SharedMemoryManager> {
    let mm = Arc::new(MemoryManager::new());
    Arc::new(SharedMemoryManager::new(mm).expect("create shm"))
}

/// Property: Unauthenticated connections cannot allocate shared memory
#[test]
fn prop_mem_alloc_requires_auth() {
    let rt = Runtime::new().unwrap();

    proptest!(|(
        size in 1u64..1024u64,
        id in "[a-z]{1,10}",
    )| {
        let is_denied = rt.block_on(async {
            let connection_state = ConnectionState::new();
            let shm = create_test_shm();

            // Create handler with unauthenticated state
            let handler = NinePExtensionsHandler::new(
                Arc::new(MockWasmProvider),
                Arc::new(MockStorageProvider),
                connection_state,
                shm,
            );

            // Attempt mem_alloc without auth
            let result = handler.handle_mem_alloc(size, id.clone()).await;

            match result {
                Ok(NinePMessage::Error { errno, .. }) => errno == 1, // EPERM
                _ => false,
            }
        });

        prop_assert!(is_denied, "MemAlloc should require authentication");
    });
}

/// Property: Unauthenticated connections cannot borrow shared memory
#[test]
fn prop_mem_borrow_requires_auth() {
    let rt = Runtime::new().unwrap();

    proptest!(|(
        id in "[a-z]{1,10}",
        write in proptest::bool::ANY,
    )| {
        let is_denied = rt.block_on(async {
            let connection_state = ConnectionState::new();
            let shm = create_test_shm();

            let handler = NinePExtensionsHandler::new(
                Arc::new(MockWasmProvider),
                Arc::new(MockStorageProvider),
                connection_state.clone(),
                shm,
            );

            // Attempt mem_borrow without auth
            let result = handler.handle_mem_borrow(id.clone(), write, &connection_state).await;

            match result {
                Ok(NinePMessage::Error { errno, .. }) => errno == 1, // EPERM
                _ => false,
            }
        });

        prop_assert!(is_denied, "MemBorrow should require authentication");
    });
}

/// Property: Unauthenticated connections cannot release shared memory
#[test]
fn prop_mem_release_requires_auth() {
    let rt = Runtime::new().unwrap();

    proptest!(|(
        id in "[a-z]{1,10}",
    )| {
        let is_denied = rt.block_on(async {
            let connection_state = ConnectionState::new();
            let shm = create_test_shm();

            let handler = NinePExtensionsHandler::new(
                Arc::new(MockWasmProvider),
                Arc::new(MockStorageProvider),
                connection_state.clone(),
                shm,
            );

            // Attempt mem_release without auth
            let result = handler.handle_mem_release(id.clone(), &connection_state).await;

            match result {
                Ok(NinePMessage::Error { errno, .. }) => errno == 1, // EPERM
                _ => false,
            }
        });

        prop_assert!(is_denied, "MemRelease should require authentication");
    });
}

/// Property: Unauthenticated connections cannot invoke WASM functions
#[test]
fn prop_wasm_invoke_requires_auth() {
    let rt = Runtime::new().unwrap();

    proptest!(|(
        path in "/[a-z]{1,10}",
        function in "[a-z]{1,10}",
    )| {
        let is_denied = rt.block_on(async {
            let connection_state = ConnectionState::new();
            let shm = create_test_shm();

            let handler = NinePExtensionsHandler::new(
                Arc::new(MockWasmProvider),
                Arc::new(MockStorageProvider),
                connection_state,
                shm,
            );

            // Attempt WASM invoke without auth
            let result = handler.handle_wasm_invoke(path.clone(), function.clone(), vec![]).await;

            match result {
                Ok(NinePMessage::Error { errno, .. }) => errno == 1, // EPERM
                _ => false,
            }
        });

        prop_assert!(is_denied, "WASM invoke should require authentication");
    });
}

/// Integration test: All extension operations fail without auth
#[tokio::test]
async fn test_all_extensions_require_auth() {
    let connection_state = ConnectionState::new();
    let shm = create_test_shm();

    let handler = NinePExtensionsHandler::new(
        Arc::new(MockWasmProvider),
        Arc::new(MockStorageProvider),
        connection_state.clone(),
        shm,
    );

    // Test MemAlloc
    let result = handler.handle_mem_alloc(1024, "test".to_string()).await.unwrap();
    assert!(matches!(result, NinePMessage::Error { errno: 1, .. }), "MemAlloc should deny unauthenticated");

    // Test MemBorrow
    let result = handler.handle_mem_borrow("test".to_string(), false, &connection_state).await.unwrap();
    assert!(matches!(result, NinePMessage::Error { errno: 1, .. }), "MemBorrow should deny unauthenticated");

    // Test MemRelease
    let result = handler.handle_mem_release("test".to_string(), &connection_state).await.unwrap();
    assert!(matches!(result, NinePMessage::Error { errno: 1, .. }), "MemRelease should deny unauthenticated");

    // Test WASM invoke
    let result = handler.handle_wasm_invoke("/test".to_string(), "func".to_string(), vec![]).await.unwrap();
    assert!(matches!(result, NinePMessage::Error { errno: 1, .. }), "WASM invoke should deny unauthenticated");
}

// ============================================================================
// FID Collision Detection Tests
// ============================================================================

/// Test that next_fid skips already-used fids
#[tokio::test]
async fn test_fid_collision_detection() {
    let state = ConnectionState::new();

    // Allocate several fids
    let fid1 = state.next_fid().await;
    let fid2 = state.next_fid().await;
    let fid3 = state.next_fid().await;

    // All should be different
    assert_ne!(fid1, fid2);
    assert_ne!(fid2, fid3);
    assert_ne!(fid1, fid3);

    // Create file handles for fid1 and fid2
    state.create_fid(fid1, "/test1".to_string(), 0, false, None).await;
    state.create_fid(fid2, "/test2".to_string(), 0, false, None).await;

    // fid3 should still be available since we didn't use it yet
    // But if we get next_fid again, it should skip fid1 and fid2 if they're in range
    let fid4 = state.next_fid().await;
    assert_ne!(fid4, fid1);
    assert_ne!(fid4, fid2);
}

/// Test that auth sessions are cleaned up after verification
#[tokio::test]
async fn test_auth_session_cleanup() {
    let state = ConnectionState::new();

    // Create an auth session
    let _challenge = state.create_auth_session(42, "test-server".to_string(), None).await;

    // Session count should be 1
    assert_eq!(state.auth_session_count().await, 1);

    // Get the challenge (should exist)
    assert!(state.get_auth_challenge(42).await.is_some());

    // Remove the session manually (simulating cleanup)
    let removed = state.remove_auth_session(42).await;
    assert!(removed.is_some());

    // Session count should be 0
    assert_eq!(state.auth_session_count().await, 0);

    // Challenge should no longer exist
    assert!(state.get_auth_challenge(42).await.is_none());
}

/// Test multiple auth sessions don't leak
#[tokio::test]
async fn test_multiple_auth_sessions_no_leak() {
    let state = ConnectionState::new();

    // Create multiple sessions
    for i in 0..10 {
        state.create_auth_session(i, format!("server-{}", i), None).await;
    }

    assert_eq!(state.auth_session_count().await, 10);

    // Remove all
    for i in 0..10 {
        state.remove_auth_session(i).await;
    }

    assert_eq!(state.auth_session_count().await, 0);
}

// ============================================================================
// Mock implementations for testing
// ============================================================================

struct MockWasmProvider;

#[async_trait::async_trait]
impl WasmProvider for MockWasmProvider {
    async fn load_translator(&self, _name: String, _mount_point: &Path, _bytecode: Vec<u8>) -> anyhow::Result<()> {
        Ok(())
    }

    async fn remove_translator(&self, _mount_point: &Path) -> anyhow::Result<()> {
        Ok(())
    }

    async fn get_translator(&self, _path: &Path) -> Option<Arc<dyn Translator>> {
        None
    }

    async fn set_translator(&self, _path: &str, _name: &str, _args: Vec<String>) -> anyhow::Result<()> {
        Ok(())
    }

    async fn list_translators(&self) -> anyhow::Result<Vec<WasmMetadata>> {
        Ok(vec![])
    }
}

struct MockStorageProvider;

#[async_trait::async_trait]
impl StorageProvider for MockStorageProvider {
    async fn read(&self, _path: &Path, _offset: u64, _size: u32) -> anyhow::Result<Vec<u8>> {
        Ok(vec![])
    }

    async fn write(&self, _path: &Path, _offset: u64, _data: &[u8]) -> anyhow::Result<u32> {
        Ok(0)
    }

    async fn stat(&self, _path: &Path) -> anyhow::Result<FileAttr> {
        anyhow::bail!("not found")
    }

    async fn read_dir(&self, _path: &Path) -> anyhow::Result<Vec<DirEntry>> {
        Ok(vec![])
    }

    async fn create_dir(&self, _path: &Path, _mode: u32) -> anyhow::Result<()> {
        Ok(())
    }

    async fn create_file(&self, _path: &Path, _mode: u32) -> anyhow::Result<()> {
        Ok(())
    }

    async fn remove_file(&self, _path: &Path) -> anyhow::Result<()> {
        Ok(())
    }

    async fn remove_dir(&self, _path: &Path) -> anyhow::Result<()> {
        Ok(())
    }

    async fn rename(&self, _from: &Path, _to: &Path) -> anyhow::Result<()> {
        Ok(())
    }

    async fn truncate(&self, _path: &Path, _size: u64) -> anyhow::Result<()> {
        Ok(())
    }

    async fn set_permissions(&self, _path: &Path, _mode: u32) -> anyhow::Result<()> {
        Ok(())
    }
}
