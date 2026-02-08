//! Property-based tests for 9P.e protocol messages
//! Ruthlessly validates protocol correctness based on formal specification

use proptest::prelude::*;
use quickcheck::TestResult;
use quickcheck_macros::quickcheck;
use arbitrary::Arbitrary;
use quickcheck::{Arbitrary as QCArbitrary, Gen};
use serde::{Serialize, Deserialize};

/// Core 9P.e message types that MUST satisfy protocol properties
#[derive(Debug, Clone, PartialEq, Arbitrary, Serialize, Deserialize)]
pub enum NinePMessage {
    /// Traditional 9P2000 messages (backward compatibility)
    Version { msize: u32, version: String },
    Auth { afid: u32, uname: String, aname: String },
    Attach { fid: u32, afid: u32, uname: String, aname: String },
    Walk { fid: u32, newfid: u32, wnames: Vec<String> },
    Open { fid: u32, mode: u8 },
    Create { fid: u32, name: String, perm: u32, mode: u8 },
    Read { fid: u32, offset: u64, count: u32, data: Vec<u8> },
    Write { fid: u32, offset: u64, data: Vec<u8> },
    Clunk { fid: u32 },
    Remove { fid: u32 },
    Stat { fid: u32, data: Vec<u8> },
    Wstat { fid: u32, stat: Vec<u8> },

    /// 9P.e async extensions
    StreamInit { stream_id: u32, fid: u32, mode: u8 },
    StreamData { stream_id: u32, chunk_id: u32, data: Vec<u8> },
    StreamEnd { stream_id: u32, final_chunk: u32 },
    MultiplexChannel { channel_id: u32, priority: u8 },

    /// Capability-based security
    CapabilityGrant { cap_id: u64, fid: u32, permissions: u32 },
    CapabilityRevoke { cap_id: u64 },

    /// Synthetic file operations
    SyntheticCreate { fid: u32, generator: String, params: Vec<u8> },
    SyntheticUpdate { fid: u32, new_params: Vec<u8> },

    /// Translator operations
    TranslatorSpawn { translator_id: u32, code: Vec<u8>, config: Vec<u8> },
    TranslatorMessage { translator_id: u32, data: Vec<u8> },
    TranslatorKill { translator_id: u32 },

    /// GHOSTDAG consensus
    ConsensusPropose { block_hash: [u8; 32], parent_hashes: Vec<[u8; 32]> },
    ConsensusVote { block_hash: [u8; 32], vote: bool },
    ConsensusCommit { block_hash: [u8; 32], blue_score: u64 },
}

fn string_strategy(max: usize) -> BoxedStrategy<String> {
    any::<String>()
        .prop_map(move |s| s.chars().take(max).collect::<String>())
        .boxed()
}

fn bytes_strategy(max: usize) -> BoxedStrategy<Vec<u8>> {
    proptest::collection::vec(any::<u8>(), 0..=max).boxed()
}

