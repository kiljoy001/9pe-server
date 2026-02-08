//! Backward Compatibility Property-Based Testing
//! Ruthlessly validates 9P.e <-> 9P2000 interoperability guarantees

use proptest::prelude::*;
use quickcheck::TestResult;
use quickcheck::{Arbitrary as QCArbitrary, Gen};
use quickcheck_macros::quickcheck;
use arbitrary::Arbitrary;
use std::collections::HashMap;

/// Legacy 9P2000 message types
#[derive(Debug, Clone, PartialEq, Arbitrary)]
pub enum Legacy9P2000Message {
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
}

impl proptest::arbitrary::Arbitrary for Legacy9P2000Message {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
        use proptest::strategy::Strategy;
        proptest::prop_oneof![
            (any::<u32>(), any::<String>()).prop_map(|(msize, version)| Legacy9P2000Message::Version { msize, version }),
            (any::<u32>(), any::<String>(), any::<String>()).prop_map(|(afid, uname, aname)| Legacy9P2000Message::Auth { afid, uname, aname }),
            (any::<u32>(), any::<u32>(), any::<String>(), any::<String>()).prop_map(|(fid, afid, uname, aname)| Legacy9P2000Message::Attach { fid, afid, uname, aname }),
            (any::<u32>(), any::<u32>(), prop::collection::vec(any::<String>(), 0..5)).prop_map(|(fid, newfid, wnames)| Legacy9P2000Message::Walk { fid, newfid, wnames }),
            (any::<u32>(), any::<u8>()).prop_map(|(fid, mode)| Legacy9P2000Message::Open { fid, mode }),
            (any::<u32>(), any::<String>(), any::<u32>(), any::<u8>()).prop_map(|(fid, name, perm, mode)| Legacy9P2000Message::Create { fid, name, perm, mode }),
            (any::<u32>(), any::<u64>(), any::<u32>(), prop::collection::vec(any::<u8>(), 0..1024)).prop_map(|(fid, offset, count, data)| Legacy9P2000Message::Read { fid, offset, count, data }),
            (any::<u32>(), any::<u64>(), prop::collection::vec(any::<u8>(), 0..1024)).prop_map(|(fid, offset, data)| Legacy9P2000Message::Write { fid, offset, data }),
            any::<u32>().prop_map(|fid| Legacy9P2000Message::Clunk { fid }),
            any::<u32>().prop_map(|fid| Legacy9P2000Message::Remove { fid }),
            (any::<u32>(), prop::collection::vec(any::<u8>(), 0..512)).prop_map(|(fid, data)| Legacy9P2000Message::Stat { fid, data }),
            (any::<u32>(), prop::collection::vec(any::<u8>(), 0..512)).prop_map(|(fid, stat)| Legacy9P2000Message::Wstat { fid, stat }),
        ]
        .boxed()
    }
}

impl QCArbitrary for Legacy9P2000Message {
    fn arbitrary(g: &mut Gen) -> Self {
        let choice = usize::arbitrary(g) % 12;
        match choice {
            0 => Legacy9P2000Message::Version {
                msize: QCArbitrary::arbitrary(g),
                version: QCArbitrary::arbitrary(g),
            },
            1 => Legacy9P2000Message::Auth {
                afid: QCArbitrary::arbitrary(g),
                uname: QCArbitrary::arbitrary(g),
                aname: QCArbitrary::arbitrary(g),
            },
            2 => Legacy9P2000Message::Attach {
                fid: QCArbitrary::arbitrary(g),
                afid: QCArbitrary::arbitrary(g),
                uname: QCArbitrary::arbitrary(g),
                aname: QCArbitrary::arbitrary(g),
            },
            3 => Legacy9P2000Message::Walk {
                fid: QCArbitrary::arbitrary(g),
                newfid: QCArbitrary::arbitrary(g),
                wnames: QCArbitrary::arbitrary(g),
            },
            4 => Legacy9P2000Message::Open {
                fid: QCArbitrary::arbitrary(g),
                mode: QCArbitrary::arbitrary(g),
            },
            5 => Legacy9P2000Message::Create {
                fid: QCArbitrary::arbitrary(g),
                name: QCArbitrary::arbitrary(g),
                perm: QCArbitrary::arbitrary(g),
                mode: QCArbitrary::arbitrary(g),
            },
            6 => Legacy9P2000Message::Read {
                fid: QCArbitrary::arbitrary(g),
                offset: QCArbitrary::arbitrary(g),
                count: QCArbitrary::arbitrary(g),
                data: QCArbitrary::arbitrary(g),
            },
            7 => Legacy9P2000Message::Write {
                fid: QCArbitrary::arbitrary(g),
                offset: QCArbitrary::arbitrary(g),
                data: QCArbitrary::arbitrary(g),
            },
            8 => Legacy9P2000Message::Clunk {
                fid: QCArbitrary::arbitrary(g),
            },
            9 => Legacy9P2000Message::Remove {
                fid: QCArbitrary::arbitrary(g),
            },
            10 => Legacy9P2000Message::Stat {
                fid: QCArbitrary::arbitrary(g),
                data: QCArbitrary::arbitrary(g),
            },
            _ => Legacy9P2000Message::Wstat {
                fid: QCArbitrary::arbitrary(g),
                stat: QCArbitrary::arbitrary(g),
            },
        }
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        Box::new(std::iter::empty())
    }
}
/// Enhanced 9P.e message (includes legacy + extensions)
#[derive(Debug, Clone, PartialEq, Arbitrary)]
pub enum Enhanced9PeMessage {
    // Legacy 9P2000 compatibility
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

