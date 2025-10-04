//! Dynamic scaling algorithm for bounded GHOSTDAG
//! Adapts DAG size based on workload patterns

use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Dynamic scaling metrics and algorithm
pub struct DynamicScaler {
    /// Current maximum size
    current_size: Arc<RwLock<usize>>,

    /// Throughput history (ops/sec over time)
    throughput_history: Arc<RwLock<VecDeque<f64>>>,

    /// Fill rate history (percentage full)
    fill_history: Arc<RwLock<VecDeque<f64>>>,

    /// Fork depth history (max fork depth seen)
    fork_depth_history: Arc<RwLock<VecDeque<u64>>>,

    /// Last scale decision timestamp
    last_scale_time: Arc<RwLock<std::time::Instant>>,

    /// Scale decision cooldown (seconds)
    cooldown_secs: u64,
}

/// Scaling parameters
#[derive(Debug, Clone)]
pub struct ScalingParams {
    pub min_size: usize,
    pub max_size: usize,
    pub initial_size: usize,
    pub scale_factor: f64,
    pub scale_up_threshold: f64,
    pub scale_down_threshold: f64,
    pub history_window: usize,
    pub cooldown_secs: u64,
}

impl Default for ScalingParams {
    fn default() -> Self {
        Self {
            min_size: 100,
            max_size: 10_000,
            initial_size: 1_000,
            scale_factor: 1.5,
            scale_up_threshold: 0.8,
            scale_down_threshold: 0.2,
            history_window: 100,
            cooldown_secs: 60,
        }
    }
}

impl DynamicScaler {
    pub fn new(params: ScalingParams) -> Self {
        Self {
            current_size: Arc::new(RwLock::new(params.initial_size)),
            throughput_history: Arc::new(RwLock::new(VecDeque::with_capacity(params.history_window))),
            fill_history: Arc::new(RwLock::new(VecDeque::with_capacity(params.history_window))),
            fork_depth_history: Arc::new(RwLock::new(VecDeque::with_capacity(params.history_window))),
            last_scale_time: Arc::new(RwLock::new(std::time::Instant::now() - std::time::Duration::from_secs(params.cooldown_secs + 1))),
            cooldown_secs: params.cooldown_secs,
        }
    }

    /// Record current metrics
    pub async fn record_metrics(&self, throughput: f64, fill_rate: f64, fork_depth: u64) {
        let mut tput = self.throughput_history.write().await;
        let mut fill = self.fill_history.write().await;
        let mut fork = self.fork_depth_history.write().await;

        // Maintain sliding window
        if tput.len() >= 100 {
            tput.pop_front();
        }
        if fill.len() >= 100 {
            fill.pop_front();
        }
        if fork.len() >= 100 {
            fork.pop_front();
        }

        tput.push_back(throughput);
        fill.push_back(fill_rate);
        fork.push_back(fork_depth);
    }

    /// Calculate scaling decision based on three-factor algorithm
    pub async fn calculate_scale_decision(&self, params: &ScalingParams) -> ScaleDecision {
        // Check cooldown
        let last_scale = self.last_scale_time.read().await;
        if last_scale.elapsed().as_secs() < self.cooldown_secs {
            return ScaleDecision::Hold;
        }

        // Get current metrics
        let tput = self.throughput_history.read().await;
        let fill = self.fill_history.read().await;
        let fork = self.fork_depth_history.read().await;

        // Need enough data
        if tput.len() < 10 || fill.len() < 10 {
            return ScaleDecision::Hold;
        }

        // Calculate averages
        let avg_throughput = tput.iter().sum::<f64>() / tput.len() as f64;
        let avg_fill = fill.iter().sum::<f64>() / fill.len() as f64;
        let max_fork = fork.iter().max().copied().unwrap_or(0);

        // Calculate pressure score with weights
        let throughput_norm = (avg_throughput / 100.0).min(1.0); // Normalize to 0-1
        let fork_norm = (max_fork as f64 / 100.0).min(1.0); // Normalize to 0-1

        // Weights: fill=0.5, throughput=0.3, fork=0.2
        let pressure_score = 0.5 * avg_fill + 0.3 * throughput_norm + 0.2 * fork_norm;


        // Determine scaling direction
        if pressure_score > params.scale_up_threshold {
            ScaleDecision::ScaleUp
        } else if pressure_score < params.scale_down_threshold {
            ScaleDecision::ScaleDown
        } else {
            ScaleDecision::Hold
        }
    }

    /// Apply scaling decision
    pub async fn apply_scale(&self, decision: ScaleDecision, params: &ScalingParams) -> usize {
        match decision {
            ScaleDecision::ScaleUp => {
                let mut size = self.current_size.write().await;
                let new_size = ((*size as f64 * params.scale_factor) as usize).min(params.max_size);
                if new_size > *size {
                    tracing::info!("Scaling DAG up: {} → {} blocks", *size, new_size);
                    *size = new_size;
                    *self.last_scale_time.write().await = std::time::Instant::now();
                }
                *size
            }
            ScaleDecision::ScaleDown => {
                let mut size = self.current_size.write().await;
                let new_size = ((*size as f64 / params.scale_factor) as usize).max(params.min_size);
                if new_size < *size {
                    tracing::info!("Scaling DAG down: {} → {} blocks", *size, new_size);
                    *size = new_size;
                    *self.last_scale_time.write().await = std::time::Instant::now();
                }
                *size
            }
            ScaleDecision::Hold => {
                *self.current_size.read().await
            }
        }
    }

    /// Get current size
    pub async fn get_current_size(&self) -> usize {
        *self.current_size.read().await
    }

    /// Predict future size based on trend
    pub async fn predict_size_needed(&self, horizon_secs: u64) -> usize {
        let tput = self.throughput_history.read().await;
        if tput.len() < 2 {
            return *self.current_size.read().await;
        }

        // Simple linear extrapolation
        let recent = tput.iter().rev().take(10).copied().collect::<Vec<_>>();
        let trend = if recent.len() > 1 {
            (recent[0] - recent[recent.len() - 1]) / recent.len() as f64
        } else {
            0.0
        };

        let projected_tput = recent[0] + trend * horizon_secs as f64;
        let size = self.current_size.read().await;

        // Estimate size needed for projected throughput
        // Rule: 1 block per op, with 20% buffer
        let needed = (projected_tput * horizon_secs as f64 * 1.2) as usize;
        needed.max(*size)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScaleDecision {
    ScaleUp,
    ScaleDown,
    Hold,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_scaling_decisions() {
        let params = ScalingParams::default();
        let scaler = DynamicScaler::new(params.clone());

        // Simulate high load
        for _ in 0..20 {
            scaler.record_metrics(50.0, 0.9, 10).await;
        }

        let decision = scaler.calculate_scale_decision(&params).await;
        assert_eq!(decision, ScaleDecision::ScaleUp);

        // Simulate low load
        for _ in 0..20 {
            scaler.record_metrics(1.0, 0.1, 0).await;
        }

        // Wait for cooldown
        tokio::time::sleep(tokio::time::Duration::from_secs(61)).await;

        let decision = scaler.calculate_scale_decision(&params).await;
        assert_eq!(decision, ScaleDecision::ScaleDown);
    }
}