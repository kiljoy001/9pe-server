//! Integration tests for timing-dependent consensus behavior
//! These tests verify cooldown periods, scaling decisions, and temporal ordering

use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout};

use ninep_server::consensus::{
    dynamic_scaling::{DynamicScaler, ScaleDecision, ScalingParams},
    BoundedGhostdag, NamespaceOp,
};

/// Test that cooldown periods prevent rapid scaling
#[tokio::test]
async fn test_cooldown_prevents_rapid_scaling() {
    let params = ScalingParams {
        min_size: 100,
        max_size: 1000,
        initial_size: 500,
        scale_factor: 1.5,
        scale_up_threshold: 0.8,
        scale_down_threshold: 0.2,
        history_window: 50,
        cooldown_secs: 2, // Short cooldown for testing
    };

    let scaler = Arc::new(DynamicScaler::new(params.clone()));

    // Record enough high load metrics to trigger scaling (need >10 for algorithm)
    // Using higher fill rate and fork depth to exceed the 0.8 threshold
    for _ in 0..25 {
        scaler.record_metrics(100.0, 0.95, 20).await;
    }

    // First scale decision should work when we have enough data
    // Let's calculate: 0.5 * 0.95 + 0.3 * 1.0 + 0.2 * 0.2 = 0.475 + 0.3 + 0.04 = 0.815 > 0.8
    let decision1 = scaler.calculate_scale_decision(&params).await;
    assert_eq!(
        decision1,
        ScaleDecision::ScaleUp,
        "High load should trigger scale up"
    );

    let size1 = scaler.apply_scale(decision1, &params).await;
    assert!(
        size1 > params.initial_size,
        "Size should increase after scaling"
    );

    // Continue adding high load metrics
    for _ in 0..10 {
        scaler.record_metrics(100.0, 0.95, 20).await;
    }

    // Immediate second decision should be blocked by cooldown (apply_scale sets the cooldown timer)
    let decision2 = scaler.calculate_scale_decision(&params).await;
    assert_eq!(
        decision2,
        ScaleDecision::Hold,
        "Cooldown should prevent immediate scaling"
    );

    // After cooldown expires, scaling should work again
    sleep(Duration::from_secs(params.cooldown_secs + 1)).await;

    let decision3 = scaler.calculate_scale_decision(&params).await;
    // Should be able to scale again (either up or hold, but not blocked)
    assert_ne!(
        decision3,
        ScaleDecision::Hold,
        "After cooldown, scaling should be unblocked"
    );
}

/// Test scaling behavior under sustained high load
#[tokio::test]
async fn test_sustained_high_load_scaling() {
    let params = ScalingParams {
        min_size: 100,
        max_size: 1000,
        initial_size: 200,
        scale_factor: 1.5,
        scale_up_threshold: 0.8,
        scale_down_threshold: 0.2,
        history_window: 30,
        cooldown_secs: 1,
    };

    let scaler = Arc::new(DynamicScaler::new(params.clone()));
    let initial_size = scaler.get_current_size().await;

    // Simulate sustained high load that exceeds 0.8 threshold
    // Calculate: 0.5 * 0.95 + 0.3 * 0.8 + 0.2 * 0.15 = 0.475 + 0.24 + 0.03 = 0.745 (too low)
    // Let's use: 0.5 * 0.98 + 0.3 * 1.0 + 0.2 * 0.5 = 0.49 + 0.3 + 0.1 = 0.89 > 0.8
    for _ in 0..25 {
        scaler.record_metrics(100.0, 0.98, 50).await;
    }

    // Should trigger scale up
    sleep(Duration::from_millis(100)).await; // Brief pause for metrics to settle
    let decision = scaler.calculate_scale_decision(&params).await;
    assert_eq!(
        decision,
        ScaleDecision::ScaleUp,
        "High sustained load should trigger scale up"
    );

    let new_size = scaler.apply_scale(decision, &params).await;
    assert!(
        new_size > initial_size,
        "Size should increase after scaling up"
    );
    assert!(
        new_size <= params.max_size,
        "Size should not exceed maximum"
    );
}

