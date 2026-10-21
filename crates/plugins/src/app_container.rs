//! Windows AppContainer launch for the worker, opt-in until it has been
//! verified on Windows (`NANATYPE_PLUGIN_APPCONTAINER=1`).
//!
//! The worker starts suspended in a per-user AppContainer with no
//! capabilities: no network, and only what is readable by every app package
//! (system files) plus the worker executable, which is granted read and
//! execute here. It inherits exactly its two pipe ends, gets no environment
//! but the container's own LOCALAPPDATA, TEMP and TMP, has the system
//! directory as its working directory, and joins the same job object as an
//! unconfined worker before its first instruction.
#![allow(unsafe_code)]

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::{Mutex, PoisonError};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, HANDLE, HANDLE_FLAG_INHERIT, LocalFree,
    SetHandleInformation,
};
use windows_sys::Win32::Security::Authorization::{
    EXPLICIT_ACCESS_W, GRANT_ACCESS, GetNamedSecurityInfoW, NO_MULTIPLE_TRUSTEE, SE_FILE_OBJECT,
    SetEntriesInAclW, SetNamedSecurityInfoW, TRUSTEE_IS_SID, TRUSTEE_IS_WELL_KNOWN_GROUP,
    TRUSTEE_W,
};
use windows_sys::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
};
use windows_sys::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, FreeSid, NO_INHERITANCE, PSECURITY_DESCRIPTOR, PSID,
    SECURITY_ATTRIBUTES, SECURITY_CAPABILITIES,
};
use windows_sys::Win32::Storage::FileSystem::{FILE_GENERIC_EXECUTE, FILE_GENERIC_READ};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, INFINITE, InitializeProcThreadAttributeList,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, PROCESS_INFORMATION, ResumeThread,
    STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW, TerminateProcess,
    UpdateProcThreadAttribute, WaitForSingleObject,
};

/// The environment variable that turns AppContainer launch on.
pub const OPT_IN: &str = "NANATYPE_PLUGIN_APPCONTAINER";
/// The per-user AppContainer profile every worker shares.
const PROFILE: &str = "NanaType.PluginWorker";

/// Whether workers are launched into the AppContainer.
pub(crate) fn enabled() -> bool {
    std::env::var_os(OPT_IN).is_some_and(|value| value == "1")
}

fn wide(text: impl AsRef<OsStr>) -> Vec<u16> {
    text.as_ref().encode_wide().chain([0]).collect()
}

fn check(ok: i32) -> io::Result<()> {
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Prefix an error with the step that failed, so a bare OS code says where
/// it came from.
fn at(step: &'static str) -> impl FnOnce(io::Error) -> io::Error {
    move |error| io::Error::new(error.kind(), format!("{step}: {error}"))
}

fn win32(error: u32) -> io::Result<()> {
    if error == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(error as i32))
    }
}

/// An AppContainer SID, freed with `FreeSid` as both creating calls require.
struct Sid(PSID);
impl Drop for Sid {
    fn drop(&mut self) {
        // SAFETY: the SID came from CreateAppContainerProfile or
        // DeriveAppContainerSidFromAppContainerName and is freed once.
        unsafe { FreeSid(self.0) };
    }
}

/// The profile's SID, creating the profile on first use.
fn container_sid() -> io::Result<Sid> {
    // Two threads creating the profile at once can see ERROR_BAD_ENVIRONMENT
    // from the one that loses, so creation is one at a time.
    static CREATING: Mutex<()> = Mutex::new(());
    let _creating = CREATING.lock().unwrap_or_else(PoisonError::into_inner);
    let name = wide(PROFILE);
    let display = wide("NanaType plugin worker");
    let mut sid: PSID = null_mut();
    // SAFETY: the strings are NUL-terminated and outlive the call; no
    // capabilities are passed; `sid` receives an allocation we own.
    let created = unsafe {
        CreateAppContainerProfile(
            name.as_ptr(),
            display.as_ptr(),
            display.as_ptr(),
            null(),
            0,
            &mut sid,
        )
    };
    if created >= 0 {
        return Ok(Sid(sid));
    }
    // HRESULT_FROM_WIN32(ERROR_ALREADY_EXISTS): a previous run made it.
    if created as u32 != 0x8007_0000 | ERROR_ALREADY_EXISTS {
        return Err(io::Error::from_raw_os_error(created));
    }
    // SAFETY: as above.
    let derived = unsafe { DeriveAppContainerSidFromAppContainerName(name.as_ptr(), &mut sid) };
    if derived < 0 {
        return Err(io::Error::from_raw_os_error(derived));
    }
    Ok(Sid(sid))
}

