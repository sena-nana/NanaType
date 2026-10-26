//! Pictures leave the machine smaller than they are stored.
//!
//! A still image is scaled to the size a partner window actually draws, then
//! encoded as WebP. Lossless WebP is kept when it fits in one Steam message,
//! so pixel art stays sharp; otherwise a lossy WebP with alpha is used.
//! WebP has no scan that a partner can paint halfway through, so a small still
//! preview travels with the track and the full file follows on its own stream.
//! Short animations are re-encoded as GIF, which the display already plays.
//! Longer animations, and anything that is not a still or a short GIF/WebP,
//! contribute their first frame only.

use std::io::Cursor;

use image::codecs::gif::{GifEncoder, Repeat};
use image::{
    AnimationDecoder, ExtendedColorType, ImageDecoder, ImageFormat, ImageReader, RgbaImage,
};

use crate::protocol::MAX_ASSET;

/// Budget of one full picture: one Steam message of bytes. With the chunk
/// header and hash it travels as two messages.
pub const WIRE_ASSET: usize = 512 * 1024;
pub const PREVIEW_MAX: usize = 24 * 1024;

const AVATAR_EDGE: u32 = 256;
const PICTURE_EDGE: u32 = 1280;
const PREVIEW_EDGE: u32 = 160;
const ANIM_EDGE: u32 = 320;
const MAX_ANIM_FRAMES: usize = 24;
const MAX_SOURCE_EDGE: u32 = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    Avatar,
    Picture,
}

impl Role {
    fn edge(self) -> u32 {
        match self {
            Self::Avatar => AVATAR_EDGE,
            Self::Picture => PICTURE_EDGE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Webp,
    Gif,
}

pub(crate) struct Encoded {
    pub bytes: Vec<u8>,
    pub extension: &'static str,
    pub preview: Vec<u8>,
}

struct Decoded {
    still: RgbaImage,
    animation: Option<Vec<(RgbaImage, u32)>>,
}

pub(crate) fn wire_kind(bytes: &[u8]) -> Option<Kind> {
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(Kind::Webp)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(Kind::Gif)
    } else {
        None
    }
}

/// How each resource string in a track should be scaled.
pub(crate) fn roles(value: &serde_json::Value) -> std::collections::BTreeMap<String, Role> {
    let mut roles = std::collections::BTreeMap::new();
    walk(value, &mut roles);
    roles
}

fn walk(value: &serde_json::Value, roles: &mut std::collections::BTreeMap<String, Role>) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, value) in object {
                if let Some(reference) = value.as_str() {
                    if key == "avatar" {
                        note(roles, reference, Role::Avatar);
                    } else if key == "asset" {
                        note(roles, reference, Role::Picture);
                    }
                }
                walk(value, roles);
            }
        }
        serde_json::Value::Array(items) => {
            for value in items {
                walk(value, roles);
            }
        }
        _ => {}
    }
}

fn note(roles: &mut std::collections::BTreeMap<String, Role>, reference: &str, role: Role) {
    match roles.get(reference) {
        Some(Role::Picture) => {}
        _ => {
            roles.insert(reference.to_owned(), role);
        }
    }
}

pub(crate) fn encode_picture(bytes: &[u8], role: Role) -> Result<Encoded, String> {
    if bytes.len() > MAX_ASSET {
        return Err("image exceeds source limit".into());
    }
    let Decoded { still, animation } = decode(bytes)?;
    let preview = preview_webp(&still)?;
    let (payload, extension) = match animation {
        Some(frames) => match encode_gif(&frames, role.edge().min(ANIM_EDGE)) {
            Ok(gif) if gif.len() <= WIRE_ASSET && wire_kind(&gif) == Some(Kind::Gif) => {
                (gif, "gif")
            }
            _ => (still_webp(&still, role.edge())?, "webp"),
        },
        None => (still_webp(&still, role.edge())?, "webp"),
    };
    if payload.len() > WIRE_ASSET || wire_kind(&payload).is_none() {
        return Err("encoded picture exceeds budget".into());
    }
    Ok(Encoded {
        bytes: payload,
        extension,
        preview,
    })
}

fn decode(bytes: &[u8]) -> Result<Decoded, String> {
    let format = image::guess_format(bytes).map_err(|e| e.to_string())?;
    match format {
        ImageFormat::Gif => decode_animation(GifSource::Gif, bytes),
        ImageFormat::WebP => decode_animation(GifSource::WebP, bytes),
        _ => Ok(Decoded {
            still: decode_still(bytes)?,
            animation: None,
        }),
    }
}

enum GifSource {
    Gif,
    WebP,
}

