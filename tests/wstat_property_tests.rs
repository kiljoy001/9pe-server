//! Property-based tests for wstat file attribute changes
//! Tests that wstat operations properly modify file attributes and maintain consistency

use proptest::prelude::*;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::path::PathBuf;
use tempfile::TempDir;
use std::os::unix::fs::PermissionsExt;

use ninep_server::consensus::{BoundedGhostdag, NamespaceOp, Block, BlockState};
use ninep_server::server::handler::{PublicBasicOpsHandler, PublicConnectionState};
use ninep_server::protocol::NinePeeMessage;

/// Strategy for generating valid file permissions
fn file_permissions_strategy() -> impl Strategy<Value = u32> {
    prop::oneof![
        Just(0o644), // Regular file
        Just(0o755), // Executable
        Just(0o600), // Owner only
        Just(0o666), // World writable
        Just(0o777), // All permissions
        0o000u32..=0o777u32, // Any valid permission
    ]
}

/// Strategy for generating file names
fn filename_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec("[a-zA-Z0-9_-]", 1..20)
        .prop_map(|chars| chars.join(""))
}

/// Strategy for generating file sizes for truncation
fn file_size_strategy() -> impl Strategy<Value = u64> {
    prop::oneof![
        Just(0u64),           // Empty file
        1u64..100u64,         // Small files
        100u64..1024u64,      // Medium files
        1024u64..10240u64,    // Large files
    ]
}

/// Test that wstat permission changes are applied correctly
#[tokio::test]
async fn prop_wstat_permission_changes() {
    let strategy = (filename_strategy(), file_permissions_strategy(), file_permissions_strategy());
    let mut runner = proptest::test_runner::TestRunner::default();

    runner.run(&strategy, |(filename, initial_perm, new_perm)| {
        futures::executor::block_on(async {
            // Setup
            let temp_dir = TempDir::new().unwrap();
            let dag = Arc::new(BoundedGhostdag::new("test_consensus".to_string()));
            let connection_state = PublicConnectionState::new();

            let mut handler = PublicBasicOpsHandler::new(
                temp_dir.path().to_path_buf(),
                connection_state.clone(),
            );
            handler.set_consensus_dag(dag.clone());

            // Create initial file with initial permissions
            let file_path = temp_dir.path().join(&filename);
            std::fs::write(&file_path, b"test content").unwrap();
            std::fs::set_permissions(&file_path, std::fs::Permissions::from_mode(initial_perm)).unwrap();

            // Attach and walk to the file
            let attach_result = handler.handle_attach(1, 0, "test".to_string(), "/".to_string()).await;
            prop_assert!(matches!(attach_result, Ok(NinePeeMessage::Attach { .. })));

            let walk_result = handler.handle_walk(1, 2, vec![filename.clone()]).await;
            prop_assert!(walk_result.is_ok());

            // Create stat data with new permissions
            let new_stat = Stat {
                size: 0,  // Will be calculated
                typ: 0,
                dev: 0,
                qid: Qid { qtype: 0, version: 0, path: 0 },
                mode: new_perm,
                atime: 0,
                mtime: 0,
                length: 0,
                name: filename.clone(),
                uid: "test".to_string(),
                gid: "test".to_string(),
                muid: "test".to_string(),
            };

            let stat_data = bincode::serialize(&new_stat).unwrap();

            // Perform wstat operation
            let result = handler.handle_wstat(2, stat_data).await;
            prop_assert!(result.is_ok());

            // Verify permissions were changed
            let metadata = std::fs::metadata(&file_path).unwrap();
            let actual_perm = metadata.permissions().mode() & 0o777;
            prop_assert_eq!(actual_perm, new_perm,
                "File permissions should be updated to new value");

            // Verify file content unchanged
            let content = std::fs::read(&file_path).unwrap();
            prop_assert_eq!(content, b"test content",
                "File content should remain unchanged after permission change");

            Ok(())
        })
    }).unwrap();
}

