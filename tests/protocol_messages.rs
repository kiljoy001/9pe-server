use ninepe_server::protocol::NinePMessage;

#[test]
fn read_message_contains_payload() {
    let msg = NinePMessage::Read {
        fid: 1,
        offset: 0,
        count: 5,
        data: b"hello".to_vec(),
    };

    if let NinePMessage::Read { data, .. } = msg {
        assert_eq!(data, b"hello".to_vec());
    } else {
        panic!("unexpected variant");
    }
}