fn decode_animation(source: GifSource, bytes: &[u8]) -> Result<Decoded, String> {
    let cursor = Cursor::new(bytes);
    let mut collected = Vec::new();
    let mut extra = false;
    match source {
        GifSource::Gif => {
            let decoder = image::codecs::gif::GifDecoder::new(cursor).map_err(|e| e.to_string())?;
            guard(decoder.dimensions())?;
            for frame in decoder.into_frames() {
                if push_frame(
                    &mut collected,
                    frame.map_err(|e| e.to_string())?,
                    &mut extra,
                )? {
                    break;
                }
            }
        }
        GifSource::WebP => {
            let decoder =
                image::codecs::webp::WebPDecoder::new(cursor).map_err(|e| e.to_string())?;
            if !decoder.has_animation() {
                return Ok(Decoded {
                    still: decode_still(bytes)?,
                    animation: None,
                });
            }
            guard(decoder.dimensions())?;
            for frame in decoder.into_frames() {
                if push_frame(
                    &mut collected,
                    frame.map_err(|e| e.to_string())?,
                    &mut extra,
                )? {
                    break;
                }
            }
        }
    }
    let Some((first, _)) = collected.first() else {
        return Err("image has no frames".into());
    };
    if extra || collected.len() < 2 {
        return Ok(Decoded {
            still: first.clone(),
            animation: None,
        });
    }
    Ok(Decoded {
        still: first.clone(),
        animation: Some(collected),
    })
}

fn push_frame(
    collected: &mut Vec<(RgbaImage, u32)>,
    frame: image::Frame,
    extra: &mut bool,
) -> Result<bool, String> {
    if collected.len() >= MAX_ANIM_FRAMES {
        *extra = true;
        return Ok(true);
    }
    let (numer, denom) = frame.delay().numer_denom_ms();
    let ms = u32::try_from(u64::from(numer) / u64::from(denom.max(1)).max(1)).unwrap_or(100);
    let image = frame.into_buffer();
    guard(image.dimensions())?;
    collected.push((image, ms.clamp(10, 1_000)));
    Ok(false)
}

fn decode_still(bytes: &[u8]) -> Result<RgbaImage, String> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_EDGE);
    limits.max_image_height = Some(MAX_SOURCE_EDGE);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(|e| e.to_string())?;
    guard((image.width(), image.height()))?;
    Ok(image.to_rgba8())
}

fn guard(size: (u32, u32)) -> Result<(), String> {
    if size.0 == 0 || size.1 == 0 || size.0 > MAX_SOURCE_EDGE || size.1 > MAX_SOURCE_EDGE {
        Err("image dimensions are not usable".into())
    } else {
        Ok(())
    }
}

fn still_webp(img: &RgbaImage, max_edge: u32) -> Result<Vec<u8>, String> {
    let fitted = downscale(img, max_edge);
    let lossless = lossless_webp(&fitted)?;
    if lossless.len() <= WIRE_ASSET {
        return Ok(lossless);
    }
    let mut edge = fitted.width().max(fitted.height()).max(1);
    let mut current = fitted;
    loop {
        for quality in [75.0, 55.0, 35.0, 15.0] {
            let bytes = lossy_webp(&current, quality)?;
            if bytes.len() <= WIRE_ASSET {
                return Ok(bytes);
            }
        }
        if edge <= PREVIEW_EDGE {
            return Err("picture does not fit the transfer budget".into());
        }
        edge = (edge * 3 / 4).max(PREVIEW_EDGE);
        current = downscale(img, edge);
    }
}

fn preview_webp(img: &RgbaImage) -> Result<Vec<u8>, String> {
    let fitted = downscale(img, PREVIEW_EDGE);
    for quality in [45.0, 25.0, 10.0] {
        let bytes = lossy_webp(&fitted, quality)?;
        if bytes.len() <= PREVIEW_MAX && wire_kind(&bytes) == Some(Kind::Webp) {
            return Ok(bytes);
        }
    }
    let lossless = lossless_webp(&fitted)?;
    if lossless.len() <= PREVIEW_MAX {
        return Ok(lossless);
    }
    Err("preview exceeds budget".into())
}

fn encode_gif(frames: &[(RgbaImage, u32)], max_edge: u32) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    {
        let mut encoder = GifEncoder::new(&mut bytes);
        encoder
            .set_repeat(Repeat::Infinite)
            .map_err(|e| e.to_string())?;
        for (image, ms) in frames {
            encoder
                .encode_frame(image::Frame::from_parts(
                    downscale(image, max_edge),
                    0,
                    0,
                    image::Delay::from_numer_denom_ms((*ms).max(10), 1),
                ))
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(bytes)
}

fn downscale(img: &RgbaImage, max_edge: u32) -> RgbaImage {
    let (width, height) = img.dimensions();
    let long = width.max(height).max(1);
    if long <= max_edge {
        return img.clone();
    }
    let width = (u64::from(width) * u64::from(max_edge) / u64::from(long)).max(1) as u32;
    let height = (u64::from(height) * u64::from(max_edge) / u64::from(long)).max(1) as u32;
    image::imageops::resize(img, width, height, image::imageops::FilterType::Triangle)
}

fn lossless_webp(img: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut bytes)
        .encode(
            img.as_raw(),
            img.width(),
            img.height(),
            ExtendedColorType::Rgba8,
        )
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}