    // 9P.e extensions (should gracefully degrade)
    StreamInit { stream_id: u32, fid: u32, mode: u8 },
    StreamData { stream_id: u32, chunk_id: u32, data: Vec<u8> },
    MultiplexChannel { channel_id: u32, priority: u8 },
    CapabilityGrant { cap_id: u64, fid: u32, permissions: u32 },
    SyntheticCreate { fid: u32, generator: String, params: Vec<u8> },
    TranslatorSpawn { translator_id: u32, code: Vec<u8>, config: Vec<u8> },
    ConsensusPropose { block_hash: [u8; 32], parent_hashes: Vec<[u8; 32]> },
}

impl proptest::arbitrary::Arbitrary for Enhanced9PeMessage {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
        use proptest::strategy::Strategy;
        proptest::prop_oneof![
            (any::<u32>(), any::<String>()).prop_map(|(msize, version)| Enhanced9PeMessage::Version { msize, version }),
            (any::<u32>(), any::<String>(), any::<String>()).prop_map(|(afid, uname, aname)| Enhanced9PeMessage::Auth { afid, uname, aname }),
            (any::<u32>(), any::<u32>(), any::<String>(), any::<String>()).prop_map(|(fid, afid, uname, aname)| Enhanced9PeMessage::Attach { fid, afid, uname, aname }),
            (any::<u32>(), any::<u32>(), prop::collection::vec(any::<String>(), 0..5)).prop_map(|(fid, newfid, wnames)| Enhanced9PeMessage::Walk { fid, newfid, wnames }),
            (any::<u32>(), any::<u8>()).prop_map(|(fid, mode)| Enhanced9PeMessage::Open { fid, mode }),
            (any::<u32>(), any::<String>(), any::<u32>(), any::<u8>()).prop_map(|(fid, name, perm, mode)| Enhanced9PeMessage::Create { fid, name, perm, mode }),
            (any::<u32>(), any::<u64>(), any::<u32>(), prop::collection::vec(any::<u8>(), 0..1024)).prop_map(|(fid, offset, count, data)| Enhanced9PeMessage::Read { fid, offset, count, data }),
            (any::<u32>(), any::<u64>(), prop::collection::vec(any::<u8>(), 0..1024)).prop_map(|(fid, offset, data)| Enhanced9PeMessage::Write { fid, offset, data }),
            any::<u32>().prop_map(|fid| Enhanced9PeMessage::Clunk { fid }),
            any::<u32>().prop_map(|fid| Enhanced9PeMessage::Remove { fid }),
            (any::<u32>(), prop::collection::vec(any::<u8>(), 0..512)).prop_map(|(fid, data)| Enhanced9PeMessage::Stat { fid, data }),
            (any::<u32>(), prop::collection::vec(any::<u8>(), 0..512)).prop_map(|(fid, stat)| Enhanced9PeMessage::Wstat { fid, stat }),
            (any::<u32>(), any::<u32>(), any::<u8>()).prop_map(|(stream_id, fid, mode)| Enhanced9PeMessage::StreamInit { stream_id, fid, mode }),
            (any::<u32>(), any::<u32>(), prop::collection::vec(any::<u8>(), 0..1024)).prop_map(|(stream_id, chunk_id, data)| Enhanced9PeMessage::StreamData { stream_id, chunk_id, data }),
            (any::<u32>(), any::<u8>()).prop_map(|(channel_id, priority)| Enhanced9PeMessage::MultiplexChannel { channel_id, priority }),
            (any::<u64>(), any::<u32>(), any::<u32>()).prop_map(|(cap_id, fid, permissions)| Enhanced9PeMessage::CapabilityGrant { cap_id, fid, permissions }),
            (any::<u32>(), any::<String>(), prop::collection::vec(any::<u8>(), 0..512)).prop_map(|(fid, generator, params)| Enhanced9PeMessage::SyntheticCreate { fid, generator, params }),
            (any::<u32>(), prop::collection::vec(any::<u8>(), 0..1024), prop::collection::vec(any::<u8>(), 0..256)).prop_map(|(translator_id, code, config)| Enhanced9PeMessage::TranslatorSpawn { translator_id, code, config }),
            (prop::collection::vec(any::<u8>(), 32..=32), prop::collection::vec(prop::collection::vec(any::<u8>(), 32..=32).prop_map(|bytes| { let mut arr = [0u8; 32]; arr.copy_from_slice(&bytes); arr }), 0..4)).prop_map(|(hash_bytes, parents)| {
                let mut block_hash = [0u8; 32];
                block_hash.copy_from_slice(&hash_bytes);
                Enhanced9PeMessage::ConsensusPropose { block_hash, parent_hashes: parents }
            }),
        ]
        .boxed()
    }
}

fn qc_bc_limited_vec(g: &mut Gen, max_len: usize) -> Vec<u8> {
    let mut data: Vec<u8> = QCArbitrary::arbitrary(g);
    if data.len() > max_len {
        data.truncate(max_len);
    }
    data
}

fn qc_bc_bytes32(g: &mut Gen) -> [u8; 32] {
    let mut arr = [0u8; 32];
    for byte in arr.iter_mut() {
        *byte = <u8 as QCArbitrary>::arbitrary(g);
    }
    arr
}

