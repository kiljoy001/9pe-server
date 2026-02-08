use ninepe_server::compatibility::{MessageTranslator, CompatibilitySession};
use ninepe_server::protocol::NinePMessage;

#[test]
fn test_translate_tversion() {
    let session = CompatibilitySession::new();
    let translator = MessageTranslator::new(session);
    
    // Tversion: size[4]=19, type[1]=100, tag[2]=0, msize[4]=8192, version[2]=6, "9P2000"
    let mut data = vec![19, 0, 0, 0, 100, 0, 0];
    data.extend_from_slice(&8192u32.to_le_bytes());
    let version = "9P2000";
    data.extend_from_slice(&(version.len() as u16).to_le_bytes());
    data.extend_from_slice(version.as_bytes());
    
    let msg = translator.translate_legacy_to_9pe(&data).unwrap();
    if let NinePMessage::Version { msize, version: v } = msg {
        assert_eq!(msize, 8192);
        assert_eq!(v, "9P2000");
    } else {
        panic!("Wrong message type");
    }
}

#[test]
fn test_translate_tattach() {
    let session = CompatibilitySession::new();
    let translator = MessageTranslator::new(session);
    
    // Tattach: size, type=104, tag=0, fid=1, afid=NOFID, uname="alice", aname="root"
    let mut data = vec![0, 0, 0, 0, 104, 0, 0];
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&0xFFFFFFFFu32.to_le_bytes());
    
    let uname = "alice";
    data.extend_from_slice(&(uname.len() as u16).to_le_bytes());
    data.extend_from_slice(uname.as_bytes());
    
    let aname = "root";
    data.extend_from_slice(&(aname.len() as u16).to_le_bytes());
    data.extend_from_slice(aname.as_bytes());
    
    // Update size
    let size = data.len() as u32;
    data[0..4].copy_from_slice(&size.to_le_bytes());
    
    let msg = translator.translate_legacy_to_9pe(&data).unwrap();
    if let NinePMessage::Attach { fid, afid, uname: u, aname: a } = msg {
        assert_eq!(fid, 1);
        assert_eq!(afid, 0xFFFFFFFF);
        assert_eq!(u, "alice");
        assert_eq!(a, "root");
    } else {
        panic!("Wrong message type");
    }
}

#[test]
fn test_translate_rerror_to_legacy() {
    let mut session = CompatibilitySession::new();
    session.is_legacy = true;
    let translator = MessageTranslator::new(session);
    
    let msg = NinePMessage::Error {
        ename: "file not found".to_string(),
        errno: 2,
    };
    
    let legacy_data = translator.translate_9pe_to_legacy(&msg).unwrap();
    assert_eq!(legacy_data[4], 107); // Rerror
    
    let ename_len = u16::from_le_bytes([legacy_data[7], legacy_data[8]]) as usize;
    let ename = String::from_utf8_lossy(&legacy_data[9..9+ename_len]);
    assert_eq!(ename, "file not found");
}

#[test]
fn test_translate_twalk() {
    let session = CompatibilitySession::new();
    let translator = MessageTranslator::new(session);
    
    // Twalk: type=110, fid=1, newfid=2, nwname=2, wname=["a", "b"]
    let mut data = vec![0, 0, 0, 0, 110, 0, 0];
    data.extend_from_slice(&1u32.to_le_bytes()); // fid
    data.extend_from_slice(&2u32.to_le_bytes()); // newfid
    data.extend_from_slice(&2u16.to_le_bytes()); // nwname
    
    // wname[0]
    let name1 = "a";
    data.extend_from_slice(&(name1.len() as u16).to_le_bytes());
    data.extend_from_slice(name1.as_bytes());
    
    // wname[1]
    let name2 = "b";
    data.extend_from_slice(&(name2.len() as u16).to_le_bytes());
    data.extend_from_slice(name2.as_bytes());
    
    // Update size
    let size = data.len() as u32;
    data[0..4].copy_from_slice(&size.to_le_bytes());
    
    let msg = translator.translate_legacy_to_9pe(&data).unwrap();
    if let NinePMessage::Walk { fid, newfid, wnames } = msg {
        assert_eq!(fid, 1);
        assert_eq!(newfid, 2);
        assert_eq!(wnames.len(), 2);
        assert_eq!(wnames[0], "a");
        assert_eq!(wnames[1], "b");
    } else {
        panic!("Wrong message type: {:?}", msg);
    }
}