fn lossy_webp(img: &RgbaImage, quality: f32) -> Result<Vec<u8>, String> {
    let encoded = webp::Encoder::from_rgba(img.as_raw(), img.width(), img.height()).encode(quality);
    let bytes = encoded.to_vec();
    if wire_kind(&bytes) == Some(Kind::Webp) {
        Ok(bytes)
    } else {
        Err("lossy WebP encode failed".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
        let image = RgbaImage::from_pixel(width, height, image::Rgba(color));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn flat_color_stays_lossless_and_avatars_shrink() {
        let wide = solid(2000, 8, [220, 20, 60, 255]);
        let avatar = encode_picture(&wide, Role::Avatar).unwrap();
        let picture = encode_picture(&wide, Role::Picture).unwrap();
        assert_eq!(avatar.extension, "webp");
        assert!(avatar.bytes.len() <= WIRE_ASSET);
        assert!(avatar.preview.len() <= PREVIEW_MAX);
        let avatar_img = image::load_from_memory(&avatar.bytes).unwrap();
        let picture_img = image::load_from_memory(&picture.bytes).unwrap();
        assert!(avatar_img.width().max(avatar_img.height()) <= AVATAR_EDGE);
        assert!(picture_img.width().max(picture_img.height()) <= PICTURE_EDGE);
        assert!(avatar_img.width() < picture_img.width());
        let preview = image::load_from_memory(&avatar.preview).unwrap();
        assert!(preview.width().max(preview.height()) <= PREVIEW_EDGE);
    }

    #[test]
    fn noise_falls_back_to_lossy_within_one_message() {
        let mut pixels = vec![0u8; 480 * 480 * 4];
        for pixel in pixels.chunks_mut(4) {
            pixel[0] = rand::random();
            pixel[1] = rand::random();
            pixel[2] = rand::random();
            pixel[3] = 255;
        }
        let image = RgbaImage::from_raw(480, 480, pixels).unwrap();
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        let encoded = encode_picture(&png, Role::Picture).unwrap();
        assert_eq!(encoded.extension, "webp");
        assert!(encoded.bytes.len() <= WIRE_ASSET);
        assert_eq!(wire_kind(&encoded.bytes), Some(Kind::Webp));
        // VP8L is lossless. A noisy frame must have taken the lossy path.
        assert_ne!(&encoded.bytes[12..16], b"VP8L");
    }

    #[test]
    fn a_short_gif_stays_animated() {
        let mut bytes = Vec::new();
        {
            let mut writer = GifEncoder::new(&mut bytes);
            writer.set_repeat(Repeat::Infinite).unwrap();
            for color in [[255, 0, 0, 255], [0, 0, 255, 255]] {
                writer
                    .encode_frame(image::Frame::from_parts(
                        RgbaImage::from_pixel(8, 8, image::Rgba(color)),
                        0,
                        0,
                        image::Delay::from_numer_denom_ms(40, 1),
                    ))
                    .unwrap();
            }
        }
        let encoded = encode_picture(&bytes, Role::Picture).unwrap();
        assert_eq!(encoded.extension, "gif");
        assert_eq!(wire_kind(&encoded.bytes), Some(Kind::Gif));
        assert_eq!(wire_kind(&encoded.preview), Some(Kind::Webp));
        let frames: Vec<_> = image::codecs::gif::GifDecoder::new(Cursor::new(encoded.bytes))
            .unwrap()
            .into_frames()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(frames.len(), 2);
    }

    #[test]
    fn roles_scale_avatars_below_portraits_and_inline_images() {
        let value = serde_json::json!({
            "avatar": "file:face.png",
            "portraits": [{"variant": "default", "asset": "skin:hero.png"}],
            "doc": {"asset": "lib:cats/wave"}
        });
        let roles = roles(&value);
        assert_eq!(roles["file:face.png"], Role::Avatar);
        assert_eq!(roles["skin:hero.png"], Role::Picture);
        assert_eq!(roles["lib:cats/wave"], Role::Picture);
    }

    #[test]
    fn an_image_used_as_both_keeps_the_larger_role() {
        let value = serde_json::json!({
            "avatar": "skin:hero.png",
            "portraits": [{"asset": "skin:hero.png"}]
        });
        assert_eq!(roles(&value)["skin:hero.png"], Role::Picture);
    }
}
