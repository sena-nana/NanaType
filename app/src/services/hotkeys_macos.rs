//! Carbon hot keys on a worker thread.
//!
//! Registration lives on this thread, woken with `CFRunLoopWakeUp`, the same
//! shape as the Windows message loop. The key itself is delivered to
//! `GetApplicationEventTarget`, which the UI run loop already pumps, so the
//! handler only reports the action. AppKit activation stays on the main thread.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Arc, Mutex};

use nanatype_settings::Hotkey;

use super::{HotkeyState, Shared};
use crate::msg::GlobalAction;

const EVENT_CLASS_KEYBOARD: u32 = u32::from_be_bytes(*b"keyb");
const EVENT_HOT_KEY_PRESSED: u32 = 5;
const PARAM_DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");
const TYPE_EVENT_HOT_KEY_ID: u32 = u32::from_be_bytes(*b"hkid");
const SIGNATURE: u32 = u32::from_be_bytes(*b"NTYP");
/// `eventNotHandledErr`: let the next handler see the event.
const EVENT_NOT_HANDLED: OSStatus = -9874;

const CMD: u32 = 1 << 8;
const SHIFT: u32 = 1 << 9;
const OPTION: u32 = 1 << 11;
const CONTROL: u32 = 1 << 12;

type OSStatus = i32;
type EventRef = *mut c_void;
type EventHandlerRef = *mut c_void;
type EventHandlerCallRef = *mut c_void;
type EventTargetRef = *mut c_void;
type EventHotKeyRef = *mut c_void;

#[repr(C, packed(2))]
#[derive(Copy, Clone)]
struct EventHotKeyId {
    signature: u32,
    id: u32,
}

#[repr(C, packed(2))]
#[derive(Copy, Clone)]
struct EventTypeSpec {
    event_class: u32,
    event_kind: u32,
}

#[repr(C)]
struct SourceContext {
    version: isize,
    info: *mut c_void,
    retain: Option<unsafe extern "C" fn(*const c_void) -> *const c_void>,
    release: Option<unsafe extern "C" fn(*const c_void)>,
    copy_description: Option<unsafe extern "C" fn(*const c_void) -> *const c_void>,
    equal: Option<unsafe extern "C" fn(*const c_void, *const c_void) -> u8>,
    hash: Option<unsafe extern "C" fn(*const c_void) -> usize>,
    schedule: Option<unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void)>,
    cancel: Option<unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void)>,
    perform: Option<unsafe extern "C" fn(*mut c_void)>,
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn InstallEventHandler(
        target: EventTargetRef,
        handler: Option<
            unsafe extern "C" fn(EventHandlerCallRef, EventRef, *mut c_void) -> OSStatus,
        >,
        num_types: u64,
        types: *const EventTypeSpec,
        user_data: *mut c_void,
        out_ref: *mut EventHandlerRef,
    ) -> OSStatus;
    fn RemoveEventHandler(handler: EventHandlerRef) -> OSStatus;
    fn GetApplicationEventTarget() -> EventTargetRef;
    fn RegisterEventHotKey(
        key_code: u32,
        modifiers: u32,
        hot_key_id: EventHotKeyId,
        target: EventTargetRef,
        options: u32,
        out_ref: *mut EventHotKeyRef,
    ) -> OSStatus;
    fn UnregisterEventHotKey(hot_key: EventHotKeyRef) -> OSStatus;
    fn GetEventParameter(
        event: EventRef,
        name: u32,
        desired_type: u32,
        actual_type: *mut u32,
        buffer_size: u64,
        actual_size: *mut u64,
        data: *mut c_void,
    ) -> OSStatus;

}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopAddSource(run_loop: *mut c_void, source: *mut c_void, mode: *const c_void);
    fn CFRunLoopRun();
    fn CFRunLoopWakeUp(run_loop: *mut c_void);
    fn CFRunLoopSourceCreate(
        allocator: *const c_void,
        order: isize,
        context: *mut SourceContext,
    ) -> *mut c_void;
    fn CFRunLoopSourceSignal(source: *mut c_void);
    static kCFRunLoopDefaultMode: *const c_void;
}

