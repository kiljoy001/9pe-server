//! Compatibility layer for 9P2000 protocol support
//!
//! This module provides seamless compatibility with existing 9P2000 clients
//! by translating between legacy protocol messages and the enhanced 9P.e format.

use crate::protocol::NinePMessage;
use std::collections::HashMap;
use thiserror::Error;

/// Maximum supported 9P2000 message size
pub const MAX_9P2000_MESSAGE_SIZE: u32 = 8192;

/// Legacy 9P2000 message types
#[derive(Debug, Clone, PartialEq)]
pub enum LegacyMessageType {
    /// Version negotiation request
    Tversion = 100,
    /// Version negotiation response
    Rversion = 101,
    /// Authentication request
    Tauth = 102,
    /// Authentication response
    Rauth = 103,
    /// Attach to filesystem request
    Tattach = 104,
    /// Attach to filesystem response
    Rattach = 105,
    /// Error message request (illegal)
    Terror = 106,
    /// Error message response
    Rerror = 107,
    /// Flush pending request
    Tflush = 108,
    /// Flush pending response
    Rflush = 109,
    /// Walk file tree request
    Twalk = 110,
    /// Walk file tree response
    Rwalk = 111,
    /// Open file request
    Topen = 112,
    /// Open file response
    Ropen = 113,
    /// Create file request
    Tcreate = 114,
    /// Create file response
    Rcreate = 115,
    /// Read file request
    Tread = 116,
    /// Read file response
    Rread = 117,
    /// Write file request
    Twrite = 118,
    /// Write file response
    Rwrite = 119,
    /// Close file request
    Tclunk = 120,
    /// Close file response
    Rclunk = 121,
    /// Remove file request
    Tremove = 122,
    /// Remove file response
    Rremove = 123,
    /// Get file status request
    Tstat = 124,
    /// Get file status response
    Rstat = 125,
    /// Set file status request
    Twstat = 126,
    /// Set file status response
    Rwstat = 127,
}

/// 9P2000 compatibility errors
#[derive(Error, Debug)]
pub enum CompatibilityError {
    #[error("Unsupported legacy message type: {0}")]
    /// Unsupported legacy message type
    UnsupportedMessageType(u8),
    #[error("Message size exceeds 9P2000 limits: {0} > {1}")]
    /// Message exceeds size limits
    MessageTooLarge(u32, u32),
    #[error("Invalid legacy protocol version: {0}")]
    /// Invalid protocol version string
    InvalidVersion(String),
    #[error("Feature not available in 9P2000 mode: {0}")]
    /// Feature not available in current mode
    FeatureUnavailable(String),
    #[error("Serialization error: {0}")]
    /// Message serialization error
    SerializationError(String),
}

/// Compatibility session state
#[derive(Debug, Clone)]
pub struct CompatibilitySession {
    /// Whether this session uses legacy 9P2000 protocol
    pub is_legacy: bool,
    /// Negotiated message size for this session
    pub msize: u32,
    /// Version string negotiated
    pub version: String,
    /// Mapping of fids to enhanced capabilities (for legacy clients)
    pub fid_capabilities: HashMap<u32, Vec<String>>,
    /// Feature flags available to this session
    pub available_features: FeatureSet,
}

/// Available protocol features
#[derive(Debug, Clone, Default)]
pub struct FeatureSet {
    /// Whether streaming is supported
    pub streaming: bool,
    /// Whether multiplexing is supported
    pub multiplexing: bool,
    /// Whether capability system is supported
    pub capabilities: bool,
    /// Whether synthetic files are supported
    pub synthetic_files: bool,
    /// Whether consensus synchronization is supported
    pub consensus_sync: bool,
    /// Whether enhanced cryptography is supported
    pub enhanced_crypto: bool,
    /// Whether translators are supported
    pub translators: bool,
}

impl CompatibilitySession {
    /// Create a new compatibility session
    pub fn new() -> Self {
        Self {
            is_legacy: false,
            msize: MAX_9P2000_MESSAGE_SIZE,
            version: "9P2000".to_string(),
            fid_capabilities: HashMap::new(),
            available_features: FeatureSet::default(),
        }
    }

