//! Offline stroke-order data (the NTHZ data format). Geometry keeps its
//! upstream order; packages declare it as `[data.<id>]` with `format = "nthz"`.
mod packed;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub enum StrokeCommand {
    Move([f32; 2]),
    Line([f32; 2]),
    Quad([[f32; 2]; 2]),
    Cubic([[f32; 2]; 3]),
    Close,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StrokeGlyph {
    pub strokes: Vec<Vec<StrokeCommand>>,
    pub medians: Vec<Vec<[f32; 2]>>,
}

#[derive(Debug)]
pub struct StrokeData {
    pub hash: [u8; 32],
    pub format: u32,
    pub notice: String,
    pub source: String,
    pub source_sha256: String,
    storage: packed::Packed,
}

impl PartialEq for StrokeData {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}

impl StrokeData {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 32 * 1024 * 1024 {
            return Err("stroke data exceeds 32 MiB".into());
        }
        if !bytes.starts_with(packed::MAGIC) {
            return Err("stroke data requires the NTHZ v1 binary container".into());
        }
        packed::Packed::parse(bytes)
    }

    pub fn len(&self) -> usize {
        self.storage.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Only the requested 64-character block is decompressed. Eight blocks are
    /// retained; already returned glyphs remain valid when their block is evicted.
    pub fn glyph(&self, character: char) -> Option<Arc<StrokeGlyph>> {
        self.storage.glyph(character).ok().flatten()
    }

    /// Imports validate every block without populating the runtime cache.
    pub fn validate_all(&self) -> Result<(), String> {
        self.storage.validate_all()
    }
}
