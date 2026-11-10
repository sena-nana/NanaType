//! Reading lines aloud. [`Narrator`] is the policy (what is said, when it
//! stops); the engine behind it starts on first use.
use super::App;
use crate::msg::{Msg, TtsMsg};
use nanatype_core::Cmd;
use nanatype_model::{Document, HistoryItem, Line, LineId, unix_millis};
use nanatype_settings::{Character, TtsSettings};
use nanatype_tts::{Event, Speak, Tts, speakable, speakable_units};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Partner lines older than this when they arrive (a room just joined)
/// are not read.
const PARTNER_FRESH_MS: u64 = 10_000;

/// What is said and when it stops, over any [`Speak`] engine.
pub struct Narrator<S: Speak> {
    pub engine: S,
    /// Utterances started and not finished.
    talking: HashSet<u64>,
    /// Partner lines already read.
    partners: HashSet<(String, LineId)>,
    /// What each utterance says, while it is said (word sizes for the mouth).
    texts: HashMap<u64, String>,
    /// The line whose typing follows its voice.
    paced: Option<Paced>,
}

/// A line typed along with its utterance.
struct Paced {
    id: u64,
    line: LineId,
    /// The reveal unit of each byte of the spoken text.
    units: Vec<u32>,
}

impl<S: Speak> Narrator<S> {
    pub fn new(engine: S) -> Self {
        Self {
            engine,
            talking: HashSet::new(),
            partners: HashSet::new(),
            texts: HashMap::new(),
            paced: None,
        }
    }

    /// A line began on the display (sent, from the queue or replayed): it
    /// cuts off what is being said and is read in its character's voice.
    pub fn line_started(
        &mut self,
        tts: &TtsSettings,
        characters: &[Character],
        line: &Line,
    ) -> Option<u64> {
        if !tts.enabled {
            return None;
        }
        self.engine.stop();
        self.paced = None;
        let (text, units) = speakable_units(&line.doc)?;
        let voice = characters
            .iter()
            .find(|c| c.name == line.speaker.name)
            .and_then(|c| c.voice.as_deref())
            .or(tts.voice.as_deref());
        let id = self.engine.speak(&text, voice, tts.rate, tts.volume);
        self.texts.insert(id, text);
        if tts.pace_typing {
            self.paced = Some(Paced {
                id,
                line: line.id,
                units,
            });
        }
        Some(id)
    }

    /// The line whose typing follows its voice, when that is `line`.
    pub fn paces(&self, line: LineId) -> bool {
        self.paced.as_ref().is_some_and(|p| p.line == line)
    }

    /// What the typewriter does about an engine event: type up to a spoken
    /// word, or on its own once the voice is over.
    pub fn pace(&mut self, event: &Event) -> Option<Cmd> {
        let p = self.paced.as_ref()?;
        match *event {
            Event::Word { id, start, len } if id == p.id => {
                let end = (start + len.max(1)).min(p.units.len());
                let through = p.units.get(start..end)?.iter().max()? + 1;
                Some(Cmd::SpeechReached {
                    line: p.line,
                    through,
                })
            }
            Event::Finished(id) if id == p.id => {
                let line = p.line;
                self.paced = None;
                Some(Cmd::SpeechEnded(line))
            }
            _ => None,
        }
    }

    /// How many characters a spoken word has, for the mouth.
    pub fn word_chars(&self, event: &Event) -> Option<usize> {
        let Event::Word { id, start, len } = *event else {
            return None;
        };
        let word = self.texts.get(&id)?.get(start..start + len)?;
        Some(word.chars().filter(|c| c.is_alphanumeric()).count().max(1))
    }

    /// Skip, clear or switching off: silence everything.
    pub fn silence(&mut self) {
        self.engine.stop();
    }

    /// A viewer's message reached the history; read after what is said.
    pub fn chat(&mut self, tts: &TtsSettings, doc: &Document) -> Option<u64> {
        if !(tts.enabled && tts.chat) {
            return None;
        }
        self.queue(tts, None, doc)
    }