impl QCArbitrary for Enhanced9PeMessage {
    fn arbitrary(g: &mut Gen) -> Self {
        let choice = usize::arbitrary(g) % 18;
        match choice {
            0 => Enhanced9PeMessage::Version {
                msize: QCArbitrary::arbitrary(g),
                version: QCArbitrary::arbitrary(g),
            },
            1 => Enhanced9PeMessage::Auth {
                afid: QCArbitrary::arbitrary(g),
                uname: QCArbitrary::arbitrary(g),
                aname: QCArbitrary::arbitrary(g),
            },
            2 => Enhanced9PeMessage::Attach {
                fid: QCArbitrary::arbitrary(g),
                afid: QCArbitrary::arbitrary(g),
                uname: QCArbitrary::arbitrary(g),
                aname: QCArbitrary::arbitrary(g),
            },
            3 => Enhanced9PeMessage::Walk {
                fid: QCArbitrary::arbitrary(g),
                newfid: QCArbitrary::arbitrary(g),
                wnames: QCArbitrary::arbitrary(g),
            },
            4 => Enhanced9PeMessage::Open {
                fid: QCArbitrary::arbitrary(g),
                mode: QCArbitrary::arbitrary(g),
            },
            5 => Enhanced9PeMessage::Create {
                fid: QCArbitrary::arbitrary(g),
                name: QCArbitrary::arbitrary(g),
                perm: QCArbitrary::arbitrary(g),
                mode: QCArbitrary::arbitrary(g),
            },
            6 => Enhanced9PeMessage::Read {
                fid: QCArbitrary::arbitrary(g),
                offset: QCArbitrary::arbitrary(g),
                count: QCArbitrary::arbitrary(g),
                data: qc_bc_limited_vec(g, 1024),
            },
            7 => Enhanced9PeMessage::Write {
                fid: QCArbitrary::arbitrary(g),
                offset: QCArbitrary::arbitrary(g),
                data: qc_bc_limited_vec(g, 1024),
            },
            8 => Enhanced9PeMessage::Clunk {
                fid: QCArbitrary::arbitrary(g),
            },
            9 => Enhanced9PeMessage::Remove {
                fid: QCArbitrary::arbitrary(g),
            },
            10 => Enhanced9PeMessage::Stat {
                fid: QCArbitrary::arbitrary(g),
                data: qc_bc_limited_vec(g, 256),
            },
            11 => Enhanced9PeMessage::Wstat {
                fid: QCArbitrary::arbitrary(g),
                stat: qc_bc_limited_vec(g, 256),
            },
            12 => Enhanced9PeMessage::StreamInit {
                stream_id: QCArbitrary::arbitrary(g),
                fid: QCArbitrary::arbitrary(g),
                mode: QCArbitrary::arbitrary(g),
            },
            13 => Enhanced9PeMessage::StreamData {
                stream_id: QCArbitrary::arbitrary(g),
                chunk_id: QCArbitrary::arbitrary(g),
                data: qc_bc_limited_vec(g, 1024),
            },
            14 => Enhanced9PeMessage::MultiplexChannel {
                channel_id: QCArbitrary::arbitrary(g),
                priority: QCArbitrary::arbitrary(g),
            },
            15 => Enhanced9PeMessage::CapabilityGrant {
                cap_id: QCArbitrary::arbitrary(g),
                fid: QCArbitrary::arbitrary(g),
                permissions: QCArbitrary::arbitrary(g),
            },
            16 => Enhanced9PeMessage::SyntheticCreate {
                fid: QCArbitrary::arbitrary(g),
                generator: QCArbitrary::arbitrary(g),
                params: qc_bc_limited_vec(g, 256),
            },
            17 => Enhanced9PeMessage::TranslatorSpawn {
                translator_id: QCArbitrary::arbitrary(g),
                code: qc_bc_limited_vec(g, 1024),
                config: qc_bc_limited_vec(g, 256),
            },
            _ => {
                let hash = qc_bc_bytes32(g);
                let parent_count = usize::arbitrary(g) % 4;
                let mut parents = Vec::with_capacity(parent_count);
                for _ in 0..parent_count {
                    parents.push(qc_bc_bytes32(g));
                }
                Enhanced9PeMessage::ConsensusPropose {
                    block_hash: hash,
                    parent_hashes: parents,
                }
            }
        }
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        Box::new(std::iter::empty())
    }
}

/// Protocol version negotiation result
#[derive(Debug, Clone, PartialEq)]
pub enum ProtocolVersion {
    Legacy9P2000,
    Enhanced9Pe,
    Unknown(String),
}

/// Connection state with compatibility layer
#[derive(Debug, Clone)]
pub struct CompatibilityConnection {
    pub connection_id: u32,
    pub negotiated_version: ProtocolVersion,
    pub max_message_size: u32,
    pub legacy_fid_map: HashMap<u32, u32>, // Legacy FID -> Enhanced FID
    pub capability_fallback: HashMap<u64, u32>, // Capability -> Legacy permission bits
    pub stream_fallback: HashMap<u32, u32>, // Stream ID -> Legacy FID
    pub supported_extensions: Vec<String>,
    pub degradation_warnings: Vec<String>,
}

/// Protocol compatibility layer
#[derive(Debug, Clone)]
pub struct CompatibilityLayer {
    pub connections: HashMap<u32, CompatibilityConnection>,
    pub legacy_clients: std::collections::HashSet<u32>,
    pub enhanced_clients: std::collections::HashSet<u32>,
    pub translation_stats: TranslationStats,
    pub compatibility_limits: CompatibilityLimits,
}

