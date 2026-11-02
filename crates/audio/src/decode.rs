use std::io::Cursor;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Longest sound decoded into memory.
pub const MAX_SECONDS: u32 = 30;

/// Interleaved-free mono or stereo f32 samples.
#[derive(Debug, Clone, PartialEq)]
pub struct Pcm {
    pub rate: u32,
    /// 1 or 2.
    pub channels: u16,
    /// Interleaved samples.
    pub samples: Vec<f32>,
}

impl Pcm {
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels.max(1))
    }

    pub fn duration_ms(&self) -> u32 {
        (self.frames() as u64 * 1000 / u64::from(self.rate.max(1))) as u32
    }

    /// Sample of `channel` (0 or 1) at fractional frame `pos`, linear interp.
    #[inline]
    pub fn sample(&self, pos: f64, channel: usize) -> f32 {
        let ch = usize::from(self.channels.max(1));
        let c = channel.min(ch - 1);
        let i = pos as usize;
        let frac = (pos - i as f64) as f32;
        let a = self.samples.get(i * ch + c).copied().unwrap_or(0.0);
        let b = self.samples.get((i + 1) * ch + c).copied().unwrap_or(0.0);
        a + (b - a) * frac
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    Unsupported(String),
    Corrupt(String),
    TooLong,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(e) => write!(f, "unsupported audio: {e}"),
            Self::Corrupt(e) => write!(f, "corrupt audio: {e}"),
            Self::TooLong => write!(f, "sounds longer than {MAX_SECONDS}s are not loaded"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Decode WAV, OGG Vorbis, FLAC or MP3 bytes. `extension` is a hint only.
pub fn decode(bytes: Vec<u8>, extension: Option<&str>) -> Result<Pcm, DecodeError> {
    decode_up_to(bytes, extension, MAX_SECONDS)
}

/// [`decode`] for sounds up to `max_seconds` long (a line read aloud).
pub fn decode_up_to(
    bytes: Vec<u8>,
    extension: Option<&str>,
    max_seconds: u32,
) -> Result<Pcm, DecodeError> {
    let mut hint = Hint::new();
    if let Some(ext) = extension {
        hint.with_extension(ext);
    }
    let stream = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| DecodeError::Unsupported(e.to_string()))?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| DecodeError::Unsupported("no audio track".into()))?;
    let track_id = track.id;
    let rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| DecodeError::Unsupported("unknown sample rate".into()))?;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| DecodeError::Unsupported(e.to_string()))?;
    let mut channels = 0u16;
    let mut out: Vec<f32> = Vec::new();
    let limit = rate as usize * max_seconds as usize * 2;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(symphonia::core::errors::Error::ResetRequired) => break,
            Err(e) => return Err(DecodeError::Corrupt(e.to_string())),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => return Err(DecodeError::Corrupt(e.to_string())),
        };
        let spec = *decoded.spec();
        let src_channels = spec.channels.count().max(1);
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buffer.copy_interleaved_ref(decoded);
        let samples = buffer.samples();
        if src_channels <= 2 {
            channels = src_channels as u16;
            out.extend_from_slice(samples);
        } else {
            // Fold surround down to stereo: front left/right only.
            channels = 2;
            for frame in samples.chunks(src_channels) {
                out.push(frame[0]);
                out.push(frame[1]);
            }
        }
        if out.len() > limit {
            return Err(DecodeError::TooLong);
        }
    }
    if out.is_empty() || channels == 0 {
        return Err(DecodeError::Corrupt("no samples".into()));
    }
    Ok(Pcm {
        rate,
        channels,
        samples: out,
    })
}
