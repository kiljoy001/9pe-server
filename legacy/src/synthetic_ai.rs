//! AI Synthetic Files - Demonstrate AI inference through filesystem
//!
//! This module shows how AI models can be exposed as files

use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::RwLock;
use std::collections::VecDeque;

use crate::synthetic::SyntheticGenerator;

/// Simple prompt-response AI file
/// Write a prompt, read the response
pub struct AIPromptFile {
    /// Current prompt buffer
    prompt: Arc<RwLock<String>>,
    /// Response cache
    response: Arc<RwLock<String>>,
    /// Model name for display
    model_name: String,
}

impl AIPromptFile {
    pub fn new(model_name: &str) -> Self {
        Self {
            prompt: Arc::new(RwLock::new(String::new())),
            response: Arc::new(RwLock::new(String::new())),
            model_name: model_name.to_string(),
        }
    }

    async fn generate_response(&self, prompt: &str) -> String {
        // In a real implementation, this would call an AI model
        // For now, we'll simulate with a simple response
        format!(
            "Response from {}: I received your prompt '{}'. \
            In a real system with pebbling memory optimization, \
            I could run a 7B parameter model using only 120MB of active memory!",
            self.model_name, prompt
        )
    }
}

#[async_trait]
impl SyntheticGenerator for AIPromptFile {
    async fn generate(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        let response = self.response.read().await;
        let bytes = response.as_bytes();
        let start = offset.min(bytes.len() as u64) as usize;
        let end = (start + count as usize).min(bytes.len());
        Ok(bytes[start..end].to_vec())
    }

    async fn size(&self) -> u64 {
        self.response.read().await.len() as u64
    }
}

/// Bidirectional AI file - write prompts, read responses
pub struct BidirectionalAIFile {
    prompt_file: Arc<AIPromptFile>,
}

impl BidirectionalAIFile {
    pub fn new(model_name: &str) -> Self {
        Self {
            prompt_file: Arc::new(AIPromptFile::new(model_name)),
        }
    }

    pub async fn write(&self, data: &[u8]) -> Result<u32> {
        let prompt = String::from_utf8_lossy(data);

        // Update prompt
        *self.prompt_file.prompt.write().await = prompt.to_string();

        // Generate response
        let response = self.prompt_file.generate_response(&prompt).await;
        *self.prompt_file.response.write().await = response;

        Ok(data.len() as u32)
    }

    pub async fn read(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        self.prompt_file.generate(offset, count).await
    }
}

/// Streaming chat interface
pub struct ChatStreamFile {
    /// Message history
    messages: Arc<RwLock<VecDeque<String>>>,
    /// Max messages to keep
    max_messages: usize,
    /// Model name
    model_name: String,
}

impl ChatStreamFile {
    pub fn new(model_name: &str, max_messages: usize) -> Self {
        Self {
            messages: Arc::new(RwLock::new(VecDeque::new())),
            max_messages,
            model_name: model_name.to_string(),
        }
    }

    pub async fn add_message(&self, msg: String) {
        let mut messages = self.messages.write().await;
        messages.push_back(msg);
        if messages.len() > self.max_messages {
            messages.pop_front();
        }
    }

    pub async fn get_chat_history(&self) -> String {
        let messages = self.messages.read().await;
        messages.iter()
            .map(|m| format!("{}\n", m))
            .collect::<String>()
    }
}

#[async_trait]
impl SyntheticGenerator for ChatStreamFile {
    async fn generate(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        let history = self.get_chat_history().await;
        let bytes = history.as_bytes();
        let start = offset.min(bytes.len() as u64) as usize;
        let end = (start + count as usize).min(bytes.len());
        Ok(bytes[start..end].to_vec())
    }

