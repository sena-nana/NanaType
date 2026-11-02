//! Windows: the OneCore voices through WinRT's `SpeechSynthesizer`, beside
//! the SAPI ones. A OneCore line is synthesized to a WAV stream with its
//! word boundaries, decoded and played through the app's audio output (so on
//! the output device chosen in Settings › Sound); its words are reported as
//! the sound reaches them.
use std::sync::Arc;
use std::time::{Duration, Instant};

use nanatype_audio::{AudioHandle, Bus, Pcm, Voice as AudioVoice};
use windows::Media::Core::SpeechCue;
use windows::Media::SpeechSynthesis::{SpeechSynthesizer, VoiceInformation};
use windows::Storage::Streams::DataReader;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::core::{HSTRING, Interface};

use crate::sapi::Sapi;
use crate::timed::{Clip, Timed, Word};
use crate::{Engine, Event, Utterance, Voice, merge_voices, utf16_range};

/// What a OneCore voice id starts with, to tell it from a SAPI token.
const PREFIX: &str = "onecore:";
/// Longest line read, in seconds of sound.
const MAX_SECONDS: u32 = 600;

/// Both engines, as one.
pub(crate) struct System {
    sapi: Option<Sapi>,
    onecore: Option<OneCore>,
}

impl System {
    pub fn new(audio: Option<AudioHandle>) -> Option<Self> {
        // SAPI first: it initializes COM on this thread for both.
        let sapi = Sapi::new();
        let onecore = audio.and_then(OneCore::new);
        (sapi.is_some() || onecore.is_some()).then_some(Self { sapi, onecore })
    }
}

impl Engine for System {
    fn speak(&mut self, u: Utterance) {
        let onecore = u.voice.as_deref().is_some_and(|v| v.starts_with(PREFIX));
        match (&mut self.sapi, &mut self.onecore) {
            (Some(sapi), _) if !onecore => sapi.speak(u),
            (_, Some(one)) => one.speak(u),
            // A OneCore voice without the audio output: SAPI's default.
            (Some(sapi), None) => sapi.speak(Utterance { voice: None, ..u }),
            (None, None) => {}
        }
    }

    fn stop(&mut self) {
        if let Some(sapi) = &mut self.sapi {
            sapi.stop();
        }
        if let Some(one) = &mut self.onecore {
            one.stop();
        }
    }

    fn voices(&mut self) -> Vec<Voice> {
        let onecore = self
            .onecore
            .as_mut()
            .map(|o| o.voices())
            .unwrap_or_default();
        let sapi = self.sapi.as_mut().map(|s| s.voices()).unwrap_or_default();
        merge_voices(onecore, sapi)
    }

    fn poll(&mut self, out: &mut dyn FnMut(Event)) {
        if let Some(sapi) = &mut self.sapi {
            sapi.poll(out);
        }
        if let Some(one) = &mut self.onecore {
            one.poll(out);
        }
    }

    fn busy(&self) -> bool {
        self.sapi.as_ref().is_some_and(Sapi::busy)
            || self.onecore.as_ref().is_some_and(OneCore::busy)
    }
}

pub(crate) struct OneCore {
    synth: SpeechSynthesizer,
    audio: AudioHandle,
    timed: Timed,
    /// Utterances that could not be synthesized: reported started and
    /// finished on the next poll.
    failed: Vec<u64>,
    /// This thread's COM initialization is ours to undo.
    com: bool,
}

impl OneCore {
    fn new(audio: AudioHandle) -> Option<Self> {
        let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        let synth = SpeechSynthesizer::new().and_then(|s| {
            s.Options()?.SetIncludeWordBoundaryMetadata(true)?;
            Ok(s)
        });
        match synth {
            Ok(synth) => Some(Self {
                synth,
                audio,
                timed: Timed::default(),
                failed: Vec::new(),
                com,
            }),
            Err(_) => {
                if com {
                    unsafe { CoUninitialize() };
                }
                None
            }
        }
    }

    fn installed() -> Vec<VoiceInformation> {
        SpeechSynthesizer::AllVoices()
            .map(|all| all.into_iter().collect())
            .unwrap_or_default()
    }

