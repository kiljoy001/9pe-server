//! Modern Plumber - Inter-application messaging via WASM translator
//!
//! Philosophy: All application communication flows through files via pattern matching rules
//! Examples:
//!   echo "file.txt:123" > /plumb/send    -> routes to editor
//!   echo "https://..." > /plumb/send     -> routes to browser
//!   echo "user@host" > /plumb/send       -> routes to terminal

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use serde::{Serialize, Deserialize};
use regex::Regex;
use async_trait::async_trait;

use crate::synthetic::SyntheticGenerator;

/// A plumbing message - data routed between applications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlumbMessage {
    /// Source application that sent the message
    pub src: String,

    /// Destination port/application
    pub dst: String,

    /// Working directory context
    pub wdir: String,

    /// Type of data (text, file, url, etc.)
    pub data_type: String,

    /// The actual data payload
    pub data: String,

    /// Attributes for routing decisions
    pub attributes: HashMap<String, String>,
}

impl PlumbMessage {
    pub fn new(data: String) -> Self {
        PlumbMessage {
            src: "unknown".to_string(),
            dst: "".to_string(),
            wdir: "/".to_string(),
            data_type: "text".to_string(),
            data,
            attributes: HashMap::new(),
        }
    }
}

/// A plumbing rule that matches messages and routes them
#[derive(Debug, Clone)]
pub struct PlumbRule {
    /// Pattern to match against message data
    pub pattern: Regex,

    /// Target port/application
    pub target_port: String,

    /// Optional command to execute
    pub command: Option<String>,

    /// Rule priority (higher = checked first)
    pub priority: u32,

    /// Rule description
    pub description: String,
}

impl PlumbRule {
    pub fn new(pattern: &str, target_port: &str) -> Result<Self> {
        Ok(PlumbRule {
            pattern: Regex::new(pattern)?,
            target_port: target_port.to_string(),
            command: None,
            priority: 0,
            description: format!("Route {} to {}", pattern, target_port),
        })
    }

    pub fn with_command(mut self, command: &str) -> Self {
        self.command = Some(command.to_string());
        self
    }

    pub fn with_priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }

    pub fn matches(&self, message: &PlumbMessage) -> bool {
        self.pattern.is_match(&message.data)
    }
}

/// Port where applications can send/receive messages
#[derive(Debug)]
pub struct PlumbPort {
    pub name: String,
    pub messages: Arc<RwLock<Vec<PlumbMessage>>>,
    pub subscribers: Arc<RwLock<Vec<String>>>, // App IDs that read from this port
}