type HitFn = Box<dyn Fn(GlobalAction) + Send>;
type StatusFn = Box<dyn Fn(HashMap<GlobalAction, HotkeyState>) + Send>;

pub struct State {
    shared: Arc<Mutex<Shared>>,
    run_loop: AtomicPtr<c_void>,
    source: AtomicPtr<c_void>,
    installed: AtomicBool,
    /// Hot-key ref, the id passed to Carbon, and the action it fires.
    registered: Mutex<Vec<(usize, u32, GlobalAction)>>,
    hit: Mutex<HitFn>,
    status: Mutex<StatusFn>,
}

impl State {
    pub fn new(
        shared: Arc<Mutex<Shared>>,
        hit: impl Fn(GlobalAction) + Send + 'static,
        status: impl Fn(HashMap<GlobalAction, HotkeyState>) + Send + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            shared,
            run_loop: AtomicPtr::new(std::ptr::null_mut()),
            source: AtomicPtr::new(std::ptr::null_mut()),
            installed: AtomicBool::new(false),
            registered: Mutex::new(Vec::new()),
            hit: Mutex::new(Box::new(hit)),
            status: Mutex::new(Box::new(status)),
        })
    }

    fn rebind(&self) {
        let mut registered = self.registered.lock().unwrap_or_else(|e| e.into_inner());
        for (ptr, _, _) in registered.drain(..) {
            unsafe {
                let _ = UnregisterEventHotKey(ptr as EventHotKeyRef);
            }
        }
        let (bindings, suspended) = {
            let shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            (shared.bindings.clone(), shared.suspended)
        };
        let mut report = HashMap::new();
        if !suspended && self.installed.load(Ordering::Acquire) {
            let target = unsafe { GetApplicationEventTarget() };
            for (i, (action, key)) in bindings.iter().enumerate() {
                let id = (i as u32) + 1;
                let state = match virtual_key(&key.key) {
                    None => HotkeyState::Invalid,
                    Some(code) => {
                        let mut hot_key = std::ptr::null_mut();
                        let ok = unsafe {
                            RegisterEventHotKey(
                                code,
                                modifiers(key),
                                EventHotKeyId {
                                    signature: SIGNATURE,
                                    id,
                                },
                                target,
                                0,
                                &mut hot_key,
                            )
                        };
                        if ok == 0 && !hot_key.is_null() {
                            registered.push((hot_key as usize, id, *action));
                            HotkeyState::Registered
                        } else {
                            HotkeyState::Conflict
                        }
                    }
                };
                report.insert(*action, state);
            }
        } else if !suspended {
            for (action, _) in &bindings {
                report.insert(*action, HotkeyState::Unsupported);
            }
        }
        drop(registered);
        self.shared.lock().unwrap_or_else(|e| e.into_inner()).status = report.clone();
        (self.status.lock().unwrap_or_else(|e| e.into_inner()))(report);
    }
}

pub fn poke(state: &State) {
    let source = state.source.load(Ordering::Acquire);
    let run_loop = state.run_loop.load(Ordering::Acquire);
    if source.is_null() || run_loop.is_null() {
        return;
    }
    unsafe {
        CFRunLoopSourceSignal(source);
        CFRunLoopWakeUp(run_loop);
    }
}

pub fn run(state: Arc<State>) {
    let user = Arc::as_ptr(&state) as *mut c_void;
    let mut handler = std::ptr::null_mut();
    let types = [EventTypeSpec {
        event_class: EVENT_CLASS_KEYBOARD,
        event_kind: EVENT_HOT_KEY_PRESSED,
    }];
    let installed = unsafe {
        InstallEventHandler(
            GetApplicationEventTarget(),
            Some(on_hotkey),
            1,
            types.as_ptr(),
            user,
            &mut handler,
        )
    };
    state.installed.store(installed == 0, Ordering::Release);
    let mut context = SourceContext {
        version: 0,
        info: user,
        retain: None,
        release: None,
        copy_description: None,
        equal: None,
        hash: None,
        schedule: None,
        cancel: None,
        perform: Some(on_perform),
    };
    unsafe {
        let source = CFRunLoopSourceCreate(std::ptr::null(), 0, &mut context);
        let run_loop = CFRunLoopGetCurrent();
        CFRunLoopAddSource(run_loop, source, kCFRunLoopDefaultMode);
        // Published before the first bind, so a poke that lands during it
        // is also run, and a poke that landed earlier is covered by this bind.
        state.source.store(source, Ordering::Release);
        state.run_loop.store(run_loop, Ordering::Release);
        state.rebind();
        CFRunLoopRun();
        if !handler.is_null() {
            let _ = RemoveEventHandler(handler);
        }
    }
}

