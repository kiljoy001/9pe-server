///! Ollama LLM worker for distributed grid computing
///!
///! Executes LLM inference tasks using local Ollama server,
///! enabling distributed AI workloads across the 9PE grid.

use anyhow::{Result, Context};
use serde::{Serialize, Deserialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::process::Command;
use tracing::{debug, info, warn, error};

use super::work_distribution::{JobRequest, JobRequirements, NodeCapabilities, PartialResult};
use super::crypto::WorkProof;

/// Ollama worker executes LLM inference tasks
pub struct OllamaWorker {
    node_id: String,
    ollama_url: String,
    capabilities: OllamaCapabilities,
}

impl OllamaWorker {
    pub fn new(node_id: String, ollama_url: Option<String>) -> Self {
        let ollama_url = ollama_url.unwrap_or_else(|| "http://localhost:11434".to_string());

        Self {
            node_id,
            ollama_url,
            capabilities: OllamaCapabilities::default(),
        }
    }

    /// Initialize and detect available models
    pub async fn initialize(&mut self) -> Result<()> {
        info!("Initializing Ollama worker on {}", self.node_id);

        // Check if Ollama is running
        let available = self.check_ollama_available().await?;
        if !available {
            anyhow::bail!("Ollama server not available at {}", self.ollama_url);
        }

        // Detect available models
        self.capabilities.available_models = self.list_available_models().await?;

        // Detect GPU capabilities
        self.capabilities.has_gpu = self.detect_gpu().await;

        // Detect pebbling support (check for checkpoint-enabled build)
        self.capabilities.has_pebbling = self.detect_pebbling().await;

        info!("Ollama worker initialized with {} models, GPU: {}, Pebbling: {}",
            self.capabilities.available_models.len(),
            self.capabilities.has_gpu,
            self.capabilities.has_pebbling
        );

        Ok(())
    }

    /// Execute LLM inference job
    pub async fn execute_job(&self, job: &JobRequest) -> Result<PartialResult> {
        let start_time = SystemTime::now();

        // Parse job request
        let llm_request: LLMRequest = serde_json::from_slice(&job.input_data)
            .context("Failed to parse LLM request")?;

        info!("Executing LLM job {} with model {}", job.id, llm_request.model);

        // Execute inference
        let response = self.execute_inference(&llm_request).await?;

        // Calculate execution time
        let execution_time_ms = start_time.elapsed()?.as_millis() as u64;

        // Serialize response
        let result_data = serde_json::to_vec(&response)?;

        // Create proof of work
        let proof = WorkProof {
            nonce: 0, // TODO: Implement actual PoW
            hash: self.hash_result(&result_data),
            difficulty: 1,
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

    /// Execute inference via Ollama API
    async fn execute_inference(&self, request: &LLMRequest) -> Result<LLMResponse> {
        let client = reqwest::Client::new();

        let api_request = serde_json::json!({
            "model": request.model,
            "prompt": request.prompt,
            "stream": false,
            "options": request.options,
        });

        let response = client
            .post(format!("{}/api/generate", self.ollama_url))
            .json(&api_request)
            .send()
            .await
            .context("Failed to call Ollama API")?;

        if !response.status().is_success() {
            anyhow::bail!("Ollama API error: {}", response.status());
        }

        let ollama_response: serde_json::Value = response.json().await?;

        Ok(LLMResponse {
            text: ollama_response["response"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            model: request.model.clone(),
            done: ollama_response["done"].as_bool().unwrap_or(true),
            total_duration: ollama_response["total_duration"].as_u64(),
            eval_count: ollama_response["eval_count"].as_u64(),
        })
    }

    /// Check if Ollama is available
    async fn check_ollama_available(&self) -> Result<bool> {
        let client = reqwest::Client::new();

        match client
            .get(format!("{}/api/tags", self.ollama_url))
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await
        {
            Ok(response) => Ok(response.status().is_success()),
            Err(_) => Ok(false),
        }
    }

    /// List available Ollama models
    async fn list_available_models(&self) -> Result<Vec<String>> {
        let client = reqwest::Client::new();

        let response = client
            .get(format!("{}/api/tags", self.ollama_url))
            .send()
            .await?;

        let tags: serde_json::Value = response.json().await?;

        let models = tags["models"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m["name"].as_str())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();

        Ok(models)
    }

    /// Detect GPU availability
    async fn detect_gpu(&self) -> bool {
        // Check for GPU via system info
        if let Ok(output) = Command::new("lspci").output().await {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.contains("VGA") || stdout.contains("3D") || stdout.contains("Display")
        } else {
            false
        }
    }

    /// Detect pebbling/checkpointing support
    async fn detect_pebbling(&self) -> bool {
        // Try to load a model and check logs for pebbling activation
        // For now, assume checkpoint build if Ollama is available
        // TODO: Add actual detection via model loading
        true
    }

    /// Generate node capabilities for grid registration
    pub fn get_node_capabilities(&self) -> NodeCapabilities {
        let mut capabilities = vec!["llm_inference".to_string()];

        if self.capabilities.has_pebbling {
            capabilities.push("pebbling_optimization".to_string());
        }

        for model in &self.capabilities.available_models {
            capabilities.push(format!("model_{}", model.replace(":", "_")));
        }

        NodeCapabilities {
            cpu_cores: num_cpus::get() as u32,
            memory_gb: 16, // TODO: Detect actual memory
            has_gpu: self.capabilities.has_gpu,
            gpu_memory_gb: if self.capabilities.has_gpu { Some(16) } else { None },
            storage_gb: 100, // TODO: Detect actual storage
            capabilities,
            geographic_region: None,
        }
    }

    fn hash_result(&self, data: &[u8]) -> Vec<u8> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        hasher.finish().to_le_bytes().to_vec()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMRequest {
    pub model: String,
    pub prompt: String,
    pub options: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMResponse {
    pub text: String,
    pub model: String,
    pub done: bool,
    pub total_duration: Option<u64>,
    pub eval_count: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct OllamaCapabilities {
    pub available_models: Vec<String>,
    pub has_gpu: bool,
    pub has_pebbling: bool,
}

/// Helper to create LLM job requests
pub fn create_llm_job(
    model: String,
    prompt: String,
    priority: u32,
) -> Result<JobRequest> {
    let llm_request = LLMRequest {
        model: model.clone(),
        prompt,
        options: None,
    };

    let input_data = serde_json::to_vec(&llm_request)?;

    Ok(JobRequest {
        id: String::new(), // Will be set by WorkDistributor
        work_type: "llm_inference".to_string(),
        input_data,
        requirements: JobRequirements {
            min_nodes: 1,
            min_cpu_cores: Some(4),
            min_memory_gb: Some(8),
            requires_gpu: false, // Optional GPU
            required_capabilities: vec![
                "llm_inference".to_string(),
                format!("model_{}", model.replace(":", "_")),
            ],
            geographic_constraints: None,
        },
        priority,
        timeout_seconds: 300,
        submitted_at: 0, // Will be set by WorkDistributor
    })
}
