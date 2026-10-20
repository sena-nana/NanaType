//! Operating-system confinement for the worker process, applied before it
//! reads any request. This is defence in depth below the Wasm interpreter:
//! macOS denies new files, network and child processes. Windows job limits
//! do not provide file or network isolation.
//!
//! - macOS: a deny-by-default Seatbelt profile (`sandbox_init`). Pipes the
//!   host opened before the call keep working; new files, sockets, process
//!   creation and service lookups are refused.
//! - Windows: the host places the worker in a job object (see
//!   `confine_process`) that forbids child processes and desktop, clipboard and
//!   system-setting access, and kills the worker with the host. With
//!   `NANATYPE_PLUGIN_APPCONTAINER=1` the worker also starts in an
//!   AppContainer (see `app_container`); that path is not yet verified on
//!   Windows, so it is off by default.
//! - Elsewhere: no OS confinement; only the interpreter boundary applies.

/// The confinement applied in this worker, for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confinement {
    Seatbelt,
    JobObject,
    /// A job object, inside an AppContainer.
    AppContainer,
    None,
}

#[cfg(target_os = "macos")]
const PROFILE: &str = "(version 1)\n(deny default)\n(allow sysctl-read)\n";

/// Confine the current (worker) process. An error means it must not run plugins.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub fn confine_worker() -> Result<Confinement, String> {
    use std::ffi::{CStr, CString, c_char, c_int};
    unsafe extern "C" {
        fn sandbox_init(profile: *const c_char, flags: u64, errorbuf: *mut *mut c_char) -> c_int;
        fn sandbox_free_error(errorbuf: *mut c_char);
    }
    let profile = CString::new(PROFILE).map_err(|e| e.to_string())?;
    let mut error: *mut c_char = std::ptr::null_mut();
    // SAFETY: `profile` is a valid NUL-terminated string for the call's
    // duration (flags 0 = literal profile text); a non-null `error` is owned
    // by the library and released with `sandbox_free_error`.
    let status = unsafe { sandbox_init(profile.as_ptr(), 0, &mut error) };
    if status == 0 {
        return Ok(Confinement::Seatbelt);
    }
    let message = if error.is_null() {
        "sandbox_init failed".to_owned()
    } else {
        // SAFETY: non-null `error` points to a NUL-terminated message.
        let text = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { sandbox_free_error(error) };
        text
    };
    Err(format!("OS sandbox unavailable: {message}"))
}

#[cfg(windows)]
#[allow(unsafe_code)]
pub fn confine_worker() -> Result<Confinement, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TokenIsAppContainer};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    // The host assigns the job object before the worker reads anything; the
    // token says whether it also started the worker in an AppContainer.
    let mut token: HANDLE = std::ptr::null_mut();
    let mut contained = 0u32;
    let mut size = 0u32;
    // SAFETY: the pseudo-handle needs no closing; `token` is closed below;
    // TokenIsAppContainer writes one DWORD.
    let known = unsafe {
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) != 0 && {
            let ok = GetTokenInformation(
                token,
                TokenIsAppContainer,
                (&raw mut contained).cast(),
                size_of::<u32>() as u32,
                &mut size,
            ) != 0;
            CloseHandle(token);
            ok
        }
    };
    Ok(if known && contained != 0 {
        Confinement::AppContainer
    } else {
        Confinement::JobObject
    })
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn confine_worker() -> Result<Confinement, String> {
    Ok(Confinement::None)
}

/// Committed memory a Windows worker may use; Wasm memory is capped far lower.
#[cfg(windows)]
const PROCESS_MEMORY: usize = 256 * 1024 * 1024;

/// The job a Windows worker runs in. Dropping it (or the host exiting) closes
/// the handle, and `KILL_ON_JOB_CLOSE` ends the worker.
#[cfg(windows)]
pub(crate) struct Job(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
// SAFETY: a job handle is a kernel handle usable from any thread.
#[allow(unsafe_code)]
unsafe impl Send for Job {}
#[cfg(windows)]
#[allow(unsafe_code)]
impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle came from `CreateJobObjectW` and is closed once.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}

/// Place a freshly spawned worker in a job before it is sent any request.
#[cfg(windows)]
pub(crate) fn confine_child(child: &std::process::Child) -> std::io::Result<Job> {
    use std::os::windows::io::AsRawHandle;
    confine_process(child.as_raw_handle() as _)
}