impl proptest::arbitrary::Arbitrary for NinePMessage {
    type Parameters = ();
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
        use proptest::array::uniform32;
        prop_oneof![
            (any::<u32>(), string_strategy(32)).prop_map(|(msize, version)| NinePMessage::Version { msize, version }),
            (any::<u32>(), string_strategy(24), string_strategy(24)).prop_map(|(afid, uname, aname)| NinePMessage::Auth { afid, uname, aname }),
            (any::<u32>(), any::<u32>(), string_strategy(24), string_strategy(24)).prop_map(|(fid, afid, uname, aname)| NinePMessage::Attach { fid, afid, uname, aname }),
            (any::<u32>(), any::<u32>(), proptest::collection::vec(string_strategy(16), 0..5)).prop_map(|(fid, newfid, wnames)| NinePMessage::Walk { fid, newfid, wnames }),
            (any::<u32>(), any::<u8>()).prop_map(|(fid, mode)| NinePMessage::Open { fid, mode }),
            (any::<u32>(), string_strategy(16), any::<u32>(), any::<u8>()).prop_map(|(fid, name, perm, mode)| NinePMessage::Create { fid, name, perm, mode }),
            (any::<u32>(), any::<u64>(), any::<u32>(), bytes_strategy(4096)).prop_map(|(fid, offset, count, data)| NinePMessage::Read { fid, offset, count, data }),
            (any::<u32>(), any::<u64>(), bytes_strategy(4096)).prop_map(|(fid, offset, data)| NinePMessage::Write { fid, offset, data }),
            any::<u32>().prop_map(|fid| NinePMessage::Clunk { fid }),
            any::<u32>().prop_map(|fid| NinePMessage::Remove { fid }),
            (any::<u32>(), bytes_strategy(512)).prop_map(|(fid, data)| NinePMessage::Stat { fid, data }),
            (any::<u32>(), bytes_strategy(512)).prop_map(|(fid, stat)| NinePMessage::Wstat { fid, stat }),
            (any::<u32>(), any::<u32>(), any::<u8>()).prop_map(|(stream_id, fid, mode)| NinePMessage::StreamInit { stream_id, fid, mode }),
            (any::<u32>(), any::<u32>(), bytes_strategy(64 * 1024)).prop_map(|(stream_id, chunk_id, data)| NinePMessage::StreamData { stream_id, chunk_id, data }),
            (any::<u32>(), any::<u32>()).prop_map(|(stream_id, final_chunk)| NinePMessage::StreamEnd { stream_id, final_chunk }),
            (any::<u32>(), any::<u8>()).prop_map(|(channel_id, priority)| NinePMessage::MultiplexChannel { channel_id, priority }),
            (any::<u64>(), any::<u32>(), any::<u32>()).prop_map(|(cap_id, fid, permissions)| NinePMessage::CapabilityGrant { cap_id, fid, permissions }),
            any::<u64>().prop_map(|cap_id| NinePMessage::CapabilityRevoke { cap_id }),
            (any::<u32>(), string_strategy(16), bytes_strategy(512)).prop_map(|(fid, generator, params)| NinePMessage::SyntheticCreate { fid, generator, params }),
            (any::<u32>(), bytes_strategy(512)).prop_map(|(fid, new_params)| NinePMessage::SyntheticUpdate { fid, new_params }),
            (any::<u32>(), bytes_strategy(1024), bytes_strategy(256)).prop_map(|(translator_id, code, config)| NinePMessage::TranslatorSpawn { translator_id, code, config }),
            (any::<u32>(), bytes_strategy(1024)).prop_map(|(translator_id, data)| NinePMessage::TranslatorMessage { translator_id, data }),
            any::<u32>().prop_map(|translator_id| NinePMessage::TranslatorKill { translator_id }),
            (uniform32(any::<u8>()), proptest::collection::vec(uniform32(any::<u8>()), 0..4)).prop_map(|(block_hash, parent_hashes)| NinePMessage::ConsensusPropose { block_hash, parent_hashes }),
            (uniform32(any::<u8>()), any::<bool>()).prop_map(|(block_hash, vote)| NinePMessage::ConsensusVote { block_hash, vote }),
            (uniform32(any::<u8>()), any::<u64>()).prop_map(|(block_hash, blue_score)| NinePMessage::ConsensusCommit { block_hash, blue_score }),
        ]
        .boxed()
    }
}

fn qc_limited_string(g: &mut Gen, max: usize) -> String {
    let mut s = <String as QCArbitrary>::arbitrary(g);
    if s.len() > max {
        s.truncate(max);
    }
    s
}

fn qc_limited_bytes(g: &mut Gen, max: usize) -> Vec<u8> {
    let len = <usize as QCArbitrary>::arbitrary(g) % (max + 1);
    (0..len).map(|_| <u8 as QCArbitrary>::arbitrary(g)).collect()
}

fn qc_hash32(g: &mut Gen) -> [u8; 32] {
    let mut arr = [0u8; 32];
    for byte in arr.iter_mut() {
        *byte = <u8 as QCArbitrary>::arbitrary(g);
    }
    arr
}

#[allow(dead_code)]
fn qc_u32(g: &mut Gen) -> u32 {
    <u32 as QCArbitrary>::arbitrary(g)
}

#[allow(dead_code)]
fn qc_u64(g: &mut Gen) -> u64 {
    <u64 as QCArbitrary>::arbitrary(g)
}

#[allow(dead_code)]
fn qc_usize(g: &mut Gen) -> usize {
    <usize as QCArbitrary>::arbitrary(g)
}

#[allow(dead_code)]
fn qc_bool(g: &mut Gen) -> bool {
    <bool as QCArbitrary>::arbitrary(g)
}

#[allow(dead_code)]
fn qc_u8(g: &mut Gen) -> u8 {
    <u8 as QCArbitrary>::arbitrary(g)
}