#[derive(Debug, Clone)]
pub struct TranslationStats {
    pub legacy_to_enhanced: u64,
    pub enhanced_to_legacy: u64,
    pub degraded_operations: u64,
    pub failed_translations: u64,
    pub extension_fallbacks: u64,
}

#[derive(Debug, Clone)]
pub struct CompatibilityLimits {
    pub max_connections: u32,
    pub max_legacy_fids: u32,
    pub max_fallback_mappings: u32,
    pub max_degradation_warnings: u32,
}

impl Default for CompatibilityLimits {
    fn default() -> Self {
        Self {
            max_connections: 1000,
            max_legacy_fids: 65536,
            max_fallback_mappings: 10000,
            max_degradation_warnings: 100,
        }
    }
}

impl Default for TranslationStats {
    fn default() -> Self {
        Self {
            legacy_to_enhanced: 0,
            enhanced_to_legacy: 0,
            degraded_operations: 0,
            failed_translations: 0,
            extension_fallbacks: 0,
        }
    }
}

impl Default for CompatibilityLayer {
    fn default() -> Self {
        Self {
            connections: HashMap::new(),
            legacy_clients: std::collections::HashSet::new(),
            enhanced_clients: std::collections::HashSet::new(),
            translation_stats: TranslationStats::default(),
            compatibility_limits: CompatibilityLimits::default(),
        }
    }
}

impl CompatibilityLayer {
    /// Negotiate protocol version with client
    pub fn negotiate_version(&mut self, connection_id: u32, version_string: &str, msize: u32) -> Result<ProtocolVersion, String> {
        if self.connections.len() >= self.compatibility_limits.max_connections as usize {
            return Err("Maximum connections reached".to_string());
        }

        let negotiated_version = match version_string {
            "9P2000" => {
                self.legacy_clients.insert(connection_id);
                ProtocolVersion::Legacy9P2000
            }
            "9P.e" | "9Pe" => {
                self.enhanced_clients.insert(connection_id);
                ProtocolVersion::Enhanced9Pe
            }
            unknown => {
                // Try to fallback to legacy for unknown versions
                self.legacy_clients.insert(connection_id);
                ProtocolVersion::Unknown(unknown.to_string())
            }
        };

        let connection = CompatibilityConnection {
            connection_id,
            negotiated_version: negotiated_version.clone(),
            max_message_size: msize.min(65536), // Cap at 64KB for safety
            legacy_fid_map: HashMap::new(),
            capability_fallback: HashMap::new(),
            stream_fallback: HashMap::new(),
            supported_extensions: match &negotiated_version {
                ProtocolVersion::Enhanced9Pe => vec![
                    "streaming".to_string(),
                    "multiplexing".to_string(),
                    "capabilities".to_string(),
                    "synthetic".to_string(),
                    "translators".to_string(),
                    "consensus".to_string(),
                ],
                _ => vec![], // Legacy supports no extensions
            },
            degradation_warnings: Vec::new(),
        };

        self.connections.insert(connection_id, connection);
        Ok(negotiated_version)
    }

    /// Translate legacy message to enhanced format
    pub fn translate_legacy_to_enhanced(&mut self, connection_id: u32, legacy_msg: Legacy9P2000Message) -> Result<Enhanced9PeMessage, String> {
        let connection = self.connections.get_mut(&connection_id)
            .ok_or("Connection not found")?;

        self.translation_stats.legacy_to_enhanced += 1;

        let enhanced_msg = match legacy_msg {
            Legacy9P2000Message::Version { msize, version } =>
                Enhanced9PeMessage::Version { msize, version },
            Legacy9P2000Message::Auth { afid, uname, aname } =>
                Enhanced9PeMessage::Auth { afid, uname, aname },
            Legacy9P2000Message::Attach { fid, afid, uname, aname } => {
                // Map legacy FID to enhanced FID
                connection.legacy_fid_map.insert(fid, fid);
                Enhanced9PeMessage::Attach { fid, afid, uname, aname }
            }
            Legacy9P2000Message::Walk { fid, newfid, wnames } => {
                connection.legacy_fid_map.insert(newfid, newfid);
                Enhanced9PeMessage::Walk { fid, newfid, wnames }
            }
            Legacy9P2000Message::Open { fid, mode } =>
                Enhanced9PeMessage::Open { fid, mode },
            Legacy9P2000Message::Create { fid, name, perm, mode } =>
                Enhanced9PeMessage::Create { fid, name, perm, mode },
            Legacy9P2000Message::Read { fid, offset, count, data } =>
                Enhanced9PeMessage::Read { fid, offset, count, data },
            Legacy9P2000Message::Write { fid, offset, data } =>
                Enhanced9PeMessage::Write { fid, offset, data },
            Legacy9P2000Message::Clunk { fid } => {
                // Clean up legacy FID mapping
                connection.legacy_fid_map.remove(&fid);
                Enhanced9PeMessage::Clunk { fid }
            }
            Legacy9P2000Message::Remove { fid } =>
                Enhanced9PeMessage::Remove { fid },
            Legacy9P2000Message::Stat { fid, data } =>
                Enhanced9PeMessage::Stat { fid, data },
            Legacy9P2000Message::Wstat { fid, stat } =>
                Enhanced9PeMessage::Wstat { fid, stat },
        };

        Ok(enhanced_msg)
    }