/// Place a worker process in a new job: one process only, no desktop,
/// clipboard or system-setting access, a committed-memory cap, and
/// termination with the host.
#[cfg(windows)]
#[allow(unsafe_code)]
pub(crate) fn confine_process(
    process: windows_sys::Win32::Foundation::HANDLE,
) -> std::io::Result<Job> {
    use windows_sys::Win32::System::JobObjects::*;
    let fail = |ok: i32| {
        if ok == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    };
    // SAFETY: null attributes and name create an anonymous, unshared job.
    let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if handle.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    let job = Job(handle);
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
        | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION
        | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
    limits.BasicLimitInformation.ActiveProcessLimit = 1;
    limits.ProcessMemoryLimit = PROCESS_MEMORY;
    let ui = JOBOBJECT_BASIC_UI_RESTRICTIONS {
        UIRestrictionsClass: JOB_OBJECT_UILIMIT_DESKTOP
            | JOB_OBJECT_UILIMIT_DISPLAYSETTINGS
            | JOB_OBJECT_UILIMIT_EXITWINDOWS
            | JOB_OBJECT_UILIMIT_GLOBALATOMS
            | JOB_OBJECT_UILIMIT_HANDLES
            | JOB_OBJECT_UILIMIT_READCLIPBOARD
            | JOB_OBJECT_UILIMIT_WRITECLIPBOARD
            | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS,
    };
    // SAFETY: each structure is fully initialised and its size is passed; the
    // caller keeps the process handle open for the call.
    unsafe {
        fail(SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ))?;
        fail(SetInformationJobObject(
            job.0,
            JobObjectBasicUIRestrictions,
            (&raw const ui).cast(),
            size_of::<JOBOBJECT_BASIC_UI_RESTRICTIONS>() as u32,
        ))?;
        fail(AssignProcessToJobObject(job.0, process))?;
    }
    Ok(job)
}

#[cfg(all(test, windows))]
#[allow(unsafe_code)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, Read, Write},
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    use windows_sys::Win32::System::{JobObjects::*, Memory::*};

    #[test]
    fn windows_job_limits_and_close_are_enforced() {
        const CHILD: &str = "NANATYPE_JOB_PROBE";
        if std::env::var_os(CHILD).is_some() {
            // The host applies its production job before releasing this gate.
            std::io::stdin().read_exact(&mut [0]).unwrap();
            let spawned = Command::new(std::env::current_exe().unwrap())
                .arg("--list")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            assert!(
                !spawned.is_ok_and(|s| s.success()),
                "child process escaped the one-process limit"
            );
            // Commit, without touching pages, more than the worker's entire cap.
            let memory = unsafe {
                VirtualAlloc(
                    std::ptr::null(),
                    PROCESS_MEMORY + 4096,
                    MEM_RESERVE | MEM_COMMIT,
                    PAGE_READWRITE,
                )
            };
            if !memory.is_null() {
                unsafe { VirtualFree(memory, 0, MEM_RELEASE) };
                panic!("worker exceeded its committed-memory limit");
            }
            println!("job_probe_ok");
            std::io::stdout().flush().unwrap();
            // Remain alive so the parent can verify KILL_ON_JOB_CLOSE.
            std::io::stdin().read_exact(&mut [0]).unwrap();
            return;
        }
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "os_sandbox::tests::windows_job_limits_and_close_are_enforced",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let job = confine_child(&child).unwrap();
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        let mut ui = JOBOBJECT_BASIC_UI_RESTRICTIONS::default();
        // Read back the actual kernel configuration, not a copy of our constants.
        unsafe {
            assert_ne!(
                QueryInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    (&raw mut limits).cast(),
                    size_of_val(&limits) as u32,
                    std::ptr::null_mut()
                ),
                0
            );
            assert_ne!(
                QueryInformationJobObject(
                    job.0,
                    JobObjectBasicUIRestrictions,
                    (&raw mut ui).cast(),
                    size_of_val(&ui) as u32,
                    std::ptr::null_mut()
                ),
                0
            );
        }
        assert_eq!(limits.BasicLimitInformation.ActiveProcessLimit, 1);
        assert_eq!(limits.ProcessMemoryLimit, PROCESS_MEMORY);
        assert_eq!(
            ui.UIRestrictionsClass,
            JOB_OBJECT_UILIMIT_DESKTOP
                | JOB_OBJECT_UILIMIT_DISPLAYSETTINGS
                | JOB_OBJECT_UILIMIT_EXITWINDOWS
                | JOB_OBJECT_UILIMIT_GLOBALATOMS
                | JOB_OBJECT_UILIMIT_HANDLES
                | JOB_OBJECT_UILIMIT_READCLIPBOARD
                | JOB_OBJECT_UILIMIT_WRITECLIPBOARD
                | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS
        );
        child.stdin.as_mut().unwrap().write_all(&[1]).unwrap();
        let output = std::io::BufReader::new(child.stdout.take().unwrap());
        let (send, ready) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let ok = output
                .lines()
                .map(|line| line.unwrap())
                .any(|line| line.contains("job_probe_ok"));
            let _ = send.send(ok);
        });
        assert!(
            ready.recv_timeout(Duration::from_secs(30)).unwrap(),
            "confined child failed its probes"
        );
        reader.join().unwrap();
        assert!(
            child.try_wait().unwrap().is_none(),
            "probe must still be waiting for input"
        );
        drop(job);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("worker survived the last job handle");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