/// Test that wstat file truncation works correctly
#[tokio::test]
async fn prop_wstat_file_truncation() {
    let strategy = (filename_strategy(), file_size_strategy(), file_size_strategy());
    let mut runner = proptest::test_runner::TestRunner::default();

    runner.run(&strategy, |(filename, initial_size, new_size)| {
        futures::executor::block_on(async {
            // Setup
            let temp_dir = TempDir::new().unwrap();
            let dag = Arc::new(BoundedGhostdag::new("test_consensus".to_string()));
            let connection_state = PublicConnectionState::new();

            let mut handler = PublicBasicOpsHandler::new(
                temp_dir.path().to_path_buf(),
                connection_state.clone(),
            );
            handler.set_consensus_dag(dag.clone());

            // Create initial file with specific size
            let file_path = temp_dir.path().join(&filename);
            let initial_content = vec![b'x'; initial_size as usize];
            std::fs::write(&file_path, &initial_content).unwrap();

            // Attach and walk to the file
            let attach_result = handler.handle_attach(1, 0, "test".to_string(), "/".to_string()).await;
            prop_assert!(matches!(attach_result, Ok(NinePeeMessage::Attach { .. })));

            let walk_result = handler.handle_walk(1, 2, vec![filename.clone()]).await;
            prop_assert!(walk_result.is_ok());

            // Create stat data with new length for truncation
            let new_stat = Stat {
                size: 0,
                typ: 0,
                dev: 0,
                qid: Qid { qtype: 0, version: 0, path: 0 },
                mode: 0o644,
                atime: 0,
                mtime: 0,
                length: new_size,
                name: filename.clone(),
                uid: "test".to_string(),
                gid: "test".to_string(),
                muid: "test".to_string(),
            };

            let stat_data = bincode::serialize(&new_stat).unwrap();

            // Perform wstat operation
            let result = handler.handle_wstat(2, stat_data).await;
            prop_assert!(result.is_ok());

            // Verify file was truncated/extended to correct size
            let final_content = std::fs::read(&file_path).unwrap();
            prop_assert_eq!(final_content.len() as u64, new_size,
                "File should be truncated/extended to new size");

            // If truncated, verify remaining content is correct
            if new_size <= initial_size {
                let expected_content = &initial_content[..new_size as usize];
                prop_assert_eq!(&final_content, expected_content,
                    "Truncated content should match original prefix");
            } else {
                // If extended, verify original content is preserved
                prop_assert_eq!(&final_content[..initial_size as usize], &initial_content,
                    "Original content should be preserved when extending");
                // Extended part should be zeros
                for &byte in &final_content[initial_size as usize..] {
                    prop_assert_eq!(byte, 0, "Extended content should be zero-filled");
                }
            }

            Ok(())
        })
    }).unwrap();
}

/// Test that wstat file renaming works correctly and logs to consensus
#[tokio::test]
async fn prop_wstat_file_rename() {
    let strategy = (filename_strategy(), filename_strategy());
    let mut runner = proptest::test_runner::TestRunner::default();

    runner.run(&strategy, |(old_name, new_name)| {
        futures::executor::block_on(async {
            // Skip if names are identical
            if old_name == new_name {
                return Ok(());
            }

            // Setup
            let temp_dir = TempDir::new().unwrap();
            let dag = Arc::new(BoundedGhostdag::new("test_consensus".to_string()));
            let connection_state = PublicConnectionState::new();

            let mut handler = PublicBasicOpsHandler::new(
                temp_dir.path().to_path_buf(),
                connection_state.clone(),
            );
            handler.set_consensus_dag(dag.clone());

            // Create initial file
            let old_path = temp_dir.path().join(&old_name);
            let new_path = temp_dir.path().join(&new_name);
            std::fs::write(&old_path, b"test content").unwrap();

            // Attach and walk to the file
            let attach_result = handler.handle_attach(1, 0, "test".to_string(), "/".to_string()).await;
            prop_assert!(matches!(attach_result, Ok(NinePeeMessage::Attach { .. })));

            let walk_result = handler.handle_walk(1, 2, vec![old_name.clone()]).await;
            prop_assert!(walk_result.is_ok());

            let initial_stats = dag.get_stats().await;
            let initial_block_count = initial_stats.total_blocks;

            // Create stat data with new name
            let new_stat = Stat {
                size: 0,
                typ: 0,
                dev: 0,
                qid: Qid { qtype: 0, version: 0, path: 0 },
                mode: 0o644,
                atime: 0,
                mtime: 0,
                length: 0,
                name: new_name.clone(),
                uid: "test".to_string(),
                gid: "test".to_string(),
                muid: "test".to_string(),
            };

            let stat_data = bincode::serialize(&new_stat).unwrap();

            // Perform wstat operation
            let result = handler.handle_wstat(2, stat_data).await;
            prop_assert!(result.is_ok());

            // Verify file was renamed
            prop_assert!(!old_path.exists(), "Old file should not exist after rename");
            prop_assert!(new_path.exists(), "New file should exist after rename");

            // Verify content preserved
            let content = std::fs::read(&new_path).unwrap();
            prop_assert_eq!(content, b"test content",
                "File content should be preserved during rename");

            // Verify rename was logged to consensus
            let final_stats = dag.get_stats().await;
            let blocks_added = final_stats.total_blocks - initial_block_count;
            prop_assert!(blocks_added >= 1, "Rename operation should log to consensus DAG");

            Ok(())
        })
    }).unwrap();
}