impl QCArbitrary for NinePMessage {
    fn arbitrary(g: &mut Gen) -> Self {
        match <usize as QCArbitrary>::arbitrary(g) % 25 {
            0 => NinePMessage::Version { msize: qc_u32(g), version: qc_limited_string(g, 32) },
            1 => NinePMessage::Auth { afid: qc_u32(g), uname: qc_limited_string(g, 24), aname: qc_limited_string(g, 24) },
            2 => NinePMessage::Attach { fid: qc_u32(g), afid: qc_u32(g), uname: qc_limited_string(g, 24), aname: qc_limited_string(g, 24) },
            3 => {
                let len = <usize as QCArbitrary>::arbitrary(g) % 4;
                let wnames = (0..len).map(|_| qc_limited_string(g, 16)).collect();
                NinePMessage::Walk { fid: qc_u32(g), newfid: qc_u32(g), wnames }
            }
            4 => NinePMessage::Open { fid: qc_u32(g), mode: qc_u8(g) },
            5 => NinePMessage::Create { fid: qc_u32(g), name: qc_limited_string(g, 16), perm: qc_u32(g), mode: qc_u8(g) },
            6 => NinePMessage::Read { fid: qc_u32(g), offset: qc_u64(g), count: qc_u32(g), data: qc_limited_bytes(g, 4096) },
            7 => NinePMessage::Write { fid: qc_u32(g), offset: qc_u64(g), data: qc_limited_bytes(g, 4096) },
            8 => NinePMessage::Clunk { fid: qc_u32(g) },
            9 => NinePMessage::Remove { fid: qc_u32(g) },
            10 => NinePMessage::Stat { fid: qc_u32(g), data: qc_limited_bytes(g, 512) },
            11 => NinePMessage::Wstat { fid: qc_u32(g), stat: qc_limited_bytes(g, 512) },
            12 => NinePMessage::StreamInit { stream_id: qc_u32(g), fid: qc_u32(g), mode: qc_u8(g) },
            13 => NinePMessage::StreamData { stream_id: qc_u32(g), chunk_id: qc_u32(g), data: qc_limited_bytes(g, 64 * 1024) },
            14 => NinePMessage::StreamEnd { stream_id: qc_u32(g), final_chunk: qc_u32(g) },
            15 => NinePMessage::MultiplexChannel { channel_id: qc_u32(g), priority: qc_u8(g) },
            16 => NinePMessage::CapabilityGrant { cap_id: qc_u64(g), fid: qc_u32(g), permissions: qc_u32(g) },
            17 => NinePMessage::CapabilityRevoke { cap_id: qc_u64(g) },
            18 => NinePMessage::SyntheticCreate { fid: qc_u32(g), generator: qc_limited_string(g, 16), params: qc_limited_bytes(g, 512) },
            19 => NinePMessage::SyntheticUpdate { fid: qc_u32(g), new_params: qc_limited_bytes(g, 512) },
            20 => NinePMessage::TranslatorSpawn { translator_id: qc_u32(g), code: qc_limited_bytes(g, 1024), config: qc_limited_bytes(g, 256) },
            21 => NinePMessage::TranslatorMessage { translator_id: qc_u32(g), data: qc_limited_bytes(g, 1024) },
            22 => NinePMessage::TranslatorKill { translator_id: qc_u32(g) },
            23 => {
                let parent_len = <usize as QCArbitrary>::arbitrary(g) % 4;
                let mut parents = Vec::with_capacity(parent_len);
                for _ in 0..parent_len {
                    parents.push(qc_hash32(g));
                }
                NinePMessage::ConsensusPropose { block_hash: qc_hash32(g), parent_hashes: parents }
            }
            24 => NinePMessage::ConsensusVote { block_hash: qc_hash32(g), vote: <bool as QCArbitrary>::arbitrary(g) },
            _ => NinePMessage::ConsensusCommit { block_hash: qc_hash32(g), blue_score: <u64 as QCArbitrary>::arbitrary(g) },
        }
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        Box::new(std::iter::empty())
    }
}

/// Message serialization/deserialization properties
#[derive(Debug, Clone)]
pub struct ProtocolProperty;

impl ProtocolProperty {
    /// THEOREM 1: Message serialization is deterministic and reversible
    pub fn serialize_deserialize_identity(msg: &NinePMessage) -> bool {
        let serialized = bincode::serialize(msg).unwrap();
        let deserialized: NinePMessage = bincode::deserialize(&serialized).unwrap();
        *msg == deserialized
    }