/// Let the container read and run `path`. Fails where the user may not
/// change the file's DACL, as under Program Files, where every app package
/// can already read and execute it.
fn grant_read_execute(path: &Path, sid: &Sid) -> io::Result<()> {
    let name = wide(path);
    let mut dacl: *mut ACL = null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: `name` is NUL-terminated; the descriptor the call allocates
    // (which `dacl` points into) is released with LocalFree below.
    win32(unsafe {
        GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        )
    })?;
    let entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: FILE_GENERIC_READ | FILE_GENERIC_EXECUTE,
        grfAccessMode: GRANT_ACCESS,
        grfInheritance: NO_INHERITANCE,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_WELL_KNOWN_GROUP,
            ptstrName: sid.0.cast(),
        },
    };
    let mut merged: *mut ACL = null_mut();
    // SAFETY: one fully initialised entry; `dacl` is valid while
    // `descriptor` lives; `merged` is a LocalAlloc'd ACL we free.
    let result = win32(unsafe { SetEntriesInAclW(1, &entry, dacl, &mut merged) }).and_then(|()| {
        // SAFETY: `merged` is a valid ACL; only the DACL is written.
        let written = win32(unsafe {
            SetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                merged,
                null(),
            )
        });
        // SAFETY: allocated by SetEntriesInAclW.
        unsafe { LocalFree(merged.cast()) };
        written
    });
    // SAFETY: allocated by GetNamedSecurityInfoW.
    unsafe { LocalFree(descriptor) };
    result
}

/// A worker in the AppContainer. Dropping it closes the handle; reaping is
/// the owner's (see `process::ChildGuard`).
pub(crate) struct Contained {
    process: HANDLE,
    pid: u32,
}
// SAFETY: a process handle is a kernel handle usable from any thread.
unsafe impl Send for Contained {}
impl Contained {
    pub(crate) fn id(&self) -> u32 {
        self.pid
    }
    pub(crate) fn terminate(&mut self) {
        // SAFETY: a live process handle with PROCESS_TERMINATE (we created it).
        unsafe { TerminateProcess(self.process, 1) };
    }
    pub(crate) fn wait_exit(&mut self) {
        // SAFETY: as above; SYNCHRONIZE access is included.
        unsafe { WaitForSingleObject(self.process, INFINITE) };
    }
}
impl Drop for Contained {
    fn drop(&mut self) {
        // SAFETY: closed once.
        unsafe { CloseHandle(self.process) };
    }
}

/// A process-thread attribute list, deleted on drop.
struct Attributes(Vec<usize>);
impl Attributes {
    fn new(count: u32) -> io::Result<Self> {
        let mut size = 0;
        // SAFETY: a null list asks only for the size it needs.
        unsafe { InitializeProcThreadAttributeList(null_mut(), count, 0, &mut size) };
        let mut buffer = vec![0usize; size.div_ceil(size_of::<usize>())];
        // SAFETY: `buffer` holds at least `size` bytes, aligned for pointers.
        check(unsafe {
            InitializeProcThreadAttributeList(buffer.as_mut_ptr().cast(), count, 0, &mut size)
        })?;
        Ok(Self(buffer))
    }
    fn list(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.0.as_mut_ptr().cast()
    }
    /// SAFETY: `value` must stay valid until the process is created.
    unsafe fn set<T>(&mut self, attribute: usize, value: *const T, size: usize) -> io::Result<()> {
        // SAFETY: per the caller's contract.
        check(unsafe {
            UpdateProcThreadAttribute(
                self.list(),
                0,
                attribute,
                value.cast(),
                size,
                null_mut(),
                null(),
            )
        })
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        // SAFETY: initialised in `new`.
        unsafe { DeleteProcThreadAttributeList(self.list()) };
    }
}

