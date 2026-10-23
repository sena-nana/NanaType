//! Debounced, atomic file writes on a worker thread, so a slider drag does
//! not write the settings file sixty times a second and the UI thread never
//! touches the disk.

use std::collections::HashMap;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, after, never, unbounded};

const DEBOUNCE: Duration = Duration::from_millis(500);

enum Job {
    Write(PathBuf, Vec<u8>),
    /// Write everything pending now and reply.
    Flush(Sender<()>),
}

#[derive(Clone)]
pub struct Persist {
    tx: Sender<Job>,
}

impl Persist {
    pub fn start(on_error: impl Fn(String) + Send + 'static) -> Self {
        let (tx, rx) = unbounded();
        thread::Builder::new()
            .name("nanatype-persist".into())
            .spawn(move || run(rx, on_error))
            .expect("spawn persist thread");
        Self { tx }
    }

    pub fn json<T: serde::Serialize>(&self, path: PathBuf, value: &T) {
        match serde_json::to_vec_pretty(value) {
            Ok(bytes) => drop(self.tx.send(Job::Write(path, bytes))),
            Err(e) => nanatype_diag::report!(crate::diag::PERSIST_FAILED; "{}: {e}", path.display()),
        }
    }

    /// Block until pending writes are on disk (exit), at most `timeout`.
    pub fn flush(&self, timeout: Duration) {
        let (tx, rx) = crossbeam_channel::bounded(1);
        if self.tx.send(Job::Flush(tx)).is_ok() {
            let _ = rx.recv_timeout(timeout);
        }
    }
}

fn run(rx: Receiver<Job>, on_error: impl Fn(String)) {
    let mut pending: HashMap<PathBuf, Vec<u8>> = HashMap::new();
    let mut due: Option<Instant> = None;
    let write_all = |pending: &mut HashMap<PathBuf, Vec<u8>>| {
        for (path, bytes) in pending.drain() {
            if let Err(e) = nanatype_settings::write_atomic(&path, &bytes) {
                on_error(format!("{}: {e}", path.display()));
            }
        }
    };
    loop {
        let timer = match due {
            Some(at) => after(at.saturating_duration_since(Instant::now())),
            None => never(),
        };
        crossbeam_channel::select! {
            recv(rx) -> job => match job {
                Ok(Job::Write(path, bytes)) => {
                    pending.insert(path, bytes);
                    due.get_or_insert_with(|| Instant::now() + DEBOUNCE);
                }
                Ok(Job::Flush(reply)) => {
                    write_all(&mut pending);
                    due = None;
                    let _ = reply.send(());
                }
                Err(_) => {
                    write_all(&mut pending);
                    return;
                }
            },
            recv(timer) -> _ => {
                write_all(&mut pending);
                due = None;
            }
        }
    }
}