#[test]
fn test_translate_topen() {
    let session = CompatibilitySession::new();
    let translator = MessageTranslator::new(session);
    
    // Topen: type=112, fid=1, mode=0 (OREAD)
    let mut data = vec![0, 0, 0, 0, 112, 0, 0];
    data.extend_from_slice(&1u32.to_le_bytes()); // fid
    data.push(0); // mode
    
    // Update size
    let size = data.len() as u32;
    data[0..4].copy_from_slice(&size.to_le_bytes());
    
    let msg = translator.translate_legacy_to_9pe(&data).unwrap();
    if let NinePMessage::Open { fid, mode } = msg {
        assert_eq!(fid, 1);
        assert_eq!(mode, 0);
    } else {
        panic!("Wrong message type: {:?}", msg);
    }
}

#[test]
fn test_translate_tcreate() {
    let session = CompatibilitySession::new();
    let translator = MessageTranslator::new(session);
    
    // Tcreate: type=114, fid=1, name="foo", perm=0644, mode=1 (OWRITE)
    let mut data = vec![0, 0, 0, 0, 114, 0, 0];
    data.extend_from_slice(&1u32.to_le_bytes()); // fid
    
    let name = "foo";
    data.extend_from_slice(&(name.len() as u16).to_le_bytes()); // name_len
    data.extend_from_slice(name.as_bytes()); // name
    
    data.extend_from_slice(&0o644u32.to_le_bytes()); // perm
    data.push(1); // mode
    
    // Update size
    let size = data.len() as u32;
    data[0..4].copy_from_slice(&size.to_le_bytes());
    
    let msg = translator.translate_legacy_to_9pe(&data).unwrap();
    if let NinePMessage::Create { fid, name, perm, mode } = msg {
        assert_eq!(fid, 1);
        assert_eq!(name, "foo");
        assert_eq!(perm, 0o644);
        assert_eq!(mode, 1);
    } else {
        panic!("Wrong message type: {:?}", msg);
    }
}

#[test]
fn test_translate_tremove() {
    let session = CompatibilitySession::new();
    let translator = MessageTranslator::new(session);
    
    // Tremove: type=122, fid=1
    let mut data = vec![0, 0, 0, 0, 122, 0, 0];
    data.extend_from_slice(&1u32.to_le_bytes()); // fid
    
    // Update size
    let size = data.len() as u32;
    data[0..4].copy_from_slice(&size.to_le_bytes());
    
    let msg = translator.translate_legacy_to_9pe(&data).unwrap();
    if let NinePMessage::Remove { fid } = msg {
        assert_eq!(fid, 1);
    } else {
        panic!("Wrong message type: {:?}", msg);
    }
}

#[test]
fn test_translate_tstat() {
    let session = CompatibilitySession::new();
    let translator = MessageTranslator::new(session);
    
    // Tstat: type=124, fid=1
    let mut data = vec![0, 0, 0, 0, 124, 0, 0];
    data.extend_from_slice(&1u32.to_le_bytes()); // fid
    
    // Update size
    let size = data.len() as u32;
    data[0..4].copy_from_slice(&size.to_le_bytes());
    
    let msg = translator.translate_legacy_to_9pe(&data).unwrap();
    if let NinePMessage::Stat { fid, .. } = msg {
        assert_eq!(fid, 1);
    } else {
        panic!("Wrong message type: {:?}", msg);
    }
}

#[test]
fn test_translate_twstat() {
    let session = CompatibilitySession::new();
    let translator = MessageTranslator::new(session);
    
    // Twstat: type=126, fid=1, stat_len=4, dummy data
    let mut data = vec![0, 0, 0, 0, 126, 0, 0];
    data.extend_from_slice(&1u32.to_le_bytes()); // fid
    data.extend_from_slice(&4u16.to_le_bytes()); // stat_len
    data.extend_from_slice(&[0, 0, 0, 0]); // dummy
    
    // Update size
    let size = data.len() as u32;
    data[0..4].copy_from_slice(&size.to_le_bytes());
    
    let _ = translator.translate_legacy_to_9pe(&data);
}

#[test]
fn test_translate_tflush_error() {
    let session = CompatibilitySession::new();
    let translator = MessageTranslator::new(session);
    
    // Tflush: type=108, oldtag=0
    let mut data = vec![0, 0, 0, 0, 108, 0, 0];
    data.extend_from_slice(&0u16.to_le_bytes()); // oldtag
    
    // Update size
    let size = data.len() as u32;
    data[0..4].copy_from_slice(&size.to_le_bytes());
    
    let result = translator.translate_legacy_to_9pe(&data);
    assert!(result.is_err());
}
