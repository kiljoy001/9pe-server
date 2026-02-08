use ninepe_server::protocol::NinePMessage;

#[test]
fn protocol_message_roundtrip_serialization() {
    let msg = NinePMessage::Version {
        msize: 4096,
        version: "9P.e".to_string(),
    };

    let bytes = bincode::serialize(&msg).expect("serialize");
    let decoded: NinePMessage = bincode::deserialize(&bytes).expect("deserialize");
    assert_eq!(decoded, msg);
}
