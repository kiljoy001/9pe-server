use ninepe_server::memory::PoolConfig;

#[test]
fn default_pool_config_has_reasonable_limits() {
    let config = PoolConfig::default();
    assert!(config.initial_size > 0);
    assert!(config.max_size >= config.initial_size);
}