    /// `u` as a clip: synthesized, decoded and loaded into the output.
    fn synthesize(&mut self, u: &Utterance) -> windows::core::Result<Option<Clip>> {
        let wanted = u.voice.as_deref().and_then(|v| v.strip_prefix(PREFIX));
        let voice = match wanted {
            Some(id) => Self::installed()
                .into_iter()
                .find(|v| v.Id().is_ok_and(|i| i == id)),
            None => None,
        };
        // An unknown voice falls back to the default one.
        let voice = match voice {
            Some(v) => v,
            None => SpeechSynthesizer::DefaultVoice()?,
        };
        self.synth.SetVoice(&voice)?;
        let options = self.synth.Options()?;
        // OneCore's 1.0 is the voice's own pace, as ours is.
        options.SetSpeakingRate(f64::from(u.rate))?;
        options.SetAudioVolume(1.0)?;
        let stream = self
            .synth
            .SynthesizeTextToStreamAsync(&HSTRING::from(u.text.as_str()))?
            .join()?;
        let words = words(&stream, &u.text);
        let size = stream.Size()? as u32;
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
        reader.LoadAsync(size)?.join()?;
        let mut bytes = vec![0u8; size as usize];
        reader.ReadBytes(&mut bytes)?;
        let Ok(pcm) = nanatype_audio::decode_up_to(bytes, Some("wav"), MAX_SECONDS) else {
            return Ok(None);
        };
        let sound: nanatype_audio::SoundId = format!("tts:{}", u.id).into();
        let length = length(&pcm);
        self.audio.load(sound.clone(), Arc::new(pcm));
        Ok(Some(Clip {
            id: u.id,
            sound,
            gain: u.volume,
            length,
            words,
        }))
    }
}

/// How long `pcm` plays.
fn length(pcm: &Pcm) -> Duration {
    Duration::from_secs_f64(pcm.frames() as f64 / f64::from(pcm.rate.max(1)))
}

/// The word boundaries of a synthesized stream, as byte ranges of `text`.
fn words(stream: &windows::Media::SpeechSynthesis::SpeechSynthesisStream, text: &str) -> Vec<Word> {
    let mut out = Vec::new();
    let Ok(tracks) = stream.TimedMetadataTracks() else {
        return out;
    };
    for track in tracks {
        if !track.Id().is_ok_and(|id| id == "SpeechWord") {
            continue;
        }
        let Ok(cues) = track.Cues() else {
            continue;
        };
        for cue in cues {
            let Ok(cue) = cue.cast::<SpeechCue>() else {
                continue;
            };
            let (Ok(at), Ok(start), Ok(end)) = (
                cue.StartTime(),
                cue.StartPositionInInput().and_then(|p| p.Value()),
                cue.EndPositionInInput().and_then(|p| p.Value()),
            ) else {
                continue;
            };
            // Positions count UTF-16 units; the end is the word's last one.
            let (start, len) = utf16_range(
                text,
                start.max(0) as usize,
                (end - start + 1).max(0) as usize,
            );
            out.push(Word {
                at: Duration::from_nanos(at.Duration.max(0) as u64 * 100),
                start,
                len,
            });
        }
    }
    out.sort_by_key(|w| w.at);
    out
}

impl Engine for OneCore {
    fn speak(&mut self, u: Utterance) {
        match self.synthesize(&u) {
            Ok(Some(clip)) => self.timed.push(clip),
            _ => self.failed.push(u.id),
        }
    }

    fn stop(&mut self) {
        let audio = &self.audio;
        self.timed
            .stop(&mut |clip| audio.silence(clip.sound.clone()));
        self.failed.clear();
    }

    fn voices(&mut self) -> Vec<Voice> {
        Self::installed()
            .into_iter()
            .filter_map(|v| {
                Some(Voice {
                    id: format!("{PREFIX}{}", v.Id().ok()?),
                    name: v.DisplayName().ok()?.to_string(),
                    language: v.Language().map(|l| l.to_string()).unwrap_or_default(),
                })
            })
            .collect()
    }

    fn poll(&mut self, out: &mut dyn FnMut(Event)) {
        for id in self.failed.drain(..) {
            out(Event::Started(id));
            out(Event::Finished(id));
        }
        let audio = &self.audio;
        self.timed.poll(
            Instant::now(),
            &mut |clip| {
                audio.play(AudioVoice {
                    sound: clip.sound.clone(),
                    bus: Bus::Speech,
                    gain: clip.gain,
                    pitch: 1.0,
                })
            },
            &mut |clip| audio.unload(clip.sound.clone()),
            out,
        );
    }

    fn busy(&self) -> bool {
        self.timed.busy() || !self.failed.is_empty()
    }
}

impl Drop for OneCore {
    fn drop(&mut self) {
        self.stop();
        if self.com {
            unsafe { CoUninitialize() };
        }
    }
}