    async fn size(&self) -> u64 {
        self.get_chat_history().await.len() as u64
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn refresh_rate_ms(&self) -> u64 {
        100 // Update every 100ms for chat
    }
}

/// Model status file - shows memory usage with pebbling
pub struct ModelStatusFile {
    model_name: String,
    total_params: u64,
    traditional_memory: u64,
    pebbled_memory: u64,
}

impl ModelStatusFile {
    pub fn new(model_name: &str, params_billions: f64) -> Self {
        let total_params = (params_billions * 1_000_000_000.0) as u64;
        // Traditional: 2 bytes per parameter (FP16)
        let traditional_memory = total_params * 2;
        // Pebbled: sqrt of traditional
        let pebbled_memory = ((traditional_memory as f64).sqrt()) as u64;

        Self {
            model_name: model_name.to_string(),
            total_params,
            traditional_memory,
            pebbled_memory,
        }
    }

    fn format_bytes(bytes: u64) -> String {
        if bytes < 1024 {
            format!("{} B", bytes)
        } else if bytes < 1024 * 1024 {
            format!("{:.2} KB", bytes as f64 / 1024.0)
        } else if bytes < 1024 * 1024 * 1024 {
            format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
        } else {
            format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
        }
    }
}

#[async_trait]
impl SyntheticGenerator for ModelStatusFile {
    async fn generate(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        let status = format!(
            "Model: {}\n\
            Parameters: {:.1}B\n\
            Traditional Memory Required: {}\n\
            Pebbled Memory Required: {}\n\
            Memory Savings: {:.1}x\n\
            Status: Ready for inference\n",
            self.model_name,
            self.total_params as f64 / 1_000_000_000.0,
            Self::format_bytes(self.traditional_memory),
            Self::format_bytes(self.pebbled_memory),
            self.traditional_memory as f64 / self.pebbled_memory as f64
        );

        let bytes = status.as_bytes();
        let start = offset.min(bytes.len() as u64) as usize;
        let end = (start + count as usize).min(bytes.len());
        Ok(bytes[start..end].to_vec())
    }

    async fn size(&self) -> u64 {
        256 // Approximate size
    }
}

/// Register all AI synthetic files
pub async fn register_ai_files(fs: &crate::synthetic::SyntheticFileSystem) {
    // LLaMA-7B interface
    fs.register(
        "/ai/llama7b/prompt".to_string(),
        Arc::new(AIPromptFile::new("LLaMA-7B"))
    ).await;

    // GPT-style model
    fs.register(
        "/ai/gpt/prompt".to_string(),
        Arc::new(AIPromptFile::new("Liberation-GPT"))
    ).await;

    // Chat interface
    fs.register(
        "/ai/chat/stream".to_string(),
        Arc::new(ChatStreamFile::new("ChatBot", 100))
    ).await;

    // Model status files showing pebbling advantages
    fs.register(
        "/ai/models/llama7b/status".to_string(),
        Arc::new(ModelStatusFile::new("LLaMA-7B", 7.0))
    ).await;

    fs.register(
        "/ai/models/llama13b/status".to_string(),
        Arc::new(ModelStatusFile::new("LLaMA-13B", 13.0))
    ).await;

    fs.register(
        "/ai/models/gpt3/status".to_string(),
        Arc::new(ModelStatusFile::new("GPT-3", 175.0))
    ).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ai_prompt_file() {
        let ai_file = AIPromptFile::new("TestModel");
        *ai_file.prompt.write().await = "Hello AI".to_string();
        let response = ai_file.generate_response("Hello AI").await;
        assert!(response.contains("TestModel"));
        assert!(response.contains("Hello AI"));
    }

    #[tokio::test]
    async fn test_model_status() {
        let status = ModelStatusFile::new("TestModel", 7.0);
        let data = status.generate(0, 1000).await.unwrap();
        let content = String::from_utf8_lossy(&data);
        assert!(content.contains("7.0B"));
        assert!(content.contains("Memory Savings"));
    }

    #[tokio::test]
    async fn test_chat_stream() {
        let chat = ChatStreamFile::new("ChatBot", 5);
        chat.add_message("User: Hello".to_string()).await;
        chat.add_message("Bot: Hi there!".to_string()).await;

        let history = chat.get_chat_history().await;
        assert!(history.contains("User: Hello"));
        assert!(history.contains("Bot: Hi there!"));
    }
}