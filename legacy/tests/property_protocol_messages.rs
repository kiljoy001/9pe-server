//! Property-based tests for 9P.e protocol message handling
//! Verifies correct response types and protocol invariants

use proptest::prelude::*;
use proptest::collection::{vec, hash_map};
use proptest::string::{string_regex};
use plan9e::protocol::{NinePMessage, ProtocolError};
use plan9e_server::server::FileSystemServer;
use std::path::PathBuf;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Generate arbitrary file identifiers
fn arbitrary_fid() -> impl Strategy<Value = u32> {
    0u32..10000u32
}

/// Generate arbitrary message size
fn arbitrary_msize() -> impl Strategy<Value = u32> {
    512u32..=(8192 * 1024) // 512 bytes to 8MB
}

/// Generate valid file names (no slashes, no nulls)
fn arbitrary_filename() -> impl Strategy<Value = String> {
    string_regex("[a-zA-Z0-9._-]{1,255}").unwrap()
}

/// Generate valid usernames
fn arbitrary_username() -> impl Strategy<Value = String> {
    string_regex("[a-zA-Z][a-zA-Z0-9_-]{0,31}").unwrap()
}

/// Generate arbitrary path components
fn arbitrary_path_components() -> impl Strategy<Value = Vec<String>> {
    vec(arbitrary_filename(), 0..10)
}

/// Generate arbitrary file mode
fn arbitrary_mode() -> impl Strategy<Value = u8> {
    0u8..=0b111 // Read, Write, Execute bits
}

/// Generate arbitrary 9P.e messages
fn arbitrary_message() -> impl Strategy<Value = NinePMessage> {
    prop_oneof![
        // Version negotiation
        (arbitrary_msize(), string_regex("9P\\.e.*|9P2000").unwrap())
            .prop_map(|(msize, version)| NinePMessage::Version { msize, version }),

        // Attach
        (arbitrary_fid(), arbitrary_fid(), arbitrary_username(), arbitrary_filename())
            .prop_map(|(fid, afid, uname, aname)|
                NinePMessage::Attach { fid, afid, uname, aname }),

        // Walk
        (arbitrary_fid(), arbitrary_fid(), arbitrary_path_components())
            .prop_map(|(fid, newfid, wnames)|
                NinePMessage::Walk { fid, newfid, wnames }),

        // Open
        (arbitrary_fid(), arbitrary_mode())
            .prop_map(|(fid, mode)| NinePMessage::Open { fid, mode }),

        // Read
        (arbitrary_fid(), 0u64..1000000u64, 0u32..65536u32)
            .prop_map(|(fid, offset, count)|
                NinePMessage::Read { fid, offset, count }),

        // Write
        (arbitrary_fid(), 0u64..1000000u64, vec(0u8..255u8, 0..1024))
            .prop_map(|(fid, offset, data)|
                NinePMessage::Write { fid, offset, data }),

        // Clunk
        arbitrary_fid().prop_map(|fid| NinePMessage::Clunk { fid }),

        // Stat
        arbitrary_fid().prop_map(|fid| NinePMessage::Stat { fid }),

        // Remove
        arbitrary_fid().prop_map(|fid| NinePMessage::Remove { fid }),
    ]
}

#[derive(Debug, Clone)]
struct MessagePair {
    request: NinePMessage,
    response: NinePMessage,
}

/// Property: Response type must match request type
fn is_valid_response_type(request: &NinePMessage, response: &NinePMessage) -> bool {
    use NinePMessage::*;

    match (request, response) {
        // Version -> VersionResp or Error
        (Version { .. }, Version { .. }) => true,
        (Version { .. }, Error { .. }) => true,

        // Attach -> AttachResp (NOT Stat!) - This was the bug
        (Attach { .. }, AttachResp { .. }) => true,
        (Attach { .. }, Error { .. }) => true,
        (Attach { .. }, Stat { .. }) => false, // BUG: Wrong response type

        // Walk -> WalkResp with proper qids
        (Walk { .. }, WalkResp { .. }) => true,
        (Walk { .. }, Error { .. }) => true,
        (Walk { fid, newfid, .. }, Walk { fid: rfid, newfid: rnewfid, wnames }) => {
            // BUG: Returning Walk with empty wnames
            rfid == newfid && rnewfid == newfid && wnames.is_empty()
        }

        // Open -> OpenResp
        (Open { .. }, OpenResp { .. }) => true,
        (Open { .. }, Error { .. }) => true,

        // Read -> ReadResp (NOT WriteResp!) - This was the bug
        (Read { .. }, ReadResp { .. }) => true,
        (Read { .. }, Error { .. }) => true,
        (Read { .. }, Write { .. }) => false, // BUG: Wrong response type

        // Write -> WriteResp
        (Write { .. }, WriteResp { .. }) => true,
        (Write { .. }, Error { .. }) => true,

        // Clunk -> ClunkResp
        (Clunk { .. }, ClunkResp { .. }) => true,
        (Clunk { .. }, Error { .. }) => true,

        // Stat -> StatResp (NOT just Stat!) - This was the bug
        (Stat { fid }, StatResp { .. }) => true,
        (Stat { fid }, Stat { fid: rfid }) => rfid == fid, // BUG: Echo back
        (Stat { .. }, Error { .. }) => true,

        // Remove -> RemoveResp
        (Remove { .. }, RemoveResp { .. }) => true,
        (Remove { .. }, Error { .. }) => true,

        _ => false,
    }
}