unsafe extern "C" fn on_perform(info: *mut c_void) {
    let state = unsafe { &*info.cast::<State>() };
    state.rebind();
}

unsafe extern "C" fn on_hotkey(
    _call: EventHandlerCallRef,
    event: EventRef,
    user: *mut c_void,
) -> OSStatus {
    let mut hot_key = EventHotKeyId {
        signature: 0,
        id: 0,
    };
    let read = unsafe {
        GetEventParameter(
            event,
            PARAM_DIRECT_OBJECT,
            TYPE_EVENT_HOT_KEY_ID,
            std::ptr::null_mut(),
            std::mem::size_of::<EventHotKeyId>() as u64,
            std::ptr::null_mut(),
            std::ptr::from_mut(&mut hot_key).cast(),
        )
    };
    let signature = unsafe { std::ptr::read_unaligned(std::ptr::addr_of!(hot_key.signature)) };
    // Hot keys registered by anything else in the process pass through.
    if read != 0 || signature != SIGNATURE {
        return EVENT_NOT_HANDLED;
    }
    let id = unsafe { std::ptr::read_unaligned(std::ptr::addr_of!(hot_key.id)) };
    let state = unsafe { &*user.cast::<State>() };
    let action = state
        .registered
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(_, registered_id, _)| *registered_id == id)
        .map(|(_, _, action)| *action);
    match action {
        Some(action) => {
            (state.hit.lock().unwrap_or_else(|e| e.into_inner()))(action);
            0
        }
        None => EVENT_NOT_HANDLED,
    }
}

/// macOS virtual key code for a W3C `KeyboardEvent.code`, when Carbon has one.
pub fn virtual_key(code: &str) -> Option<u32> {
    if let Some(letter) = code.strip_prefix("Key").filter(|v| v.len() == 1) {
        return match letter.as_bytes()[0] {
            b'A' => Some(0x00),
            b'S' => Some(0x01),
            b'D' => Some(0x02),
            b'F' => Some(0x03),
            b'H' => Some(0x04),
            b'G' => Some(0x05),
            b'Z' => Some(0x06),
            b'X' => Some(0x07),
            b'C' => Some(0x08),
            b'V' => Some(0x09),
            b'B' => Some(0x0B),
            b'Q' => Some(0x0C),
            b'W' => Some(0x0D),
            b'E' => Some(0x0E),
            b'R' => Some(0x0F),
            b'Y' => Some(0x10),
            b'T' => Some(0x11),
            b'O' => Some(0x1F),
            b'U' => Some(0x20),
            b'I' => Some(0x22),
            b'P' => Some(0x23),
            b'L' => Some(0x25),
            b'J' => Some(0x26),
            b'K' => Some(0x28),
            b'N' => Some(0x2D),
            b'M' => Some(0x2E),
            _ => None,
        };
    }
    if let Some(digit) = code.strip_prefix("Digit").filter(|v| v.len() == 1) {
        return match digit.as_bytes()[0] {
            b'1' => Some(0x12),
            b'2' => Some(0x13),
            b'3' => Some(0x14),
            b'4' => Some(0x15),
            b'5' => Some(0x17),
            b'6' => Some(0x16),
            b'7' => Some(0x1A),
            b'8' => Some(0x1C),
            b'9' => Some(0x19),
            b'0' => Some(0x1D),
            _ => None,
        };
    }
    if let Some(n) = code
        .strip_prefix("Numpad")
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|v| *v <= 9)
    {
        // Keypad 0 is 0x52; 1–7 follow, 8 is 0x5B, 9 is 0x5C.
        return Some(match n {
            0..=7 => 0x52 + n,
            8 => 0x5B,
            _ => 0x5C,
        });
    }
    Some(match code {
        "Space" => 0x31,
        "Tab" => 0x30,
        "Escape" => 0x35,
        "Enter" => 0x24,
        "Backspace" => 0x33,
        "Insert" => 0x72,
        "Delete" => 0x75,
        "Home" => 0x73,
        "End" => 0x77,
        "PageUp" => 0x74,
        "PageDown" => 0x79,
        "ArrowLeft" => 0x7B,
        "ArrowRight" => 0x7C,
        "ArrowDown" => 0x7D,
        "ArrowUp" => 0x7E,
        "Minus" => 0x1B,
        "Equal" => 0x18,
        "Comma" => 0x2B,
        "Period" => 0x2F,
        "Slash" => 0x2C,
        "Semicolon" => 0x29,
        "Quote" => 0x27,
        "BracketLeft" => 0x21,
        "BracketRight" => 0x1E,
        "Backslash" => 0x2A,
        "Backquote" => 0x32,
        "F1" => 0x7A,
        "F2" => 0x78,
        "F3" => 0x63,
        "F4" => 0x76,
        "F5" => 0x60,
        "F6" => 0x61,
        "F7" => 0x62,
        "F8" => 0x64,
        "F9" => 0x65,
        "F10" => 0x6D,
        "F11" => 0x67,
        "F12" => 0x6F,
        "F13" => 0x69,
        "F14" => 0x6B,
        "F15" => 0x71,
        "F16" => 0x6A,
        "F17" => 0x40,
        "F18" => 0x4F,
        "F19" => 0x50,
        "F20" => 0x5A,
        _ => return None,
    })
}