    /// A partner's line showed; read once, after what is said.
    pub fn partner(
        &mut self,
        tts: &TtsSettings,
        owner: &str,
        line: LineId,
        item: &HistoryItem,
        now: u64,
    ) -> Option<u64> {
        if !(tts.enabled && tts.partners) || !self.partners.insert((owner.to_owned(), line)) {
            return None;
        }
        if now.saturating_sub(item.at) > PARTNER_FRESH_MS {
            return None;
        }
        self.queue(tts, None, &item.doc)
    }

    /// A partner replayed a line: their display shows it again with no new
    /// history row. Read once, after what is said.
    pub fn partner_replay(
        &mut self,
        tts: &TtsSettings,
        owner: &str,
        line: LineId,
        doc: &Document,
    ) -> Option<u64> {
        if !(tts.enabled && tts.partners) || !self.partners.insert((owner.to_owned(), line)) {
            return None;
        }
        self.queue(tts, None, doc)
    }

    fn queue(&mut self, tts: &TtsSettings, voice: Option<&str>, doc: &Document) -> Option<u64> {
        let text = speakable(doc)?;
        let voice = voice.or(tts.voice.as_deref());
        let id = self.engine.speak(&text, voice, tts.rate, tts.volume);
        self.texts.insert(id, text);
        Some(id)
    }

    /// Whether a voice is talking.
    pub fn speaking(&self) -> bool {
        !self.talking.is_empty()
    }

    /// An engine event. Returns the new talking state when it changed.
    pub fn event(&mut self, event: &Event) -> Option<bool> {
        let before = self.speaking();
        match event {
            Event::Started(id) => {
                self.talking.insert(*id);
            }
            Event::Finished(id) => {
                self.talking.remove(id);
                self.texts.remove(id);
            }
            Event::Word { .. } | Event::Voices(_) => {}
        }
        (self.speaking() != before).then_some(self.speaking())
    }
}

/// The system engine, started the first time something is said. Voices it
/// synthesizes to sound play through the app's own output.
pub struct Lazy {
    tts: Option<Tts>,
    post: Arc<dyn Fn(Msg) + Send + Sync>,
    audio: nanatype_audio::AudioHandle,
}

impl Lazy {
    pub fn new(
        post: impl Fn(Msg) + Send + Sync + 'static,
        audio: nanatype_audio::AudioHandle,
    ) -> Self {
        Self {
            tts: None,
            post: Arc::new(post),
            audio,
        }
    }
    fn get(&mut self) -> &mut Tts {
        self.tts.get_or_insert_with(|| {
            let post = self.post.clone();
            Tts::with_audio(
                move |e| post(Msg::Tts(TtsMsg::Event(e))),
                self.audio.clone(),
            )
        })
    }
    pub fn request_voices(&mut self) {
        self.get().request_voices();
    }
}

impl Speak for Lazy {
    fn speak(&mut self, text: &str, voice: Option<&str>, rate: f32, volume: f32) -> u64 {
        self.get().speak(text, voice, rate, volume)
    }
    fn stop(&mut self) {
        if let Some(tts) = &mut self.tts {
            tts.stop();
        }
    }
}

impl App {
    /// The display started `line`.
    pub(super) fn tts_line(&mut self, line: &Line) {
        let s = &self.settings;
        self.tts.line_started(&s.tts, &s.general.characters, line);
        if self.tts.paces(line.id) {
            self.run(Cmd::SpeechPaced(line.id));
        }
    }

    pub(super) fn tts_chat(&mut self, doc: &Document) {
        self.tts.chat(&self.settings.tts, doc);
    }

    pub(crate) fn tts_partner_replay(&mut self, owner: &str, line: LineId, doc: &Document) {
        self.tts
            .partner_replay(&self.settings.tts, owner, line, doc);
    }

    pub(crate) fn tts_partner(&mut self, owner: &str, line: LineId, item: &HistoryItem) {
        self.tts
            .partner(&self.settings.tts, owner, line, item, unix_millis());
    }

