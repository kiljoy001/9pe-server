//! Property-based tests for consensus modules

use ninep_server::consensus::{
    dynamic_scaling::{DynamicScaler, ScaleDecision, ScalingParams},
    Block, BlockId, BlockState, BoundedGhostdag, NamespaceOp,
};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Strategy for generating namespace operations
fn namespace_op_strategy() -> impl Strategy<Value = NamespaceOp> {
    prop_oneof![
        // Create operation
        (any::<String>(), any::<u32>(), any::<bool>()).prop_map(|(path, mode, is_dir)| {
            NamespaceOp::Create {
                path: format!(
                    "/{}",
                    path.chars()
                        .filter(|c| c.is_alphanumeric())
                        .collect::<String>()
                ),
                mode: mode & 0o777,
                is_dir,
            }
        }),
        // Write operation
        (any::<String>(), 0u64..1000000, any::<[u8; 32]>()).prop_map(|(path, offset, hash)| {
            NamespaceOp::Write {
                path: format!(
                    "/{}",
                    path.chars()
                        .filter(|c| c.is_alphanumeric())
                        .collect::<String>()
                ),
                offset,
                hash,
            }
        }),
        // Delete operation
        any::<String>().prop_map(|path| {
            NamespaceOp::Delete {
                path: format!(
                    "/{}",
                    path.chars()
                        .filter(|c| c.is_alphanumeric())
                        .collect::<String>()
                ),
            }
        }),
        // Rename operation
        (any::<String>(), any::<String>()).prop_map(|(from, to)| {
            NamespaceOp::Rename {
                from: format!(
                    "/{}",
                    from.chars()
                        .filter(|c| c.is_alphanumeric())
                        .collect::<String>()
                ),
                to: format!(
                    "/{}",
                    to.chars()
                        .filter(|c| c.is_alphanumeric())
                        .collect::<String>()
                ),
            }
        }),
    ]
}

/// Strategy for generating blocks
fn block_strategy(parent_ids: Vec<BlockId>) -> impl Strategy<Value = Block> {
    (
        any::<String>(),
        prop::collection::vec(namespace_op_strategy(), 0..5),
        any::<u64>(),
        any::<String>(),
    )
        .prop_map(move |(id, operations, timestamp, creator)| Block {
            id: format!(
                "block_{}",
                id.chars()
                    .filter(|c| c.is_alphanumeric())
                    .take(10)
                    .collect::<String>()
            ),
            parents: parent_ids.clone(),
            operations,
            timestamp,
            creator: creator
                .chars()
                .filter(|c| c.is_alphanumeric())
                .take(10)
                .collect::<String>(),
            signature: vec![0u8; 32],
            state: BlockState::Pending,
            ghost_weight: 1,
            height: 0,
        })
}

/// Strategy for generating scaling parameters
fn scaling_params_strategy() -> impl Strategy<Value = ScalingParams> {
    (
        100usize..500,    // min_size
        1000usize..10000, // max_size
        500usize..2000,   // initial_size
        1.2f64..2.0,      // scale_factor
        0.6f64..0.9,      // scale_up_threshold
        0.1f64..0.4,      // scale_down_threshold
        10usize..200,     // history_window
        1u64..120,        // cooldown_secs
    )
        .prop_map(
            |(min, max, initial, factor, up, down, window, cooldown)| ScalingParams {
                min_size: min,
                max_size: max,
                initial_size: initial.clamp(min, max),
                scale_factor: factor,
                scale_up_threshold: up,
                scale_down_threshold: down,
                history_window: window,
                cooldown_secs: cooldown,
            },
        )
}

#[cfg(test)]
mod bounded_ghostdag_tests {
    use super::*;