fn modifiers(key: &Hotkey) -> u32 {
    let mut mods = 0;
    if key.ctrl {
        mods |= CONTROL;
    }
    if key.shift {
        mods |= SHIFT;
    }
    if key.alt {
        mods |= OPTION;
    }
    if key.meta {
        mods |= CMD;
    }
    mods
}

/// Activate this process and order the input window front.
/// `ns_view` is the content view from the raw window handle. Main thread only.
pub fn activate(ns_view: isize) -> bool {
    use objc2::rc::Retained;
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSView};

    let Some(marker) = objc2::MainThreadMarker::new() else {
        return false;
    };
    let _ = marker;
    let running = NSRunningApplication::currentApplication();
    #[allow(deprecated)]
    let activated =
        running.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);
    if ns_view == 0 {
        return activated;
    }
    // The view (and its window) stay alive for the input window's lifetime.
    // This retain is dropped at the end of the call.
    let Some(view) = (unsafe { Retained::<NSView>::retain(ns_view as *mut NSView) }) else {
        return activated;
    };
    let Some(window) = view.window() else {
        return activated;
    };
    window.makeKeyAndOrderFront(None);
    true
}

#[cfg(test)]
mod tests {
    use super::virtual_key;

    #[test]
    fn letters_digits_and_function_keys_use_the_mac_table() {
        assert_eq!(virtual_key("F8"), Some(0x64));
        assert_eq!(virtual_key("F9"), Some(0x65));
        assert_eq!(virtual_key("F1"), Some(0x7A));
        assert_eq!(virtual_key("KeyA"), Some(0x00));
        assert_eq!(virtual_key("KeyN"), Some(0x2D));
        assert_eq!(virtual_key("Digit1"), Some(0x12));
        assert_eq!(virtual_key("Digit5"), Some(0x17));
        assert_eq!(virtual_key("Numpad8"), Some(0x5B));
        assert_eq!(virtual_key("F24"), None);
        assert_eq!(virtual_key("KeyÅ"), None);
    }

    #[test]
    fn a_rare_chord_registers_and_unregisters() {
        let code = virtual_key("F19").unwrap();
        let mut hot_key = std::ptr::null_mut();
        let status = unsafe {
            super::RegisterEventHotKey(
                code,
                super::CONTROL | super::SHIFT | super::OPTION,
                super::EventHotKeyId {
                    signature: super::SIGNATURE,
                    id: 99,
                },
                super::GetApplicationEventTarget(),
                0,
                &mut hot_key,
            )
        };
        assert_eq!(status, 0, "RegisterEventHotKey");
        assert!(!hot_key.is_null());
        assert_eq!(unsafe { super::UnregisterEventHotKey(hot_key) }, 0);
    }
}