    pub(super) fn tts_msg(&mut self, msg: TtsMsg) {
        match msg {
            TtsMsg::Event(Event::Voices(voices)) => self.ui.tts_voices.set(Arc::new(voices)),
            TtsMsg::Event(event) => {
                if let Some(cmd) = self.tts.pace(&event) {
                    self.run(cmd);
                }
                if self.settings.tts.enabled
                    && let Some(chars) = self.tts.word_chars(&event)
                {
                    self.bridge.word(chars);
                }
                // Switched off mid-line, the mouth still closes.
                if let Some(speaking) = self.tts.event(&event)
                    && (self.settings.tts.enabled || !speaking)
                {
                    self.bridge_speaking(speaking);
                }
            }
            TtsMsg::Refresh => self.tts.engine.request_voices(),
            TtsMsg::Test(character) => {
                let s = &self.settings;
                let voice = character
                    .and_then(|i| s.general.characters.get(i))
                    .and_then(|c| c.voice.clone())
                    .or_else(|| s.tts.voice.clone());
                let text = self.ui.now_t("tts-sample");
                let (rate, volume) = (s.tts.rate, s.tts.volume);
                self.tts.silence();
                self.tts.engine.speak(&text, voice.as_deref(), rate, volume);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nanatype_model::Speaker;

    #[derive(Debug, Clone, PartialEq)]
    enum Call {
        Speak(String, Option<String>),
        Stop,
    }

    #[derive(Default)]
    struct Fake {
        calls: Vec<Call>,
        next: u64,
    }
    impl Speak for Fake {
        fn speak(&mut self, text: &str, voice: Option<&str>, _: f32, _: f32) -> u64 {
            self.calls
                .push(Call::Speak(text.into(), voice.map(str::to_owned)));
            self.next += 1;
            self.next
        }
        fn stop(&mut self) {
            self.calls.push(Call::Stop);
        }
    }

    fn on() -> TtsSettings {
        TtsSettings {
            enabled: true,
            voice: Some("default".into()),
            ..Default::default()
        }
    }
    fn characters() -> Vec<Character> {
        vec![
            Character {
                name: "Nana".into(),
                voice: Some("kyoko".into()),
                ..Default::default()
            },
            Character {
                name: "Guest".into(),
                ..Default::default()
            },
        ]
    }
    fn line(speaker: &str, text: &str) -> Line {
        Line::new(
            Document::plain(text),
            Speaker {
                name: speaker.into(),
                ..Default::default()
            },
        )
    }
    fn speak(text: &str, voice: &str) -> Call {
        Call::Speak(text.into(), Some(voice.into()))
    }

    #[test]
    fn a_line_cuts_off_the_last_and_uses_its_characters_voice() {
        let mut n = Narrator::new(Fake::default());
        n.line_started(&on(), &characters(), &line("Nana", "こんにちは"));
        n.line_started(&on(), &characters(), &line("Guest", "hello"));
        assert_eq!(
            n.engine.calls,
            [
                Call::Stop,
                speak("こんにちは", "kyoko"),
                Call::Stop,
                speak("hello", "default"),
            ]
        );
    }

    #[test]
    fn nothing_is_said_while_off_or_for_a_sticker() {
        let mut n = Narrator::new(Fake::default());
        assert_eq!(
            n.line_started(&TtsSettings::default(), &characters(), &line("Nana", "hi")),
            None
        );
        assert!(n.engine.calls.is_empty());
        assert_eq!(
            n.line_started(&on(), &characters(), &line("Nana", "……")),
            None
        );
        // The old line still stops: the display moved on.
        assert_eq!(n.engine.calls, [Call::Stop]);
    }

    #[test]
    fn skip_stops_and_replay_speaks_again() {
        let mut n = Narrator::new(Fake::default());
        let first = line("Nana", "again");
        n.line_started(&on(), &characters(), &first);
        n.silence();
        // A replay starts a new line with the same words.
        let replay = Line {
            id: LineId::next(),
            ..first
        };
        n.line_started(&on(), &characters(), &replay);
        assert_eq!(
            n.engine.calls,
            [
                Call::Stop,
                speak("again", "kyoko"),
                Call::Stop,
                Call::Stop,
                speak("again", "kyoko"),
            ]
        );
    }

    #[test]
    fn chat_and_partners_queue_only_when_asked_and_partners_once() {
        let mut n = Narrator::new(Fake::default());
        let doc = Document::plain("gg");
        assert_eq!(n.chat(&on(), &doc), None);
        let all = TtsSettings {
            chat: true,
            partners: true,
            ..on()
        };
        assert!(n.chat(&all, &doc).is_some());
        let now = unix_millis();
        let item = HistoryItem {
            source: nanatype_model::SourceId("peer:b".into()),
            kind: nanatype_model::SourceKind::Typewriter,
            line: None,
            author: "B".into(),
            avatar: None,
            doc: Document::plain("partner line"),
            at: now,
            uid: None,
            reply: None,
            event: None,
        };
        let id = LineId::next();
        assert!(n.partner(&all, "b", id, &item, now).is_some());
        assert_eq!(n.partner(&all, "b", id, &item, now), None);
        let old = HistoryItem { at: 0, ..item };
        assert_eq!(n.partner(&all, "b", LineId::next(), &old, now), None);
        // A replay is a line of its own, read once too.
        let again = LineId::next();
        assert!(n.partner_replay(&all, "b", again, &old.doc).is_some());
        assert_eq!(n.partner_replay(&all, "b", again, &old.doc), None);
        assert_eq!(n.partner_replay(&on(), "b", LineId::next(), &old.doc), None);
        // Neither cuts off what is being said.
        assert_eq!(
            n.engine.calls,
            [
                speak("gg", "default"),
                speak("partner line", "default"),
                speak("partner line", "default")
            ]
        );
    }

    #[test]
    fn paced_typing_follows_the_line_s_words_until_its_voice_ends() {
        let mut n = Narrator::new(Fake::default());
        let paced = TtsSettings {
            pace_typing: true,
            ..on()
        };
        // Units: 你 好 ， space w o r l d.
        let l = line("Nana", "你好， world");
        let id = n.line_started(&paced, &characters(), &l).unwrap();
        assert!(n.paces(l.id));
        // "你好" is six bytes of "你好， world": through unit 2.
        let word = Event::Word {
            id,
            start: 0,
            len: 6,
        };
        assert_eq!(
            n.pace(&word),
            Some(Cmd::SpeechReached {
                line: l.id,
                through: 2
            })
        );
        assert_eq!(n.word_chars(&word), Some(2));
        let world = Event::Word {
            id,
            start: 10,
            len: 5,
        };
        assert!(matches!(
            n.pace(&world),
            Some(Cmd::SpeechReached { through: 9, .. })
        ));
        // Another utterance's words are not this line's.
        assert_eq!(
            n.pace(&Event::Word {
                id: id + 1,
                start: 0,
                len: 1
            }),
            None
        );
        assert_eq!(n.pace(&Event::Finished(id)), Some(Cmd::SpeechEnded(l.id)));
        assert!(!n.paces(l.id));
        // Off, nothing is paced.
        let id = n.line_started(&on(), &characters(), &l).unwrap();
        assert_eq!(
            n.pace(&Event::Word {
                id,
                start: 0,
                len: 6
            }),
            None
        );
    }

    #[test]
    fn talking_follows_started_and_finished() {
        let mut n = Narrator::new(Fake::default());
        assert_eq!(n.event(&Event::Started(1)), Some(true));
        assert_eq!(n.event(&Event::Started(2)), None);
        assert_eq!(n.event(&Event::Finished(1)), None);
        assert_eq!(
            n.event(&Event::Word {
                id: 2,
                start: 0,
                len: 1
            }),
            None
        );
        assert_eq!(n.event(&Event::Finished(2)), Some(false));
        assert!(!n.speaking());
    }
}