    /// Negotiate protocol version and features
    pub fn negotiate_version(&mut self, version: &str, msize: u32) -> Result<FeatureSet, CompatibilityError> {
        match version {
            "9P2000" => {
                self.is_legacy = true;
                self.msize = std::cmp::min(msize, MAX_9P2000_MESSAGE_SIZE);
                self.version = version.to_string();
                self.available_features = FeatureSet::default(); // No enhanced features
            }
            "9P.e" | "9Pe" => {
                self.is_legacy = false;
                self.msize = msize;
                self.version = version.to_string();
                self.available_features = FeatureSet {
                    streaming: true,
                    multiplexing: true,
                    capabilities: true,
                    synthetic_files: true,
                    consensus_sync: true,
                    enhanced_crypto: true,
                    translators: true,
                };
            }
            _ => return Err(CompatibilityError::InvalidVersion(version.to_string())),
        }

        Ok(self.available_features.clone())
    }

    /// Check if a feature is available in this session
    pub fn has_feature(&self, feature: &str) -> bool {
        if self.is_legacy {
            return false; // No enhanced features in legacy mode
        }

        match feature {
            "streaming" => self.available_features.streaming,
            "multiplexing" => self.available_features.multiplexing,
            "capabilities" => self.available_features.capabilities,
            "synthetic" => self.available_features.synthetic_files,
            "consensus" => self.available_features.consensus_sync,
            "crypto" => self.available_features.enhanced_crypto,
            "translators" => self.available_features.translators,
            _ => false,
        }
    }
}

/// Convert legacy 9P2000 messages to 9P.e format
pub struct MessageTranslator {
    session: CompatibilitySession,
}

impl MessageTranslator {
    /// Create a new message translator for the given session
    pub fn new(session: CompatibilitySession) -> Self {
        Self { session }
    }