/// Test scaling behavior under sustained low load
#[tokio::test]
async fn test_sustained_low_load_scaling() {
    let params = ScalingParams {
        min_size: 100,
        max_size: 1000,
        initial_size: 800, // Start high
        scale_factor: 1.5,
        scale_up_threshold: 0.8,
        scale_down_threshold: 0.15,
        history_window: 30,
        cooldown_secs: 1,
    };

    let scaler = Arc::new(DynamicScaler::new(params.clone()));
    let initial_size = scaler.get_current_size().await;

    // Simulate sustained low load that falls below 0.15 threshold
    // Calculate: 0.5 * 0.05 + 0.3 * 0.05 + 0.2 * 0.0 = 0.025 + 0.015 + 0.0 = 0.04 < 0.15
    for _ in 0..25 {
        scaler.record_metrics(5.0, 0.05, 0).await;
    }

    sleep(Duration::from_millis(100)).await;
    let decision = scaler.calculate_scale_decision(&params).await;
    assert_eq!(
        decision,
        ScaleDecision::ScaleDown,
        "Low sustained load should trigger scale down"
    );

    let new_size = scaler.apply_scale(decision, &params).await;
    assert!(
        new_size < initial_size,
        "Size should decrease after scaling down"
    );
    assert!(
        new_size >= params.min_size,
        "Size should not go below minimum"
    );
}

/// Test that scaling respects absolute bounds
#[tokio::test]
async fn test_scaling_bounds_enforcement() {
    let params = ScalingParams {
        min_size: 50,
        max_size: 200,
        initial_size: 180, // Close to max
        scale_factor: 2.0, // Aggressive scaling
        scale_up_threshold: 0.5,
        scale_down_threshold: 0.3,
        history_window: 20,
        cooldown_secs: 1,
    };

    let scaler = Arc::new(DynamicScaler::new(params.clone()));

    // Try to scale beyond maximum
    for _ in 0..15 {
        scaler.record_metrics(100.0, 1.0, 20).await;
    }

    let size_before = scaler.get_current_size().await;
    let decision = scaler.calculate_scale_decision(&params).await;

    if decision == ScaleDecision::ScaleUp {
        let size_after = scaler.apply_scale(decision, &params).await;
        assert!(
            size_after <= params.max_size,
            "Scaling should not exceed max_size"
        );
        assert!(
            size_after >= size_before,
            "Scale up should increase or maintain size"
        );
    }

    // Now test minimum bound by starting fresh at minimum
    let low_params = ScalingParams {
        min_size: 50,
        max_size: 200,
        initial_size: 60, // Close to min
        scale_factor: 2.0,
        scale_up_threshold: 0.7,
        scale_down_threshold: 0.8, // High threshold to force scale down
        history_window: 20,
        cooldown_secs: 1,
    };

    let low_scaler = Arc::new(DynamicScaler::new(low_params.clone()));

    // Force scale down
    for _ in 0..15 {
        low_scaler.record_metrics(1.0, 0.01, 0).await;
    }

    sleep(Duration::from_secs(2)).await; // Wait past cooldown

    let size_before = low_scaler.get_current_size().await;
    let decision = low_scaler.calculate_scale_decision(&low_params).await;

    if decision == ScaleDecision::ScaleDown {
        let size_after = low_scaler.apply_scale(decision, &low_params).await;
        assert!(
            size_after >= low_params.min_size,
            "Scaling should not go below min_size"
        );
        assert!(
            size_after <= size_before,
            "Scale down should decrease or maintain size"
        );
    }
}

/// Test temporal ordering in block processing
#[tokio::test]
async fn test_block_temporal_ordering() {
    let dag = BoundedGhostdag::new("test_node".to_string());

    // Create blocks with different timestamps
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let block1 = ninep_server::consensus::Block {
        id: "block_1".to_string(),
        parents: vec![],
        operations: vec![NamespaceOp::Create {
            path: "/file1".to_string(),
            mode: 644,
            is_dir: false,
        }],
        timestamp: now,
        creator: "node1".to_string(),
        signature: vec![],
        state: ninep_server::consensus::BlockState::Pending,
        ghost_weight: 1,
        height: 0,
    };

    let block2 = ninep_server::consensus::Block {
        id: "block_2".to_string(),
        parents: vec!["block_1".to_string()],
        operations: vec![NamespaceOp::Write {
            path: "/file1".to_string(),
            offset: 0,
            hash: [1u8; 32],
        }],
        timestamp: now + 10, // Later timestamp
        creator: "node2".to_string(),
        signature: vec![],
        state: ninep_server::consensus::BlockState::Pending,
        ghost_weight: 1,
        height: 1,
    };

    // Add blocks in order
    dag.add_block(block1)
        .await
        .expect("Block 1 should be added");
    dag.add_block(block2)
        .await
        .expect("Block 2 should be added");

    let stats = dag.get_stats().await;
    assert_eq!(stats.total_blocks, 2, "Both blocks should be added");
    assert!(stats.tip_count >= 1, "Should have at least one tip");
}

