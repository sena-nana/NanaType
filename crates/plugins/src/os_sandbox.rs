//! Operating-system confinement for the worker process, applied before it
//! reads any request. This is defence in depth below the Wasm interpreter:
//! even a runtime escape would find no files, network or child processes.
//!
//! - macOS: a deny-by-default Seatbelt profile (`sandbox_init`). Pipes the
//!   host opened before the call keep working; new files, sockets, process
//!   creation and service lookups are refused.
//! - Windows: the host places the worker in a job object (see
//!   `confine_child`) that forbids child processes and desktop, clipboard and
//!   system-setting access, and kills the worker with the host. AppContainer
//!   isolation is not implemented.
//! - Elsewhere: no OS confinement; only the interpreter boundary applies.

/// The confinement applied in this worker, for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confinement {
    Seatbelt,
    JobObject,
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
pub fn confine_worker() -> Result<Confinement, String> {
    // The host assigns the job object before the worker reads anything.
    Ok(Confinement::JobObject)
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

/// Place a freshly spawned worker in a job before it is sent any request:
/// one process only, no desktop, clipboard or system-setting access, a
/// committed-memory cap, and termination with the host.
#[cfg(windows)]
#[allow(unsafe_code)]
pub(crate) fn confine_child(child: &std::process::Child) -> std::io::Result<Job> {
    use std::os::windows::io::AsRawHandle;
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
    // process handle stays valid while `child` is borrowed.
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
        fail(AssignProcessToJobObject(job.0, child.as_raw_handle() as _))?;
    }
    Ok(job)
}