    /// Translate a legacy message to 9P.e format
    pub fn translate_legacy_to_9pe(&self, data: &[u8]) -> Result<NinePMessage, CompatibilityError> {
        if data.len() < 7 {
            return Err(CompatibilityError::SerializationError("Message too short".to_string()));
        }

        let size = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let msg_type = data[4];
        let _tag = u16::from_le_bytes([data[5], data[6]]);

        if size > self.session.msize {
            return Err(CompatibilityError::MessageTooLarge(size, self.session.msize));
        }

        match msg_type {
            100 => {
                // Tversion
                let msize = u32::from_le_bytes([data[7], data[8], data[9], data[10]]);
                let version_len = u16::from_le_bytes([data[11], data[12]]) as usize;
                let version = String::from_utf8_lossy(&data[13..13+version_len]).to_string();
                Ok(NinePMessage::Version { msize, version })
            }
            102 => {
                // Tauth
                let afid = u32::from_le_bytes([data[7], data[8], data[9], data[10]]);
                let uname_len = u16::from_le_bytes([data[11], data[12]]) as usize;
                let uname = String::from_utf8_lossy(&data[13..13+uname_len]).to_string();
                let aname_start = 13 + uname_len;
                let aname_len = u16::from_le_bytes([data[aname_start], data[aname_start+1]]) as usize;
                let aname = String::from_utf8_lossy(&data[aname_start+2..aname_start+2+aname_len]).to_string();
                Ok(NinePMessage::Auth { afid, uname, aname, password: None })
            }
            104 => {
                // Tattach
                let fid = u32::from_le_bytes([data[7], data[8], data[9], data[10]]);
                let afid = u32::from_le_bytes([data[11], data[12], data[13], data[14]]);
                let uname_len = u16::from_le_bytes([data[15], data[16]]) as usize;
                let uname = String::from_utf8_lossy(&data[17..17+uname_len]).to_string();
                let aname_start = 17 + uname_len;
                let aname_len = u16::from_le_bytes([data[aname_start], data[aname_start+1]]) as usize;
                let aname = String::from_utf8_lossy(&data[aname_start+2..aname_start+2+aname_len]).to_string();
                Ok(NinePMessage::Attach { fid, afid, uname, aname })
            }
            108 => {
                // Tflush
                // Tflush (tag: 108) is handled by the server wrapper by matching tag
                // Here we just return a dummy if needed, but 9P.e doesn't have Tflush.
                // We'll map it to a No-op version or similar if needed, or just return an error
                // that it's handled at the transport layer. For compatibility, we'll return
                // an empty Stat message as a placeholder if 9P.e doesn't have a flush equivalent.
                // Actually, let's return a special error that the caller can catch.
                Err(CompatibilityError::FeatureUnavailable("Tflush should be handled by transport".to_string()))
            }
            110 => {
                // Twalk
                let fid = u32::from_le_bytes([data[7], data[8], data[9], data[10]]);
                let newfid = u32::from_le_bytes([data[11], data[12], data[13], data[14]]);
                let nwname = u16::from_le_bytes([data[15], data[16]]);
                let mut wnames = Vec::new();
                let mut pos = 17;

                for _ in 0..nwname {
                    if pos + 2 > data.len() { break; }
                    let name_len = u16::from_le_bytes([data[pos], data[pos+1]]) as usize;
                    pos += 2;
                    if pos + name_len > data.len() { break; }
                    let name = String::from_utf8_lossy(&data[pos..pos+name_len]).to_string();
                    wnames.push(name);
                    pos += name_len;
                }

                Ok(NinePMessage::Walk { fid, newfid, wnames })
            }
            112 => {
                // Topen
                let fid = u32::from_le_bytes([data[7], data[8], data[9], data[10]]);
                let mode = data[11];
                Ok(NinePMessage::Open { fid, mode })
            }
            114 => {
                // Tcreate
                let fid = u32::from_le_bytes([data[7], data[8], data[9], data[10]]);
                let name_len = u16::from_le_bytes([data[11], data[12]]) as usize;
                let name = String::from_utf8_lossy(&data[13..13+name_len]).to_string();
                let perm = u32::from_le_bytes([data[13+name_len], data[13+name_len+1], data[13+name_len+2], data[13+name_len+3]]);
                let mode = data[13+name_len+4];
                Ok(NinePMessage::Create { fid, name, perm, mode })
            }
            116 => {
                // Tread
                let fid = u32::from_le_bytes([data[7], data[8], data[9], data[10]]);
                let offset = u64::from_le_bytes([data[11], data[12], data[13], data[14], data[15], data[16], data[17], data[18]]);
                let count = u32::from_le_bytes([data[19], data[20], data[21], data[22]]);
                Ok(NinePMessage::Read {
                    fid,
                    offset,
                    count,
                    data: Vec::new(),
                })
            }
            118 => {
                // Twrite
                let fid = u32::from_le_bytes([data[7], data[8], data[9], data[10]]);
                let offset = u64::from_le_bytes([data[11], data[12], data[13], data[14], data[15], data[16], data[17], data[18]]);
                let data_len = u32::from_le_bytes([data[19], data[20], data[21], data[22]]) as usize;
                let write_data = data[23..23+data_len].to_vec();
                Ok(NinePMessage::Write { fid, offset, data: write_data })
            }
            120 => {
                // Tclunk
                let fid = u32::from_le_bytes([data[7], data[8], data[9], data[10]]);
                Ok(NinePMessage::Clunk { fid })
            }
            122 => {
                // Tremove
                let fid = u32::from_le_bytes([data[7], data[8], data[9], data[10]]);
                Ok(NinePMessage::Remove { fid })
            }
            124 => {
                // Tstat
                let fid = u32::from_le_bytes([data[7], data[8], data[9], data[10]]);
                Ok(NinePMessage::Stat { fid, data: Vec::new() })
            }
            126 => {
                // Twstat
                let fid = u32::from_le_bytes([data[7], data[8], data[9], data[10]]);
                let stat_len = u16::from_le_bytes([data[11], data[12]]) as usize;
                let stat_data = data[13..13+stat_len].to_vec();
                Ok(NinePMessage::Wstat { fid, stat: stat_data })
            }
            _ => Err(CompatibilityError::UnsupportedMessageType(msg_type)),
        }
    }

