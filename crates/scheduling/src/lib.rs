//! Scheduling preferences for the work a frame waits on.
//!
//! [`choose_adapter`] hands the GPU chosen in settings to the renderer.
//!
//! [`apply_gpu`] raises the whole process's GPU scheduling priority, so the
//! display and its Spout sender still draw while a game fills the GPU.
//!
//! The CPU preference covers only the threads a frame waits on: the UI thread,
//! which lays out, draws and sends every window, and the timer thread, which
//! wakes it for the next reveal. Each marks itself with [`latency_critical`]
//! for as long as it runs; [`apply`] sets every marked thread, and threads
//! marked later start with the current preference. The rest of the process
//! (chat, downloads, decoding, file work) keeps the system's scheduling, so it
//! does not compete with a game or encoder for the fastest cores.

mod adapter;
mod gpu;

use std::sync::{LazyLock, Mutex, MutexGuard};

pub use adapter::{GpuChoice, choose_adapter, gpu_choice};
pub use gpu::apply_gpu;

/// Why a CPU preference is not in effect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The system refused the thread priority.
    Priority(String),
    /// The system refused the preferred CPU sets.
    Affinity(String),
    /// The core information could not be read, with the system's reason
    /// when there is one.
    Topology(Option<String>),
    TopologyIncomplete,
    /// Every core is of one kind: there are no faster cores to prefer.
    NoHybrid,
    /// The faster cores are all reserved for other processes.
    NoPerformanceCores,
    /// This platform has no CPU scheduling preferences.
    Unsupported,
}

impl Problem {
    /// Not a failure: the system has nothing to prefer.
    pub fn not_needed(&self) -> bool {
        matches!(self, Self::NoHybrid | Self::NoPerformanceCores)
    }
}

/// What the last [`apply`] achieved.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// The system has faster cores to prefer.
    pub affinity_available: bool,
    pub priority_error: Option<Problem>,
    pub affinity_error: Option<Problem>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Preference {
    high: bool,
    performance_cores: bool,
}

#[derive(Default)]
struct Registry {
    preference: Preference,
    next: u64,
    threads: Vec<(u64, backend::Thread)>,
    backend: backend::Topology,
    status: Status,
}

static REGISTRY: LazyLock<Mutex<Registry>> = LazyLock::new(Mutex::default);

fn registry() -> MutexGuard<'static, Registry> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

/// Keeps the calling thread under the preference until dropped. Drop it on
/// the thread that made it, before the thread ends.
#[must_use = "the thread is marked only while this lives"]
pub struct Critical {
    key: u64,
}

/// Marks the calling thread as one a frame waits on.
pub fn latency_critical() -> Critical {
    let mut registry = registry();
    registry.next += 1;
    let key = registry.next;
    if let Some(thread) = backend::Thread::current() {
        let preference = registry.preference;
        let sets = registry.backend.sets(preference.performance_cores);
        // A thread that cannot take the preference runs as it would have.
        let _ = thread.apply(preference.high, sets);
        registry.threads.push((key, thread));
    }
    Critical { key }
}

impl Drop for Critical {
    fn drop(&mut self) {
        registry().threads.retain(|(key, _)| *key != self.key);
    }
}

/// Sets the preference on every marked thread, now and to come.
pub fn apply(high: bool, performance_cores: bool) -> Status {
    let mut registry = registry();
    let preference = Preference {
        high,
        performance_cores,
    };
    if registry.preference == preference && registry.backend.probed() {
        return registry.status.clone();
    }
    registry.preference = preference;
    // Probed at an explicit enable, so a failed topology query is retried.
    if performance_cores || !registry.backend.probed() {
        registry.backend.probe();
    }
    let available = registry.backend.available();
    let mut status = Status {
        affinity_available: available.is_ok(),
        priority_error: None,
        affinity_error: available.err().filter(|_| performance_cores),
    };
    let sets = registry.backend.sets(performance_cores);
    for (_, thread) in &registry.threads {
        let (priority, affinity) = thread.apply(high, sets);
        status.priority_error = status.priority_error.or(priority.err());
        status.affinity_error = status.affinity_error.or(affinity.err());
    }
    registry.status = status.clone();
    status
}

/// What the last [`apply`] achieved, without applying anything.
pub fn status() -> Status {
    registry().status.clone()
}

