use crate::{Error, Result};
use crossbeam_channel::Sender;
use std::ffi::{CStr, CString, c_char, c_int, c_void};

pub struct NativeEvent {
    pub id: u64,
    pub kind: i32,
    pub text: String,
}
struct Delivery {
    id: u64,
    tx: Sender<NativeEvent>,
    overflow: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

unsafe extern "C" fn deliver(context: *mut c_void, kind: c_int, text: *const c_char) {
    // Native code keeps Delivery alive until every retained callback is released.
    let delivery = unsafe { &*context.cast::<Delivery>() };
    if !text.is_null() && unsafe { CStr::from_ptr(text) }.to_bytes().len() > 65536 {
        delivery
            .overflow
            .store(true, std::sync::atomic::Ordering::Release);
        return;
    }
    let text = if text.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned()
    };
    if delivery
        .tx
        .try_send(NativeEvent {
            id: delivery.id,
            kind,
            text,
        })
        .is_err()
    {
        delivery
            .overflow
            .store(true, std::sync::atomic::Ordering::Release);
    }
}
unsafe extern "C" fn release(context: *mut c_void) {
    drop(unsafe { Box::from_raw(context.cast::<Delivery>()) });
}

#[cfg(any(target_os = "macos", windows))]
unsafe extern "C" {
    fn nt_microphone_permission() -> c_int;
    fn nt_system_permission() -> c_int;
    fn nt_system_languages(buffer: *mut c_char, size: usize) -> usize;
    fn nt_system_available(language: *const c_char) -> c_int;
    fn nt_system_create(
        language: *const c_char,
        context: *mut c_void,
        callback: unsafe extern "C" fn(*mut c_void, c_int, *const c_char),
        release: unsafe extern "C" fn(*mut c_void),
    ) -> *mut c_void;
    fn nt_system_append(handle: *mut c_void, samples: *const f32, length: usize) -> c_int;
    fn nt_system_end(handle: *mut c_void);
    fn nt_system_poll(handle: *mut c_void);
    fn nt_system_close(handle: *mut c_void);
}

pub fn microphone_permission() -> Result<()> {
    #[cfg(any(target_os = "macos", windows))]
    if unsafe { nt_microphone_permission() } == 0 {
        return Err(Error::key("speech-permission"));
    }
    Ok(())
}
pub fn system_permission() -> Result<()> {
    #[cfg(any(target_os = "macos", windows))]
    if unsafe { nt_system_permission() } != 0 {
        return Ok(());
    }
    Err(Error::key("speech-permission"))
}
pub fn languages() -> Vec<String> {
    #[cfg(any(target_os = "macos", windows))]
    {
        let size = unsafe { nt_system_languages(std::ptr::null_mut(), 0) };
        let mut bytes = vec![0u8; size + 1];
        unsafe {
            nt_system_languages(bytes.as_mut_ptr().cast(), bytes.len());
        }
        let mut values: Vec<String> = String::from_utf8_lossy(&bytes[..size])
            .lines()
            .map(|s| s.replace('_', "-"))
            .collect();
        values.sort();
        values.dedup();
        values
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    Vec::new()
}
pub fn available(language: &str) -> bool {
    let Ok(language) = CString::new(language) else {
        return false;
    };
    #[cfg(any(target_os = "macos", windows))]
    return unsafe { nt_system_available(language.as_ptr()) != 0 };
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = language;
        false
    }
}

pub struct Native(*mut c_void);
impl Native {
    pub fn new(
        language: &str,
        id: u64,
        tx: Sender<NativeEvent>,
        overflow: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<Self> {
        let language =
            CString::new(language).map_err(|_| Error::key("speech-language-unavailable"))?;
        let delivery = Box::into_raw(Box::new(Delivery { id, tx, overflow })).cast();
        #[cfg(any(target_os = "macos", windows))]
        let handle = unsafe { nt_system_create(language.as_ptr(), delivery, deliver, release) };
        #[cfg(not(any(target_os = "macos", windows)))]
        let handle = {
            let _ = language;
            unsafe { release(delivery) };
            std::ptr::null_mut()
        };
        if handle.is_null() {
            Err(Error::key("speech-system-unavailable"))
        } else {
            Ok(Self(handle))
        }
    }
    pub fn append(&self, samples: &[f32]) -> Result<()> {
        #[cfg(any(target_os = "macos", windows))]
        if unsafe { nt_system_append(self.0, samples.as_ptr(), samples.len()) } == 0 {
            return Err(Error::key("speech-overloaded"));
        }
        let _ = samples;
        Ok(())
    }
    pub fn end(&self) {
        #[cfg(any(target_os = "macos", windows))]
        unsafe {
            nt_system_end(self.0);
        }
    }
    pub fn poll(&self) {
        #[cfg(any(target_os = "macos", windows))]
        unsafe {
            nt_system_poll(self.0);
        }
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        #[cfg(any(target_os = "macos", windows))]
        unsafe {
            nt_system_close(self.0);
        }
    }
}