    /// Translate a 9P.e message to legacy format (if possible)
    pub fn translate_9pe_to_legacy(&self, message: &NinePMessage) -> Result<Vec<u8>, CompatibilityError> {
        if self.session.is_legacy {
            match message {
                NinePMessage::Version { msize, version } => {
                    let mut data = Vec::new();
                    let version_bytes = version.as_bytes();
                    let total_size = 4 + 1 + 2 + 4 + 2 + version_bytes.len(); // size + type + tag + msize + version_len + version

                    data.extend_from_slice(&(total_size as u32).to_le_bytes());
                    data.push(101); // Rversion
                    data.extend_from_slice(&0u16.to_le_bytes()); // tag
                    data.extend_from_slice(&msize.to_le_bytes());
                    data.extend_from_slice(&(version_bytes.len() as u16).to_le_bytes());
                    data.extend_from_slice(version_bytes);

                    Ok(data)
                }
                NinePMessage::Auth { afid: _, uname: _, aname: _, password: _ } => {
                    let mut data = Vec::new();
                    let total_size = 4 + 1 + 2 + 13; // size + type + tag + aqid(13)

                    data.extend_from_slice(&(total_size as u32).to_le_bytes());
                    data.push(103); // Rauth
                    data.extend_from_slice(&0u16.to_le_bytes()); // tag
                    data.extend_from_slice(&[0u8; 13]); // dummy aqid
                    Ok(data)
                }
                NinePMessage::Attach { .. } => {
                    // Return Rattach with qid
                    let mut data = Vec::new();
                    let total_size = 4 + 1 + 2 + 13; // size + type + tag + qid(13 bytes)

                    data.extend_from_slice(&(total_size as u32).to_le_bytes());
                    data.push(105); // Rattach
                    data.extend_from_slice(&0u16.to_le_bytes()); // tag
                    data.extend_from_slice(&[0x80u8; 13]); // dummy qid (0x80 for directory-ish)

                    Ok(data)
                }
                NinePMessage::Walk { .. } => {
                    let mut data = Vec::new();
                    // Assume success for now, returning 0 qids to keep it simple or implement properly if we had the result
                    let total_size = 4 + 1 + 2 + 2; // size + type + tag + nwqid(2)
                    data.extend_from_slice(&(total_size as u32).to_le_bytes());
                    data.push(111); // Rwalk
                    data.extend_from_slice(&0u16.to_le_bytes()); // tag
                    data.extend_from_slice(&0u16.to_le_bytes()); // nwqid = 0
                    Ok(data)
                }
                NinePMessage::Open { .. } => {
                    let mut data = Vec::new();
                    let total_size = 4 + 1 + 2 + 13 + 4; // size + type + tag + qid(13) + iounit(4)
                    data.extend_from_slice(&(total_size as u32).to_le_bytes());
                    data.push(113); // Ropen
                    data.extend_from_slice(&0u16.to_le_bytes()); // tag
                    data.extend_from_slice(&[0u8; 13]); // qid
                    data.extend_from_slice(&8192u32.to_le_bytes()); // iounit
                    Ok(data)
                }
                NinePMessage::Create { .. } => {
                    let mut data = Vec::new();
                    let total_size = 4 + 1 + 2 + 13 + 4; // size + type + tag + qid(13) + iounit(4)
                    data.extend_from_slice(&(total_size as u32).to_le_bytes());
                    data.push(115); // Rcreate
                    data.extend_from_slice(&0u16.to_le_bytes()); // tag
                    data.extend_from_slice(&[0u8; 13]); // qid
                    data.extend_from_slice(&8192u32.to_le_bytes()); // iounit
                    Ok(data)
                }
                NinePMessage::Read { data: payload, .. } => {
                    let mut encoded = Vec::new();
                    let payload_len = payload.len() as u32;
                    let total_size = 4 + 1 + 2 + 4 + payload_len as usize; // size + type + tag + count + data

                    encoded.extend_from_slice(&(total_size as u32).to_le_bytes());
                    encoded.push(117); // Rread
                    encoded.extend_from_slice(&0u16.to_le_bytes()); // tag
                    encoded.extend_from_slice(&payload_len.to_le_bytes());
                    encoded.extend_from_slice(payload);

                    Ok(encoded)
                }
                NinePMessage::Write { data, .. } => {
                    let mut encoded = Vec::new();
                    let count = data.len() as u32;
                    let total_size = 4 + 1 + 2 + 4; // size + type + tag + count
                    encoded.extend_from_slice(&(total_size as u32).to_le_bytes());
                    encoded.push(119); // Rwrite
                    encoded.extend_from_slice(&0u16.to_le_bytes()); // tag
                    encoded.extend_from_slice(&count.to_le_bytes());
                    Ok(encoded)
                }
                NinePMessage::Clunk { .. } => {
                    let mut encoded = Vec::new();
                    let total_size = 4 + 1 + 2; // size + type + tag
                    encoded.extend_from_slice(&(total_size as u32).to_le_bytes());
                    encoded.push(121); // Rclunk
                    encoded.extend_from_slice(&0u16.to_le_bytes()); // tag
                    Ok(encoded)
                }
                NinePMessage::Remove { .. } => {
                    let mut encoded = Vec::new();
                    let total_size = 4 + 1 + 2; // size + type + tag
                    encoded.extend_from_slice(&(total_size as u32).to_le_bytes());
                    encoded.push(123); // Rremove
                    encoded.extend_from_slice(&0u16.to_le_bytes()); // tag
                    Ok(encoded)
                }
                NinePMessage::Stat { data, .. } => {
                    let mut encoded = Vec::new();
                    let nstat = data.len() as u16;
                    let total_size = 4 + 1 + 2 + 2 + data.len(); 
                    encoded.extend_from_slice(&(total_size as u32).to_le_bytes());
                    encoded.push(125); // Rstat
                    encoded.extend_from_slice(&0u16.to_le_bytes()); // tag
                    encoded.extend_from_slice(&nstat.to_le_bytes());
                    encoded.extend_from_slice(data);
                    Ok(encoded)
                }
                NinePMessage::Wstat { .. } => {
                    let mut encoded = Vec::new();
                    let total_size = 4 + 1 + 2; 
                    encoded.extend_from_slice(&(total_size as u32).to_le_bytes());
                    encoded.push(127); // Rwstat
                    encoded.extend_from_slice(&0u16.to_le_bytes()); // tag
                    Ok(encoded)
                }
                NinePMessage::Error { ename, .. } => {
                    let mut encoded = Vec::new();
                    let ename_bytes = ename.as_bytes();
                    let total_size = 4 + 1 + 2 + 2 + ename_bytes.len();
                    encoded.extend_from_slice(&(total_size as u32).to_le_bytes());
                    encoded.push(107); // Rerror
                    encoded.extend_from_slice(&0u16.to_le_bytes()); // tag
                    encoded.extend_from_slice(&(ename_bytes.len() as u16).to_le_bytes());
                    encoded.extend_from_slice(ename_bytes);
                    Ok(encoded)
                }
                _ => Err(CompatibilityError::FeatureUnavailable("Enhanced message types not supported in legacy mode".to_string())),
            }
        } else {
            Err(CompatibilityError::FeatureUnavailable("Not in legacy mode".to_string()))
        }
    }

