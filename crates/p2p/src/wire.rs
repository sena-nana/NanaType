//! Steam Networking Sockets delivers one message at a time, and one message
//! cannot exceed 512 KiB. Frames and pictures are split here and reassembled
//! before the session sees them. Message boundaries stay intact.

use crate::picture::WIRE_ASSET;
use crate::protocol::{self, MAX_FRAME};

/// `k_cbMaxSteamNetworkingSocketsMessageSizeSend`.
pub const MESSAGE_MAX: usize = 512 * 1024;

const HEADER: usize = 6;
const FIRST: u8 = 1;
const LAST: u8 = 2;
const KIND_FRAME: u8 = 1;
const KIND_WANT: u8 = 2;
const KIND_ASSET: u8 = 3;

/// One complete session payload, before it is split into Steam messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packet {
    /// One zlib-compressed control frame.
    Frame(Vec<u8>),
    /// Ask the peer for a picture, by its blake3 hex.
    Want(String),
    /// The picture bytes named by `hash`.
    Asset { hash: String, bytes: Vec<u8> },
}

/// Split `packet` into Steam messages that share `id`.
pub fn chunks(id: u32, packet: &Packet) -> Result<Vec<Vec<u8>>, String> {
    let (kind, hash, body, cap) = match packet {
        Packet::Frame(bytes) => {
            if bytes.is_empty() || bytes.len() > MAX_FRAME {
                return Err("Session frame exceeds limit".into());
            }
            (KIND_FRAME, None, bytes.as_slice(), MAX_FRAME)
        }
        Packet::Want(hash) => {
            if !protocol::valid_hash(hash) {
                return Err("Invalid asset hash".into());
            }
            (KIND_WANT, None, hash.as_bytes(), 64)
        }
        Packet::Asset { hash, bytes } => {
            if !protocol::valid_hash(hash) || bytes.len() > WIRE_ASSET {
                return Err("Asset exceeds limit".into());
            }
            (
                KIND_ASSET,
                Some(hash.as_bytes()),
                bytes.as_slice(),
                WIRE_ASSET,
            )
        }
    };
    if body.len() > cap {
        return Err("Session message exceeds limit".into());
    }
    let mut out = Vec::new();
    let mut offset = 0;
    let mut first = true;
    while first || offset < body.len() {
        let hash_len = if first && hash.is_some() { 64 } else { 0 };
        let room = MESSAGE_MAX - HEADER - hash_len;
        let n = (body.len() - offset).min(room);
        let last = offset + n == body.len();
        let mut message = Vec::with_capacity(HEADER + hash_len + n);
        message.push(kind);
        message.push(if first { FIRST } else { 0 } | if last { LAST } else { 0 });
        message.extend_from_slice(&id.to_be_bytes());
        if let Some(hash) = hash.filter(|_| first) {
            message.extend_from_slice(hash);
        }
        message.extend_from_slice(&body[offset..offset + n]);
        if message.len() > MESSAGE_MAX {
            return Err("Session message exceeds limit".into());
        }
        out.push(message);
        offset += n;
        first = false;
        if last {
            break;
        }
    }
    Ok(out)
}

#[derive(Default)]
struct Partial {
    id: u32,
    kind: u8,
    hash: Option<String>,
    buf: Vec<u8>,
    open: bool,
}

/// Reassembles one direction. Control frames and pictures reassemble apart,
/// because they travel on different lanes and may interleave.
#[derive(Default)]
pub struct Assembler {
    frame: Partial,
    asset: Partial,
}

