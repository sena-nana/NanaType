//! The `text.stroke_time_map` processor: stroke-order data laid over the
//! shaped text. Font geometry, shaping, ink colours and retirement stay owned
//! by NanaUI; caching of whole maps lives in the pipeline view.
use nana_ui::runtime::{StableNodeId, TextShaper, UiWorld};
use nana_ui_core::MaterialTexture;
use nanatype_theme::{StrokeCommand, StrokeData, StrokeGlyph};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
};

const CELL: usize = 96;
const MAX_SIDE: f32 = 2048.;
type CellKey = ([u8; 32], char);
static CELLS: LazyLock<Mutex<HashMap<CellKey, Arc<[u8]>>>> = LazyLock::new(Default::default);

/// Layout snapshot for `text.stroke_time_map` v1, taken on the UI thread.
/// Owned and `Send`: rasterising strokes and stamping cells run elsewhere.
pub(super) struct StrokeJob {
    data: Arc<StrokeData>,
    width: u32,
    height: u32,
    scale: f32,
    glyphs: Vec<(char, ttf_parser::Rect, tiny_skia::Transform)>,
}

/// Walk the shaped text once; only font boxes are read here.
pub(super) fn stroke_time_job(
    world: &UiWorld,
    id: StableNodeId,
    text: &nana_ui_core::RichText,
    data: &Arc<StrokeData>,
) -> Option<StrokeJob> {
    let (_, layout) = world.text_layout(id)?;
    let physical = layout.physical_size();
    let size = [physical.0.max(1.), physical.1.max(1.)];
    let scale = (MAX_SIDE / size[0].max(size[1])).min(1.5);
    let mut glyphs = Vec::new();
    let engine = nana_ui::NanaTextShaper::default().text_engine()?;
    let engine = engine.lock().unwrap_or_else(|e| e.into_inner());
    let vertical = layout.is_vertical();
    for line in &layout.lines {
        for run in layout.line_runs(line) {
            if run.is_object() {
                continue;
            }
            let Some(font) = engine.fonts().face_data(run.font) else {
                continue;
            };
            let Ok(face) = ttf_parser::Face::parse(font.bytes(), font.index()) else {
                continue;
            };
            let unit_scale = run.font_size_px / f32::from(face.units_per_em());
            let centre = layout.physical_x_of_block(line.metrics.baseline_y_px, physical.0);
            let mut pen = run.origin_x_px;
            for glyph in &run.glyphs {
                let character = text
                    .text()
                    .get(glyph.cluster as usize..)
                    .and_then(|s| s.chars().next());
                if let Some(character) = character
                    && let Some(bbox) =
                        face.glyph_bounding_box(ttf_parser::GlyphId(glyph.glyph_id as u16))
                {
                    let (x, y) = if vertical {
                        (centre + glyph.offset_x_px, pen - glyph.offset_y_px)
                    } else {
                        (
                            pen + glyph.offset_x_px,
                            line.metrics.baseline_y_px - glyph.offset_y_px,
                        )
                    };
                    let transform =
                        tiny_skia::Transform::from_row(unit_scale, 0., 0., -unit_scale, x, y);
                    glyphs.push((character, bbox, transform));
                }
                pen += glyph.advance_px;
            }
        }
    }
    Some(StrokeJob {
        data: data.clone(),
        width: (size[0] * scale).ceil() as u32,
        height: (size[1] * scale).ceil() as u32,
        scale,
        glyphs,
    })
}

impl StrokeJob {
    /// Per-texel writing time: alpha is the threshold, B marks stroke-covered
    /// cells; the extent maps target coordinates onto the texture.
    pub(super) fn run(self) -> (MaterialTexture, [f32; 4]) {
        let Self {
            data,
            width,
            height,
            scale,
            glyphs,
        } = self;
        let mut rgba = vec![0; width as usize * height as usize * 4];
        for (character, bbox, transform) in glyphs {
            // Ruby, Latin and inline objects have no Hanzi mask and use the normal fade.
            let Some(strokes) = data.glyph(character) else {
                continue;
            };
            let cell = {
                let mut cells = CELLS.lock().unwrap_or_else(|e| e.into_inner());
                if cells.len() >= 1024 {
                    cells.clear();
                }
                cells.get(&(data.hash, character)).cloned()
            };
            // Rasterise outside the lock so concurrent jobs do not serialise.
            let cell = cell.unwrap_or_else(|| {
                let cell: Arc<[u8]> = stroke_cell(&strokes).into();
                CELLS
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert((data.hash, character), cell.clone());
                cell
            });
            stamp(&mut rgba, width, height, scale, &cell, bbox, transform);
        }
        (
            MaterialTexture {
                size: [width, height],
                rgba: rgba.into(),
            },
            [width as f32 / scale, height as f32 / scale, 0., 0.],
        )
    }
}

