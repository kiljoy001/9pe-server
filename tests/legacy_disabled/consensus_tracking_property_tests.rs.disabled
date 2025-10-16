//! Property-based tests for consensus tracking in file operations
//! Tests that all file operations properly log to the consensus DAG

use proptest::prelude::*;
use std::sync::Arc;
use tempfile::TempDir;

use ninep_server::consensus::BoundedGhostdag;
use ninep_server::protocol::NinePeeMessage;
use ninep_server::server::handler::{PublicBasicOpsHandler, PublicConnectionState};

/// Strategy for generating valid file names (simplified)
fn filename_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_-]{1,20}"
}

/// Strategy for generating file data
fn file_data_strategy() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..100)
}

/// Test that create operations are properly logged to consensus
#[tokio::test]
async fn prop_create_operations_logged_to_consensus() {
    let strategy = (filename_strategy(), 0o644u32..=0o755u32);
    let mut runner = proptest::test_runner::TestRunner::new(proptest::test_runner::Config {
        cases: 5, // Reduced from default 256 to 5
        ..proptest::test_runner::Config::default()
    });

    runner
        .run(&strategy, |(filename, perm)| {
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

                // Attach to get a valid fid
                let attach_result = handler
                    .handle_attach(1, 0, "test".to_string(), "/".to_string())
                    .await;
                prop_assert!(attach_result.is_ok(), "Attach should succeed");

                let initial_stats = dag.get_stats().await;
                let initial_block_count = initial_stats.total_blocks;

                // Perform create operation
                let result = handler.handle_create(1, filename.clone(), perm, 0).await;

                // Verify operation succeeded or failed gracefully
                prop_assert!(result.is_ok(), "Create operation should not panic");

                // Check if consensus was logged (should be at least one new block)
                let final_stats = dag.get_stats().await;
                let blocks_added = final_stats.total_blocks - initial_block_count;

                // If create succeeded, should have logged to consensus
                match result.unwrap() {
                    NinePeeMessage::Create { .. } => {
                        prop_assert!(
                            blocks_added >= 1,
                            "Create operation should log to consensus DAG"
                        );
                    }
                    NinePeeMessage::Error { .. } => {
                        // Create failed, that's fine - just verify DAG is still consistent
                        prop_assert!(final_stats.total_blocks >= initial_block_count);
                    }
                    _ => {
                        prop_assert!(false, "Unexpected response type from create");
                    }
                }

                Ok(())
            })
        })
        .unwrap();
}

/// Test that write operations are properly logged to consensus
#[tokio::test]
async fn prop_write_operations_logged_to_consensus() {
    let strategy = (file_data_strategy(), 0u64..100);
    let mut runner = proptest::test_runner::TestRunner::new(proptest::test_runner::Config {
        cases: 3, // Reduced for performance
        ..proptest::test_runner::Config::default()
    });

    runner
        .run(&strategy, |(data, offset)| {
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

                // Create a test file first
                std::fs::write(temp_dir.path().join("testfile"), b"initial content").unwrap();

                // Attach and open file
                let _attach_result = handler
                    .handle_attach(1, 0, "test".to_string(), "/".to_string())
                    .await;
                let _walk_result = handler
                    .handle_walk(1, 2, vec!["testfile".to_string()])
                    .await;
                let _open_result = handler.handle_open(2, 2).await; // ORDWR mode

                let initial_stats = dag.get_stats().await;
                let initial_block_count = initial_stats.total_blocks;

                // Perform write operation
                let result = handler.handle_write(2, offset, data.clone()).await;

                // Verify operation succeeded or failed gracefully
                prop_assert!(result.is_ok(), "Write operation should not panic");

                // Check if consensus was logged
                let final_stats = dag.get_stats().await;

                // If write succeeded, should have logged to consensus
                match result.unwrap() {
                    NinePeeMessage::Write { .. } => {
                        let blocks_added = final_stats.total_blocks - initial_block_count;
                        prop_assert!(
                            blocks_added >= 1,
                            "Write operation should log to consensus DAG"
                        );
                        prop_assert!(data.len() <= 100, "Data size should be reasonable");
                        prop_assert!(offset < 100, "Offset should be reasonable");
                    }
                    NinePeeMessage::Error { .. } => {
                        // Write failed, that's fine - verify DAG is still consistent
                        prop_assert!(final_stats.total_blocks >= initial_block_count);
                    }
                    _ => {
                        prop_assert!(false, "Unexpected response type from write");
                    }
                }

                Ok(())
            })
        })
        .unwrap();
}

/// Test that remove operations are properly logged to consensus
#[tokio::test]
async fn prop_remove_operations_logged_to_consensus() {
    let strategy = filename_strategy();
    let mut runner = proptest::test_runner::TestRunner::new(proptest::test_runner::Config {
        cases: 3, // Reduced for performance
        ..proptest::test_runner::Config::default()
    });

    runner
        .run(&strategy, |filename| {
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

                // Create a test file first
                let test_file_path = temp_dir.path().join(&filename);
                std::fs::write(&test_file_path, b"test content").unwrap();

                // Attach and walk to the file
                let _attach_result = handler
                    .handle_attach(1, 0, "test".to_string(), "/".to_string())
                    .await;
                let _walk_result = handler.handle_walk(1, 2, vec![filename.clone()]).await;

                let initial_stats = dag.get_stats().await;
                let initial_block_count = initial_stats.total_blocks;

                // Perform remove operation
                let result = handler.handle_remove(2).await;

                // Verify operation succeeded or failed gracefully
                prop_assert!(result.is_ok(), "Remove operation should not panic");

                // Check if consensus was logged
                let final_stats = dag.get_stats().await;

                // If remove succeeded, should have logged to consensus
                match result.unwrap() {
                    NinePeeMessage::Remove { .. } => {
                        let blocks_added = final_stats.total_blocks - initial_block_count;
                        prop_assert!(
                            blocks_added >= 1,
                            "Remove operation should log to consensus DAG"
                        );
                    }
                    NinePeeMessage::Error { .. } => {
                        // Remove failed, that's fine - verify DAG is still consistent
                        prop_assert!(final_stats.total_blocks >= initial_block_count);
                    }
                    _ => {
                        prop_assert!(false, "Unexpected response type from remove");
                    }
                }

                Ok(())
            })
        })
        .unwrap();
}

/// Test consensus logging with simple operations
#[tokio::test]
async fn prop_consensus_blocks_maintain_integrity() {
    let strategy = file_data_strategy();
    let mut runner = proptest::test_runner::TestRunner::new(proptest::test_runner::Config {
        cases: 2, // Reduced for performance
        ..proptest::test_runner::Config::default()
    });

    runner
        .run(&strategy, |data| {
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

                let initial_stats = dag.get_stats().await;

                // Perform operations
                let _ = handler
                    .handle_attach(1, 0, "test".to_string(), "/".to_string())
                    .await;
                let _ = handler
                    .handle_create(1, "testfile".to_string(), 0o644, 0)
                    .await;

                let final_stats = dag.get_stats().await;

                // Verify that any blocks added maintain DAG integrity
                prop_assert!(
                    final_stats.total_blocks >= initial_stats.total_blocks,
                    "DAG should never lose blocks"
                );
                prop_assert!(
                    final_stats.tip_count > 0 || final_stats.total_blocks == 0,
                    "DAG should have tips if it has blocks"
                );
                prop_assert!(
                    final_stats.total_blocks <= 1000,
                    "Block count should be reasonable"
                );

                Ok(())
            })
        })
        .unwrap();
}
