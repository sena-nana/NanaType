//! NTHZ v1 container: metadata, Unicode index, block table, zlib payloads.
//! Glyph format 2 stores absolute MLQCZ commands as axis deltas, with
//! quarter-unit fixed-point medians. Storage order never changes stroke order.
use super::{StrokeCommand, StrokeData, StrokeGlyph};
use serde::Deserialize;
use std::{
    collections::VecDeque,
    io::Read as _,
    sync::{Arc, Mutex, OnceLock},
};

pub(super) const MAGIC: &[u8; 8] = b"NTHZ\0\0\0\x01";
const GROUP: usize = 64;
const CACHE_BLOCKS: usize = 8;
const MAX_RAW: usize = 32 * 1024 * 1024;
const MAX_BLOCK: usize = 2 * 1024 * 1024;

#[derive(Debug)]
struct Entry {
    character: char,
    offset: usize,
    len: usize,
}

#[derive(Debug)]
struct Block {
    offset: usize,
    len: usize,
    raw_len: usize,
}

type Glyphs = Vec<Arc<StrokeGlyph>>;

#[derive(Debug)]
pub(super) struct Packed {
    entries: Vec<Entry>,
    blocks: Vec<Block>,
    payload: Box<[u8]>,
    cache: Mutex<VecDeque<(usize, Glyphs)>>,
    verified: OnceLock<Result<(), String>>,
}

#[derive(Deserialize)]
struct Metadata {
    format: u32,
    notice: String,
    source: String,
    source_sha256: String,
}

