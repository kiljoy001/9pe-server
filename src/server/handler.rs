//! Message handler - no more God Object!

use anyhow::Result;
use std::path::PathBuf;

/// Handles 9P.e protocol messages
pub struct MessageHandler {
    root: PathBuf,
    max_message_size: u32,
}

impl MessageHandler {
    pub fn new(root: PathBuf, max_message_size: u32) -> Result<Self> {
        Ok(Self {
            root,
            max_message_size,
        })
    }

    // Message handling methods would go here
    // Each method handles one message type
    // No giant match statement!
}