/// Test concurrent block processing with timing
#[tokio::test]
async fn test_concurrent_block_processing_timing() {
    let dag = Arc::new(BoundedGhostdag::new("test_node".to_string()));

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Create multiple blocks concurrently
    let mut handles = vec![];

    for i in 0..10 {
        let dag_clone = dag.clone();
        let block = ninep_server::consensus::Block {
            id: format!("concurrent_block_{}", i),
            parents: if i == 0 {
                vec![]
            } else {
                vec![format!("concurrent_block_{}", i - 1)]
            },
            operations: vec![NamespaceOp::Create {
                path: format!("/concurrent_file_{}", i),
                mode: 644,
                is_dir: false,
            }],
            timestamp: now + i as u64,
            creator: format!("node_{}", i % 3),
            signature: vec![],
            state: ninep_server::consensus::BlockState::Pending,
            ghost_weight: 1,
            height: i as u64,
        };

        handles.push(tokio::spawn(async move {
            // Add small delay to ensure concurrent processing
            sleep(Duration::from_millis(i * 10)).await;
            dag_clone.add_block(block).await
        }));
    }

    // Wait for all blocks to be processed with timeout
    let results = timeout(Duration::from_secs(5), async {
        let mut results = vec![];
        for handle in handles {
            results.push(handle.await.unwrap());
        }
        results
    })
    .await
    .expect("Block processing should complete within timeout");

    // Verify results
    let successful_adds = results.iter().filter(|r| r.is_ok()).count();
    assert!(
        successful_adds > 0,
        "At least some blocks should be added successfully"
    );

    let stats = dag.get_stats().await;
    assert!(stats.total_blocks > 0, "Some blocks should be in the DAG");
}

/// Test scaling prediction with time horizon
#[tokio::test]
async fn test_scaling_prediction_temporal() {
    let params = ScalingParams::default();
    let scaler = Arc::new(DynamicScaler::new(params));

    // Record increasing throughput over time
    for i in 0..20 {
        scaler.record_metrics(i as f64 * 5.0, 0.5, 5).await;
        sleep(Duration::from_millis(50)).await; // Small delay to simulate time progression
    }

    // Test predictions for different time horizons
    let prediction_short = scaler.predict_size_needed(60).await; // 1 minute
    let prediction_long = scaler.predict_size_needed(300).await; // 5 minutes
    let current_size = scaler.get_current_size().await;

    assert!(
        prediction_short >= current_size,
        "Short-term prediction should be at least current size"
    );
    assert!(
        prediction_long >= prediction_short,
        "Longer horizon should predict larger size with increasing trend"
    );
}

/// Test that operations respect temporal causality
#[tokio::test]
async fn test_operation_temporal_causality() {
    let dag = Arc::new(BoundedGhostdag::new("causality_test".to_string()));

    let base_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Create file first
    let create_block = ninep_server::consensus::Block {
        id: "create_block".to_string(),
        parents: vec![],
        operations: vec![NamespaceOp::Create {
            path: "/causality_test".to_string(),
            mode: 644,
            is_dir: false,
        }],
        timestamp: base_time,
        creator: "creator".to_string(),
        signature: vec![],
        state: ninep_server::consensus::BlockState::Pending,
        ghost_weight: 1,
        height: 0,
    };

    // Write to file after creation (causal dependency)
    let write_block = ninep_server::consensus::Block {
        id: "write_block".to_string(),
        parents: vec!["create_block".to_string()],
        operations: vec![NamespaceOp::Write {
            path: "/causality_test".to_string(),
            offset: 0,
            hash: [42u8; 32],
        }],
        timestamp: base_time + 5, // Must be after create
        creator: "writer".to_string(),
        signature: vec![],
        state: ninep_server::consensus::BlockState::Pending,
        ghost_weight: 1,
        height: 1,
    };

    // Delete file last (causal dependency on both)
    let delete_block = ninep_server::consensus::Block {
        id: "delete_block".to_string(),
        parents: vec!["write_block".to_string()],
        operations: vec![NamespaceOp::Delete {
            path: "/causality_test".to_string(),
        }],
        timestamp: base_time + 10, // Must be after write
        creator: "deleter".to_string(),
        signature: vec![],
        state: ninep_server::consensus::BlockState::Pending,
        ghost_weight: 1,
        height: 2,
    };

    // Add blocks in correct temporal/causal order
    dag.add_block(create_block)
        .await
        .expect("Create should succeed");
    dag.add_block(write_block)
        .await
        .expect("Write should succeed after create");
    dag.add_block(delete_block)
        .await
        .expect("Delete should succeed after write");

    let stats = dag.get_stats().await;
    assert_eq!(
        stats.total_blocks, 3,
        "All causally-ordered blocks should be accepted"
    );
}