impl PlumbPort {
    pub fn new(name: String) -> Self {
        PlumbPort {
            name,
            messages: Arc::new(RwLock::new(Vec::new())),
            subscribers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn send_message(&self, message: PlumbMessage) {
        let mut messages = self.messages.write().await;
        messages.push(message);

        // Keep only last 100 messages
        if messages.len() > 100 {
            messages.drain(0..messages.len() - 100);
        }
    }

    pub async fn get_messages(&self) -> Vec<PlumbMessage> {
        self.messages.read().await.clone()
    }

    pub async fn get_latest_message(&self) -> Option<PlumbMessage> {
        let messages = self.messages.read().await;
        messages.last().cloned()
    }
}

/// The main plumber that routes messages based on rules
pub struct ModernPlumber {
    rules: Arc<RwLock<Vec<PlumbRule>>>,
    ports: Arc<RwLock<HashMap<String, Arc<PlumbPort>>>>,
    message_log: Arc<RwLock<Vec<PlumbMessage>>>,
}

impl ModernPlumber {
    pub fn new() -> Self {
        let mut plumber = ModernPlumber {
            rules: Arc::new(RwLock::new(Vec::new())),
            ports: Arc::new(RwLock::new(HashMap::new())),
            message_log: Arc::new(RwLock::new(Vec::new())),
        };

        // Create default ports
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                plumber.create_port("edit").await.ok();
                plumber.create_port("web").await.ok();
                plumber.create_port("terminal").await.ok();
                plumber.create_port("image").await.ok();

                // Add default rules
                plumber.add_default_rules().await.ok();
            });
        });

        plumber
    }

    pub async fn create_port(&self, name: &str) -> Result<()> {
        let mut ports = self.ports.write().await;
        ports.insert(name.to_string(), Arc::new(PlumbPort::new(name.to_string())));
        Ok(())
    }

    pub async fn get_port(&self, name: &str) -> Option<Arc<PlumbPort>> {
        let ports = self.ports.read().await;
        ports.get(name).cloned()
    }

    pub async fn add_rule(&self, rule: PlumbRule) -> Result<()> {
        let mut rules = self.rules.write().await;

        // Insert in priority order (highest first)
        let insert_pos = rules.iter()
            .position(|r| r.priority < rule.priority)
            .unwrap_or(rules.len());

        rules.insert(insert_pos, rule);
        Ok(())
    }

    async fn add_default_rules(&self) -> Result<()> {
        // File editing rules
        self.add_rule(PlumbRule::new(
            r"^([^:]+):(\d+)$",  // file.txt:123
            "edit"
        )?.with_priority(100)).await?;

        // URL rules
        self.add_rule(PlumbRule::new(
            r"^https?://\S+$",   // http://example.com
            "web"
        )?.with_priority(90)).await?;

        // SSH/terminal rules
        self.add_rule(PlumbRule::new(
            r"^[a-zA-Z0-9_-]+@[a-zA-Z0-9.-]+$",  // user@host
            "terminal"
        )?.with_command("ssh {}")
        .with_priority(80)).await?;

        // Image files
        self.add_rule(PlumbRule::new(
            r"\.(jpg|jpeg|png|gif|webp)$",
            "image"
        )?.with_priority(70)).await?;

        // Default rule - send to edit
        self.add_rule(PlumbRule::new(
            r".*",  // Catch all
            "edit"
        )?.with_priority(1)).await?;

        Ok(())
    }

    pub async fn plumb_message(&self, mut message: PlumbMessage) -> Result<()> {
        // Find matching rule
        let rules = self.rules.read().await;
        let matching_rule = rules.iter()
            .find(|rule| rule.matches(&message));

        if let Some(rule) = matching_rule {
            message.dst = rule.target_port.clone();

            // Route to target port
            if let Some(port) = self.get_port(&rule.target_port).await {
                port.send_message(message.clone()).await;
            }
        }

        // Log the message
        let mut log = self.message_log.write().await;
        log.push(message);

        // Keep only last 1000 messages
        if log.len() > 1000 {
            log.drain(0..log.len() - 1000);
        }

        Ok(())
    }

    pub async fn get_log(&self) -> Vec<PlumbMessage> {
        self.message_log.read().await.clone()
    }

    pub async fn list_ports(&self) -> Vec<String> {
        let ports = self.ports.read().await;
        ports.keys().cloned().collect()
    }

    pub async fn list_rules(&self) -> Vec<PlumbRule> {
        self.rules.read().await.clone()
    }
}

/// Synthetic file generator for plumber send interface
pub struct PlumberSendGenerator {
    plumber: Arc<ModernPlumber>,
}

impl PlumberSendGenerator {
    pub fn new(plumber: Arc<ModernPlumber>) -> Self {
        PlumberSendGenerator { plumber }
    }
}

#[async_trait]
impl SyntheticGenerator for PlumberSendGenerator {
    async fn generate(&self, _offset: u64, _count: u32) -> Result<Vec<u8>> {
        let usage = r#"Plumber Send Interface

Usage: echo "message" > /plumb/send

Examples:
  echo "file.txt:123" > /plumb/send        # Edit file at line 123
  echo "https://example.com" > /plumb/send # Open URL in browser
  echo "user@host" > /plumb/send           # SSH to host
  echo "image.jpg" > /plumb/send           # View image

This file accepts messages and routes them to appropriate applications
based on pattern matching rules.
"#;
        Ok(usage.as_bytes().to_vec())
    }

    async fn size(&self) -> u64 {
        512
    }
}

/// Synthetic file generator for port message reading
pub struct PlumberPortGenerator {
    plumber: Arc<ModernPlumber>,
    port_name: String,
}

impl PlumberPortGenerator {
    pub fn new(plumber: Arc<ModernPlumber>, port_name: String) -> Self {
        PlumberPortGenerator { plumber, port_name }
    }
}

#[async_trait]
impl SyntheticGenerator for PlumberPortGenerator {
    async fn generate(&self, _offset: u64, _count: u32) -> Result<Vec<u8>> {
        if let Some(port) = self.plumber.get_port(&self.port_name).await {
            let messages = port.get_messages().await;

            if messages.is_empty() {
                Ok("# No messages\n".as_bytes().to_vec())
            } else {
                let mut output = format!("# Messages for port: {}\n", self.port_name);
                for (i, msg) in messages.iter().enumerate() {
                    output.push_str(&format!(
                        "{}. {} -> {} ({}): {}\n",
                        i + 1,
                        msg.src,
                        msg.dst,
                        msg.data_type,
                        msg.data
                    ));
                }
                Ok(output.as_bytes().to_vec())
            }
        } else {
            Ok(format!("# Port '{}' not found\n", self.port_name).as_bytes().to_vec())
        }
    }

