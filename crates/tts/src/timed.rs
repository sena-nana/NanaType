//! Utterances synthesized to audio ahead of time, played one after another
//! with their word boundaries kept in step with the sound.
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::Event;

/// A word of a clip: when it is heard, and its byte range in the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Word {
    pub at: Duration,
    pub start: usize,
    pub len: usize,
}

/// One utterance as sound.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Clip {
    pub id: u64,
    pub sound: nanatype_audio::SoundId,
    pub gain: f32,
    pub length: Duration,
    /// In time order.
    pub words: Vec<Word>,
}

struct Playing {
    clip: Clip,
    since: Instant,
    /// Words already reported.
    said: usize,
}

/// The queue of clips and the one playing.
#[derive(Default)]
pub(crate) struct Timed {
    queue: VecDeque<Clip>,
    playing: Option<Playing>,
}

impl Timed {
    pub fn push(&mut self, clip: Clip) {
        self.queue.push_back(clip);
    }

    pub fn busy(&self) -> bool {
        self.playing.is_some() || !self.queue.is_empty()
    }

    /// Report what was heard by `now`: start the next clip when one ends
    /// (`start` plays it, `done` lets its sound go), its words as their
    /// moment comes.
    pub fn poll(
        &mut self,
        now: Instant,
        start: &mut dyn FnMut(&Clip),
        done: &mut dyn FnMut(&Clip),
        out: &mut dyn FnMut(Event),
    ) {
        loop {
            if self.playing.is_none() {
                let Some(clip) = self.queue.pop_front() else {
                    return;
                };
                start(&clip);
                out(Event::Started(clip.id));
                self.playing = Some(Playing {
                    clip,
                    since: now,
                    said: 0,
                });
            }
            let p = self.playing.as_mut().expect("playing");
            let heard = now.saturating_duration_since(p.since);
            while let Some(w) = p.clip.words.get(p.said).filter(|w| w.at <= heard) {
                out(Event::Word {
                    id: p.clip.id,
                    start: w.start,
                    len: w.len,
                });
                p.said += 1;
            }
            if heard < p.clip.length {
                return;
            }
            let p = self.playing.take().expect("playing");
            done(&p.clip);
            out(Event::Finished(p.clip.id));
        }
    }

    /// Silence everything; `end` lets each clip's sound go.
    pub fn stop(&mut self, end: &mut dyn FnMut(&Clip)) {
        if let Some(p) = self.playing.take() {
            end(&p.clip);
        }
        for clip in self.queue.drain(..) {
            end(&clip);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(id: u64, ms: u64, words: &[(u64, usize, usize)]) -> Clip {
        Clip {
            id,
            sound: format!("tts:{id}").into(),
            gain: 1.0,
            length: Duration::from_millis(ms),
            words: words
                .iter()
                .map(|&(at, start, len)| Word {
                    at: Duration::from_millis(at),
                    start,
                    len,
                })
                .collect(),
        }
    }

    #[test]
    fn clips_play_in_turn_with_their_words_on_time() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut timed = Timed::default();
        timed.push(clip(1, 500, &[(0, 0, 5), (300, 6, 5)]));
        timed.push(clip(2, 200, &[(50, 0, 2)]));
        let mut started = Vec::new();
        let mut ended = Vec::new();
        let mut events = Vec::new();
        let mut poll = |timed: &mut Timed, at| {
            timed.poll(
                at,
                &mut |c| started.push(c.id),
                &mut |c| ended.push(c.id),
                &mut |e| events.push(e),
            )
        };
        poll(&mut timed, ms(0));
        poll(&mut timed, ms(299));
        poll(&mut timed, ms(310));
        assert!(timed.busy());
        // The first ends and the second starts in the same look.
        poll(&mut timed, ms(520));
        poll(&mut timed, ms(580));
        poll(&mut timed, ms(800));
        assert!(!timed.busy());
        assert_eq!(started, [1, 2]);
        assert_eq!(ended, [1, 2]);
        assert_eq!(
            events,
            [
                Event::Started(1),
                Event::Word {
                    id: 1,
                    start: 0,
                    len: 5
                },
                Event::Word {
                    id: 1,
                    start: 6,
                    len: 5
                },
                Event::Finished(1),
                Event::Started(2),
                Event::Word {
                    id: 2,
                    start: 0,
                    len: 2
                },
                Event::Finished(2),
            ]
        );
    }

    #[test]
    fn stopping_lets_every_clip_go() {
        let mut timed = Timed::default();
        timed.push(clip(1, 500, &[]));
        timed.push(clip(2, 500, &[]));
        timed.poll(Instant::now(), &mut |_| {}, &mut |_| {}, &mut |_| {});
        let mut gone = Vec::new();
        timed.stop(&mut |c| gone.push(c.id));
        assert_eq!(gone, [1, 2]);
        assert!(!timed.busy());
    }
}
