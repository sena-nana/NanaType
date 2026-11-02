//! Debounced, atomic file writes on a worker thread, so a slider drag does
//! not write the settings file sixty times a second and the UI thread never
//! touches the disk. Saved history lines are appended here too, in order and
//! without the debounce.

use std::collections::HashMap;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, after, never, unbounded};

const DEBOUNCE: Duration = Duration::from_millis(500);

enum Job {
    Write(PathBuf, Vec<u8>),
    /// Add to the end of a file, making it (and its folder) if needed.
    Append(PathBuf, Vec<u8>),
    /// Delete a file (and the `.bak` its writes keep), dropping any write
    /// still pending for it.
    Remove(PathBuf),
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
            Err(e) => {
                nanatype_diag::report!(crate::diag::PERSIST_FAILED; "{}: {e}", path.display())
            }
        }
    }

    pub fn append(&self, path: PathBuf, bytes: Vec<u8>) {
        let _ = self.tx.send(Job::Append(path, bytes));
    }

    pub fn remove(&self, path: PathBuf) {
        let _ = self.tx.send(Job::Remove(path));
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
                Ok(Job::Append(path, bytes)) => {
                    if let Err(e) = append(&path, &bytes) {
                        on_error(format!("{}: {e}", path.display()));
                    }
                }
                Ok(Job::Remove(path)) => {
                    pending.remove(&path);
                    let mut bak = path.clone().into_os_string();
                    bak.push(".bak");
                    for file in [path, PathBuf::from(bak)] {
                        match std::fs::remove_file(&file) {
                            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                                on_error(format!("{}: {e}", file.display()));
                            }
                            _ => {}
                        }
                    }
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

fn append(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(bytes)
}