/// Test that wstat operations with invalid data fail gracefully
#[tokio::test]
async fn prop_wstat_invalid_data_handling() {
    let strategy = prop::collection::vec(any::<u8>(), 0..100);
    let mut runner = proptest::test_runner::TestRunner::default();

    runner.run(&strategy, |invalid_data| {
        futures::executor::block_on(async {
            // Setup
            let temp_dir = TempDir::new().unwrap();
            let dag = Arc::new(BoundedGhostdag::new("test_consensus".to_string()));
            let connection_state = PublicConnectionState::new();

            let mut handler = PublicBasicOpsHandler::new(
                temp_dir.path().to_path_buf(),
                connection_state.clone(),
            );
            handler.set_consensus_dag(dag.clone());

            // Create test file
            let file_path = temp_dir.path().join("testfile");
            std::fs::write(&file_path, b"test content").unwrap();

            // Attach and walk to the file
            let attach_result = handler.handle_attach(1, 0, "test".to_string(), "/".to_string()).await;
            prop_assert!(matches!(attach_result, Ok(NinePeeMessage::Attach { .. })));

            let walk_result = handler.handle_walk(1, 2, vec!["testfile".to_string()]).await;
            prop_assert!(walk_result.is_ok());

            // Try wstat with invalid data
            let result = handler.handle_wstat(2, invalid_data).await;

            // Should either succeed (if data happens to be valid) or return error
            if let Ok(msg) = result {
                // If successful, file should still exist and be readable
                prop_assert!(file_path.exists(), "File should still exist after wstat");
                let _ = std::fs::read(&file_path).unwrap();
            } else {
                // If failed, file should be unchanged
                prop_assert!(file_path.exists(), "File should still exist after failed wstat");
                let content = std::fs::read(&file_path).unwrap();
                prop_assert_eq!(content, b"test content",
                    "File content should be unchanged after failed wstat");
            }

            Ok(())
        })
    }).unwrap();
}

/// Test that wstat operations maintain consensus integrity under concurrent access
#[tokio::test]
async fn prop_wstat_concurrent_operations_consensus_integrity() {
    let strategy = prop::collection::vec(
        (filename_strategy(), file_permissions_strategy()),
        2..5
    );
    let mut runner = proptest::test_runner::TestRunner::default();

    runner.run(&strategy, |operations| {
        futures::executor::block_on(async {
            // Setup
            let temp_dir = TempDir::new().unwrap();
            let dag = Arc::new(BoundedGhostdag::new("test_consensus".to_string()));
            let connection_state = PublicConnectionState::new();

            let mut handler = PublicBasicOpsHandler::new(
                temp_dir.path().to_path_buf(),
                connection_state.clone(),
            );
            handler.set_consensus_dag(dag.clone());

            // Create test files
            for (i, (filename, _)) in operations.iter().enumerate() {
                let file_path = temp_dir.path().join(filename);
                std::fs::write(&file_path, format!("content_{}", i)).unwrap();
            }

            let initial_stats = dag.get_stats().await;
            let initial_block_count = initial_stats.total_blocks;

            // Perform concurrent wstat operations
            let mut handles = vec![];
            for (i, (filename, new_perm)) in operations.iter().enumerate() {
                let handler = Arc::new(handler.clone());
                let filename = filename.clone();
                let new_perm = *new_perm;
                let fid = (i + 1) as u32;

                let handle = tokio::spawn(async move {
                    // Attach and walk to file
                    let _ = handler.handle_attach(fid, 0, "test".to_string(), "/".to_string()).await;
                    let _ = handler.handle_walk(fid, fid + 100, vec![filename.clone()]).await;

                    // Create stat data for permission change
                    let new_stat = Stat {
                        size: 0,
                        typ: 0,
                        dev: 0,
                        qid: Qid { qtype: 0, version: 0, path: 0 },
                        mode: new_perm,
                        atime: 0,
                        mtime: 0,
                        length: 0,
                        name: filename,
                        uid: "test".to_string(),
                        gid: "test".to_string(),
                        muid: "test".to_string(),
                    };

                    let stat_data = bincode::serialize(&new_stat).unwrap();
                    let _ = handler.handle_wstat(fid + 100, stat_data).await;
                });
                handles.push(handle);
            }

            // Wait for all operations to complete
            for handle in handles {
                let _ = handle.await;
            }

            // Check that consensus maintained integrity
            let final_stats = dag.get_stats().await;
            prop_assert!(final_stats.total_blocks >= initial_block_count,
                        "Consensus DAG should never lose blocks");
            prop_assert!(final_stats.tip_count > 0 || final_stats.total_blocks == 0,
                        "DAG should have tips if it has blocks");

            Ok(())
        })
    }).unwrap();
}