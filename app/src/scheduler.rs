//! The timer thread. Wakes and visual cues wait in a min-heap and are posted
//! to the UI thread when due; the UI thread never polls. Audio cues do not
//! come here: they go straight to the mixer, which starts them on the exact
//! sample.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::thread;
use std::time::Instant;

use crossbeam_channel::{Receiver, Sender, after, never, unbounded};
use nanatype_core::Wake;
use nanatype_playback::CueKind;

use crate::clock;
use crate::msg::Msg;

enum Job {
    OutputCleanup,
    Wake(Wake),
    Cue { epoch: u64, kind: CueKind },
    Post(Box<Msg>),
}

enum Command {
    OutputCleanup(Option<Instant>),
    Add(Instant, Job),
    Cancel(u64),
}

#[derive(Clone)]
pub struct Scheduler {
    tx: Sender<Command>,
}

struct Entry {
    at: Instant,
    seq: u64,
    job: Job,
}

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        (self.at, self.seq) == (other.at, other.seq)
    }
}
impl Eq for Entry {}
impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Entry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.at, self.seq).cmp(&(other.at, other.seq))
    }
}

impl Scheduler {
    pub fn output_cleanup(&self, at: Option<u64>) {
        let _ = self.tx.send(Command::OutputCleanup(at.map(clock::instant)));
    }
    pub fn start(post: impl Fn(Msg) + Send + 'static) -> Self {
        let (tx, rx) = unbounded();
        thread::Builder::new()
            .name("nanatype-timer".into())
            .spawn(move || run(rx, post))
            .expect("spawn timer thread");
        Self { tx }
    }

    pub fn wake(&self, wake: Wake) {
        let _ = self
            .tx
            .send(Command::Add(clock::instant(wake.at_ms), Job::Wake(wake)));
    }

    pub fn cue(&self, epoch: u64, at_ms: u64, kind: CueKind) {
        let _ = self.tx.send(Command::Add(
            clock::instant(at_ms),
            Job::Cue { epoch, kind },
        ));
    }

    /// Deliver `msg` after `delay`, on this timer instead of a sleeping thread.
    pub fn post_after(&self, delay: std::time::Duration, msg: Msg) {
        let _ = self.tx.send(Command::Add(
            Instant::now() + delay,
            Job::Post(Box::new(msg)),
        ));
    }

    /// Drop pending cues of `epoch` (wakes carry their own staleness check).
    pub fn cancel(&self, epoch: u64) {
        let _ = self.tx.send(Command::Cancel(epoch));
    }
}

fn run(rx: Receiver<Command>, post: impl Fn(Msg)) {
    let mut heap: BinaryHeap<Reverse<Entry>> = BinaryHeap::new();
    let mut seq = 0u64;
    loop {
        let timeout = match heap.peek() {
            Some(Reverse(e)) => after(e.at.saturating_duration_since(Instant::now())),
            None => never(),
        };
        crossbeam_channel::select! {
            recv(rx) -> cmd => match cmd {
                Ok(Command::OutputCleanup(at)) => {
                    heap.retain(|Reverse(e)| !matches!(e.job, Job::OutputCleanup));
                    if let Some(at) = at {
                        seq += 1;
                        heap.push(Reverse(Entry { at, seq, job: Job::OutputCleanup }));
                    }
                }
                Ok(Command::Add(at, job)) => {
                    seq += 1;
                    heap.push(Reverse(Entry { at, seq, job }));
                }
                Ok(Command::Cancel(epoch)) => {
                    heap.retain(|Reverse(e)| !matches!(e.job, Job::Cue { epoch: x, .. } if x == epoch));
                }
                Err(_) => return,
            },
            recv(timeout) -> _ => {}
        }
        let now = Instant::now();
        while heap.peek().is_some_and(|Reverse(e)| e.at <= now) {
            let Reverse(entry) = heap.pop().expect("peeked");
            match entry.job {
                Job::OutputCleanup => post(Msg::OutputCleanup),
                Job::Wake(w) => post(Msg::Wake {
                    epoch: w.epoch,
                    reason: w.reason,
                }),
                Job::Cue { kind, .. } => post(Msg::Cue(kind)),
                Job::Post(msg) => post(*msg),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn output_cleanup_replaces_and_cancels_a_single_deadline() {
        let (tx, rx) = unbounded();
        let (posted, received) = unbounded();
        let worker = thread::spawn(move || {
            run(rx, |msg| {
                assert!(matches!(msg, Msg::OutputCleanup));
                posted.send(()).unwrap();
            })
        });
        let now = Instant::now();
        for _ in 0..20 {
            tx.send(Command::OutputCleanup(Some(
                now + Duration::from_millis(100),
            )))
            .unwrap();
        }
        tx.send(Command::OutputCleanup(None)).unwrap();
        tx.send(Command::OutputCleanup(Some(
            now + Duration::from_millis(50),
        )))
        .unwrap();
        received.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(received.recv_timeout(Duration::from_millis(150)).is_err());
        drop(tx);
        worker.join().unwrap();
    }
}