fn stamp(
    out: &mut [u8],
    width: u32,
    height: u32,
    scale: f32,
    cell: &[u8],
    bbox: ttf_parser::Rect,
    transform: tiny_skia::Transform,
) {
    let Some(inverse) = transform.invert() else {
        return;
    };
    let mut corners = [
        tiny_skia::Point::from_xy(bbox.x_min as f32, bbox.y_min as f32),
        tiny_skia::Point::from_xy(bbox.x_max as f32, bbox.y_max as f32),
    ];
    transform.map_points(&mut corners);
    let x0 = (corners[0].x.min(corners[1].x) * scale).floor().max(0.) as u32;
    let x1 = (corners[0].x.max(corners[1].x) * scale)
        .ceil()
        .min(width as f32) as u32;
    let y0 = (corners[0].y.min(corners[1].y) * scale).floor().max(0.) as u32;
    let y1 = (corners[0].y.max(corners[1].y) * scale)
        .ceil()
        .min(height as f32) as u32;
    let bw = f32::from(bbox.x_max - bbox.x_min).max(1.);
    let bh = f32::from(bbox.y_max - bbox.y_min).max(1.);
    for y in y0..y1 {
        for x in x0..x1 {
            let mut p =
                tiny_skia::Point::from_xy((x as f32 + 0.5) / scale, (y as f32 + 0.5) / scale);
            inverse.map_point(&mut p);
            let u = ((p.x - bbox.x_min as f32) / bw * CELL as f32).clamp(0., (CELL - 1) as f32)
                as usize;
            let v = ((bbox.y_max as f32 - p.y) / bh * CELL as f32).clamp(0., (CELL - 1) as f32)
                as usize;
            let i = (y as usize * width as usize + x as usize) * 4;
            out[i..i + 4].copy_from_slice(&[255, 255, 255, cell[v * CELL + u]]);
        }
    }
}

fn distance(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> (f32, f32) {
    let d = [b[0] - a[0], b[1] - a[1]];
    let length2 = d[0] * d[0] + d[1] * d[1];
    let t = if length2 > 0. {
        ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / length2
    } else {
        0.
    }
    .clamp(0., 1.);
    (
        (p[0] - a[0] - d[0] * t).powi(2) + (p[1] - a[1] - d[1] * t).powi(2),
        t,
    )
}

fn stroke_cell(glyph: &StrokeGlyph) -> Vec<u8> {
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    // Outline bounds align the reference's ink rectangle to the actual glyph.
    let paths: Vec<_> = glyph.strokes.iter().map(|s| path(s)).collect();
    for p in paths.iter().flatten() {
        let b = p.bounds();
        min[0] = min[0].min(b.left());
        min[1] = min[1].min(b.top());
        max[0] = max[0].max(b.right());
        max[1] = max[1].max(b.bottom());
    }
    if !min[0].is_finite() {
        for p in glyph.medians.iter().flatten() {
            for i in 0..2 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
    }
    let sx = (CELL as f32 - 1.) / (max[0] - min[0]).max(1.);
    let sy = (CELL as f32 - 1.) / (max[1] - min[1]).max(1.);
    let tr = tiny_skia::Transform::from_row(sx, 0., 0., -sy, -min[0] * sx, max[1] * sy);
    let coverage: Vec<_> = paths
        .iter()
        .map(|p| {
            let mut bitmap = tiny_skia::Pixmap::new(CELL as u32, CELL as u32).unwrap();
            if let Some(p) = p {
                let mut paint = tiny_skia::Paint::default();
                paint.set_color_rgba8(255, 255, 255, 255);
                bitmap.fill_path(p, &paint, tiny_skia::FillRule::Winding, tr, None);
            }
            bitmap
        })
        .collect();
    let lengths: Vec<f32> = glyph
        .medians
        .iter()
        .map(|m| {
            m.windows(2)
                .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
                .sum::<f32>()
                .max(64.)
        })
        .collect();
    let total: f32 = lengths.iter().sum();
    let mut out = vec![0; CELL * CELL];
    for y in 0..CELL {
        for x in 0..CELL {
            let p = [min[0] + x as f32 / sx, max[1] - y as f32 / sy];
            let inside_any = coverage
                .iter()
                .any(|m| m.data()[(y * CELL + x) * 4 + 3] > 0);
            let mut best = f32::INFINITY;
            let mut at = 0.;
            let mut before = 0.;
            for (i, median) in glyph.medians.iter().enumerate() {
                if !inside_any || coverage[i].data()[(y * CELL + x) * 4 + 3] > 0 {
                    let mut along = 0.;
                    for w in median.windows(2) {
                        let len = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
                        let (d, t) = distance(p, w[0], w[1]);
                        if d < best {
                            best = d;
                            at = before + (along + t * len).min(lengths[i]);
                        }
                        along += len;
                    }
                }
                before += lengths[i];
            }
            // Every point has a time, including ink outside reference outlines.
            out[y * CELL + x] = (4. + at / total * 246.).round().clamp(4., 250.) as u8;
        }
    }
    out
}

fn path(commands: &[StrokeCommand]) -> Option<tiny_skia::Path> {
    use StrokeCommand::*;
    let mut p = tiny_skia::PathBuilder::new();
    for command in commands {
        match *command {
            Move([x, y]) => p.move_to(x, y),
            Line([x, y]) => p.line_to(x, y),
            Quad([[x1, y1], [x, y]]) => p.quad_to(x1, y1, x, y),
            Cubic([[x1, y1], [x2, y2], [x, y]]) => p.cubic_to(x1, y1, x2, y2, x, y),
            Close => p.close(),
        }
    }
    p.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stroke_times_follow_order_and_cover_the_entire_font_rectangle() {
        use StrokeCommand::*;
        let glyph = StrokeGlyph {
            strokes: vec![
                vec![
                    Move([0., 80.]),
                    Line([100., 80.]),
                    Line([100., 100.]),
                    Line([0., 100.]),
                    Close,
                ],
                vec![
                    Move([40., 0.]),
                    Line([60., 0.]),
                    Line([60., 70.]),
                    Line([40., 70.]),
                    Close,
                ],
            ],
            medians: vec![vec![[0., 90.], [100., 90.]], vec![[50., 70.], [50., 0.]]],
        };
        let cell = stroke_cell(&glyph);
        assert!(cell.iter().all(|v| (4..=250).contains(v)));
        assert!(cell[5 * CELL + 5] < cell[5 * CELL + 80]);
        assert!(cell[5 * CELL + 80] < cell[90 * CELL + 48]);
    }
}