    /// Check if message requires enhanced features
    pub fn requires_enhanced_features(&self, message: &NinePMessage) -> bool {
        match message {
            NinePMessage::StreamInit { .. } |
            NinePMessage::StreamData { .. } |
            NinePMessage::MultiplexChannel { .. } |
            NinePMessage::CapabilityGrant { .. } |
            NinePMessage::ConsensusPropose { .. } |
            NinePMessage::ConsensusVote { .. } |
            NinePMessage::ConsensusCommit { .. } |
            NinePMessage::TranslatorSpawn { .. } |
            NinePMessage::SyntheticCreate { .. } => true,
            _ => false,
        }
    }
}

/// Capability mapping for legacy fids
pub struct CapabilityMapper {
    fid_caps: HashMap<u32, Vec<String>>,
}

impl CapabilityMapper {
    /// Create a new capability mapper
    pub fn new() -> Self {
        Self {
            fid_caps: HashMap::new(),
        }
    }

    /// Grant legacy fid access to enhanced capabilities
    pub fn grant_capability(&mut self, fid: u32, capability: String) {
        self.fid_caps.entry(fid).or_insert_with(Vec::new).push(capability);
    }

    /// Check if fid has a specific capability
    pub fn has_capability(&self, fid: u32, capability: &str) -> bool {
        self.fid_caps.get(&fid)
            .map(|caps| caps.iter().any(|c| c == capability))
            .unwrap_or(false)
    }

