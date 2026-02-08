use ninepe_server::crypto::CryptoSystem;

#[test]
fn crypto_system_has_session_capacity() {
    let crypto = CryptoSystem::new();
    assert!(crypto.limits.max_sessions > 0);
}