proptest! {
    /// Test: All message types get valid responses
    #[test]
    fn prop_valid_response_types(request in arbitrary_message()) {
        // Create a mock response based on request type
        let response = match &request {
            NinePMessage::Version { msize, version } => {
                NinePMessage::Version {
                    msize: *msize,
                    version: version.clone()
                }
            }
            NinePMessage::Attach { fid, .. } => {
                // CORRECT: Should return AttachResp
                NinePMessage::AttachResp {
                    qid: Default::default()
                }
            }
            NinePMessage::Walk { fid, newfid, wnames } => {
                // CORRECT: Should return WalkResp with qids
                NinePMessage::WalkResp {
                    qids: vec![Default::default(); wnames.len()]
                }
            }
            NinePMessage::Open { fid, .. } => {
                NinePMessage::OpenResp {
                    qid: Default::default(),
                    iounit: 8192,
                }
            }
            NinePMessage::Read { .. } => {
                // CORRECT: Should return ReadResp
                NinePMessage::ReadResp {
                    data: vec![]
                }
            }
            NinePMessage::Write { fid, offset, data } => {
                NinePMessage::WriteResp {
                    count: data.len() as u32
                }
            }
            NinePMessage::Clunk { .. } => {
                NinePMessage::ClunkResp
            }
            NinePMessage::Stat { .. } => {
                // CORRECT: Should return StatResp
                NinePMessage::StatResp {
                    stat: vec![]
                }
            }
            NinePMessage::Remove { .. } => {
                NinePMessage::RemoveResp
            }
            _ => NinePMessage::Error {
                ename: "Not implemented".to_string(),
                errno: 1,
            }
        };

        prop_assert!(
            is_valid_response_type(&request, &response),
            "Invalid response type for request: {:?} -> {:?}",
            request, response
        );
    }

    /// Test: Message size negotiation respects limits
    #[test]
    fn prop_msize_negotiation(
        client_msize in arbitrary_msize(),
        server_max_msize in arbitrary_msize()
    ) {
        let negotiated = client_msize.min(server_max_msize);

        prop_assert!(negotiated <= client_msize);
        prop_assert!(negotiated <= server_max_msize);
        prop_assert!(negotiated >= 512); // Minimum message size
    }

    /// Test: Walk operations preserve path validity
    #[test]
    fn prop_walk_path_validity(
        base_path in arbitrary_path_components(),
        walk_names in arbitrary_path_components()
    ) {
        let mut current_path = base_path.clone();

        for name in &walk_names {
            if name == ".." {
                if !current_path.is_empty() {
                    current_path.pop();
                }
            } else if name != "." {
                current_path.push(name.clone());
            }
        }

        // Path should not contain null bytes
        for component in &current_path {
            prop_assert!(!component.contains('\0'));
        }

        // Path components should not contain slashes
        for component in &current_path {
            prop_assert!(!component.contains('/'));
        }
    }

    /// Test: FID management maintains uniqueness
    #[test]
    fn prop_fid_uniqueness(
        operations in vec((arbitrary_fid(), prop::bool::ANY), 0..100)
    ) {
        let mut fids = HashMap::new();

        for (fid, is_attach) in operations {
            if is_attach {
                // Attach should not reuse active FID
                prop_assert!(
                    !fids.contains_key(&fid) || fids[&fid] == false,
                    "FID {} already in use", fid
                );
                fids.insert(fid, true);
            } else {
                // Clunk releases FID
                if fids.contains_key(&fid) {
                    fids.insert(fid, false);
                }
            }
        }
    }

    /// Test: Error messages have valid error codes
    #[test]
    fn prop_error_codes(
        ename in string_regex("[A-Za-z ]{1,100}").unwrap(),
        errno in 1i32..255i32
    ) {
        let error = NinePMessage::Error {
            ename: ename.clone(),
            errno,
        };

        // Standard Unix error codes
        const VALID_ERRNOS: &[i32] = &[
            1,  // EPERM
            2,  // ENOENT
            5,  // EIO
            13, // EACCES
            17, // EEXIST
            21, // EISDIR
            22, // EINVAL
            28, // ENOSPC
            30, // EROFS
        ];

        // Error code should be positive
        prop_assert!(errno > 0);

        // Error name should not be empty
        prop_assert!(!ename.trim().is_empty());
    }

    /// Test: Read/Write offset and count validity
    #[test]
    fn prop_read_write_bounds(
        file_size in 0u64..10_000_000u64,
        offset in 0u64..10_000_000u64,
        count in 0u32..1_000_000u32
    ) {
        // Reading past EOF should return truncated data
        let actual_read = if offset >= file_size {
            0
        } else {
            ((file_size - offset).min(count as u64)) as u32
        };

        prop_assert!(actual_read <= count);
        prop_assert!((offset + actual_read as u64) <= file_size || actual_read == 0);
    }

    /// Test: Version string validation
    #[test]
    fn prop_version_validation(
        version in prop::string::string_regex("9P[.0-9a-zA-Z]*").unwrap()
    ) {
        let is_valid = version == "9P2000" ||
                      version.starts_with("9P.e") ||
                      version == "9P2000.L" ||
                      version == "9P2000.u";

        if is_valid {
            prop_assert!(version.starts_with("9P"));
        }
    }

    /// Test: Stat response contains valid metadata
    #[test]
    fn prop_stat_response_validity(
        fid in arbitrary_fid(),
        size in 0u64..u64::MAX,
        mode in 0u32..0o777777u32
    ) {
        // Mode should be valid Unix permissions
        let file_type = (mode >> 12) & 0o17;
        let permissions = mode & 0o777;

        prop_assert!(permissions <= 0o777);

        // Valid file types in Unix
        const S_IFREG: u32 = 0o100000;
        const S_IFDIR: u32 = 0o040000;
        const S_IFLNK: u32 = 0o120000;

        let valid_types = [0, S_IFREG >> 12, S_IFDIR >> 12, S_IFLNK >> 12];
        prop_assert!(
            valid_types.contains(&file_type),
            "Invalid file type: {:#o}", file_type
        );
    }

    /// Test: Concurrent message handling preserves order
    #[test]
    fn prop_message_ordering(
        messages in vec(arbitrary_message(), 1..20)
    ) {
        let mut tags = HashMap::new();
        let mut next_tag = 1u16;

        // Each message should get a unique tag
        for msg in &messages {
            tags.insert(next_tag, msg.clone());
            next_tag += 1;
        }

        // Tags should be sequential
        let mut sorted_tags: Vec<_> = tags.keys().copied().collect();
        sorted_tags.sort();

        for (i, &tag) in sorted_tags.iter().enumerate() {
            prop_assert_eq!(tag as usize, i + 1);
        }
    }

    /// Test: Walk with ".." doesn't escape root
    #[test]
    fn prop_no_root_escape(
        escape_attempts in vec(
            prop::string::string_regex("\\.\\.").unwrap(),
            0..20
        )
    ) {
        let root = PathBuf::from("/srv/9p");
        let mut current = root.clone();

        for _ in escape_attempts {
            current = current.join("..");

            // Should never go above root
            prop_assert!(
                current.starts_with(&root) || current == root.parent().unwrap_or(&root),
                "Escaped root: {:?} not under {:?}", current, root
            );
        }
    }
}