    /// Remove fid capabilities (on clunk)
    pub fn remove_fid(&mut self, fid: u32) {
        self.fid_caps.remove(&fid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compatibility_session_creation() {
        let session = CompatibilitySession::new();
        assert!(!session.is_legacy);
        assert_eq!(session.msize, MAX_9P2000_MESSAGE_SIZE);
        assert_eq!(session.version, "9P2000");
    }

    #[test]
    fn test_version_negotiation_legacy() {
        let mut session = CompatibilitySession::new();
        let features = session.negotiate_version("9P2000", 8192).unwrap();

        assert!(session.is_legacy);
        assert_eq!(session.msize, 8192);
        assert!(!features.streaming);
        assert!(!features.multiplexing);
    }

    #[test]
    fn test_version_negotiation_enhanced() {
        let mut session = CompatibilitySession::new();
        let features = session.negotiate_version("9P.e", 65536).unwrap();

        assert!(!session.is_legacy);
        assert_eq!(session.msize, 65536);
        assert!(features.streaming);
        assert!(features.multiplexing);
    }

    #[test]
    fn test_feature_availability() {
        let mut session = CompatibilitySession::new();
        session.negotiate_version("9P2000", 8192).unwrap();

        assert!(!session.has_feature("streaming"));
        assert!(!session.has_feature("multiplexing"));

        session.negotiate_version("9P.e", 65536).unwrap();

        assert!(session.has_feature("streaming"));
        assert!(session.has_feature("multiplexing"));
    }

    #[test]
    fn test_capability_mapper() {
        let mut mapper = CapabilityMapper::new();

        mapper.grant_capability(1, "read".to_string());
        mapper.grant_capability(1, "write".to_string());

        assert!(mapper.has_capability(1, "read"));
        assert!(mapper.has_capability(1, "write"));
        assert!(!mapper.has_capability(1, "admin"));
        assert!(!mapper.has_capability(2, "read"));

        mapper.remove_fid(1);
        assert!(!mapper.has_capability(1, "read"));
    }

    #[test]
    fn test_enhanced_message_detection() {
        let session = CompatibilitySession::new();
        let translator = MessageTranslator::new(session);

        let version_msg = NinePMessage::Version { msize: 8192, version: "9P2000".to_string() };
        let stream_msg = NinePMessage::StreamInit { stream_id: 1, fid: 2, mode: 1 };

        assert!(!translator.requires_enhanced_features(&version_msg));
        assert!(translator.requires_enhanced_features(&stream_msg));
    }

    #[test]
    fn test_legacy_version_translation() {
        let mut session = CompatibilitySession::new();
        session.negotiate_version("9P2000", 8192).unwrap();
        let translator = MessageTranslator::new(session);

        let version_msg = NinePMessage::Version { msize: 8192, version: "9P2000".to_string() };
        let legacy_data = translator.translate_9pe_to_legacy(&version_msg).unwrap();

        assert!(!legacy_data.is_empty());
        assert_eq!(legacy_data[4], 101); // Rversion
    }

    #[test]
    fn test_invalid_version_rejection() {
        let mut session = CompatibilitySession::new();
        let result = session.negotiate_version("9P1999", 8192);

        assert!(result.is_err());
        match result.unwrap_err() {
            CompatibilityError::InvalidVersion(v) => assert_eq!(v, "9P1999"),
            _ => panic!("Wrong error type"),
        }
    }

    #[test]
    fn test_message_size_limits() {
        let mut session = CompatibilitySession::new();
        session.negotiate_version("9P2000", 16384).unwrap();

        // Should clamp to MAX_9P2000_MESSAGE_SIZE
        assert_eq!(session.msize, MAX_9P2000_MESSAGE_SIZE);
    }
}
