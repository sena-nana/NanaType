//! Telegram animation formats feed the same bounded, atomic PNG-frame cache.
use std::io::{Cursor, Read};

use image::{Rgba, RgbaImage};
use shiguredo_libvpx::{Decoder, DecoderCodec, DecoderConfig};
use webm_iterable::{
    WebmIterator,
    matroska_spec::{Master, MatroskaSpec as Tag},
};

type Save<'a> = &'a mut dyn FnMut(RgbaImage, u64) -> Result<(), String>;

pub fn tgs(bytes: &[u8], save: Save<'_>) -> Result<(), String> {
    let mut json = String::new();
    flate2::read::GzDecoder::new(bytes)
        .take(2 * 1024 * 1024 + 1)
        .read_to_string(&mut json)
        .map_err(|e| e.to_string())?;
    if json.len() > 2 * 1024 * 1024 {
        return Err("TGS definition exceeds 2 MiB".into());
    }
    let animation = rasterlottie::Animation::from_json_str(&json).map_err(|e| e.to_string())?;
    let fps = animation.frame_rate;
    let frames = animation.out_point - animation.in_point;
    if animation.width == 0
        || animation.height == 0
        || animation.width > 512
        || animation.height > 512
        || !fps.is_finite()
        || fps <= 0.0
        || fps > 60.0
        || !frames.is_finite()
        || frames <= 0.0
        || frames / fps > 3.1
        || !animation.in_point.is_finite()
    {
        return Err("Invalid Telegram TGS dimensions, frame rate or duration".into());
    }
    // Unsupported Lottie features fail explicitly; no external assets or scripts.
    let prepared = rasterlottie::Renderer::default()
        .prepare(&animation)
        .map_err(|e| e.to_string())?;
    for i in 0..frames.ceil() as u32 {
        let frame = prepared
            .render_frame(animation.in_point + i as f32, Default::default())
            .map_err(|e| e.to_string())?;
        let mut image = RgbaImage::from_raw(frame.width, frame.height, frame.pixels)
            .ok_or("Invalid TGS raster frame")?;
        // tiny-skia returns premultiplied pixels; PNG and the shared cache use
        // straight alpha, which the GPU uploader premultiplies exactly once.
        for pixel in image.pixels_mut() {
            let alpha = u32::from(pixel.0[3]);
            for channel in &mut pixel.0[..3] {
                *channel = (u32::from(*channel) * 255 + alpha / 2)
                    .checked_div(alpha)
                    .unwrap_or(0)
                    .min(255) as u8;
            }
        }
        let start = (i as f64 * 1000.0 / fps as f64).round() as u64;
        let end = ((i as f64 + 1.0).min(frames as f64) * 1000.0 / fps as f64).round() as u64;
        save(image, end.saturating_sub(start))?;
    }
    Ok(())
}

struct Packet {
    timestamp: u64,
    data: Vec<u8>,
    alpha: Option<Vec<u8>>,
    duration: Option<u64>,
}

