///! llama.cpp worker for distributed grid computing
///!
///! Executes LLM inference tasks using local llama-server,
///! enabling distributed AI workloads across the 9PE grid.

use anyhow::{Result, Context};
use serde::{Serialize, Deserialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn, error};

use super::work_distribution::{JobRequest, JobRequirements, NodeCapabilities, PartialResult};
use super::crypto::{WorkProof, ComputationProof, Signature};

/// llama.cpp worker executes LLM inference tasks
pub struct LlamaCppWorker {
    node_id: String,
    server_url: String,
    model_name: String,
}

/// LLM inference request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMRequest {
    pub prompt: String,
    pub max_tokens: Option<usize>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub stop: Option<Vec<String>>,
}

/// LLM inference response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMResponse {
    pub content: String,
    pub model: String,
    pub tokens_predicted: usize,
    pub tokens_evaluated: usize,
    pub truncated: bool,
    pub stopped_eos: bool,
}

impl LlamaCppWorker {
    /// Create new llama.cpp worker
    pub fn new(node_id: String, server_url: Option<String>) -> Self {
        Self {
            node_id,
            server_url: server_url.unwrap_or_else(|| "http://localhost:8080".to_string()),
            model_name: "unknown".to_string(),
        }
    }

    /// Check if llama-server is available
    pub async fn check_availability(&self) -> Result<bool> {
        let client = reqwest::Client::new();
        let response = client
            .get(&format!("{}/health", self.server_url))
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await;

        Ok(response.is_ok())
    }

    /// Get node capabilities
    pub async fn get_capabilities(&self) -> Result<NodeCapabilities> {
        // Try to get model info from server
        let client = reqwest::Client::new();
        let props_response = client
            .get(&format!("{}/props", self.server_url))
            .send()
            .await;

        let (memory_gb, gpu_memory_gb) = if let Ok(resp) = props_response {
            if let Ok(props) = resp.json::<serde_json::Value>().await {
                // Rough estimate based on context size
                let ctx_size = props["default_generation_settings"]["n_ctx"]
                    .as_u64()
                    .unwrap_or(2048);
                let memory = (ctx_size * 4 / 1024 / 1024 / 1024) as u32; // bytes to GB
                (memory.max(4), Some(8)) // Min 4GB, assume 8GB GPU
            } else {
                (8, Some(8))
            }
        } else {
            (8, Some(8))
        };

        Ok(NodeCapabilities {
            cpu_cores: num_cpus::get() as u32,
            memory_gb,
            has_gpu: true, // Assume GPU if using llama.cpp
            gpu_memory_gb,
            storage_gb: 100, // Placeholder
            capabilities: vec!["llm_inference".to_string(), "text_generation".to_string()],
            geographic_region: None,
        })
    }

    /// Execute inference job
    pub async fn execute_job(&mut self, job: &JobRequest) -> Result<PartialResult> {
        let start_time = SystemTime::now();

        // Parse job input_data as LLM request
        let request: LLMRequest = serde_json::from_slice(&job.input_data)
            .context("Failed to parse LLM request")?;

        info!("Executing LLM job: {} (prompt length: {} chars)",
              job.id, request.prompt.len());

        // Call llama-server
        let response = self.call_llama_server(&request).await
            .context("Failed to call llama-server")?;

        let execution_time_ms = SystemTime::now()
            .duration_since(start_time)?
            .as_millis() as u64;

        info!("LLM job completed in {}ms, generated {} tokens",
              execution_time_ms, response.tokens_predicted);

        // Serialize response
        let result_data = serde_json::to_vec(&response)?;

        // Create proof of work (simplified for now)
        let input_hash = self.hash_result(&job.input_data);
        let output_hash = self.hash_result(&result_data);

        let proof = WorkProof {
            work_id: job.id.clone(),
            result_hash: output_hash.clone(),
            computation_proof: ComputationProof::HashProof {
                input_hash,
                output_hash,
                steps: response.tokens_predicted as u64,
            },
            node_signature: Signature {
                algorithm: "placeholder".to_string(),
                data: vec![0; 64], // TODO: Real signature
            },
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        };

        Ok(PartialResult {
            job_id: job.id.clone(),
            node_id: self.node_id.clone(),
            data: result_data,
            proof,
            execution_time_ms,
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        })
    }

    /// Call llama-server HTTP API
    async fn call_llama_server(&self, request: &LLMRequest) -> Result<LLMResponse> {
        let client = reqwest::Client::new();

        let mut request_body = serde_json::json!({
            "prompt": request.prompt,
            "n_predict": request.max_tokens.unwrap_or(512),
            "stream": false,
        });

        if let Some(temp) = request.temperature {
            request_body["temperature"] = serde_json::json!(temp);
        }
        if let Some(top_p) = request.top_p {
            request_body["top_p"] = serde_json::json!(top_p);
        }
        if let Some(stop) = &request.stop {
            request_body["stop"] = serde_json::json!(stop);
        }

        debug!("Calling llama-server: {}", self.server_url);

        let response = client
            .post(&format!("{}/completion", self.server_url))
            .json(&request_body)
            .timeout(std::time::Duration::from_secs(300)) // 5 min timeout
            .send()
            .await
            .context("Failed to send request to llama-server")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("llama-server returned error {}: {}", status, error_text);
        }

        let response_json = response.json::<serde_json::Value>().await
            .context("Failed to parse llama-server response")?;

        Ok(LLMResponse {
            content: response_json["content"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            model: response_json["model"]
                .as_str()
                .unwrap_or("unknown")
                .to_string(),
            tokens_predicted: response_json["tokens_predicted"]
                .as_u64()
                .unwrap_or(0) as usize,
            tokens_evaluated: response_json["tokens_evaluated"]
                .as_u64()
                .unwrap_or(0) as usize,
            truncated: response_json["truncated"]
                .as_bool()
                .unwrap_or(false),
            stopped_eos: response_json["stopped_eos"]
                .as_bool()
                .unwrap_or(false),
        })
    }

    /// Hash result for proof of work
    fn hash_result(&self, data: &[u8]) -> Vec<u8> {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(data);
        hasher.finalize().to_vec()
    }
}

/// Create an LLM inference job request
pub fn create_llm_job(
    prompt: String,
    max_tokens: Option<usize>,
    temperature: Option<f32>,
) -> Result<JobRequest> {
    let request = LLMRequest {
        prompt,
        max_tokens,
        temperature,
        top_p: None,
        stop: None,
    };

    let input_data = serde_json::to_vec(&request)?;

    Ok(JobRequest {
        id: uuid::Uuid::new_v4().to_string(),
        work_type: "llm_inference".to_string(),
        input_data,
        requirements: JobRequirements {
            min_nodes: 1,
            min_cpu_cores: Some(1),
            min_memory_gb: Some(4),
            requires_gpu: false, // Optional but preferred
            required_capabilities: vec!["llm_inference".to_string()],
            geographic_constraints: None,
        },
        priority: 5,
        timeout_seconds: 300,
        submitted_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_llama_worker_creation() {
        let worker = LlamaCppWorker::new(
            "test-node".to_string(),
            Some("http://localhost:8080".to_string()),
        );
        assert_eq!(worker.node_id, "test-node");
    }

    #[tokio::test]
    async fn test_create_llm_job() {
        let job = create_llm_job(
            "Hello world".to_string(),
            Some(100),
            Some(0.7),
        ).unwrap();

        assert_eq!(job.work_type, "llm_inference");
        assert!(job.input_data.len() > 0);
    }
}