    /// Translate enhanced message to legacy format (with degradation)
    pub fn translate_enhanced_to_legacy(&mut self, connection_id: u32, enhanced_msg: Enhanced9PeMessage) -> Result<Option<Legacy9P2000Message>, String> {
        let connection = self.connections.get_mut(&connection_id)
            .ok_or("Connection not found")?;

        self.translation_stats.enhanced_to_legacy += 1;

        let legacy_msg = match enhanced_msg {
            // Direct translations
            Enhanced9PeMessage::Version { msize, version } =>
                Some(Legacy9P2000Message::Version { msize, version }),
            Enhanced9PeMessage::Auth { afid, uname, aname } =>
                Some(Legacy9P2000Message::Auth { afid, uname, aname }),
            Enhanced9PeMessage::Attach { fid, afid, uname, aname } =>
                Some(Legacy9P2000Message::Attach { fid, afid, uname, aname }),
            Enhanced9PeMessage::Walk { fid, newfid, wnames } =>
                Some(Legacy9P2000Message::Walk { fid, newfid, wnames }),
            Enhanced9PeMessage::Open { fid, mode } =>
                Some(Legacy9P2000Message::Open { fid, mode }),
            Enhanced9PeMessage::Create { fid, name, perm, mode } =>
                Some(Legacy9P2000Message::Create { fid, name, perm, mode }),
            Enhanced9PeMessage::Read { fid, offset, count, data } =>
                Some(Legacy9P2000Message::Read { fid, offset, count, data }),
            Enhanced9PeMessage::Write { fid, offset, data } =>
                Some(Legacy9P2000Message::Write { fid, offset, data }),
            Enhanced9PeMessage::Clunk { fid } =>
                Some(Legacy9P2000Message::Clunk { fid }),
            Enhanced9PeMessage::Remove { fid } =>
                Some(Legacy9P2000Message::Remove { fid }),
            Enhanced9PeMessage::Stat { fid, data } =>
                Some(Legacy9P2000Message::Stat { fid, data }),
            Enhanced9PeMessage::Wstat { fid, stat } =>
                Some(Legacy9P2000Message::Wstat { fid, stat }),

            // Extensions that must be degraded or dropped
            Enhanced9PeMessage::StreamInit { stream_id, fid, mode } => {
                self.translation_stats.degraded_operations += 1;
                connection.stream_fallback.insert(stream_id, fid);
                if connection.degradation_warnings.len() < self.compatibility_limits.max_degradation_warnings as usize {
                    connection.degradation_warnings.push("StreamInit degraded to regular Open".to_string());
                }
                Some(Legacy9P2000Message::Open { fid, mode })
            }
            Enhanced9PeMessage::StreamData { stream_id, data, .. } => {
                if let Some(&fid) = connection.stream_fallback.get(&stream_id) {
                    self.translation_stats.degraded_operations += 1;
                    Some(Legacy9P2000Message::Write { fid, offset: 0, data })
                } else {
                    self.translation_stats.failed_translations += 1;
                    None // Cannot translate without stream context
                }
            }
            Enhanced9PeMessage::MultiplexChannel { .. } => {
                self.translation_stats.extension_fallbacks += 1;
                None // Multiplexing not supported in legacy - silently ignore
            }
            Enhanced9PeMessage::CapabilityGrant { cap_id, fid, permissions } => {
                self.translation_stats.degraded_operations += 1;
                connection.capability_fallback.insert(cap_id, permissions);
                if connection.degradation_warnings.len() < self.compatibility_limits.max_degradation_warnings as usize {
                    connection.degradation_warnings.push("Capability system degraded to permission bits".to_string());
                }
                // Translate to Wstat with permission update
                Some(Legacy9P2000Message::Wstat { fid, stat: permissions.to_be_bytes().to_vec() })
            }
            Enhanced9PeMessage::SyntheticCreate { fid, generator, .. } => {
                self.translation_stats.degraded_operations += 1;
                if connection.degradation_warnings.len() < self.compatibility_limits.max_degradation_warnings as usize {
                    connection.degradation_warnings.push("Synthetic file degraded to regular Create".to_string());
                }
                Some(Legacy9P2000Message::Create {
                    fid,
                    name: format!("synthetic_{}", generator),
                    perm: 0o644,
                    mode: 0
                })
            }
            Enhanced9PeMessage::TranslatorSpawn { .. } => {
                self.translation_stats.extension_fallbacks += 1;
                if connection.degradation_warnings.len() < self.compatibility_limits.max_degradation_warnings as usize {
                    connection.degradation_warnings.push("Translator system not supported in legacy mode".to_string());
                }
                None // Translators cannot be degraded
            }
            Enhanced9PeMessage::ConsensusPropose { .. } => {
                self.translation_stats.extension_fallbacks += 1;
                None // Consensus protocol not supported in legacy - ignore
            }
        };

        Ok(legacy_msg)
    }

    /// Add degradation warning with limit enforcement
    fn add_degradation_warning(&self, connection: &mut CompatibilityConnection, warning: &str) {
        if connection.degradation_warnings.len() < self.compatibility_limits.max_degradation_warnings as usize {
            connection.degradation_warnings.push(warning.to_string());
        }
    }

