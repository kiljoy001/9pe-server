use ninepee::crypto::CryptoSystem;

#[test]
fn create_and_encrypt_session() {
    let mut crypto = CryptoSystem::new();
    let session_id = crypto.create_session([1u8; 32]).expect("session");

    let aad = b"demo";
    let plaintext = b"hello";
    let encrypted = crypto
        .encrypt_and_sign(session_id, plaintext, aad)
        .expect("encrypt");
    let decrypted = crypto
        .verify_and_decrypt(session_id, &encrypted)
        .expect("decrypt");

    assert_eq!(decrypted, plaintext);
}
