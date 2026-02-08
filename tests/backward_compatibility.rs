use ninepe_server::compatibility::MAX_9P2000_MESSAGE_SIZE;

#[test]
fn legacy_message_size_is_reasonable() {
    assert!(MAX_9P2000_MESSAGE_SIZE >= 8192);
}