impl Assembler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, message: &[u8]) -> Result<Option<Packet>, String> {
        if message.len() < HEADER || message.len() > MESSAGE_MAX {
            return Err("Session message exceeds limit".into());
        }
        let kind = message[0];
        let flags = message[1];
        let id = u32::from_be_bytes(message[2..6].try_into().expect("header"));
        let first = flags & FIRST != 0;
        let last = flags & LAST != 0;
        let rest = &message[HEADER..];
        let slot = match kind {
            KIND_FRAME | KIND_WANT => &mut self.frame,
            KIND_ASSET => &mut self.asset,
            _ => return Err("Unexpected session message".into()),
        };
        if first {
            if slot.open {
                return Err("Session message is incomplete".into());
            }
            let (hash, body) = if kind == KIND_ASSET {
                if rest.len() < 64 {
                    return Err("Invalid asset hash".into());
                }
                let hash = std::str::from_utf8(&rest[..64])
                    .map_err(|_| "Invalid asset hash".to_string())?;
                if !protocol::valid_hash(hash) {
                    return Err("Invalid asset hash".into());
                }
                (Some(hash.to_owned()), &rest[64..])
            } else {
                (None, rest)
            };
            slot.id = id;
            slot.kind = kind;
            slot.hash = hash;
            slot.buf = body.to_vec();
            slot.open = !last;
        } else {
            if !slot.open || slot.id != id || slot.kind != kind {
                return Err("Session message is incomplete".into());
            }
            append(&mut slot.buf, rest, cap_for(kind))?;
            if last {
                slot.open = false;
            }
        }
        if !last {
            append_cap(&slot.buf, kind)?;
            return Ok(None);
        }
        let kind = slot.kind;
        let hash = slot.hash.take();
        let buf = std::mem::take(&mut slot.buf);
        slot.open = false;
        finish(kind, hash, buf).map(Some)
    }
}

fn cap_for(kind: u8) -> usize {
    match kind {
        KIND_ASSET => WIRE_ASSET,
        KIND_WANT => 64,
        _ => MAX_FRAME,
    }
}

fn append(buf: &mut Vec<u8>, extra: &[u8], cap: usize) -> Result<(), String> {
    if buf.len().saturating_add(extra.len()) > cap {
        return Err("Session message exceeds limit".into());
    }
    buf.extend_from_slice(extra);
    Ok(())
}

fn append_cap(buf: &[u8], kind: u8) -> Result<(), String> {
    if buf.len() > cap_for(kind) {
        Err("Session message exceeds limit".into())
    } else {
        Ok(())
    }
}

fn finish(kind: u8, hash: Option<String>, buf: Vec<u8>) -> Result<Packet, String> {
    match kind {
        KIND_FRAME => {
            if buf.is_empty() || buf.len() > MAX_FRAME {
                return Err("Session frame exceeds limit".into());
            }
            Ok(Packet::Frame(buf))
        }
        KIND_WANT => {
            let hash = String::from_utf8(buf).map_err(|_| "Invalid asset hash".to_string())?;
            if !protocol::valid_hash(&hash) {
                return Err("Invalid asset hash".into());
            }
            Ok(Packet::Want(hash))
        }
        KIND_ASSET => {
            let hash = hash.ok_or("Invalid asset hash")?;
            if buf.len() > WIRE_ASSET || blake3::hash(&buf).to_hex().as_str() != hash {
                return Err("Asset hash mismatch".into());
            }
            Ok(Packet::Asset { hash, bytes: buf })
        }
        _ => Err("Unexpected session message".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(packet: &Packet) {
        let parts = chunks(7, packet).unwrap();
        assert!(parts.iter().all(|part| part.len() <= MESSAGE_MAX));
        let mut asm = Assembler::new();
        let mut done = None;
        for (i, part) in parts.iter().enumerate() {
            let got = asm.push(part).unwrap();
            if i + 1 == parts.len() {
                done = got;
            } else {
                assert!(got.is_none());
            }
        }
        assert_eq!(done.as_ref(), Some(packet));
    }

    #[test]
    fn small_frame_is_one_message() {
        let packet = Packet::Frame(vec![0x78, 1, 2, 3]);
        let parts = chunks(1, &packet).unwrap();
        assert_eq!(parts.len(), 1);
        roundtrip(&packet);
    }

    #[test]
    fn frame_larger_than_one_steam_message_roundtrips() {
        roundtrip(&Packet::Frame(vec![0x78; MESSAGE_MAX + 1000]));
    }

    #[test]
    fn asset_at_the_steam_limit_roundtrips_and_checks_its_hash() {
        let bytes = vec![9u8; WIRE_ASSET];
        let hash = blake3::hash(&bytes).to_hex().to_string();
        let packet = Packet::Asset {
            hash: hash.clone(),
            bytes,
        };
        assert!(chunks(3, &packet).unwrap().len() > 1);
        roundtrip(&packet);
        roundtrip(&Packet::Want(hash));
    }

    #[test]
    fn a_truncated_message_does_not_finish() {
        let packet = Packet::Frame(vec![1; 32]);
        let mut part = chunks(1, &packet).unwrap().remove(0);
        part[1] &= !super::LAST;
        let mut asm = Assembler::new();
        assert!(asm.push(&part).unwrap().is_none());
        assert!(asm.push(&part).is_err());
    }
}