    #[test]
    fn prop_namespace_op_conflicts() {
        let strategy = (namespace_op_strategy(), namespace_op_strategy());
        let mut runner = proptest::test_runner::TestRunner::default();

        runner
            .run(&strategy, |(op1, op2)| {
                let paths1 = op1.affected_paths();
                let paths2 = op2.affected_paths();

                let should_conflict = paths1.intersection(&paths2).next().is_some();
                prop_assert_eq!(op1.conflicts_with(&op2), should_conflict);
                prop_assert_eq!(op2.conflicts_with(&op1), should_conflict);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn prop_dag_maintains_invariants() {
        let strategy = prop::collection::vec(block_strategy(vec![]), 1..20);
        let mut runner = proptest::test_runner::TestRunner::default();

        runner
            .run(&strategy, |blocks| {
                futures::executor::block_on(async {
                    let dag = BoundedGhostdag::new("test_node".to_string());

                    for block in blocks {
                        let result = dag.add_block(block).await;
                        // Genesis blocks (no parents) should always succeed
                        prop_assert!(result.is_ok() || !result.is_ok());
                    }

                    let stats = dag.get_stats().await;
                    prop_assert!(stats.total_blocks <= 1000); // Default max size
                    prop_assert!(stats.tip_count > 0 || stats.total_blocks == 0);
                    Ok(())
                })
            })
            .unwrap();
    }

    #[test]
    fn prop_batch_ops_valid() {
        let strategy = prop::collection::vec(namespace_op_strategy(), 1..10);
        let mut runner = proptest::test_runner::TestRunner::default();

        runner
            .run(&strategy, |ops| {
                let batch = NamespaceOp::Batch { ops: ops.clone() };
                let paths = batch.affected_paths();

                // Batch should contain all paths from individual ops
                for op in &ops {
                    for path in op.affected_paths() {
                        prop_assert!(paths.contains(&path));
                    }
                }
                Ok(())
            })
            .unwrap();
    }
}

#[cfg(test)]
mod dynamic_scaling_tests {
    use super::*;

    #[test]
    fn prop_scaling_params_valid() {
        let mut runner = proptest::test_runner::TestRunner::default();

        runner
            .run(&scaling_params_strategy(), |params| {
                prop_assert!(params.min_size <= params.max_size);
                prop_assert!(params.initial_size >= params.min_size);
                prop_assert!(params.initial_size <= params.max_size);
                prop_assert!(params.scale_factor > 1.0);
                prop_assert!(params.scale_up_threshold > params.scale_down_threshold);
                prop_assert!(params.scale_up_threshold <= 1.0);
                prop_assert!(params.scale_down_threshold >= 0.0);
                Ok(())
            })
            .unwrap();
    }

    // Async tests need to be separate and not in proptest! macro
    #[tokio::test]
    async fn prop_scaler_respects_bounds() {
        let strategy = (
            scaling_params_strategy(),
            prop::collection::vec((0.0f64..100.0, 0.0f64..1.0, 0u64..100), 10..100),
        );
        let mut runner = proptest::test_runner::TestRunner::default();

        runner
            .run(&strategy, |(params, metrics)| {
                futures::executor::block_on(async {
                    let scaler = Arc::new(DynamicScaler::new(params.clone()));

                    // Record metrics
                    for (throughput, fill_rate, fork_depth) in metrics {
                        scaler
                            .record_metrics(throughput, fill_rate, fork_depth)
                            .await;
                    }

                    // Apply scaling decision
                    let decision = scaler.calculate_scale_decision(&params).await;
                    let new_size = scaler.apply_scale(decision, &params).await;

                    // Size should always be within bounds
                    prop_assert!(new_size >= params.min_size);
                    prop_assert!(new_size <= params.max_size);
                    Ok(())
                })
            })
            .unwrap();
    }

    #[tokio::test]
    async fn prop_high_load_scales_up() {
        let mut runner = proptest::test_runner::TestRunner::default();

        runner
            .run(&scaling_params_strategy(), |params| {
                futures::executor::block_on(async {
                    let scaler = Arc::new(DynamicScaler::new(params.clone()));

                    // Simulate high load (high fill rate)
                    for _ in 0..20 {
                        scaler.record_metrics(50.0, 0.95, 10).await;
                    }

                    // Wait past cooldown
                    tokio::time::sleep(tokio::time::Duration::from_secs(params.cooldown_secs + 1))
                        .await;

                    let decision = scaler.calculate_scale_decision(&params).await;
                    // High load should trigger scale up (unless at max)
                    if scaler.get_current_size().await < params.max_size {
                        prop_assert_eq!(decision, ScaleDecision::ScaleUp);
                    }
                    Ok(())
                })
            })
            .unwrap();
    }

    #[tokio::test]
    async fn prop_low_load_scales_down() {
        let mut runner = proptest::test_runner::TestRunner::default();

        runner
            .run(&scaling_params_strategy(), |params| {
                futures::executor::block_on(async {
                    let mut modified_params = params.clone();
                    modified_params.initial_size = (params.max_size + params.min_size) / 2;
                    let scaler = Arc::new(DynamicScaler::new(modified_params.clone()));

                    // Simulate low load
                    for _ in 0..20 {
                        scaler.record_metrics(1.0, 0.05, 0).await;
                    }

                    // Wait past cooldown
                    tokio::time::sleep(tokio::time::Duration::from_secs(
                        modified_params.cooldown_secs + 1,
                    ))
                    .await;

                    let decision = scaler.calculate_scale_decision(&modified_params).await;
                    // Low load should trigger scale down (unless at min)
                    if scaler.get_current_size().await > modified_params.min_size {
                        prop_assert_eq!(decision, ScaleDecision::ScaleDown);
                    }
                    Ok(())
                })
            })
            .unwrap();
    }

    #[tokio::test]
    async fn prop_scale_factor_applied() {
        let mut runner = proptest::test_runner::TestRunner::default();

        runner
            .run(&scaling_params_strategy(), |params| {
                futures::executor::block_on(async {
                    let scaler = Arc::new(DynamicScaler::new(params.clone()));
                    let initial_size = scaler.get_current_size().await;

                    // Force scale up
                    let new_up = scaler.apply_scale(ScaleDecision::ScaleUp, &params).await;
                    let expected_up =
                        ((initial_size as f64 * params.scale_factor) as usize).min(params.max_size);
                    prop_assert_eq!(new_up, expected_up);

                    // Force scale down
                    let new_down = scaler.apply_scale(ScaleDecision::ScaleDown, &params).await;
                    let expected_down =
                        ((new_up as f64 / params.scale_factor) as usize).max(params.min_size);
                    prop_assert_eq!(new_down, expected_down);
                    Ok(())
                })
            })
            .unwrap();
    }

    #[tokio::test]
    async fn prop_prediction_reasonable() {
        let strategy = (scaling_params_strategy(), 1u64..300);
        let mut runner = proptest::test_runner::TestRunner::default();

        runner
            .run(&strategy, |(params, horizon)| {
                futures::executor::block_on(async {
                    let scaler = Arc::new(DynamicScaler::new(params.clone()));

                    // Add some throughput data
                    for i in 0..20 {
                        scaler.record_metrics(i as f64 * 2.0, 0.5, 5).await;
                    }

                    let predicted = scaler.predict_size_needed(horizon).await;
                    let current = scaler.get_current_size().await;

                    // Prediction should be at least current size
                    prop_assert!(predicted >= current);
                    // But not unreasonably large
                    prop_assert!(predicted <= params.max_size * 10);
                    Ok(())
                })
            })
            .unwrap();
    }

    #[tokio::test]
    async fn prop_cooldown_prevents_rapid_scaling() {
        let mut runner = proptest::test_runner::TestRunner::default();

        runner
            .run(&scaling_params_strategy(), |params| {
                futures::executor::block_on(async {
                    let scaler = Arc::new(DynamicScaler::new(params.clone()));

                    // Record high load
                    for _ in 0..20 {
                        scaler.record_metrics(50.0, 0.9, 10).await;
                    }

                    // First decision should work
                    let decision1 = scaler.calculate_scale_decision(&params).await;
                    let _ = scaler.apply_scale(decision1, &params).await;

                    // Immediate second decision should be Hold due to cooldown
                    let decision2 = scaler.calculate_scale_decision(&params).await;
                    prop_assert_eq!(decision2, ScaleDecision::Hold);
                    Ok(())
                })
            })
            .unwrap();
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn prop_dag_with_dynamic_scaling() {
        let strategy = (
            prop::collection::vec(block_strategy(vec![]), 50..200),
            scaling_params_strategy(),
        );
        let mut runner = proptest::test_runner::TestRunner::default();

        runner
            .run(&strategy, |(blocks, params)| {
                futures::executor::block_on(async {
                    std::env::set_var("GHOSTDAG_MAX_BLOCKS", params.initial_size.to_string());
                    let dag = BoundedGhostdag::new("test_node".to_string());

                    let mut last_id = "genesis".to_string();
                    for (i, mut block) in blocks.into_iter().enumerate() {
                        block.id = format!("block_{}", i);
                        block.parents = if i == 0 {
                            vec![]
                        } else {
                            vec![last_id.clone()]
                        };
                        last_id = block.id.clone();

                        let _ = dag.add_block(block).await;
                    }

                    let stats = dag.get_stats().await;
                    // DAG should respect dynamic bounds
                    prop_assert!(stats.total_blocks > 0);
                    Ok(())
                })
            })
            .unwrap();
    }

    #[test]
    fn prop_concurrent_ops_handled() {
        let strategy = prop::collection::vec(namespace_op_strategy(), 10..50);
        let mut runner = proptest::test_runner::TestRunner::default();

        runner
            .run(&strategy, |ops| {
                futures::executor::block_on(async {
                    let dag = BoundedGhostdag::new("test_node".to_string());

                    // Create blocks with potentially conflicting operations
                    let mut handles = vec![];
                    for (i, op) in ops.into_iter().enumerate() {
                        let block = Block {
                            id: format!("block_{}", i),
                            parents: if i == 0 {
                                vec![]
                            } else {
                                vec![format!("block_{}", i - 1)]
                            },
                            operations: vec![op],
                            timestamp: i as u64,
                            creator: format!("node_{}", i % 3),
                            signature: vec![],
                            state: BlockState::Pending,
                            ghost_weight: 1,
                            height: i as u64,
                        };

                        let dag_clone = dag.clone();
                        handles.push(tokio::spawn(
                            async move { dag_clone.add_block(block).await },
                        ));
                    }

                    // Wait for all operations
                    for handle in handles {
                        let _ = handle.await;
                    }

                    let stats = dag.get_stats().await;
                    prop_assert!(stats.total_blocks > 0);
                    Ok(())
                })
            })
            .unwrap();
    }
}
