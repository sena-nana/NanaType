//! System-wide shortcuts.
//!
//! Windows: one worker thread owns every `RegisterHotKey` registration and
//! blocks in `GetMessageW`; rebinding wakes it with a thread message, so
//! nothing polls. The summon key raises the input window from that thread,
//! because the foreground right granted by `WM_HOTKEY` belongs to it.
//!
//! macOS: the same worker owns `RegisterEventHotKey` and sleeps in
//! `CFRunLoop`. Carbon delivers the key to the application event target,
//! which the UI run loop already pumps, so the handler only posts the
//! action. Raising the input window happens later on the UI thread:
//! AppKit will not take it from the hotkey thread.

use std::collections::HashMap;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};

use nanatype_settings::Hotkey;

use crate::msg::GlobalAction;

#[cfg(target_os = "macos")]
#[path = "hotkeys_macos.rs"]
mod mac;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyState {
    Registered,
    /// Another program owns this chord.
    Conflict,
    /// The key cannot be registered (unknown key name).
    Invalid,
    Unsupported,
}

pub type Bindings = Vec<(GlobalAction, Hotkey)>;

struct Shared {
    bindings: Bindings,
    suspended: bool,
    status: HashMap<GlobalAction, HotkeyState>,
}

pub struct Hotkeys {
    shared: Arc<Mutex<Shared>>,
    input_hwnd: Arc<AtomicIsize>,
    /// The quick input bar, raised by the summon key instead of the input
    /// window while it is set (summon presence).
    quick_hwnd: Arc<AtomicIsize>,
    /// Captured before the hotkey thread restores the native window.
    summon_restore: Arc<std::sync::atomic::AtomicBool>,
    #[cfg(windows)]
    thread_id: Arc<std::sync::atomic::AtomicU32>,
    #[cfg(target_os = "macos")]
    mac: Arc<mac::State>,
}

impl Hotkeys {
    /// `hit` runs on the hotkey thread; `status` after each re-registration.
    pub fn start(
        hit: impl Fn(GlobalAction) + Send + 'static,
        status: impl Fn(HashMap<GlobalAction, HotkeyState>) + Send + 'static,
    ) -> Self {
        let shared = Arc::new(Mutex::new(Shared {
            bindings: Vec::new(),
            suspended: false,
            status: HashMap::new(),
        }));
        let input_hwnd = Arc::new(AtomicIsize::new(0));
        let quick_hwnd = Arc::new(AtomicIsize::new(0));
        let summon_restore = Arc::new(std::sync::atomic::AtomicBool::new(false));
        #[cfg(windows)]
        {
            let thread_id = Arc::new(std::sync::atomic::AtomicU32::new(0));
            let (s, h, t) = (shared.clone(), input_hwnd.clone(), thread_id.clone());
            let (q, restore) = (quick_hwnd.clone(), summon_restore.clone());
            std::thread::Builder::new()
                .name("nanatype-hotkeys".into())
                .spawn(move || win::run(s, [h, q], t, restore, hit, status))
                .expect("spawn hotkey thread");
            Self {
                shared,
                input_hwnd,
                quick_hwnd,
                summon_restore,
                thread_id,
            }
        }
        #[cfg(target_os = "macos")]
        {
            let state = mac::State::new(shared.clone(), hit, status);
            let thread_state = Arc::clone(&state);
            std::thread::Builder::new()
                .name("nanatype-hotkeys".into())
                .spawn(move || mac::run(thread_state))
                .expect("spawn hotkey thread");
            Self {
                shared,
                input_hwnd,
                quick_hwnd,
                mac: state,
                summon_restore,
            }
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = hit;
            let _ = status;
            Self {
                shared,
                input_hwnd,
                quick_hwnd,
                summon_restore,
            }
        }
    }

    pub fn set_input_window(&self, hwnd: isize) {
        self.input_hwnd.store(hwnd, Ordering::Release);
    }

    pub fn input_window(&self) -> isize {
        self.input_hwnd.load(Ordering::Acquire)
    }

    /// `0` sends the summon key back to the input window.
    pub fn set_quick_window(&self, hwnd: isize) {
        self.quick_hwnd.store(hwnd, Ordering::Release);
    }

    pub fn quick_window(&self) -> isize {
        self.quick_hwnd.load(Ordering::Acquire)
    }

    pub(crate) fn take_summon_restore(&self) -> bool {
        self.summon_restore.swap(false, Ordering::AcqRel)
    }