/// A pipe whose `host` end stays with this process and whose other end the
/// worker inherits.
fn pipe(host_reads: bool) -> io::Result<(File, OwnedHandle)> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let (mut read, mut write): (HANDLE, HANDLE) = (null_mut(), null_mut());
    // SAFETY: out-pointers to locals; both handles are wrapped at once.
    check(unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) })?;
    // SAFETY: fresh, owned pipe handles.
    let (read, write) = unsafe {
        (
            OwnedHandle::from_raw_handle(read),
            OwnedHandle::from_raw_handle(write),
        )
    };
    let (host, worker) = if host_reads {
        (read, write)
    } else {
        (write, read)
    };
    // SAFETY: a valid handle; only its inheritance flag changes.
    check(unsafe { SetHandleInformation(raw(&host), HANDLE_FLAG_INHERIT, 0) })?;
    Ok((File::from(host), worker))
}

fn raw(handle: &OwnedHandle) -> HANDLE {
    std::os::windows::io::AsRawHandle::as_raw_handle(handle)
}

/// Launch `executable args…` in the AppContainer with only `env`, confined
/// by the worker job before it runs. Returns the process, its stdin and
/// stdout, and the job (closing it ends the worker).
pub(crate) fn spawn(
    executable: &Path,
    args: &[&str],
    env: &[(&str, &str)],
) -> io::Result<(Contained, File, File, crate::os_sandbox::Job)> {
    let sid = container_sid().map_err(at("AppContainer profile"))?;
    // Under Program Files the user cannot write the DACL and need not.
    let _ = grant_read_execute(executable, &sid);
    let (stdin, worker_stdin) = pipe(false).map_err(at("stdin pipe"))?;
    let (stdout, worker_stdout) = pipe(true).map_err(at("stdout pipe"))?;
    let inherited = [raw(&worker_stdin), raw(&worker_stdout)];
    let capabilities = SECURITY_CAPABILITIES {
        AppContainerSid: sid.0,
        Capabilities: null_mut(),
        CapabilityCount: 0,
        Reserved: 0,
    };
    let mut attributes = Attributes::new(2).map_err(at("InitializeProcThreadAttributeList"))?;
    // SAFETY: both values outlive CreateProcessW below.
    unsafe {
        attributes
            .set(
                PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
                &capabilities,
                size_of::<SECURITY_CAPABILITIES>(),
            )
            .map_err(at("security capabilities attribute"))?;
        attributes
            .set(
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                inherited.as_ptr(),
                size_of_val(&inherited),
            )
            .map_err(at("handle list attribute"))?;
    }
    let startup = STARTUPINFOEXW {
        StartupInfo: STARTUPINFOW {
            cb: size_of::<STARTUPINFOEXW>() as u32,
            dwFlags: STARTF_USESTDHANDLES,
            hStdInput: inherited[0],
            hStdOutput: inherited[1],
            hStdError: null_mut(),
            ..Default::default()
        },
        lpAttributeList: attributes.list(),
    };
    // A Windows path cannot hold `"`, and the arguments are our own flags.
    debug_assert!(
        args.iter()
            .all(|arg| !arg.is_empty() && !arg.contains([' ', '\t', '"']))
    );
    let mut line = wide(format!("\"{}\" {}", executable.display(), args.join(" ")));
    // CreateProcessW fails with ERROR_ENVVAR_NOT_FOUND when an AppContainer
    // process's environment has no LOCALAPPDATA (and with ERROR_FILE_NOT_FOUND
    // when it is empty), so the host's is passed on. Windows replaces it with
    // the container's own folder and adds TEMP and TMP beneath it, so the
    // worker never sees the host's path.
    let local = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is not set"))?;
    let mut vars = vec![(OsStr::new("LOCALAPPDATA"), local.as_os_str())];
    vars.extend(
        env.iter()
            .map(|(key, value)| (OsStr::new(key), OsStr::new(value))),
    );
    vars.sort_by_key(|(key, _)| key.to_ascii_uppercase());
    // `KEY=value` strings, sorted by name and each NUL-terminated, then an
    // empty one.
    let mut block = Vec::new();
    for (key, value) in vars {
        block.extend(key.encode_wide());
        block.push(u16::from(b'='));
        block.extend(value.encode_wide());
        block.push(0);
    }
    block.push(0);
    let mut system = vec![0u16; 260];
    // SAFETY: the buffer's length is passed; the result is its used length.
    let used = unsafe { GetSystemDirectoryW(system.as_mut_ptr(), system.len() as u32) } as usize;
    if used == 0 || used >= system.len() {
        return Err(at("GetSystemDirectoryW")(io::Error::last_os_error()));
    }
    system.truncate(used);
    system.push(0);
    let program = wide(executable);
    let mut info = PROCESS_INFORMATION::default();
    // SAFETY: every pointer is valid for the call; `line` is mutable as
    // CreateProcessW requires; handles are inherited only from the list.
    check(unsafe {
        windows_sys::Win32::System::Threading::CreateProcessW(
            program.as_ptr(),
            line.as_mut_ptr(),
            null(),
            null(),
            1,
            EXTENDED_STARTUPINFO_PRESENT
                | CREATE_NO_WINDOW
                | CREATE_SUSPENDED
                | CREATE_UNICODE_ENVIRONMENT,
            block.as_ptr().cast(),
            system.as_ptr(),
            &startup.StartupInfo,
            &mut info,
        )
    })
    .map_err(at("CreateProcessW"))?;
    // SAFETY: CreateProcessW returned these handles to us.
    let thread = unsafe { OwnedHandle::from_raw_handle(info.hThread) };
    let mut process = Contained {
        process: info.hProcess,
        pid: info.dwProcessId,
    };
    // The worker owns its ends now.
    drop((worker_stdin, worker_stdout, attributes));
    let started = crate::os_sandbox::confine_process(process.process)
        .map_err(at("worker job"))
        .and_then(|job| {
            // SAFETY: the primary thread of the process we just created.
            if unsafe { ResumeThread(raw(&thread)) } == u32::MAX {
                Err(at("ResumeThread")(io::Error::last_os_error()))
            } else {
                Ok(job)
            }
        });
    match started {
        Ok(job) => Ok((process, stdin, stdout, job)),
        Err(error) => {
            process.terminate();
            process.wait_exit();
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::spawn;
    use crate::{Confinement, confine_worker};
    use std::io::Read;
    use std::process::Command;

    const PROBE: &str = "NANATYPE_CONTAINER_PROBE";

    /// In the contained child: report each attempt as `name=allowed|denied`.
    fn probe(dir: &str, port: &str) {
        let attempts = [
            (
                "read_prepared_file",
                std::fs::read(format!("{dir}\\secret.txt")).is_ok(),
            ),
            (
                "write_file",
                std::fs::write(format!("{dir}\\written.txt"), b"x").is_ok(),
            ),
            ("list_dir", std::fs::read_dir(dir).is_ok()),
            (
                "connect",
                std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok(),
            ),
            (
                "spawn",
                Command::new("cmd.exe")
                    .arg("/c")
                    .arg("exit")
                    .status()
                    .is_ok(),
            ),
        ];
        println!("confinement={:?}", confine_worker().unwrap());
        for (name, allowed) in attempts {
            println!("{name}={}", if allowed { "allowed" } else { "denied" });
        }
    }

    #[test]
    fn contained_workers_cannot_touch_files_network_or_processes() {
        if let Ok(probe_args) = std::env::var(PROBE) {
            let (dir, port) = probe_args.split_once('|').unwrap();
            probe(dir, port);
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("secret.txt"), b"host data").unwrap();
        // Something to connect to, so a failure means blocked, not refused.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port().to_string();
        let value = format!("{}|{port}", dir.path().display());
        let (mut child, input, mut output, _job) = spawn(
            &std::env::current_exe().unwrap(),
            &[
                "--exact",
                "app_container::tests::contained_workers_cannot_touch_files_network_or_processes",
                "--nocapture",
                "--test-threads=1",
            ],
            &[(PROBE, &value)],
        )
        .unwrap();
        drop(input);
        let mut stdout = String::new();
        output.read_to_string(&mut stdout).unwrap();
        child.wait_exit();
        assert!(
            stdout.contains(&format!("confinement={:?}", Confinement::AppContainer)),
            "{stdout}"
        );
        for name in [
            "read_prepared_file",
            "write_file",
            "list_dir",
            "connect",
            "spawn",
        ] {
            assert!(
                stdout.contains(&format!("{name}=denied")),
                "{name} was not denied:\n{stdout}"
            );
        }
        assert!(!dir.path().join("written.txt").exists());
    }
}