#[cfg(windows)]
mod backend {
    use super::Problem;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::SystemInformation::{
        CpuSetInformation, GetSystemCpuSetInformation, SYSTEM_CPU_SET_INFORMATION,
    };
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentThreadId, OpenThread, SetThreadPriority,
        SetThreadSelectedCpuSets, THREAD_PRIORITY_ABOVE_NORMAL, THREAD_PRIORITY_NORMAL,
        THREAD_QUERY_LIMITED_INFORMATION, THREAD_SET_LIMITED_INFORMATION,
    };

    /// A real handle to a marked thread, closed with it.
    pub(super) struct Thread(HANDLE);

    // SAFETY: a thread handle may be used from any thread.
    unsafe impl Send for Thread {}

    impl Thread {
        pub(super) fn current() -> Option<Self> {
            // SAFETY: opens this thread by its id; the handle is closed on drop.
            unsafe {
                OpenThread(
                    THREAD_SET_LIMITED_INFORMATION | THREAD_QUERY_LIMITED_INFORMATION,
                    false,
                    GetCurrentThreadId(),
                )
            }
            .ok()
            .map(Self)
        }

        /// Sets priority and preferred CPU sets; `None` lets the system choose.
        pub(super) fn apply(
            &self,
            high: bool,
            sets: Option<&[u32]>,
        ) -> (Result<(), Problem>, Result<(), Problem>) {
            // SAFETY: the handle is open with the rights both calls need.
            unsafe {
                let priority = SetThreadPriority(
                    self.0,
                    if high {
                        THREAD_PRIORITY_ABOVE_NORMAL
                    } else {
                        THREAD_PRIORITY_NORMAL
                    },
                )
                .map_err(|error| Problem::Priority(error.message()));
                let affinity = SetThreadSelectedCpuSets(self.0, sets.unwrap_or(&[]))
                    .ok()
                    .map_err(|error| Problem::Affinity(error.message()));
                (priority, affinity)
            }
        }
    }

    impl Drop for Thread {
        fn drop(&mut self) {
            // SAFETY: the handle came from OpenThread and is closed once.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    /// The performance cores, once looked up.
    #[derive(Default)]
    pub(super) struct Topology(Option<Result<Vec<u32>, Problem>>);

    impl Topology {
        pub(super) fn probed(&self) -> bool {
            self.0.is_some()
        }

        pub(super) fn probe(&mut self) {
            self.0 = Some(performance_sets());
        }

        pub(super) fn available(&self) -> Result<(), Problem> {
            match &self.0 {
                Some(Ok(_)) => Ok(()),
                Some(Err(error)) => Err(error.clone()),
                None => Err(Problem::Topology(None)),
            }
        }

        pub(super) fn sets(&self, wanted: bool) -> Option<&[u32]> {
            match &self.0 {
                Some(Ok(sets)) if wanted => Some(sets),
                _ => None,
            }
        }
    }

    struct CpuSet {
        id: u32,
        class: u8,
        flags: u8,
    }

    fn cpu_sets() -> Result<Vec<CpuSet>, Problem> {
        let mut bytes = 0;
        // SAFETY: sizing call has no input buffer, with a valid output length.
        unsafe {
            let _ =
                GetSystemCpuSetInformation(None, 0, &mut bytes, Some(GetCurrentProcess()), None);
        }
        if bytes == 0 {
            return Err(Problem::Topology(None));
        }
        // u64 storage supplies the alignment required by SYSTEM_CPU_SET_INFORMATION.
        let mut buffer = vec![0u64; (bytes as usize).div_ceil(8)];
        let capacity = bytes;
        // SAFETY: aligned writable storage spans at least `capacity` bytes.
        unsafe {
            GetSystemCpuSetInformation(
                Some(buffer.as_mut_ptr().cast()),
                capacity,
                &mut bytes,
                Some(GetCurrentProcess()),
                None,
            )
            .ok()
            .map_err(|e| Problem::Topology(Some(e.message())))?;
        }
        let mut offset = 0;
        let mut sets = Vec::new();
        while offset + 8 <= bytes as usize {
            // SAFETY: the record header is within the returned buffer.
            let pointer = unsafe { buffer.as_ptr().cast::<u8>().add(offset) };
            let size = unsafe { pointer.cast::<u32>().read_unaligned() } as usize;
            if size < 8 || offset + size > bytes as usize {
                return Err(Problem::TopologyIncomplete);
            }
            if size >= std::mem::size_of::<SYSTEM_CPU_SET_INFORMATION>() {
                // SAFETY: the complete structure fits the validated record; the union
                // is read only when its discriminant identifies CPU set information.
                let info = unsafe {
                    pointer
                        .cast::<SYSTEM_CPU_SET_INFORMATION>()
                        .read_unaligned()
                };
                if info.Type == CpuSetInformation {
                    let set = unsafe { info.Anonymous.CpuSet };
                    sets.push(CpuSet {
                        id: set.Id,
                        class: set.EfficiencyClass,
                        flags: unsafe { set.Anonymous1.AllFlags },
                    });
                }
            }
            offset += size;
        }
        Ok(sets)
    }

    fn performance_sets() -> Result<Vec<u32>, Problem> {
        let sets = cpu_sets()?;
        let selected =
            super::select_performance_sets(sets.iter().map(|set| (set.id, set.class, set.flags)));
        if selected.is_empty() {
            return Err(if sets.iter().all(|set| set.class == sets[0].class) {
                Problem::NoHybrid
            } else {
                Problem::NoPerformanceCores
            });
        }
        Ok(selected)
    }
}

#[cfg(not(windows))]
mod backend {
    use super::Problem;

    /// Other platforms keep the system's scheduling; nothing is marked.
    pub(super) struct Thread;

    impl Thread {
        pub(super) fn current() -> Option<Self> {
            None
        }

        pub(super) fn apply(
            &self,
            _high: bool,
            _sets: Option<&[u32]>,
        ) -> (Result<(), Problem>, Result<(), Problem>) {
            (Ok(()), Ok(()))
        }
    }

    #[derive(Default)]
    pub(super) struct Topology(bool);

    impl Topology {
        pub(super) fn probed(&self) -> bool {
            self.0
        }

        pub(super) fn probe(&mut self) {
            self.0 = true;
        }

        pub(super) fn available(&self) -> Result<(), Problem> {
            Err(Problem::Unsupported)
        }

        pub(super) fn sets(&self, _wanted: bool) -> Option<&[u32]> {
            None
        }
    }
}

/// The CPU sets of the fastest efficiency class, when there is a slower one.
/// Higher `EfficiencyClass` values mean faster cores. Keeps both SMT siblings
/// and IDs from every processor group; never guesses from CPU index order.
/// Sets reserved for another process (flag 2) are skipped unless reserved for
/// this one (flag 4).
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn select_performance_sets(sets: impl Iterator<Item = (u32, u8, u8)> + Clone) -> Vec<u32> {
    let Some(highest) = sets.clone().map(|set| set.1).max() else {
        return Vec::new();
    };
    if sets.clone().all(|set| set.1 == highest) {
        return Vec::new();
    }
    sets.filter(|(_, class, flags)| *class == highest && (flags & 2 == 0 || flags & 4 != 0))
        .map(|set| set.0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_fastest_class_and_skips_cores_reserved_for_others() {
        let select = |sets: &[(u32, u8, u8)]| select_performance_sets(sets.iter().copied());
        assert_eq!(
            select(&[(70, 0, 0), (4, 8, 0), (5, 8, 1), (90, 8, 2), (91, 8, 6)]),
            vec![4, 5, 91]
        );
        assert!(select(&[(4, 0, 0), (5, 0, 0)]).is_empty());
        assert!(select(&[]).is_empty());
        assert!(select(&[(1, 0, 0), (2, 1, 2)]).is_empty());
    }

    #[test]
    fn marked_threads_come_and_go_and_take_the_preference() {
        let status = apply(true, false);
        assert_eq!(status.priority_error, None, "{status:?}");
        assert_eq!(status, self::status());
        let marked = std::thread::spawn(|| {
            let critical = latency_critical();
            #[cfg(windows)]
            {
                use windows::Win32::System::Threading::{
                    GetCurrentThread, GetThreadPriority, THREAD_PRIORITY_ABOVE_NORMAL,
                };
                // SAFETY: queries this thread's own pseudo-handle.
                let priority = unsafe { GetThreadPriority(GetCurrentThread()) };
                assert_eq!(priority, THREAD_PRIORITY_ABOVE_NORMAL.0);
            }
            drop(critical);
            registry().threads.len()
        })
        .join()
        .unwrap();
        assert_eq!(marked, 0, "a dropped mark leaves the registry");
        assert_eq!(apply(false, false).priority_error, None);
    }

    #[test]
    fn only_a_missing_kind_of_core_is_not_needed() {
        assert!(Problem::NoHybrid.not_needed());
        assert!(Problem::NoPerformanceCores.not_needed());
        assert!(!Problem::Topology(None).not_needed());
        assert!(!Problem::Affinity(String::new()).not_needed());
    }
}
