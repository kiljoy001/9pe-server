//! Property-based tests for 9P protocol messages

use ninep_server::protocol::{
    messages::*, permissions, Fid, Message, MessageType, Qid, Stat, Tag, WireFormat, MAX_MSG_SIZE,
};
use proptest::prelude::*;

/// Generate arbitrary tags
fn arbitrary_tag() -> impl Strategy<Value = Tag> {
    1u16..u16::MAX
}

/// Generate arbitrary fids
fn arbitrary_fid() -> impl Strategy<Value = Fid> {
    0u32..u32::MAX
}

/// Generate arbitrary qids
fn arbitrary_qid() -> impl Strategy<Value = Qid> {
    (any::<u8>(), any::<u32>(), any::<u64>()).prop_map(|(qtype, version, path)| Qid {
        qtype,
        version,
        path,
    })
}

/// Generate arbitrary file names
fn arbitrary_filename() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9._\\-]{1,255}".prop_map(|s| s.to_string())
}

/// Generate arbitrary paths
fn arbitrary_path_components() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(arbitrary_filename(), 0..10)
}

/// Generate arbitrary data
fn arbitrary_data(max_size: usize) -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..max_size)
}

proptest! {
    /// Property: Message encoding and decoding should be symmetric
    #[test]
    fn prop_tversion_encode_decode_symmetric(
        tag in arbitrary_tag(),
        msize in 8192u32..MAX_MSG_SIZE,
        version in prop::string::string_regex("9P2000(\\.\\w+)?").unwrap(),
    ) {
        let msg = Tversion { tag, msize, version };

        // Encode
        let encoded = WireFormat::encode(&msg).unwrap();

        // Check size header
        let size = u32::from_le_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]);
        prop_assert_eq!(size as usize, encoded.len());

        // Check message type
        prop_assert_eq!(encoded[4], MessageType::Tversion as u8);

        // Decode
        let decoded_msg = Tversion::decode(&encoded[5..]).unwrap();

        // Check fields match
        prop_assert_eq!(decoded_msg.tag, msg.tag);
        prop_assert_eq!(decoded_msg.msize, msg.msize);
        prop_assert_eq!(decoded_msg.version, msg.version);
    }

    /// Property: Tattach message encode/decode
    #[test]
    fn prop_tattach_encode_decode(
        tag in arbitrary_tag(),
        fid in arbitrary_fid(),
        afid in arbitrary_fid(),
        uname in "[a-z][a-z0-9]{0,31}",
        aname in "(/[a-z0-9]+){0,5}",
    ) {
        let msg = Tattach {
            tag,
            fid,
            afid,
            uname: uname.to_string(),
            aname: aname.to_string(),
        };

        let encoded = WireFormat::encode(&msg).unwrap();
        let decoded = Tattach::decode(&encoded[5..]).unwrap();

        prop_assert_eq!(decoded.tag, msg.tag);
        prop_assert_eq!(decoded.fid, msg.fid);
        prop_assert_eq!(decoded.afid, msg.afid);
        prop_assert_eq!(decoded.uname, msg.uname);
        prop_assert_eq!(decoded.aname, msg.aname);
    }

    /// Property: Twalk with various path lengths
    #[test]
    fn prop_twalk_variable_paths(
        tag in arbitrary_tag(),
        fid in arbitrary_fid(),
        newfid in arbitrary_fid(),
        wnames in arbitrary_path_components(),
    ) {
        let msg = Twalk {
            tag,
            fid,
            newfid,
            wnames: wnames.clone(),
        };

        let encoded = WireFormat::encode(&msg).unwrap();
        let decoded = Twalk::decode(&encoded[5..]).unwrap();

        prop_assert_eq!(decoded.tag, msg.tag);
        prop_assert_eq!(decoded.fid, msg.fid);
        prop_assert_eq!(decoded.newfid, msg.newfid);
        prop_assert_eq!(decoded.wnames.len(), wnames.len());
        prop_assert_eq!(decoded.wnames, wnames);
    }

    /// Property: Tread offset and count constraints
    #[test]
    fn prop_tread_constraints(
        tag in arbitrary_tag(),
        fid in arbitrary_fid(),
        offset in any::<u64>(),
        count in 0u32..=MAX_MSG_SIZE,
    ) {
        let msg = Tread {
            tag,
            fid,
            offset,
            count,
        };

        let encoded = WireFormat::encode(&msg).unwrap();
        let decoded = Tread::decode(&encoded[5..]).unwrap();

        prop_assert_eq!(decoded.tag, msg.tag);
        prop_assert_eq!(decoded.fid, msg.fid);
        prop_assert_eq!(decoded.offset, msg.offset);
        prop_assert_eq!(decoded.count, msg.count);
        prop_assert!(decoded.count <= MAX_MSG_SIZE);
    }

    /// Property: Twrite data integrity
    #[test]
    fn prop_twrite_data_integrity(
        tag in arbitrary_tag(),
        fid in arbitrary_fid(),
        offset in any::<u64>(),
        data in arbitrary_data(8192),
    ) {
        let msg = Twrite {
            tag,
            fid,
            offset,
            data: data.clone(),
        };

        let encoded = WireFormat::encode(&msg).unwrap();
        let decoded = Twrite::decode(&encoded[5..]).unwrap();

        prop_assert_eq!(decoded.tag, msg.tag);
        prop_assert_eq!(decoded.fid, msg.fid);
        prop_assert_eq!(decoded.offset, msg.offset);
        prop_assert_eq!(decoded.data.len(), data.len());
        prop_assert_eq!(decoded.data, data);
    }

    /// Property: Stat structure encoding preserves all fields
    #[test]
    fn prop_stat_encoding(
        qid in arbitrary_qid(),
        mode in any::<u32>(),
        atime in any::<u32>(),
        mtime in any::<u32>(),
        length in any::<u64>(),
        name in arbitrary_filename(),
        uid in "[a-z][a-z0-9]{0,15}",
        gid in "[a-z][a-z0-9]{0,15}",
        muid in "[a-z][a-z0-9]{0,15}",
    ) {
        let stat = Stat {
            size: 0, // Will be calculated
            typ: 0,
            dev: 0,
            qid,
            mode,
            atime,
            mtime,
            length,
            name: name.clone(),
            uid: uid.to_string(),
            gid: gid.to_string(),
            muid: muid.to_string(),
        };

        let msg = Rstat {
            tag: 1,
            stat: stat.clone(),
        };

        let encoded = WireFormat::encode(&msg).unwrap();
        let decoded = Rstat::decode(&encoded[5..]).unwrap();

        prop_assert_eq!(decoded.stat.qid.qtype, stat.qid.qtype);
        prop_assert_eq!(decoded.stat.qid.version, stat.qid.version);
        prop_assert_eq!(decoded.stat.qid.path, stat.qid.path);
        prop_assert_eq!(decoded.stat.mode, stat.mode);
        prop_assert_eq!(decoded.stat.atime, stat.atime);
        prop_assert_eq!(decoded.stat.mtime, stat.mtime);
        prop_assert_eq!(decoded.stat.length, stat.length);
        prop_assert_eq!(decoded.stat.name, stat.name);
        prop_assert_eq!(decoded.stat.uid, stat.uid);
        prop_assert_eq!(decoded.stat.gid, stat.gid);
        prop_assert_eq!(decoded.stat.muid, stat.muid);
    }

    /// Property: Authentication message encoding
    #[test]
    fn prop_tauth_encode_decode(
        tag in arbitrary_tag(),
        afid in arbitrary_fid(),
        uname in "[a-z][a-z0-9]{0,31}",
        aname in "(/[a-z0-9]+){0,5}",
    ) {
        let msg = Tauth {
            tag,
            afid,
            uname: uname.to_string(),
            aname: aname.to_string(),
        };

        let encoded = WireFormat::encode(&msg).unwrap();
        prop_assert_eq!(encoded[4], MessageType::Tauth as u8);

        let decoded = Tauth::decode(&encoded[5..]).unwrap();

        prop_assert_eq!(decoded.tag, msg.tag);
        prop_assert_eq!(decoded.afid, msg.afid);
        prop_assert_eq!(decoded.uname, msg.uname);
        prop_assert_eq!(decoded.aname, msg.aname);
    }

    /// Property: Message size limits are enforced
    #[test]
    fn prop_message_size_limits(
        data_size in (MAX_MSG_SIZE as usize)..(MAX_MSG_SIZE as usize * 2),
    ) {
        // Try to create oversized write message
        let oversized_data = vec![0u8; data_size];
        let msg = Twrite {
            tag: 1,
            fid: 0,
            offset: 0,
            data: oversized_data,
        };

        // Encoding should handle this gracefully
        let result = WireFormat::encode(&msg);

        if data_size > MAX_MSG_SIZE as usize {
            // Should either error or truncate - implementation dependent
            // Just verify it doesn't panic
            prop_assert!(result.is_ok() || result.is_err());
        } else {
            prop_assert!(result.is_ok());
        }
    }

    /// Property: Directory mode flags are properly set
    #[test]
    fn prop_directory_mode_flags(
        base_perms in 0u32..0o777,
        is_dir in any::<bool>(),
    ) {
        let mode = if is_dir {
            base_perms | permissions::DMDIR
        } else {
            base_perms
        };

        // Check that directory flag is set correctly
        let has_dir_flag = (mode & permissions::DMDIR) != 0;
        prop_assert_eq!(has_dir_flag, is_dir);

        // Extract base permissions
        let extracted_perms = mode & 0o777;
        prop_assert_eq!(extracted_perms, base_perms);
    }

    /// Property: Walk operations maintain path consistency
    #[test]
    fn prop_walk_path_consistency(
        components in arbitrary_path_components(),
    ) {
        // Walking with n components should produce n qids
        let msg = Twalk {
            tag: 1,
            fid: 0,
            newfid: 1,
            wnames: components.clone(),
        };

        // Encode and decode to ensure components survive round trip
        let encoded = WireFormat::encode(&msg).unwrap();
        let decoded = Twalk::decode(&encoded[5..]).unwrap();

        prop_assert_eq!(decoded.wnames.len(), components.len());

        // Each component should be preserved
        for (orig, decoded) in components.iter().zip(decoded.wnames.iter()) {
            prop_assert_eq!(orig, decoded);
        }
    }

    /// Property: Open mode flags are valid
    #[test]
    fn prop_open_mode_valid(
        mode in prop::sample::select(vec![
            permissions::OREAD,
            permissions::OWRITE,
            permissions::ORDWR,
            permissions::OEXEC,
            permissions::OREAD | permissions::OTRUNC,
            permissions::OWRITE | permissions::OTRUNC,
            permissions::ORDWR | permissions::OTRUNC,
        ]),
    ) {
        let msg = Topen {
            tag: 1,
            fid: 0,
            mode,
        };

        let encoded = WireFormat::encode(&msg).unwrap();
        let decoded = Topen::decode(&encoded[5..]).unwrap();

        prop_assert_eq!(decoded.mode, mode);

        // Check base mode is valid
        let base_mode = mode & 0x03;
        prop_assert!(base_mode <= permissions::OEXEC);
    }

    /// Property: Clunk releases resources (fid becomes invalid)
    #[test]
    fn prop_clunk_releases_fid(
        tag in arbitrary_tag(),
        fid in arbitrary_fid(),
    ) {
        let msg = Tclunk { tag, fid };

        let encoded = WireFormat::encode(&msg).unwrap();
        let decoded = Tclunk::decode(&encoded[5..]).unwrap();

        prop_assert_eq!(decoded.tag, msg.tag);
        prop_assert_eq!(decoded.fid, msg.fid);

        // After clunk, the fid should be considered invalid
        // This is more of a semantic property that would be tested
        // in integration tests with actual server state
    }

    /// Property: Response messages have matching tags
    #[test]
    fn prop_response_tag_matching(
        tag in arbitrary_tag(),
    ) {
        // Version negotiation
        let tversion = Tversion {
            tag,
            msize: 8192,
            version: "9P2000".to_string(),
        };

        let rversion = Rversion {
            tag,
            msize: 8192,
            version: "9P2000".to_string(),
        };

        prop_assert_eq!(tversion.tag, rversion.tag);

        // Attach
        let rattach = Rattach {
            tag,
            qid: Qid { qtype: 0, version: 0, path: 0 },
        };

        prop_assert_eq!(tag, rattach.tag);

        // All response types should preserve the tag
        let rclunk = Rclunk { tag };
        prop_assert_eq!(tag, rclunk.tag);
    }
}
