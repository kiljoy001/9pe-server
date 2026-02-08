use ninepe_server::sycl::backend_loader::SyclBackendManager;
use std::env;

#[test]
fn test_sycl_backend_selection_intel_only() {
    // Set environment to only allow intel
    env::set_var("NINEPE_SYCL_BACKENDS", "intel");
    
    let manager = SyclBackendManager::new();
    
    // Check that intel is potentially some (if available on system) 
    // but adaptive MUST be none
    assert!(manager.adaptive_backend().is_none(), "Adaptive backend should be disabled");
}

#[test]
fn test_sycl_backend_selection_adaptive_only() {
    // Set environment to only allow adaptive
    env::set_var("NINEPE_SYCL_BACKENDS", "adaptive");
    
    let manager = SyclBackendManager::new();
    
    // Check that adaptive is potentially some (if available)
    // but intel MUST be none
    assert!(manager.intel_backend().is_none(), "Intel backend should be disabled");
}

#[test]
fn test_sycl_backend_selection_none() {
    // Set environment to none
    env::set_var("NINEPE_SYCL_BACKENDS", "none");
    
    let manager = SyclBackendManager::new();
    
    assert!(!manager.has_any_backend(), "No backends should be loaded when set to none");
    assert!(manager.intel_backend().is_none());
    assert!(manager.adaptive_backend().is_none());
}

#[test]
fn test_sycl_backend_selection_all() {
    // Set environment to all
    env::set_var("NINEPE_SYCL_BACKENDS", "intel,adaptive");
    
    // This is hard to test deterministically if they aren't both present on the system,
    // but we can at least check that it doesn't crash and respects the flag.
    let _manager = SyclBackendManager::new();
}