    /// Replace all registrations. `suspended` unregisters everything (while
    /// the user records a new chord).
    pub fn apply(&self, bindings: Bindings, suspended: bool) {
        {
            let mut s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            if s.bindings == bindings && s.suspended == suspended {
                return;
            }
            // Linux has no registration thread yet: every binding reports so.
            #[cfg(not(any(windows, target_os = "macos")))]
            {
                s.status = bindings
                    .iter()
                    .map(|(a, _)| (*a, HotkeyState::Unsupported))
                    .collect();
            }
            s.bindings = bindings;
            s.suspended = suspended;
        }
        #[cfg(windows)]
        win::poke(self.thread_id.load(Ordering::Acquire));
        #[cfg(target_os = "macos")]
        mac::poke(&self.mac);
    }

    pub fn status(&self) -> HashMap<GlobalAction, HotkeyState> {
        self.shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .status
            .clone()
    }
}

/// Bring the input window forward. Used by the hotkey thread (summon) and as
/// a best effort from the UI thread.
#[cfg(windows)]
pub fn activate(hwnd: isize) -> bool {
    win::activate(hwnd)
}

/// Bring this process forward and order the input window front.
/// `ns_view` is the content view. Must run on the main thread.
#[cfg(target_os = "macos")]
pub fn activate(ns_window: isize) -> bool {
    mac::activate(ns_window)
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn activate(_hwnd: isize) -> bool {
    false
}

#[cfg(windows)]
mod win {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};

    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        HOT_KEY_MODIFIERS, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
        MOD_SHIFT, MOD_WIN, RegisterHotKey, UnregisterHotKey, VK_MENU, keybd_event,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        ASFW_ANY, AllowSetForegroundWindow, BringWindowToTop, GetForegroundWindow, GetMessageW,
        GetWindowThreadProcessId, IsIconic, IsWindow, MSG, PM_NOREMOVE, PeekMessageW,
        PostThreadMessageW, SW_RESTORE, SW_SHOW, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
        SetForegroundWindow, SetWindowPos, ShowWindow, WM_APP, WM_HOTKEY,
    };

    use super::{HotkeyState, Shared};
    use crate::msg::GlobalAction;
    use nanatype_settings::Hotkey;

    const WM_REBIND: u32 = WM_APP + 1;

    pub fn poke(thread: u32) {
        if thread != 0 {
            unsafe {
                let _ = PostThreadMessageW(thread, WM_REBIND, WPARAM(0), LPARAM(0));
            }
        }
    }

    pub fn virtual_key(code: &str) -> Option<u32> {
        if let Some(letter) = code.strip_prefix("Key").filter(|v| v.len() == 1) {
            let b = letter.as_bytes()[0];
            return b.is_ascii_uppercase().then_some(u32::from(b));
        }
        if let Some(digit) = code.strip_prefix("Digit").filter(|v| v.len() == 1) {
            let b = digit.as_bytes()[0];
            return b.is_ascii_digit().then_some(u32::from(b));
        }
        if let Some(n) = code
            .strip_prefix('F')
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|v| (1..=24).contains(v))
        {
            return Some(0x70 + n - 1);
        }
        if let Some(n) = code
            .strip_prefix("Numpad")
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|v| *v <= 9)
        {
            return Some(0x60 + n);
        }
        Some(match code {
            "Space" => 0x20,
            "Tab" => 0x09,
            "Escape" => 0x1B,
            "Enter" => 0x0D,
            "Backspace" => 0x08,
            "Insert" => 0x2D,
            "Delete" => 0x2E,
            "Home" => 0x24,
            "End" => 0x23,
            "PageUp" => 0x21,
            "PageDown" => 0x22,
            "ArrowLeft" => 0x25,
            "ArrowUp" => 0x26,
            "ArrowRight" => 0x27,
            "ArrowDown" => 0x28,
            "Pause" => 0x13,
            "ScrollLock" => 0x91,
            "Minus" => 0xBD,
            "Equal" => 0xBB,
            "Comma" => 0xBC,
            "Period" => 0xBE,
            "Slash" => 0xBF,
            "Semicolon" => 0xBA,
            "Quote" => 0xDE,
            "BracketLeft" => 0xDB,
            "BracketRight" => 0xDD,
            "Backslash" => 0xDC,
            "Backquote" => 0xC0,
            _ => return None,
        })
    }

    fn modifiers(h: &Hotkey) -> HOT_KEY_MODIFIERS {
        let mut m = MOD_NOREPEAT;
        if h.ctrl {
            m |= MOD_CONTROL;
        }
        if h.shift {
            m |= MOD_SHIFT;
        }
        if h.alt {
            m |= MOD_ALT;
        }
        if h.meta {
            m |= MOD_WIN;
        }
        m
    }

    pub fn run(
        shared: Arc<Mutex<Shared>>,
        [input, quick]: [Arc<AtomicIsize>; 2],
        thread_id: Arc<AtomicU32>,
        restore: Arc<std::sync::atomic::AtomicBool>,
        hit: impl Fn(GlobalAction),
        status: impl Fn(HashMap<GlobalAction, HotkeyState>),
    ) {
        // A thread receives WM_HOTKEY only once it owns a message queue.
        let mut warm = MSG::default();
        unsafe {
            let _ = PeekMessageW(&mut warm, None, 0, 0, PM_NOREMOVE);
        }
        thread_id.store(unsafe { GetCurrentThreadId() }, Ordering::Release);
        let mut registered: Vec<(i32, GlobalAction)> = Vec::new();
        let rebind = |registered: &mut Vec<(i32, GlobalAction)>| {
            for (id, _) in registered.drain(..) {
                unsafe {
                    let _ = UnregisterHotKey(None, id);
                }
            }
            let (bindings, suspended) = {
                let s = shared.lock().unwrap_or_else(|e| e.into_inner());
                (s.bindings.clone(), s.suspended)
            };
            let mut report = HashMap::new();
            if !suspended {
                for (i, (action, key)) in bindings.iter().enumerate() {
                    let id = i as i32 + 1;
                    let state = match virtual_key(&key.key) {
                        None => HotkeyState::Invalid,
                        Some(vk) => {
                            if unsafe { RegisterHotKey(None, id, modifiers(key), vk) }.is_ok() {
                                registered.push((id, *action));
                                HotkeyState::Registered
                            } else {
                                HotkeyState::Conflict
                            }
                        }
                    };
                    report.insert(*action, state);
                }
            }
            shared.lock().unwrap_or_else(|e| e.into_inner()).status = report.clone();
            status(report);
        };
        rebind(&mut registered);
        let mut msg = MSG::default();
        loop {
            let got = unsafe { GetMessageW(&mut msg, None, 0, 0) };
            if got.0 <= 0 {
                break;
            }
            match msg.message {
                WM_HOTKEY => {
                    let id = msg.wParam.0 as i32;
                    if let Some((_, action)) = registered.iter().find(|(i, _)| *i == id) {
                        if action.raises_input() {
                            let mut hwnd = input.load(Ordering::Acquire);
                            if *action == GlobalAction::Summon {
                                hwnd = match quick.load(Ordering::Acquire) {
                                    0 => hwnd,
                                    quick => quick,
                                };
                                restore.store(
                                    hwnd != 0
                                        && unsafe {
                                            IsIconic(HWND(hwnd as *mut core::ffi::c_void)).as_bool()
                                        },
                                    Ordering::Release,
                                );
                            }
                            activate(hwnd);
                        }
                        hit(*action);
                    }
                }
                WM_REBIND => rebind(&mut registered),
                _ => {}
            }
        }
        for (id, _) in registered {
            unsafe {
                let _ = UnregisterHotKey(None, id);
            }
        }
    }

    pub fn activate(hwnd: isize) -> bool {
        if hwnd == 0 {
            return false;
        }
        let hwnd = HWND(hwnd as *mut core::ffi::c_void);
        unsafe {
            if !IsWindow(Some(hwnd)).as_bool() {
                return false;
            }
            // Use the foreground right from WM_HOTKEY before anything else
            // (an injected menu key can lock the foreground again).
            let _ = AllowSetForegroundWindow(ASFW_ANY);
            let _ = ShowWindow(
                hwnd,
                if IsIconic(hwnd).as_bool() {
                    SW_RESTORE
                } else {
                    SW_SHOW
                },
            );
            let _ = BringWindowToTop(hwnd);
            if SetForegroundWindow(hwnd).as_bool() {
                return true;
            }
            keybd_event(VK_MENU.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
            keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_KEYUP, 0);
            let foreground = GetForegroundWindow();
            let fg_thread = GetWindowThreadProcessId(foreground, None);
            let ours = GetCurrentThreadId();
            let attached = fg_thread != 0
                && fg_thread != ours
                && AttachThreadInput(ours, fg_thread, true).as_bool();
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
            );
            let raised = SetForegroundWindow(hwnd).as_bool();
            if attached {
                let _ = AttachThreadInput(ours, fg_thread, false);
            }
            raised
        }
    }
}
