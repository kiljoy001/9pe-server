//! 9P Protocol Messages
//!
//! Defines all 9P2000 and 9P.e extension messages with serialization support.

use super::{MessageType, Tag, Fid, Qid, Stat, Message};
use std::io::{Result as IoResult, Error, ErrorKind};

// Helper functions for wire encoding
fn write_u8(buf: &mut Vec<u8>, val: u8) -> IoResult<()> {
    buf.push(val);
    Ok(())
}

fn write_u16(buf: &mut Vec<u8>, val: u16) -> IoResult<()> {
    buf.extend_from_slice(&val.to_le_bytes());
    Ok(())
}

fn write_u32(buf: &mut Vec<u8>, val: u32) -> IoResult<()> {
    buf.extend_from_slice(&val.to_le_bytes());
    Ok(())
}

fn write_u64(buf: &mut Vec<u8>, val: u64) -> IoResult<()> {
    buf.extend_from_slice(&val.to_le_bytes());
    Ok(())
}

fn write_string(buf: &mut Vec<u8>, s: &str) -> IoResult<()> {
    let bytes = s.as_bytes();
    write_u16(buf, bytes.len() as u16)?;
    buf.extend_from_slice(bytes);
    Ok(())
}

fn write_qid(buf: &mut Vec<u8>, qid: &Qid) -> IoResult<()> {
    write_u8(buf, qid.qtype)?;
    write_u32(buf, qid.version)?;
    write_u64(buf, qid.path)?;
    Ok(())
}

fn read_u8(buf: &[u8], offset: &mut usize) -> IoResult<u8> {
    if *offset >= buf.len() {
        return Err(Error::new(ErrorKind::UnexpectedEof, "Buffer underflow"));
    }
    let val = buf[*offset];
    *offset += 1;
    Ok(val)
}

fn read_u16(buf: &[u8], offset: &mut usize) -> IoResult<u16> {
    if *offset + 2 > buf.len() {
        return Err(Error::new(ErrorKind::UnexpectedEof, "Buffer underflow"));
    }
    let val = u16::from_le_bytes([buf[*offset], buf[*offset + 1]]);
    *offset += 2;
    Ok(val)
}

fn read_u32(buf: &[u8], offset: &mut usize) -> IoResult<u32> {
    if *offset + 4 > buf.len() {
        return Err(Error::new(ErrorKind::UnexpectedEof, "Buffer underflow"));
    }
    let val = u32::from_le_bytes([
        buf[*offset], buf[*offset + 1], buf[*offset + 2], buf[*offset + 3]
    ]);
    *offset += 4;
    Ok(val)
}

fn read_u64(buf: &[u8], offset: &mut usize) -> IoResult<u64> {
    if *offset + 8 > buf.len() {
        return Err(Error::new(ErrorKind::UnexpectedEof, "Buffer underflow"));
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&buf[*offset..*offset + 8]);
    *offset += 8;
    Ok(u64::from_le_bytes(bytes))
}

fn read_string(buf: &[u8], offset: &mut usize) -> IoResult<String> {
    let len = read_u16(buf, offset)? as usize;
    if *offset + len > buf.len() {
        return Err(Error::new(ErrorKind::UnexpectedEof, "Buffer underflow"));
    }
    let s = String::from_utf8_lossy(&buf[*offset..*offset + len]).into_owned();
    *offset += len;
    Ok(s)
}

fn read_qid(buf: &[u8], offset: &mut usize) -> IoResult<Qid> {
    Ok(Qid {
        qtype: read_u8(buf, offset)?,
        version: read_u32(buf, offset)?,
        path: read_u64(buf, offset)?,
    })
}

// === Version negotiation ===

#[derive(Debug, Clone)]
pub struct Tversion {
    pub tag: Tag,
    pub msize: u32,
    pub version: String,
}

impl Tversion {
    pub fn decode(buf: &[u8]) -> IoResult<Self> {
        let mut offset = 0;
        Ok(Self {
            tag: read_u16(buf, &mut offset)?,
            msize: read_u32(buf, &mut offset)?,
            version: read_string(buf, &mut offset)?,
        })
    }
}

impl Message for Tversion {
    fn msg_type(&self) -> MessageType { MessageType::Tversion }
    fn tag(&self) -> Tag { self.tag }

