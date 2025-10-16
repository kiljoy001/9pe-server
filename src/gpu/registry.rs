use super::GpuInfo;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Registry of discovered GPU devices in the system.
static GPU_REGISTRY: once_cell::sync::Lazy<Arc<RwLock<HashMap<String, GpuInfo>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(RwLock::new(HashMap::new())));

/// Register a GPU device in the global registry.
pub fn register_gpu(info: GpuInfo) -> String {
    let id = format!("gpu_{}", uuid::Uuid::new_v4().to_string());
    let mut registry = GPU_REGISTRY.write().unwrap();
    registry.insert(id.clone(), info);
    id
}

/// Get information about a registered GPU.
pub fn get_gpu(id: &str) -> Option<GpuInfo> {
    let registry = GPU_REGISTRY.read().unwrap();
    registry.get(id).cloned()
}

/// List all registered GPUs.
pub fn list_gpus() -> Vec<(String, GpuInfo)> {
    let registry = GPU_REGISTRY.read().unwrap();
    registry
        .iter()
        .map(|(id, info)| (id.clone(), info.clone()))
        .collect()
}