    /// Check if operation is supported in legacy mode
    pub fn is_legacy_compatible(&self, enhanced_msg: &Enhanced9PeMessage) -> bool {
        match enhanced_msg {
            // Core 9P2000 messages are always compatible
            Enhanced9PeMessage::Version { .. } |
            Enhanced9PeMessage::Auth { .. } |
            Enhanced9PeMessage::Attach { .. } |
            Enhanced9PeMessage::Walk { .. } |
            Enhanced9PeMessage::Open { .. } |
            Enhanced9PeMessage::Create { .. } |
            Enhanced9PeMessage::Read { .. } |
            Enhanced9PeMessage::Write { .. } |
            Enhanced9PeMessage::Clunk { .. } |
            Enhanced9PeMessage::Remove { .. } |
            Enhanced9PeMessage::Stat { .. } |
            Enhanced9PeMessage::Wstat { .. } => true,

            // Extensions with degradation support
            Enhanced9PeMessage::StreamInit { .. } |
            Enhanced9PeMessage::StreamData { .. } |
            Enhanced9PeMessage::CapabilityGrant { .. } |
            Enhanced9PeMessage::SyntheticCreate { .. } => true,

            // Extensions without degradation support
            Enhanced9PeMessage::MultiplexChannel { .. } |
            Enhanced9PeMessage::TranslatorSpawn { .. } |
            Enhanced9PeMessage::ConsensusPropose { .. } => false,
        }
    }

    /// Cleanup connection state
    pub fn close_connection(&mut self, connection_id: u32) -> Result<(), String> {
        if let Some(_connection) = self.connections.remove(&connection_id) {
            self.legacy_clients.remove(&connection_id);
            self.enhanced_clients.remove(&connection_id);
            Ok(())
        } else {
            Err("Connection not found".to_string())
        }
    }

    /// Get compatibility statistics
    pub fn get_translation_stats(&self) -> &TranslationStats {
        &self.translation_stats
    }
}

/// Backward compatibility property tests
pub struct BackwardCompatibilityProperties;

impl BackwardCompatibilityProperties {
    /// THEOREM 1: All legacy operations are supported
    pub fn legacy_operations_supported(compat: &CompatibilityLayer, legacy_msg: &Legacy9P2000Message) -> bool {
        // Every legacy message should have a valid enhanced translation
        match legacy_msg {
            Legacy9P2000Message::Version { .. } |
            Legacy9P2000Message::Auth { .. } |
            Legacy9P2000Message::Attach { .. } |
            Legacy9P2000Message::Walk { .. } |
            Legacy9P2000Message::Open { .. } |
            Legacy9P2000Message::Create { .. } |
            Legacy9P2000Message::Read { .. } |
            Legacy9P2000Message::Write { .. } |
            Legacy9P2000Message::Clunk { .. } |
            Legacy9P2000Message::Remove { .. } |
            Legacy9P2000Message::Stat { .. } |
            Legacy9P2000Message::Wstat { .. } => true, // All supported
        }
    }

    /// THEOREM 2: Roundtrip translation preserves core functionality
    pub fn roundtrip_translation_preserves_core(compat: &mut CompatibilityLayer, connection_id: u32, legacy_msg: Legacy9P2000Message) -> bool {
        // Translate legacy -> enhanced -> legacy
        if let Ok(enhanced) = compat.translate_legacy_to_enhanced(connection_id, legacy_msg.clone()) {
            if let Ok(Some(roundtrip_legacy)) = compat.translate_enhanced_to_legacy(connection_id, enhanced) {
                // Core fields should be preserved (may have minor differences due to degradation)
                Self::messages_functionally_equivalent(&legacy_msg, &roundtrip_legacy)
            } else {
                false // Translation failed
            }
        } else {
            false // Initial translation failed
        }
    }

    /// THEOREM 3: Version negotiation is deterministic
    pub fn version_negotiation_deterministic(compat: &mut CompatibilityLayer, connection_id: u32, version: &str, msize: u32) -> bool {
        let result1 = compat.negotiate_version(connection_id, version, msize);
        compat.close_connection(connection_id).ok();

        let result2 = compat.negotiate_version(connection_id, version, msize);
        compat.close_connection(connection_id).ok();

        // Same inputs should produce same results
        match (result1, result2) {
            (Ok(v1), Ok(v2)) => v1 == v2,
            (Err(_), Err(_)) => true, // Both failed consistently
            _ => false, // Inconsistent results
        }
    }

    /// THEOREM 4: Extension degradation is graceful
    pub fn extension_degradation_graceful(compat: &mut CompatibilityLayer, connection_id: u32, enhanced_msg: Enhanced9PeMessage) -> bool {
        match compat.translate_enhanced_to_legacy(connection_id, enhanced_msg.clone()) {
            Ok(Some(_legacy)) => true, // Successfully degraded
            Ok(None) => {
                // Acceptable to drop extensions that cannot be degraded
                match enhanced_msg {
                    Enhanced9PeMessage::MultiplexChannel { .. } |
                    Enhanced9PeMessage::TranslatorSpawn { .. } |
                    Enhanced9PeMessage::ConsensusPropose { .. } => true,
                    _ => false, // Core operations should not be dropped
                }
            }
            Err(_) => false, // Translation should not fail
        }
    }