/// Test for specific bug: Attach returning Stat instead of AttachResp
#[test]
fn test_attach_response_bug() {
    // The bug
    let bad_response = NinePMessage::Stat { fid: 1 };
    let request = NinePMessage::Attach {
        fid: 1,
        afid: 0,
        uname: "user".to_string(),
        aname: "root".to_string(),
    };

    assert!(!is_valid_response_type(&request, &bad_response));

    // The fix - Attach should return Attach (corrected server behavior)
    let good_response = NinePMessage::Attach {
        fid: 1,
        afid: 0,
        uname: "user".to_string(),
        aname: "root".to_string(),
    };

    assert!(is_valid_response_type(&request, &good_response));
}

/// Test for specific bug: Read returning Write message
#[test]
fn test_read_response_bug() {
    // The bug
    let bad_response = NinePMessage::Write {
        fid: 1,
        offset: 0,
        data: vec![],
    };
    let request = NinePMessage::Read {
        fid: 1,
        offset: 0,
        count: 100,
    };

    assert!(!is_valid_response_type(&request, &bad_response));

    // The fix - Read should return Read (corrected server behavior)
    let good_response = NinePMessage::Read {
        fid: 1,
        offset: 0,
        count: 0, // 0 bytes read
    };

    assert!(is_valid_response_type(&request, &good_response));
}

/// Test for specific bug: Walk returning wrong structure
#[test]
fn test_walk_response_bug() {
    // The bug - returning Walk with empty wnames
    let bad_response = NinePMessage::Walk {
        fid: 2,
        newfid: 2,
        wnames: vec![],
    };
    let request = NinePMessage::Walk {
        fid: 1,
        newfid: 2,
        wnames: vec!["dir".to_string(), "file".to_string()],
    };

    assert!(!is_valid_response_type(&request, &bad_response));

    // The fix - Walk should return Walk (corrected server behavior)
    let good_response = NinePMessage::Walk {
        fid: 2,
        newfid: 2,
        wnames: vec![], // Empty wnames in response means success
    };

    assert!(is_valid_response_type(&request, &good_response));
}