impl Packed {
    pub(super) fn parse(bytes: &[u8]) -> Result<StrokeData, String> {
        let mut reader = Reader { bytes, at: 8 };
        let metadata_len = reader.u32()? as usize;
        let count = reader.u32()? as usize;
        let block_count = reader.u32()? as usize;
        let group = reader.u32()? as usize;
        if metadata_len > 16 * 1024
            || !(1..=12_000).contains(&count)
            || group != GROUP
            || block_count != count.div_ceil(GROUP)
        {
            return Err("invalid stroke container header".into());
        }
        let metadata: Metadata =
            serde_json::from_slice(reader.bytes(metadata_len)?).map_err(|e| e.to_string())?;
        if metadata.format != 2 {
            return Err("packed stroke data requires format 2".into());
        }
        let mut entries: Vec<Entry> = Vec::with_capacity(count);
        for _ in 0..count {
            let character = char::from_u32(reader.u32()?).ok_or("invalid stroke codepoint")?;
            if entries.last().is_some_and(|e| e.character >= character) {
                return Err("stroke index must contain unique sorted characters".into());
            }
            entries.push(Entry {
                character,
                offset: reader.u32()? as usize,
                len: reader.u32()? as usize,
            });
        }
        let mut blocks = Vec::with_capacity(block_count);
        let (mut compressed_total, mut raw_total) = (0usize, 0usize);
        for _ in 0..block_count {
            let block = Block {
                offset: reader.u32()? as usize,
                len: reader.u32()? as usize,
                raw_len: reader.u32()? as usize,
            };
            if block.offset != compressed_total
                || block.len == 0
                || block.len > MAX_RAW
                || block.raw_len == 0
                || block.raw_len > MAX_BLOCK
                || compressed_total > MAX_RAW - block.len
                || raw_total > MAX_RAW - block.raw_len
            {
                return Err("invalid stroke block bounds".into());
            }
            compressed_total += block.len;
            raw_total += block.raw_len;
            blocks.push(block);
        }
        if raw_total > MAX_RAW || compressed_total != bytes.len().saturating_sub(reader.at) {
            return Err("stroke payload length exceeds bounds".into());
        }
        for (block, entries) in blocks.iter().zip(entries.chunks(GROUP)) {
            let mut offset = 0;
            for entry in entries {
                if entry.offset != offset || entry.len == 0 || entry.len > MAX_BLOCK {
                    return Err("invalid stroke glyph bounds".into());
                }
                offset += entry.len;
                if offset > block.raw_len {
                    return Err("stroke glyph exceeds its block".into());
                }
            }
            if offset != block.raw_len {
                return Err("stroke block contains unindexed bytes".into());
            }
        }
        Ok(StrokeData {
            hash: *blake3::hash(bytes).as_bytes(),
            format: metadata.format,
            notice: metadata.notice,
            source: metadata.source,
            source_sha256: metadata.source_sha256,
            storage: Self {
                entries,
                blocks,
                payload: bytes[reader.at..].into(),
                cache: Mutex::new(VecDeque::new()),
                verified: OnceLock::new(),
            },
        })
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn validate_all(&self) -> Result<(), String> {
        self.verified
            .get_or_init(|| {
                for block in 0..self.blocks.len() {
                    // Drop each validated block immediately, keeping the cache cold.
                    self.unpack(block)?;
                }
                Ok(())
            })
            .clone()
    }

    pub(super) fn glyph(&self, character: char) -> Result<Option<Arc<StrokeGlyph>>, String> {
        let Ok(index) = self
            .entries
            .binary_search_by_key(&character, |e| e.character)
        else {
            return Ok(None);
        };
        let block = index / GROUP;
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let glyphs = if let Some(position) = cache.iter().position(|(id, _)| *id == block) {
            cache.remove(position).unwrap().1
        } else {
            self.unpack(block)?
        };
        let glyph = glyphs[index % GROUP].clone();
        cache.push_back((block, glyphs));
        while cache.len() > CACHE_BLOCKS {
            cache.pop_front();
        }
        Ok(Some(glyph))
    }

    fn unpack(&self, index: usize) -> Result<Vec<Arc<StrokeGlyph>>, String> {
        let block = &self.blocks[index];
        let compressed = &self.payload[block.offset..block.offset + block.len];
        let mut decoder = flate2::bufread::ZlibDecoder::new(compressed);
        let mut raw = Vec::with_capacity(block.raw_len);
        decoder
            .by_ref()
            .take(block.raw_len as u64 + 1)
            .read_to_end(&mut raw)
            .map_err(|e| format!("invalid stroke block {index}: {e}"))?;
        if raw.len() != block.raw_len || decoder.total_in() != block.len as u64 {
            return Err(format!("invalid decoded stroke block {index} length"));
        }
        self.entries[index * GROUP..((index + 1) * GROUP).min(self.entries.len())]
            .iter()
            .map(|entry| {
                decode_glyph(&raw[entry.offset..entry.offset + entry.len])
                    .map(Arc::new)
                    .map_err(|e| format!("invalid stroke geometry for `{}`: {e}", entry.character))
            })
            .collect()
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(len).ok_or("stroke offset overflow")?;
        let bytes = self
            .bytes
            .get(self.at..end)
            .ok_or("truncated stroke data")?;
        self.at = end;
        Ok(bytes)
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    fn varint(&mut self) -> Result<u32, String> {
        let mut value = 0;
        for shift in (0..=28).step_by(7) {
            let byte = self.bytes(1)?[0];
            if shift == 28 && byte > 15 {
                return Err("stroke varint overflow".into());
            }
            value |= u32::from(byte & 127) << shift;
            if byte < 128 {
                return Ok(value);
            }
        }
        Err("unterminated stroke varint".into())
    }

    fn count(&mut self, min: usize, max: usize) -> Result<usize, String> {
        let value = self.varint()? as usize;
        if !(min..=max).contains(&value) {
            return Err("stroke item count exceeds bounds".into());
        }
        Ok(value)
    }

    fn coordinate(&mut self, previous: i32, scale: i32) -> Result<i32, String> {
        let value = self.varint()?;
        let delta = ((value >> 1) as i32) ^ -((value & 1) as i32);
        let value = previous
            .checked_add(delta)
            .ok_or("stroke coordinate overflow")?;
        if !(-2048 * scale..=3072 * scale).contains(&value) {
            return Err("stroke coordinate exceeds bounds".into());
        }
        Ok(value)
    }

    fn point(&mut self, previous: &mut [i32; 2], scale: i32) -> Result<[f32; 2], String> {
        for value in previous.iter_mut() {
            *value = self.coordinate(*value, scale)?;
        }
        Ok(previous.map(|value| value as f32 / scale as f32))
    }
}

fn decode_glyph(bytes: &[u8]) -> Result<StrokeGlyph, String> {
    let mut reader = Reader { bytes, at: 0 };
    let count = reader.count(1, 64)?;
    let mut glyph = StrokeGlyph {
        strokes: Vec::with_capacity(count),
        medians: Vec::with_capacity(count),
    };
    for _ in 0..count {
        let count = reader.count(1, 4096)?;
        let mut path = Vec::with_capacity(count);
        let mut previous = [0; 2];
        for _ in 0..count {
            let code = reader.bytes(1)?[0];
            let mut point = || reader.point(&mut previous, 1);
            let command = match code {
                0 => StrokeCommand::Move(point()?),
                1 => StrokeCommand::Line(point()?),
                2 => StrokeCommand::Quad([point()?, point()?]),
                3 => StrokeCommand::Cubic([point()?, point()?, point()?]),
                4 => StrokeCommand::Close,
                _ => return Err("unknown stroke command".into()),
            };
            path.push(command);
        }
        if !matches!(path.first(), Some(StrokeCommand::Move(_))) {
            return Err("stroke must start with a move command".into());
        }
        glyph.strokes.push(path);
        let count = reader.count(2, 512)?;
        let mut median = Vec::with_capacity(count);
        let mut previous = [0; 2];
        for _ in 0..count {
            median.push(reader.point(&mut previous, 4)?);
        }
        glyph.medians.push(median);
    }
    if reader.at != bytes.len() {
        return Err("trailing stroke bytes".into());
    }
    Ok(glyph)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUILTIN: &[u8] = include_bytes!("../../builtin/love_letter/strokes.nthz");

    fn packed(data: &StrokeData) -> &Packed {
        &data.storage
    }

    #[test]
    fn index_load_and_full_validation_leave_cache_cold_then_reuse_eight_blocks() {
        let data = StrokeData::parse(BUILTIN).unwrap();
        let store = packed(&data);
        assert_eq!(data.format, 2);
        assert_eq!(data.len(), 6763);
        assert!(store.cache.lock().unwrap().is_empty());
        data.validate_all().unwrap();
        assert!(store.cache.lock().unwrap().is_empty());
        assert!(data.glyph('🙂').is_none());
        assert!(store.cache.lock().unwrap().is_empty());
        let first = data.glyph(store.entries[0].character).unwrap();
        assert!(Arc::ptr_eq(
            &first,
            &data.glyph(store.entries[0].character).unwrap()
        ));
        for entry in store.entries.iter().step_by(GROUP) {
            assert!(data.glyph(entry.character).is_some());
            assert!(store.cache.lock().unwrap().len() <= CACHE_BLOCKS);
        }
        assert!(!first.strokes.is_empty()); // returned glyph survives eviction
        for character in "情书喜欢心永远".chars() {
            assert!(data.glyph(character).is_some());
        }
    }

    #[test]
    fn malformed_headers_indices_blocks_and_compressed_payloads_are_rejected() {
        for end in [0, 7, 8, 16, 24, 100, BUILTIN.len() - 1] {
            assert!(StrokeData::parse(&BUILTIN[..end]).is_err());
        }
        let metadata_len = u32::from_le_bytes(BUILTIN[8..12].try_into().unwrap()) as usize;
        let index = 24 + metadata_len;
        let count = u32::from_le_bytes(BUILTIN[12..16].try_into().unwrap()) as usize;
        let block_count = count.div_ceil(GROUP);
        let table = index + count * 12;
        let payload = table + block_count * 12;
        for (offset, value) in [
            (12, 12_001u32),
            (20, 65),
            (index, 0xd800),
            (index + 4, 1),
            (index + 8, u32::MAX),
            (table, 1),
            (table + 4, u32::MAX),
            (table + 8, MAX_BLOCK as u32 + 1),
        ] {
            let mut corrupt = BUILTIN.to_vec();
            corrupt[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(StrokeData::parse(&corrupt).is_err(), "offset {offset}");
        }
        let mut duplicate = BUILTIN.to_vec();
        duplicate[index + 12..index + 16].copy_from_slice(&BUILTIN[index..index + 4]);
        assert!(StrokeData::parse(&duplicate).is_err());
        let mut corrupt = BUILTIN.to_vec();
        corrupt[payload + 10] ^= 0xff;
        let data = StrokeData::parse(&corrupt).unwrap();
        assert!(data.validate_all().is_err());
        assert!(packed(&data).cache.lock().unwrap().is_empty());
        let mut trailing = BUILTIN.to_vec();
        trailing.push(0);
        assert!(StrokeData::parse(&trailing).is_err());
        // The import path must request full validation, including for a resource
        // with a structurally valid index but corrupted compressed bytes.
        let spec = crate::DataSpec {
            path: "bad.nthz".into(),
            format: crate::DataFormat::Nthz,
            format_version: 1,
            source: "test".into(),
            license: "test".into(),
            constraints: None,
        };
        assert!(crate::parse_data(&spec, &corrupt, true).is_err());
    }

    #[test]
    fn glyph_decoder_rejects_invalid_counts_commands_varints_and_trailing_bytes() {
        // Produced by Python encode_glyph: covers every command, signed axis
        // deltas, per-stroke resets and exact quarter-unit median coordinates.
        let fixture = [
            2, 5, 0, 20, 40, 1, 19, 49, 2, 2, 14, 4, 4, 3, 4, 4, 4, 4, 4, 4, 4, 2, 2, 6, 10, 2, 3,
            0, 3, 6, 1, 12, 15, 4, 2, 13, 20, 22, 4,
        ];
        let decoded = decode_glyph(&fixture).unwrap();
        use StrokeCommand::*;
        assert_eq!(
            decoded.strokes,
            [
                vec![
                    Move([10., 20.]),
                    Line([0., -5.]),
                    Quad([[1., 2.], [3., 4.]]),
                    Cubic([[5., 6.], [7., 8.], [9., 10.]]),
                    Close
                ],
                vec![Move([-2., 3.]), Line([4., -5.]), Close],
            ]
        );
        assert_eq!(
            decoded.medians,
            [vec![[0.25, 0.75], [1.5, 1.]], vec![[-1.75, 2.5], [1., 3.]],]
        );
        // One M command and two median points at zero. Degenerate geometry
        // remains valid; the renderer can use its median fallback.
        let valid = [1, 1, 0, 0, 0, 2, 0, 0, 0, 0];
        assert!(decode_glyph(&valid).is_ok());
        for (offset, value) in [(0, 0), (0, 65), (2, 1), (2, 9), (5, 1)] {
            let mut bytes = valid;
            bytes[offset] = value;
            assert!(decode_glyph(&bytes).is_err());
        }
        assert!(decode_glyph(&[1, 1, 0, 255, 255, 255, 255, 255]).is_err());
        assert!(decode_glyph(&valid[..9]).is_err());
        assert!(decode_glyph(&[valid.as_slice(), &[0]].concat()).is_err());
        // A declared raw length cannot permit a decompression bomb: decoding
        // stops after that length + 1, even before checksum/geometry checks.
        use std::io::Write as _;
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&vec![0; 1_000_000]).unwrap();
        let payload = encoder.finish().unwrap();
        let store = Packed {
            entries: vec![Entry {
                character: '心',
                offset: 0,
                len: 10,
            }],
            blocks: vec![Block {
                offset: 0,
                len: payload.len(),
                raw_len: 10,
            }],
            payload: payload.into_boxed_slice(),
            cache: Mutex::new(VecDeque::new()),
            verified: OnceLock::new(),
        };
        assert!(store.unpack(0).is_err());
    }
}