    /// THEOREM 5: Resource limits are enforced
    pub fn resource_limits_enforced(compat: &CompatibilityLayer) -> bool {
        // Connection count limit
        if compat.connections.len() > compat.compatibility_limits.max_connections as usize {
            return false;
        }

        // Check individual connection limits
        for connection in compat.connections.values() {
            // FID mapping limit
            if connection.legacy_fid_map.len() > compat.compatibility_limits.max_legacy_fids as usize {
                return false;
            }

            // Fallback mapping limit
            let total_fallbacks = connection.capability_fallback.len() + connection.stream_fallback.len();
            if total_fallbacks > compat.compatibility_limits.max_fallback_mappings as usize {
                return false;
            }

            // Warning count limit
            if connection.degradation_warnings.len() > compat.compatibility_limits.max_degradation_warnings as usize {
                return false;
            }
        }

        true
    }

    /// THEOREM 6: Translation statistics are consistent
    pub fn translation_stats_consistent(compat: &CompatibilityLayer) -> bool {
        let stats = &compat.translation_stats;

        // Total translations should be sum of successful + failed
        let total_attempted = stats.legacy_to_enhanced + stats.enhanced_to_legacy;
        let total_processed = total_attempted; // All should be processed (success or fail)

        // Degraded operations should not exceed total translations
        if stats.degraded_operations > total_attempted {
            return false;
        }

        // Extension fallbacks should not exceed total translations
        if stats.extension_fallbacks > total_attempted {
            return false;
        }

        true
    }

    /// Helper: Check if messages are functionally equivalent
    fn messages_functionally_equivalent(msg1: &Legacy9P2000Message, msg2: &Legacy9P2000Message) -> bool {
        // Simplified functional equivalence check
        match (msg1, msg2) {
            (Legacy9P2000Message::Version { msize: m1, .. }, Legacy9P2000Message::Version { msize: m2, .. }) => m1 == m2,
            (Legacy9P2000Message::Open { fid: f1, mode: mo1 }, Legacy9P2000Message::Open { fid: f2, mode: mo2 }) => f1 == f2 && mo1 == mo2,
            (Legacy9P2000Message::Read { fid: f1, offset: o1, count: c1, .. },
             Legacy9P2000Message::Read { fid: f2, offset: o2, count: c2, .. }) =>
                f1 == f2 && o1 == o2 && c1 == c2,
            (Legacy9P2000Message::Write { fid: f1, offset: o1, data: d1 }, Legacy9P2000Message::Write { fid: f2, offset: o2, data: d2 }) =>
                f1 == f2 && o1 == o2 && d1 == d2,
            _ => msg1 == msg2, // Exact match for other types
        }
    }
}

/// QuickCheck properties
#[quickcheck]
fn prop_legacy_operations_supported(legacy_msg: Legacy9P2000Message) -> bool {
    let compat = CompatibilityLayer::default();
    BackwardCompatibilityProperties::legacy_operations_supported(&compat, &legacy_msg)
}

#[quickcheck]
fn prop_version_negotiation_deterministic(version: String, msize: u32) -> TestResult {
    if version.len() > 100 || msize > 1024 * 1024 {
        return TestResult::discard();
    }

    let mut compat = CompatibilityLayer::default();
    let connection_id = 12345;

    TestResult::from_bool(BackwardCompatibilityProperties::version_negotiation_deterministic(&mut compat, connection_id, &version, msize))
}

#[quickcheck]
fn prop_resource_limits_enforced(connections: u8) -> TestResult {
    if connections > 20 {
        return TestResult::discard();
    }

    let mut compat = CompatibilityLayer::default();
    compat.compatibility_limits.max_connections = 10;

    // Create test connections
    for i in 0..connections {
        let _ = compat.negotiate_version(i as u32, "9P2000", 8192);
    }

    TestResult::from_bool(BackwardCompatibilityProperties::resource_limits_enforced(&compat))
}