    /// THEOREM 2: Message size bounds are enforced
    pub fn message_size_bounds(msg: &NinePMessage) -> bool {
        let serialized = bincode::serialize(msg).unwrap();
        let size = serialized.len();

        match msg {
            // Traditional messages: ≤ 8KB
            NinePMessage::Version { .. } |
            NinePMessage::Auth { .. } |
            NinePMessage::Attach { .. } |
            NinePMessage::Walk { .. } |
            NinePMessage::Open { .. } |
            NinePMessage::Create { .. } |
            NinePMessage::Clunk { .. } |
            NinePMessage::Remove { .. } |
            NinePMessage::Stat { .. } |
            NinePMessage::Wstat { .. } => size <= 8192,

            // Data messages: ≤ 1MB
            NinePMessage::Read { .. } |
            NinePMessage::Write { .. } |
            NinePMessage::StreamData { .. } => size <= 1024 * 1024,

            // Translator code: ≤ 512KB
            NinePMessage::TranslatorSpawn { .. } => size <= 512 * 1024,

            // Other extended messages: ≤ 64KB
            _ => size <= 64 * 1024,
        }
    }

    /// THEOREM 3: Field validation constraints
    pub fn field_validation(msg: &NinePMessage) -> bool {
        match msg {
            NinePMessage::Version { msize, .. } => *msize >= 1024 && *msize <= 16 * 1024 * 1024,
            NinePMessage::Read { count, data, .. } => {
                *count <= 1024 * 1024 && data.len() <= 1024 * 1024
            }
            NinePMessage::Write { data, .. } => data.len() <= 1024 * 1024,
            NinePMessage::StreamData { data, .. } => data.len() <= 64 * 1024,
            NinePMessage::MultiplexChannel { priority, .. } => *priority <= 10,
            NinePMessage::CapabilityGrant { permissions, .. } => *permissions <= 0b111111111, // 9 bits max
            _ => true,
        }
    }

    /// THEOREM 4: Async stream ordering constraints
    pub fn stream_ordering_invariants(msgs: &[NinePMessage]) -> bool {
        let mut streams: std::collections::HashMap<u32, Vec<u32>> = std::collections::HashMap::new();

        for msg in msgs {
            match msg {
                NinePMessage::StreamInit { stream_id, .. } => {
                    if streams.contains_key(stream_id) {
                        return false; // Stream already initialized
                    }
                    streams.insert(*stream_id, vec![]);
                }
                NinePMessage::StreamData { stream_id, chunk_id, .. } => {
                    if let Some(chunks) = streams.get_mut(stream_id) {
                        if chunks.contains(chunk_id) {
                            return false; // Duplicate chunk
                        }
                        chunks.push(*chunk_id);
                    } else {
                        return false; // Stream not initialized
                    }
                }
                NinePMessage::StreamEnd { stream_id, .. } => {
                    if !streams.contains_key(stream_id) {
                        return false; // Stream not initialized
                    }
                }
                _ => {}
            }
        }

        true
    }

    /// THEOREM 5: Multiplexing channel isolation
    pub fn channel_isolation(msgs: &[NinePMessage]) -> bool {
        let mut channels: std::collections::HashSet<u32> = std::collections::HashSet::new();

        for msg in msgs {
            if let NinePMessage::MultiplexChannel { channel_id, .. } = msg {
                if channels.contains(channel_id) {
                    return false; // Channel already exists
                }
                channels.insert(*channel_id);
            }
        }

        // Maximum 256 concurrent channels per connection
        channels.len() <= 256
    }

    /// THEOREM 6: Capability permission hierarchies
    pub fn capability_hierarchies(msg: &NinePMessage) -> bool {
        if let NinePMessage::CapabilityGrant { permissions, .. } = msg {
            // Permission bits: read(1) + write(2) + execute(4) + admin(8) + create(16) + delete(32) + translate(64) + synthetic(128) + consensus(256)
            // Rule: admin implies all other permissions
            if (*permissions & 0b100000000) != 0 { // admin bit
                (*permissions & 0b011111111) == 0b011111111 // all other bits set
            } else {
                true
            }
        } else {
            true
        }
    }
}

/// QuickCheck property tests
#[quickcheck]
fn prop_serialize_deserialize_identity(msg: NinePMessage) -> bool {
    ProtocolProperty::serialize_deserialize_identity(&msg)
}