    fn encode(&self, buf: &mut Vec<u8>) -> IoResult<()> {
        write_u16(buf, self.tag)?;
        write_u32(buf, self.msize)?;
        write_string(buf, &self.version)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Rversion {
    pub tag: Tag,
    pub msize: u32,
    pub version: String,
}

impl Rversion {
    pub fn decode(buf: &[u8]) -> IoResult<Self> {
        let mut offset = 0;
        Ok(Self {
            tag: read_u16(buf, &mut offset)?,
            msize: read_u32(buf, &mut offset)?,
            version: read_string(buf, &mut offset)?,
        })
    }
}

impl Message for Rversion {
    fn msg_type(&self) -> MessageType { MessageType::Rversion }
    fn tag(&self) -> Tag { self.tag }

    fn encode(&self, buf: &mut Vec<u8>) -> IoResult<()> {
        write_u16(buf, self.tag)?;
        write_u32(buf, self.msize)?;
        write_string(buf, &self.version)?;
        Ok(())
    }
}

// === Attach to filesystem ===

#[derive(Debug, Clone)]
pub struct Tattach {
    pub tag: Tag,
    pub fid: Fid,
    pub afid: Fid,
    pub uname: String,
    pub aname: String,
}

impl Tattach {
    pub fn decode(buf: &[u8]) -> IoResult<Self> {
        let mut offset = 0;
        Ok(Self {
            tag: read_u16(buf, &mut offset)?,
            fid: read_u32(buf, &mut offset)?,
            afid: read_u32(buf, &mut offset)?,
            uname: read_string(buf, &mut offset)?,
            aname: read_string(buf, &mut offset)?,
        })
    }
}

impl Message for Tattach {
    fn msg_type(&self) -> MessageType { MessageType::Tattach }
    fn tag(&self) -> Tag { self.tag }

    fn encode(&self, buf: &mut Vec<u8>) -> IoResult<()> {
        write_u16(buf, self.tag)?;
        write_u32(buf, self.fid)?;
        write_u32(buf, self.afid)?;
        write_string(buf, &self.uname)?;
        write_string(buf, &self.aname)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Rattach {
    pub tag: Tag,
    pub qid: Qid,
}

impl Rattach {
    pub fn decode(buf: &[u8]) -> IoResult<Self> {
        let mut offset = 0;
        Ok(Self {
            tag: read_u16(buf, &mut offset)?,
            qid: read_qid(buf, &mut offset)?,
        })
    }
}

impl Message for Rattach {
    fn msg_type(&self) -> MessageType { MessageType::Rattach }
    fn tag(&self) -> Tag { self.tag }

    fn encode(&self, buf: &mut Vec<u8>) -> IoResult<()> {
        write_u16(buf, self.tag)?;
        write_qid(buf, &self.qid)?;
        Ok(())
    }
}

// === Walk filesystem ===

#[derive(Debug, Clone)]
pub struct Twalk {
    pub tag: Tag,
    pub fid: Fid,
    pub newfid: Fid,
    pub wnames: Vec<String>,
}

impl Twalk {
    pub fn decode(buf: &[u8]) -> IoResult<Self> {
        let mut offset = 0;
        let tag = read_u16(buf, &mut offset)?;
        let fid = read_u32(buf, &mut offset)?;
        let newfid = read_u32(buf, &mut offset)?;
        let nwname = read_u16(buf, &mut offset)?;

        let mut wnames = Vec::new();
        for _ in 0..nwname {
            wnames.push(read_string(buf, &mut offset)?);
        }

        Ok(Self { tag, fid, newfid, wnames })
    }
}

impl Message for Twalk {
    fn msg_type(&self) -> MessageType { MessageType::Twalk }
    fn tag(&self) -> Tag { self.tag }

    fn encode(&self, buf: &mut Vec<u8>) -> IoResult<()> {
        write_u16(buf, self.tag)?;
        write_u32(buf, self.fid)?;
        write_u32(buf, self.newfid)?;
        write_u16(buf, self.wnames.len() as u16)?;
        for name in &self.wnames {
            write_string(buf, name)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Rwalk {
    pub tag: Tag,
    pub qids: Vec<Qid>,
}

impl Rwalk {
    pub fn decode(buf: &[u8]) -> IoResult<Self> {
        let mut offset = 0;
        let tag = read_u16(buf, &mut offset)?;
        let nqid = read_u16(buf, &mut offset)?;

        let mut qids = Vec::new();
        for _ in 0..nqid {
            qids.push(read_qid(buf, &mut offset)?);
        }

        Ok(Self { tag, qids })
    }
}

impl Message for Rwalk {
    fn msg_type(&self) -> MessageType { MessageType::Rwalk }
    fn tag(&self) -> Tag { self.tag }

    fn encode(&self, buf: &mut Vec<u8>) -> IoResult<()> {
        write_u16(buf, self.tag)?;
        write_u16(buf, self.qids.len() as u16)?;
        for qid in &self.qids {
            write_qid(buf, qid)?;
        }
        Ok(())
    }
}

// === Open file ===

#[derive(Debug, Clone)]
pub struct Topen {
    pub tag: Tag,
    pub fid: Fid,
    pub mode: u8,
}

impl Topen {
    pub fn decode(buf: &[u8]) -> IoResult<Self> {
        let mut offset = 0;
        Ok(Self {
            tag: read_u16(buf, &mut offset)?,
            fid: read_u32(buf, &mut offset)?,
            mode: read_u8(buf, &mut offset)?,
        })
    }
}

impl Message for Topen {
    fn msg_type(&self) -> MessageType { MessageType::Topen }
    fn tag(&self) -> Tag { self.tag }

    fn encode(&self, buf: &mut Vec<u8>) -> IoResult<()> {
        write_u16(buf, self.tag)?;
        write_u32(buf, self.fid)?;
        write_u8(buf, self.mode)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Ropen {
    pub tag: Tag,
    pub qid: Qid,
    pub iounit: u32,
}

impl Ropen {
    pub fn decode(buf: &[u8]) -> IoResult<Self> {
        let mut offset = 0;
        Ok(Self {
            tag: read_u16(buf, &mut offset)?,
            qid: read_qid(buf, &mut offset)?,
            iounit: read_u32(buf, &mut offset)?,
        })
    }
}

impl Message for Ropen {
    fn msg_type(&self) -> MessageType { MessageType::Ropen }
    fn tag(&self) -> Tag { self.tag }

    fn encode(&self, buf: &mut Vec<u8>) -> IoResult<()> {
        write_u16(buf, self.tag)?;
        write_qid(buf, &self.qid)?;
        write_u32(buf, self.iounit)?;
        Ok(())
    }
}

// === Read file ===

#[derive(Debug, Clone)]
pub struct Tread {
    pub tag: Tag,
    pub fid: Fid,
    pub offset: u64,
    pub count: u32,
}

impl Tread {
    pub fn decode(buf: &[u8]) -> IoResult<Self> {
        let mut offset = 0;
        Ok(Self {
            tag: read_u16(buf, &mut offset)?,
            fid: read_u32(buf, &mut offset)?,
            offset: read_u64(buf, &mut offset)?,
            count: read_u32(buf, &mut offset)?,
        })
    }
}

impl Message for Tread {
    fn msg_type(&self) -> MessageType { MessageType::Tread }
    fn tag(&self) -> Tag { self.tag }

    fn encode(&self, buf: &mut Vec<u8>) -> IoResult<()> {
        write_u16(buf, self.tag)?;
        write_u32(buf, self.fid)?;
        write_u64(buf, self.offset)?;
        write_u32(buf, self.count)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Rread {
    pub tag: Tag,
    pub data: Vec<u8>,
}

impl Rread {
    pub fn decode(buf: &[u8]) -> IoResult<Self> {
        let mut offset = 0;
        let tag = read_u16(buf, &mut offset)?;
        let count = read_u32(buf, &mut offset)? as usize;

        if offset + count > buf.len() {
            return Err(Error::new(ErrorKind::UnexpectedEof, "Buffer underflow"));
        }

        let data = buf[offset..offset + count].to_vec();

        Ok(Self { tag, data })
    }
}

impl Message for Rread {
    fn msg_type(&self) -> MessageType { MessageType::Rread }
    fn tag(&self) -> Tag { self.tag }

    fn encode(&self, buf: &mut Vec<u8>) -> IoResult<()> {
        write_u16(buf, self.tag)?;
        write_u32(buf, self.data.len() as u32)?;
        buf.extend_from_slice(&self.data);
        Ok(())
    }
}

// === Write file ===

#[derive(Debug, Clone)]
pub struct Twrite {
    pub tag: Tag,
    pub fid: Fid,
    pub offset: u64,
    pub data: Vec<u8>,
}

impl Twrite {
    pub fn decode(buf: &[u8]) -> IoResult<Self> {
        let mut offset = 0;
        let tag = read_u16(buf, &mut offset)?;
        let fid = read_u32(buf, &mut offset)?;
        let file_offset = read_u64(buf, &mut offset)?;
        let count = read_u32(buf, &mut offset)? as usize;

        if offset + count > buf.len() {
            return Err(Error::new(ErrorKind::UnexpectedEof, "Buffer underflow"));
        }

        let data = buf[offset..offset + count].to_vec();

        Ok(Self { tag, fid, offset: file_offset, data })
    }
}

impl Message for Twrite {
    fn msg_type(&self) -> MessageType { MessageType::Twrite }
    fn tag(&self) -> Tag { self.tag }

    fn encode(&self, buf: &mut Vec<u8>) -> IoResult<()> {
        write_u16(buf, self.tag)?;
        write_u32(buf, self.fid)?;
        write_u64(buf, self.offset)?;
        write_u32(buf, self.data.len() as u32)?;
        buf.extend_from_slice(&self.data);
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Rwrite {
    pub tag: Tag,
    pub count: u32,
}

impl Rwrite {
    pub fn decode(buf: &[u8]) -> IoResult<Self> {
        let mut offset = 0;
        Ok(Self {
            tag: read_u16(buf, &mut offset)?,
            count: read_u32(buf, &mut offset)?,
        })
    }
}

impl Message for Rwrite {
    fn msg_type(&self) -> MessageType { MessageType::Rwrite }
    fn tag(&self) -> Tag { self.tag }

    fn encode(&self, buf: &mut Vec<u8>) -> IoResult<()> {
        write_u16(buf, self.tag)?;
        write_u32(buf, self.count)?;
        Ok(())
    }
}

// === Close file ===

#[derive(Debug, Clone)]
pub struct Tclunk {
    pub tag: Tag,
    pub fid: Fid,
}

impl Tclunk {
    pub fn decode(buf: &[u8]) -> IoResult<Self> {
        let mut offset = 0;
        Ok(Self {
            tag: read_u16(buf, &mut offset)?,
            fid: read_u32(buf, &mut offset)?,
        })
    }
}

impl Message for Tclunk {
    fn msg_type(&self) -> MessageType { MessageType::Tclunk }
    fn tag(&self) -> Tag { self.tag }

    fn encode(&self, buf: &mut Vec<u8>) -> IoResult<()> {
        write_u16(buf, self.tag)?;
        write_u32(buf, self.fid)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Rclunk {
    pub tag: Tag,
}

impl Rclunk {
    pub fn decode(buf: &[u8]) -> IoResult<Self> {
        let mut offset = 0;
        Ok(Self {
            tag: read_u16(buf, &mut offset)?,
        })
    }
}

impl Message for Rclunk {
    fn msg_type(&self) -> MessageType { MessageType::Rclunk }
    fn tag(&self) -> Tag { self.tag }

    fn encode(&self, buf: &mut Vec<u8>) -> IoResult<()> {
        write_u16(buf, self.tag)?;
        Ok(())
    }
}

// === Stat file ===

#[derive(Debug, Clone)]
pub struct Tstat {
    pub tag: Tag,
    pub fid: Fid,
}

impl Tstat {
    pub fn decode(buf: &[u8]) -> IoResult<Self> {
        let mut offset = 0;
        Ok(Self {
            tag: read_u16(buf, &mut offset)?,
            fid: read_u32(buf, &mut offset)?,
        })
    }
}

impl Message for Tstat {
    fn msg_type(&self) -> MessageType { MessageType::Tstat }
    fn tag(&self) -> Tag { self.tag }

    fn encode(&self, buf: &mut Vec<u8>) -> IoResult<()> {
        write_u16(buf, self.tag)?;
        write_u32(buf, self.fid)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Rstat {
    pub tag: Tag,
    pub stat: Stat,
}

impl Rstat {
    pub fn decode(buf: &[u8]) -> IoResult<Self> {
        let mut offset = 0;
        let tag = read_u16(buf, &mut offset)?;

        // Read stat (complex structure)
        let _stat_size = read_u16(buf, &mut offset)?;
        let stat = Stat {
            size: read_u16(buf, &mut offset)?,
            typ: read_u16(buf, &mut offset)?,
            dev: read_u32(buf, &mut offset)?,
            qid: read_qid(buf, &mut offset)?,
            mode: read_u32(buf, &mut offset)?,
            atime: read_u32(buf, &mut offset)?,
            mtime: read_u32(buf, &mut offset)?,
            length: read_u64(buf, &mut offset)?,
            name: read_string(buf, &mut offset)?,
            uid: read_string(buf, &mut offset)?,
            gid: read_string(buf, &mut offset)?,
            muid: read_string(buf, &mut offset)?,
        };

        Ok(Self { tag, stat })
    }
}

impl Message for Rstat {
    fn msg_type(&self) -> MessageType { MessageType::Rstat }
    fn tag(&self) -> Tag { self.tag }

    fn encode(&self, buf: &mut Vec<u8>) -> IoResult<()> {
        write_u16(buf, self.tag)?;

        // Calculate stat size
        let mut stat_buf = Vec::new();
        write_u16(&mut stat_buf, self.stat.size)?;
        write_u16(&mut stat_buf, self.stat.typ)?;
        write_u32(&mut stat_buf, self.stat.dev)?;
        write_qid(&mut stat_buf, &self.stat.qid)?;
        write_u32(&mut stat_buf, self.stat.mode)?;
        write_u32(&mut stat_buf, self.stat.atime)?;
        write_u32(&mut stat_buf, self.stat.mtime)?;
        write_u64(&mut stat_buf, self.stat.length)?;
        write_string(&mut stat_buf, &self.stat.name)?;
        write_string(&mut stat_buf, &self.stat.uid)?;
        write_string(&mut stat_buf, &self.stat.gid)?;
        write_string(&mut stat_buf, &self.stat.muid)?;

        write_u16(buf, stat_buf.len() as u16)?;
        buf.extend_from_slice(&stat_buf);
        Ok(())
    }
}

// === Authentication ===

/// Authentication request
#[derive(Debug, Clone)]
pub struct Tauth {
    pub tag: Tag,
    pub afid: Fid,
    pub uname: String,
    pub aname: String,
}

impl Tauth {
    pub fn decode(buf: &[u8]) -> IoResult<Self> {
        let mut offset = 0;
        Ok(Self {
            tag: read_u16(buf, &mut offset)?,
            afid: read_u32(buf, &mut offset)?,
            uname: read_string(buf, &mut offset)?,
            aname: read_string(buf, &mut offset)?,
        })
    }
}

impl Message for Tauth {
    fn msg_type(&self) -> MessageType { MessageType::Tauth }
    fn tag(&self) -> Tag { self.tag }

    fn encode(&self, buf: &mut Vec<u8>) -> IoResult<()> {
        write_u16(buf, self.tag)?;
        write_u32(buf, self.afid)?;
        write_string(buf, &self.uname)?;
        write_string(buf, &self.aname)?;
        Ok(())
    }
}

/// Authentication response
#[derive(Debug, Clone)]
pub struct Rauth {
    pub tag: Tag,
    pub qid: Qid,
}

impl Rauth {
    pub fn decode(buf: &[u8]) -> IoResult<Self> {
        let mut offset = 0;
        Ok(Self {
            tag: read_u16(buf, &mut offset)?,
            qid: read_qid(buf, &mut offset)?,
        })
    }
}

impl Message for Rauth {
    fn msg_type(&self) -> MessageType { MessageType::Rauth }
    fn tag(&self) -> Tag { self.tag }

    fn encode(&self, buf: &mut Vec<u8>) -> IoResult<()> {
        write_u16(buf, self.tag)?;
        write_qid(buf, &self.qid)?;
        Ok(())
    }
}