pub fn webm(bytes: &[u8], save: Save<'_>) -> Result<(), String> {
    let mut packets: Vec<Packet> = Vec::new();
    let (mut scale, mut timestamp, mut default_duration) = (1_000_000u64, 0u64, 33_333_333u64);
    let (mut width, mut height, mut track, mut alpha) = (0, 0, 0, false);
    let mut codec = String::new();
    let mut group = false;
    let mut addition_id = 1;
    let mut tracks = 0;
    let mut tags = WebmIterator::new(Cursor::new(bytes), &[]);
    tags.set_max_allowable_tag_size(Some(bytes.len()));
    for tag in tags {
        let tag = tag.map_err(|e| e.to_string())?;
        use self::Tag::*;
        match tag {
            TrackEntry(Master::Start) => {
                tracks += 1;
                if tracks > 1 {
                    return Err("Sticker WebM must have one video track".into());
                }
            }
            TrackNumber(n) => track = n,
            CodecID(id) => codec = id,
            PixelWidth(w) => width = w,
            PixelHeight(h) => height = h,
            AlphaMode(a) => alpha = a == 1,
            TimestampScale(s) => scale = s,
            DefaultDuration(d) => default_duration = d,
            Timestamp(t) => timestamp = t,
            BlockGroup(Master::Start) => {
                group = true;
                addition_id = 1;
            }
            BlockGroup(Master::End) => group = false,
            SimpleBlock(_) | Block(_) => {
                let (number, relative, lacing, data) = if matches!(tag, SimpleBlock(_)) {
                    let b = webm_iterable::matroska_spec::SimpleBlock::try_from(&tag)
                        .map_err(|e| e.to_string())?;
                    (
                        b.track,
                        b.timestamp,
                        b.lacing.is_some(),
                        b.raw_frame_data().to_vec(),
                    )
                } else {
                    let b = webm_iterable::matroska_spec::Block::try_from(&tag)
                        .map_err(|e| e.to_string())?;
                    (
                        b.track,
                        b.timestamp,
                        b.lacing.is_some(),
                        b.raw_frame_data().to_vec(),
                    )
                };
                if lacing || number != track || packets.len() >= 200 {
                    return Err("Invalid Telegram WebM packet layout or frame count".into());
                }
                let t = timestamp
                    .checked_add_signed(relative as i64)
                    .ok_or("Invalid WebM timestamp")?;
                packets.push(Packet {
                    timestamp: t,
                    data,
                    alpha: None,
                    duration: None,
                });
            }
            BlockAddID(id) => addition_id = id,
            BlockAdditional(data) if group && addition_id == 1 => {
                packets.last_mut().ok_or("WebM alpha without frame")?.alpha = Some(data);
            }
            BlockDuration(d) if group => {
                packets
                    .last_mut()
                    .ok_or("WebM duration without frame")?
                    .duration = Some(d)
            }
            _ => {}
        }
    }
    if width == 0
        || height == 0
        || width > 512
        || height > 512
        || scale == 0
        || packets.is_empty()
        || codec != "V_VP9"
        || tracks != 1
    {
        return Err("Invalid Telegram VP9 WebM dimensions or track".into());
    }
    let duration_ms = |units: u64| {
        units
            .checked_mul(scale)
            .map(|n| n / 1_000_000)
            .ok_or("WebM timestamp overflow")
    };
    let mut color =
        Decoder::new(DecoderConfig::new(DecoderCodec::Vp9)).map_err(|e| e.to_string())?;
    let mut mask =
        Decoder::new(DecoderConfig::new(DecoderCodec::Vp9)).map_err(|e| e.to_string())?;
    let start = packets[0].timestamp;
    for (i, packet) in packets.iter().enumerate() {
        let delay = if let Some(next) = packets.get(i + 1) {
            duration_ms(
                next.timestamp
                    .checked_sub(packet.timestamp)
                    .filter(|d| *d > 0)
                    .ok_or("Non-increasing WebM timestamps")?,
            )?
        } else if let Some(d) = packet.duration {
            duration_ms(d)?
        } else {
            default_duration / 1_000_000
        };
        if duration_ms(packet.timestamp - start)?.saturating_add(delay) > 3100 {
            return Err("Telegram WebM exceeds 3 seconds".into());
        }
        let mut image = decode(&mut color, &packet.data, width, height, false)?;
        if alpha {
            let data = packet.alpha.as_deref().ok_or("WebM alpha frame missing")?;
            let a = decode(&mut mask, data, width, height, true)?;
            for (pixel, mask) in image.pixels_mut().zip(a.pixels()) {
                pixel.0[3] = mask.0[0];
            }
        }
        save(image, delay)?;
    }
    Ok(())
}