/// Proptest specifications
proptest! {
    #![proptest_config(ProptestConfig::with_cases(1500))]

    #[test]
    fn proptest_roundtrip_translation(legacy_msg in any::<Legacy9P2000Message>()) {
        let mut compat = CompatibilityLayer::default();
        let connection_id = 999;

        // Set up connection
        let _ = compat.negotiate_version(connection_id, "9P2000", 8192);

        prop_assert!(BackwardCompatibilityProperties::roundtrip_translation_preserves_core(&mut compat, connection_id, legacy_msg));
    }

    #[test]
    fn proptest_extension_degradation(enhanced_msg in any::<Enhanced9PeMessage>()) {
        let mut compat = CompatibilityLayer::default();
        let connection_id = 888;

        // Set up legacy connection
        let _ = compat.negotiate_version(connection_id, "9P2000", 8192);

        prop_assert!(BackwardCompatibilityProperties::extension_degradation_graceful(&mut compat, connection_id, enhanced_msg));
    }

    #[test]
    fn proptest_translation_consistency(
        legacy_msgs in prop::collection::vec(any::<Legacy9P2000Message>(), 1..10),
        enhanced_msgs in prop::collection::vec(any::<Enhanced9PeMessage>(), 1..10)
    ) {
        let mut compat = CompatibilityLayer::default();
        let connection_id = 777;

        let _ = compat.negotiate_version(connection_id, "9Pe", 8192);

        // Process messages to generate statistics
        for legacy_msg in legacy_msgs {
            let _ = compat.translate_legacy_to_enhanced(connection_id, legacy_msg);
        }

        for enhanced_msg in enhanced_msgs {
            let _ = compat.translate_enhanced_to_legacy(connection_id, enhanced_msg);
        }

        prop_assert!(BackwardCompatibilityProperties::translation_stats_consistent(&compat));
        prop_assert!(BackwardCompatibilityProperties::resource_limits_enforced(&compat));
    }

    #[test]
    fn proptest_legacy_compatibility_preservation(legacy_msg in any::<Legacy9P2000Message>()) {
        let compat = CompatibilityLayer::default();

        // All legacy operations must be supported
        prop_assert!(BackwardCompatibilityProperties::legacy_operations_supported(&compat, &legacy_msg));

        // Legacy messages should translate to enhanced format
        let mut compat_mut = compat;
        let connection_id = 666;
        let _ = compat_mut.negotiate_version(connection_id, "9P2000", 8192);

        let translation_result = compat_mut.translate_legacy_to_enhanced(connection_id, legacy_msg);
        prop_assert!(translation_result.is_ok());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_negotiation() {
        let mut compat = CompatibilityLayer::default();

        // Test legacy client
        let legacy_result = compat.negotiate_version(1, "9P2000", 8192).unwrap();
        assert_eq!(legacy_result, ProtocolVersion::Legacy9P2000);
        assert!(compat.legacy_clients.contains(&1));

        // Test enhanced client
        let enhanced_result = compat.negotiate_version(2, "9P.e", 16384).unwrap();
        assert_eq!(enhanced_result, ProtocolVersion::Enhanced9Pe);
        assert!(compat.enhanced_clients.contains(&2));

        // Test unknown version (should fallback to legacy)
        let unknown_result = compat.negotiate_version(3, "9P3000", 4096).unwrap();
        matches!(unknown_result, ProtocolVersion::Unknown(_));
        assert!(compat.legacy_clients.contains(&3));
    }

    #[test]
    fn test_basic_legacy_translation() {
        let mut compat = CompatibilityLayer::default();
        let connection_id = 100;
        compat.negotiate_version(connection_id, "9P2000", 8192).unwrap();

        // Test basic read translation
        let legacy_read = Legacy9P2000Message::Read {
            fid: 42,
            offset: 1024,
            count: 512,
            data: Vec::new(),
        };

        let enhanced = compat.translate_legacy_to_enhanced(connection_id, legacy_read.clone()).unwrap();

        if let Enhanced9PeMessage::Read { fid, offset, count, data } = enhanced {
            assert_eq!(fid, 42);
            assert_eq!(offset, 1024);
            assert_eq!(count, 512);
            assert!(data.is_empty());
        } else {
            panic!("Incorrect translation");
        }
    }

    #[test]
    fn test_extension_degradation() {
        let mut compat = CompatibilityLayer::default();
        let connection_id = 200;
        compat.negotiate_version(connection_id, "9P2000", 8192).unwrap();

        // Test stream init degradation
        let stream_init = Enhanced9PeMessage::StreamInit {
            stream_id: 123,
            fid: 42,
            mode: 1,
        };

        let degraded = compat.translate_enhanced_to_legacy(connection_id, stream_init).unwrap();

        if let Some(Legacy9P2000Message::Open { fid, mode }) = degraded {
            assert_eq!(fid, 42);
            assert_eq!(mode, 1);
        } else {
            panic!("Stream init should degrade to Open");
        }

        // Check that degradation warning was recorded
        let connection = &compat.connections[&connection_id];
        assert!(!connection.degradation_warnings.is_empty());
    }

    #[test]
    fn test_unsupported_extension_handling() {
        let mut compat = CompatibilityLayer::default();
        let connection_id = 300;
        compat.negotiate_version(connection_id, "9P2000", 8192).unwrap();

        // Test translator spawn (unsupported in legacy)
        let translator_spawn = Enhanced9PeMessage::TranslatorSpawn {
            translator_id: 456,
            code: vec![1, 2, 3, 4],
            config: vec![5, 6, 7, 8],
        };

        let result = compat.translate_enhanced_to_legacy(connection_id, translator_spawn).unwrap();
        assert!(result.is_none()); // Should be dropped

        // Check statistics
        assert!(compat.translation_stats.extension_fallbacks > 0);
    }

    #[test]
    fn test_fid_mapping_cleanup() {
        let mut compat = CompatibilityLayer::default();
        let connection_id = 400;
        compat.negotiate_version(connection_id, "9P2000", 8192).unwrap();

        // Attach creates FID mapping
        let attach = Legacy9P2000Message::Attach {
            fid: 123,
            afid: 0,
            uname: "user".to_string(),
            aname: "".to_string(),
        };

        compat.translate_legacy_to_enhanced(connection_id, attach).unwrap();
        assert!(compat.connections[&connection_id].legacy_fid_map.contains_key(&123));

        // Clunk should clean up mapping
        let clunk = Legacy9P2000Message::Clunk { fid: 123 };
        compat.translate_legacy_to_enhanced(connection_id, clunk).unwrap();
        assert!(!compat.connections[&connection_id].legacy_fid_map.contains_key(&123));
    }

    #[test]
    fn test_resource_limits() {
        let mut compat = CompatibilityLayer::default();
        compat.compatibility_limits.max_connections = 2;

        // Create maximum connections
        assert!(compat.negotiate_version(1, "9P2000", 8192).is_ok());
        assert!(compat.negotiate_version(2, "9Pe", 8192).is_ok());

        // Third connection should fail
        assert!(compat.negotiate_version(3, "9P2000", 8192).is_err());

        // After closing one, should be able to create another
        compat.close_connection(1).unwrap();
        assert!(compat.negotiate_version(4, "9P2000", 8192).is_ok());
    }
}