    async fn size(&self) -> u64 {
        4096
    }

    fn refresh_rate_ms(&self) -> u64 {
        1000 // Refresh every second for new messages
    }
}

/// Synthetic file generator for plumber log
pub struct PlumberLogGenerator {
    plumber: Arc<ModernPlumber>,
}

impl PlumberLogGenerator {
    pub fn new(plumber: Arc<ModernPlumber>) -> Self {
        PlumberLogGenerator { plumber }
    }
}

#[async_trait]
impl SyntheticGenerator for PlumberLogGenerator {
    async fn generate(&self, _offset: u64, _count: u32) -> Result<Vec<u8>> {
        let log = self.plumber.get_log().await;

        let mut output = "# Plumber Message Log\n".to_string();
        for (i, msg) in log.iter().enumerate() {
            output.push_str(&format!(
                "[{}] {} -> {} ({}): {}\n",
                i + 1,
                msg.src,
                msg.dst,
                msg.data_type,
                msg.data
            ));
        }

        Ok(output.as_bytes().to_vec())
    }

    async fn size(&self) -> u64 {
        8192
    }

    fn refresh_rate_ms(&self) -> u64 {
        2000 // Refresh every 2 seconds
    }
}

/// Synthetic file generator for plumber rules
pub struct PlumberRulesGenerator {
    plumber: Arc<ModernPlumber>,
}

impl PlumberRulesGenerator {
    pub fn new(plumber: Arc<ModernPlumber>) -> Self {
        PlumberRulesGenerator { plumber }
    }
}

#[async_trait]
impl SyntheticGenerator for PlumberRulesGenerator {
    async fn generate(&self, _offset: u64, _count: u32) -> Result<Vec<u8>> {
        let rules = self.plumber.list_rules().await;

        let mut output = "# Plumber Rules (priority order)\n".to_string();
        for (i, rule) in rules.iter().enumerate() {
            output.push_str(&format!(
                "{}. [{}] {} -> {}\n   # {}\n",
                i + 1,
                rule.priority,
                rule.pattern.as_str(),
                rule.target_port,
                rule.description
            ));

            if let Some(ref cmd) = rule.command {
                output.push_str(&format!("   Command: {}\n", cmd));
            }
            output.push('\n');
        }

        Ok(output.as_bytes().to_vec())
    }

    async fn size(&self) -> u64 {
        2048
    }
}

/// Message processor that handles writes to /plumb/send
pub struct PlumberMessageProcessor {
    plumber: Arc<ModernPlumber>,
}

impl PlumberMessageProcessor {
    pub fn new(plumber: Arc<ModernPlumber>) -> Self {
        PlumberMessageProcessor { plumber }
    }

    pub async fn process_message(&self, data: &str) -> Result<()> {
        let message = PlumbMessage::new(data.trim().to_string());
        self.plumber.plumb_message(message).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_plumber_routing() {
        let plumber = Arc::new(ModernPlumber::new());

        // Test file:line routing
        let msg = PlumbMessage::new("main.rs:42".to_string());
        plumber.plumb_message(msg).await.unwrap();

        let edit_port = plumber.get_port("edit").await.unwrap();
        let messages = edit_port.get_messages().await;
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].data, "main.rs:42");
        assert_eq!(messages[0].dst, "edit");
    }

    #[tokio::test]
    async fn test_url_routing() {
        let plumber = Arc::new(ModernPlumber::new());

        let msg = PlumbMessage::new("https://example.com".to_string());
        plumber.plumb_message(msg).await.unwrap();

        let web_port = plumber.get_port("web").await.unwrap();
        let messages = web_port.get_messages().await;
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].dst, "web");
    }

    #[tokio::test]
    async fn test_rule_priority() {
        let plumber = Arc::new(ModernPlumber::new());

        // Add high priority rule for .rs files
        plumber.add_rule(PlumbRule::new(
            r"\.rs$",
            "rust_editor"
        ).unwrap().with_priority(200)).await.unwrap();

        let msg = PlumbMessage::new("main.rs:42".to_string());
        plumber.plumb_message(msg).await.unwrap();

        // Should go to rust_editor, not general edit (due to higher priority)
        let log = plumber.get_log().await;
        assert_eq!(log[0].dst, "rust_editor");
    }
}