fn decode(
    decoder: &mut Decoder,
    packet: &[u8],
    width: u64,
    height: u64,
    alpha: bool,
) -> Result<RgbaImage, String> {
    decoder.decode(packet).map_err(|e| e.to_string())?;
    let frame = decoder
        .next_frame()
        .map_err(|e| e.to_string())?
        .ok_or("WebM frame not decoded")?;
    if frame.is_high_depth() || frame.width() as u64 != width || frame.height() as u64 != height {
        return Err("Unsupported WebM frame dimensions or bit depth".into());
    }
    let mut image = RgbaImage::new(width as u32, height as u32);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let yv = frame.y_plane()[y as usize * frame.y_stride() + x as usize] as i32;
        if alpha {
            *pixel = Rgba([yv as u8, yv as u8, yv as u8, 255]);
            continue;
        }
        let u = frame.u_plane()[y as usize / 2 * frame.u_stride() + x as usize / 2] as i32 - 128;
        let v = frame.v_plane()[y as usize / 2 * frame.v_stride() + x as usize / 2] as i32 - 128;
        let c = (yv - 16) * 298;
        let byte = |n: i32| ((n + 128) >> 8).clamp(0, 255) as u8;
        *pixel = Rgba([
            byte(c + 409 * v),
            byte(c - 100 * u - 208 * v),
            byte(c + 516 * u),
            255,
        ]);
    }
    if decoder.next_frame().map_err(|e| e.to_string())?.is_some() {
        return Err("Multiple WebM images per packet".into());
    }
    Ok(image)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use shiguredo_libvpx::{
        CodecConfig, EncodeOptions, Encoder, EncoderConfig, ImageData, ImageFormat, Vp9Config,
    };
    use std::{io::Write, sync::atomic::AtomicBool};

    pub(crate) fn fixture(ext: &str) -> Vec<u8> {
        match ext {
            "webp" => {
                let mut bytes = Vec::new();
                image::codecs::webp::WebPEncoder::new_lossless(&mut bytes)
                    .encode(
                        &[255, 0, 0, 128].repeat(16),
                        4,
                        4,
                        image::ExtendedColorType::Rgba8,
                    )
                    .unwrap();
                bytes
            }
            "tgs" => {
                let json = r#"{"v":"5.7.6","fr":30,"ip":0,"op":2,"w":16,"h":16,"layers":[{"ty":4,"ind":1,"ip":0,"op":2,"ks":{"o":{"a":0,"k":50},"p":{"a":1,"k":[{"t":0,"s":[4,8,0],"e":[12,8,0],"o":{"x":0,"y":0},"i":{"x":1,"y":1}},{"t":1,"s":[12,8,0]}]},"a":{"a":0,"k":[0,0,0]},"s":{"a":0,"k":[100,100,100]},"r":{"a":0,"k":0}},"shapes":[{"ty":"rc","p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[6,6]},"r":{"a":0,"k":0}},{"ty":"fl","c":{"a":0,"k":[1,0,0,1]},"o":{"a":0,"k":100}}]}]}"#;
                let mut gzip =
                    flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                gzip.write_all(json.as_bytes()).unwrap();
                gzip.finish().unwrap()
            }
            "webm" => {
                let encode = |y: &[u8], u: u8, v: u8| {
                    let mut config = EncoderConfig::new(
                        4,
                        4,
                        ImageFormat::I420,
                        CodecConfig::Vp9(Vp9Config::default()),
                    );
                    config.max_quantizer = 0;
                    config.cq_level = 0;
                    let mut encoder = Encoder::new(config).unwrap();
                    encoder
                        .encode(
                            &ImageData::I420 {
                                y,
                                u: &[u; 4],
                                v: &[v; 4],
                            },
                            &EncodeOptions {
                                force_keyframe: true,
                            },
                        )
                        .unwrap();
                    let mut result = Vec::new();
                    while let Some(frame) = encoder.next_frame() {
                        result.extend_from_slice(frame.data());
                    }
                    encoder.finish().unwrap();
                    while let Some(frame) = encoder.next_frame() {
                        result.extend_from_slice(frame.data());
                    }
                    result
                };
                let red = encode(&[81; 16], 90, 240);
                let green = encode(&[145; 16], 54, 34);
                let alpha = encode(&[0, 128, 255, 255].repeat(4), 128, 128);
                use webm_iterable::matroska_spec::MatroskaSpec::*;
                let full = Master::Full;
                let group = |time: i16, data: &[u8]| {
                    let mut block = vec![0x81];
                    block.extend(time.to_be_bytes());
                    block.push(0);
                    block.extend(data);
                    BlockGroup(full(vec![
                        Block(block),
                        BlockDuration(40),
                        BlockAdditions(full(vec![BlockMore(full(vec![
                            BlockAddID(1),
                            BlockAdditional(alpha.clone()),
                        ]))])),
                    ]))
                };
                let mut bytes = Vec::new();
                let mut writer = webm_iterable::WebmWriter::new(&mut bytes);
                writer
                    .write(&Ebml(full(vec![
                        EbmlVersion(1),
                        EbmlReadVersion(1),
                        EbmlMaxIdLength(4),
                        EbmlMaxSizeLength(8),
                        DocType("webm".into()),
                        DocTypeVersion(4),
                        DocTypeReadVersion(2),
                    ])))
                    .unwrap();
                writer
                    .write(&Segment(full(vec![
                        Info(full(vec![
                            TimestampScale(1_000_000),
                            MuxingApp("NanaType tests".into()),
                            WritingApp("NanaType tests".into()),
                        ])),
                        Tracks(full(vec![TrackEntry(full(vec![
                            TrackNumber(1),
                            TrackUID(1),
                            TrackType(1),
                            CodecID("V_VP9".into()),
                            DefaultDuration(40_000_000),
                            Video(full(vec![PixelWidth(4), PixelHeight(4), AlphaMode(1)])),
                        ]))])),
                        Cluster(full(vec![Timestamp(0), group(0, &red), group(40, &green)])),
                    ])))
                    .unwrap();
                writer.flush().unwrap();
                drop(writer);
                bytes
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn telegram_formats_preserve_transparency_timing_and_cache_reuse() {
        use super::super::cache;
        let base = std::env::temp_dir().join(format!("nanatype-tg-formats-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        for ext in ["webp", "tgs", "webm"] {
            let path = base.join(format!("sticker.{ext}"));
            std::fs::write(&path, fixture(ext)).unwrap();
            let cached =
                cache::prepare(&path, &base.join("cache"), &AtomicBool::new(false)).unwrap();
            let first = image::open(cached.frame_path(0)).unwrap().into_rgba8();
            match ext {
                "webp" => {
                    assert!(!cached.animated());
                    assert_eq!(first.get_pixel(0, 0).0, [255, 0, 0, 128]);
                }
                "tgs" => {
                    assert_eq!(cached.delays, [33, 34]);
                    assert_eq!(cached.cycles, None);
                    assert_ne!(cached.hashes[0], cached.hashes[1]);
                    assert_eq!(first.get_pixel(4, 8).0, [255, 0, 0, 128]);
                    assert_eq!(first.get_pixel(15, 15).0[3], 0);
                }
                "webm" => {
                    assert_eq!(cached.delays, [40, 40]);
                    assert_eq!(cached.cycles, None);
                    let p = first.get_pixel(1, 0).0;
                    assert!(p[0] > 245 && p[1] < 10 && p[2] < 10, "{p:?}");
                    assert_eq!(p[3], 128);
                    assert_eq!(first.get_pixel(0, 0).0[3], 0);
                    let second = cache::read_premultiplied_frame(&cached, 1).unwrap();
                    let s_px = &second[2 * 4..2 * 4 + 4];
                    assert!(s_px[1] > 245, "{s_px:?}");
                }
                _ => unreachable!(),
            }
            let time = std::fs::metadata(cached.frame_path(0))
                .unwrap()
                .modified()
                .unwrap();
            let reopened =
                cache::prepare(&path, &base.join("cache"), &AtomicBool::new(false)).unwrap();
            assert_eq!(cached.key, reopened.key);
            assert_eq!(
                std::fs::metadata(reopened.frame_path(0))
                    .unwrap()
                    .modified()
                    .unwrap(),
                time
            );
        }
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn malformed_animations_fail_without_publishing_frames() {
        assert!(
            tgs(&[0x1f, 0x8b, 1], &mut |_, _| panic!(
                "invalid TGS produced frame"
            ))
            .is_err()
        );
        assert!(
            webm(&[0x1a, 0x45, 0xdf, 0xa3], &mut |_, _| panic!(
                "invalid WebM produced frame"
            ))
            .is_err()
        );
    }
}