#[quickcheck]
fn prop_message_size_bounds(msg: NinePMessage) -> bool {
    ProtocolProperty::message_size_bounds(&msg)
}

#[quickcheck]
fn prop_field_validation(msg: NinePMessage) -> bool {
    ProtocolProperty::field_validation(&msg)
}

#[quickcheck]
fn prop_stream_ordering(msgs: Vec<NinePMessage>) -> TestResult {
    if msgs.len() > 100 {
        return TestResult::discard(); // Limit test size
    }
    TestResult::from_bool(ProtocolProperty::stream_ordering_invariants(&msgs))
}

#[quickcheck]
fn prop_channel_isolation(msgs: Vec<NinePMessage>) -> TestResult {
    if msgs.len() > 300 {
        return TestResult::discard(); // Limit test size
    }
    TestResult::from_bool(ProtocolProperty::channel_isolation(&msgs))
}

#[quickcheck]
fn prop_capability_hierarchies(msg: NinePMessage) -> bool {
    ProtocolProperty::capability_hierarchies(&msg)
}

/// Proptest property specifications
proptest! {
    #![proptest_config(ProptestConfig::with_cases(10000))]

    #[test]
    fn serialize_roundtrip_property(msg in any::<NinePMessage>()) {
        prop_assert!(ProtocolProperty::serialize_deserialize_identity(&msg));
    }

    #[test]
    fn message_bounds_property(msg in any::<NinePMessage>()) {
        prop_assert!(ProtocolProperty::message_size_bounds(&msg));
    }

    #[test]
    fn field_validation_property(msg in any::<NinePMessage>()) {
        prop_assert!(ProtocolProperty::field_validation(&msg));
    }

    #[test]
    fn stream_sequence_property(msgs in prop::collection::vec(any::<NinePMessage>(), 1..50)) {
        prop_assert!(ProtocolProperty::stream_ordering_invariants(&msgs));
    }

    #[test]
    fn multiplexing_bounds_property(msgs in prop::collection::vec(any::<NinePMessage>(), 1..100)) {
        prop_assert!(ProtocolProperty::channel_isolation(&msgs));
    }

    #[test]
    fn permission_consistency_property(msg in any::<NinePMessage>()) {
        prop_assert!(ProtocolProperty::capability_hierarchies(&msg));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_edge_cases() {
        // Test maximum size message
        let large_write = NinePMessage::Write {
            fid: 42,
            offset: 0,
            data: vec![0u8; 1024 * 1024], // 1MB exactly
        };
        assert!(ProtocolProperty::message_size_bounds(&large_write));

        // Test oversized message (should fail bounds)
        let oversized_write = NinePMessage::Write {
            fid: 42,
            offset: 0,
            data: vec![0u8; 1024 * 1024 + 1], // 1MB + 1 byte
        };
        assert!(!ProtocolProperty::message_size_bounds(&oversized_write));

        // Test stream initialization followed by data
        let stream_sequence = vec![
            NinePMessage::StreamInit { stream_id: 1, fid: 42, mode: 0 },
            NinePMessage::StreamData { stream_id: 1, chunk_id: 0, data: vec![1, 2, 3] },
            NinePMessage::StreamData { stream_id: 1, chunk_id: 1, data: vec![4, 5, 6] },
            NinePMessage::StreamEnd { stream_id: 1, final_chunk: 1 },
        ];
        assert!(ProtocolProperty::stream_ordering_invariants(&stream_sequence));

        // Test invalid stream sequence (data without init)
        let invalid_sequence = vec![
            NinePMessage::StreamData { stream_id: 1, chunk_id: 0, data: vec![1, 2, 3] },
        ];
        assert!(!ProtocolProperty::stream_ordering_invariants(&invalid_sequence));

        // Test admin capability with all permissions
        let admin_cap = NinePMessage::CapabilityGrant {
            cap_id: 12345,
            fid: 42,
            permissions: 0b111111111, // admin + all other bits
        };
        assert!(ProtocolProperty::capability_hierarchies(&admin_cap));

        // Test admin capability with missing permissions (should fail)
        let invalid_admin_cap = NinePMessage::CapabilityGrant {
            cap_id: 12345,
            fid: 42,
            permissions: 0b100000000, // admin bit only
        };
        assert!(!ProtocolProperty::capability_hierarchies(&invalid_admin_cap));
    }
}
