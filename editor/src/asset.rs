use std::sync::Arc;
pub struct Preview {
    pub path: String,
    pub image: Option<String>,
    pub dimensions: Option<[u32; 2]>,
    pub font: Option<Arc<nanatype_frame::ui::preview::PreviewFont>>,
    pub pcm: Option<Arc<nanatype_audio::Pcm>>,
}
pub fn prepare(path: &str, bytes: Vec<u8>) -> Result<Preview, String> {
    use nanatype_theme::editor::{AssetKind, describe};
    let mut result = Preview {
        path: path.into(),
        image: None,
        dimensions: None,
        font: None,
        pcm: None,
    };
    match describe(path, &bytes).0 {
        AssetKind::Image => {
            let image = nanatype_frame::assets::decode_image(&bytes, 8192, 128 * 1024 * 1024)
                .map_err(|e| e.to_string())?;
            result.dimensions = Some([image.width(), image.height()]);
            result.image = nanatype_frame::media::global().and_then(|m| m.bytes_url(path, &bytes));
        }
        AssetKind::Font => {
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash(&bytes, &mut hash);
            result.font = nanatype_frame::ui::preview::preview_font(
                (path.into(), std::hash::Hasher::finish(&hash)),
                || Some(bytes),
            );
            if result.font.is_none() {
                return Err("font could not be prepared".into());
            }
        }
        AssetKind::Audio => {
            result.pcm = Some(Arc::new(
                nanatype_audio::decode(
                    bytes,
                    std::path::Path::new(path)
                        .extension()
                        .and_then(|s| s.to_str()),
                )
                .map_err(|e| e.to_string())?,
            ))
        }
        _ => {}
    }
    Ok(result)
}
