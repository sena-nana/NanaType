use serde::{Deserialize, Serialize};

/// A key chord. `key` is a W3C `KeyboardEvent.code` name (`F8`, `KeyN`, `Digit1`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hotkey {
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub alt: bool,
    /// Windows key / Command.
    #[serde(default, alias = "win")]
    pub meta: bool,
    pub key: String,
}

impl Hotkey {
    pub fn bare(key: impl Into<String>) -> Self {
        Self {
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
            key: key.into(),
        }
    }

    pub fn ctrl(key: impl Into<String>) -> Self {
        Self {
            ctrl: true,
            ..Self::bare(key)
        }
    }

    pub fn shift(key: impl Into<String>) -> Self {
        Self {
            shift: true,
            ..Self::bare(key)
        }
    }

    pub fn alt(key: impl Into<String>) -> Self {
        Self {
            alt: true,
            ..Self::bare(key)
        }
    }

    pub fn has_modifier(&self) -> bool {
        self.ctrl || self.shift || self.alt || self.meta
    }

    /// `Ctrl+Shift+F8`. `meta_label` is "Win" or "⌘".
    pub fn label(&self, meta_label: &str) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.meta {
            parts.push(meta_label);
        }
        let key = match self.key.as_str() {
            "ArrowUp" => "↑",
            "ArrowDown" => "↓",
            "ArrowLeft" => "←",
            "ArrowRight" => "→",
            "Escape" => "Esc",
            k => k
                .strip_prefix("Key")
                .or_else(|| k.strip_prefix("Digit"))
                .unwrap_or(k),
        };
        let mut out = parts.join("+");
        if !out.is_empty() {
            out.push('+');
        }
        out.push_str(key);
        out
    }

    /// Function keys and a few lock keys are fine alone: they type nothing.
    fn types_nothing(&self) -> bool {
        let k = self.key.as_str();
        (k.len() >= 2
            && k.starts_with('F')
            && k[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)))
            || matches!(k, "Pause" | "ScrollLock" | "Insert")
    }

    /// Enter, Ctrl+Enter and ⌘+Enter: the send key's choices, which no other
    /// action may take.
    pub fn is_send_chord(&self) -> bool {
        matches!(self.key.as_str(), "Enter" | "NumpadEnter")
            && !self.shift
            && !self.alt
            && !(self.ctrl && self.meta)
    }

    /// Chords every editor or the system already means something by:
    /// clipboard and undo, select all, closing and switching windows, and
    /// the input method switch.
    fn is_system_chord(&self) -> bool {
        let command = self.ctrl != self.meta;
        let k = self.key.as_str();
        (command && !self.alt && matches!(k, "KeyA" | "KeyC" | "KeyV" | "KeyX" | "KeyZ" | "KeyY"))
            || (self.alt && !self.ctrl && !self.meta && matches!(k, "F4" | "Tab" | "Space"))
            || (self.meta
                && !self.ctrl
                && !self.alt
                && matches!(k, "Tab" | "KeyQ" | "KeyW" | "KeyH" | "KeyM"))
            || (command && !self.alt && !self.shift && k == "Space")
    }

    /// Keys that move the caret, select or delete. Some modifier does each
    /// of these in some editor, so inside the input window they stay the
    /// editor's; system-wide only a chord with two of Ctrl, Alt and ⌘ is
    /// clear of them.
    fn edits_text(&self) -> bool {
        matches!(
            self.key.as_str(),
            "ArrowLeft" | "ArrowRight" | "Home" | "End" | "Backspace" | "Delete"
        )
    }

    /// Why this chord may not be a system-wide hotkey. A key that types
    /// needs Ctrl, Alt or ⌘ (Shift alone still types). `send` is never true
    /// for a hotkey today; it keeps the Enter rule in one place.
    pub fn refusal_global(&self, send: bool) -> Option<Refusal> {
        if self.key.is_empty() {
            return Some(Refusal::Types);
        }
        if self.is_send_chord() && !send {
            return Some(Refusal::SendOnly);
        }
        if !(self.ctrl || self.alt || self.meta || self.types_nothing()) {
            return Some(Refusal::Types);
        }
        let strong = [self.ctrl, self.alt, self.meta]
            .into_iter()
            .filter(|m| *m)
            .count();
        if self.is_system_chord() || (self.edits_text() && strong < 2) || self.key == "Tab" {
            return Some(Refusal::Reserved);
        }
        None
    }

    /// Why this chord may not be one of the input window's keys. `send` is
    /// whether it is for the send key, the only one that may take Enter or
    /// Ctrl+Enter.
    pub fn refusal_local(&self, send: bool) -> Option<Refusal> {
        if self.key.is_empty() {
            return Some(Refusal::Types);
        }
        if self.is_send_chord() && !send {
            return Some(Refusal::SendOnly);
        }
        if !self.is_safe_local() {
            return Some(Refusal::Types);
        }
        // Tab and Shift+Tab move focus: the keyboard's way around the window.
        let tab = self.key == "Tab" && !self.ctrl && !self.alt && !self.meta;
        if self.is_system_chord() || self.edits_text() || tab {
            return Some(Refusal::Reserved);
        }
        None
    }

    /// Function keys and media keys are fine alone; anything that types needs
    /// a modifier other than Shift, or it would swallow normal typing
    /// system-wide.
    pub fn is_safe_global(&self) -> bool {
        self.refusal_global(false).is_none()
    }

    /// Inside the input window a bare key is fine as long as it does not type
    /// text: Enter, Esc, arrows, paging and the function keys.
    pub fn is_safe_local(&self) -> bool {
        if self.key.is_empty() {
            return false;
        }
        self.has_modifier()
            || self.types_nothing()
            || matches!(
                self.key.as_str(),
                "Enter"
                    | "NumpadEnter"
                    | "Escape"
                    | "Tab"
                    | "ArrowUp"
                    | "ArrowDown"
                    | "ArrowLeft"
                    | "ArrowRight"
                    | "Home"
                    | "End"
                    | "PageUp"
                    | "PageDown"
                    | "Delete"
            )
    }
}

/// Why a chord cannot be bound to an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// It types text (or is no key at all).
    Types,
    /// The system or the editor already uses it.
    Reserved,
    /// Enter and Ctrl+Enter belong to the send key.
    SendOnly,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeySettings {
    /// Bring the input window forward.
    pub summon: Option<Hotkey>,
    /// Open the one-line quick input bar.
    pub quick_input: Option<Hotkey>,
    #[serde(rename = "continue")]
    pub continue_: Option<Hotkey>,
    pub skip: Option<Hotkey>,
    pub clear: Option<Hotkey>,
    pub toggle_mode: Option<Hotkey>,
    pub toggle_overlays: Option<Hotkey>,
    /// Bring the input window forward with live chat in view.
    pub show_chat: Option<Hotkey>,
    /// Bring the input window forward replying to the newest chat message.
    pub reply_latest: Option<Hotkey>,
    /// Bring the input window forward with the quick phrases open.
    pub quick_phrases: Option<Hotkey>,
    /// Lock or unlock the overlays (click-through).
    pub toggle_lock: Option<Hotkey>,
    /// Show the last sent line on the display again.
    pub replay: Option<Hotkey>,
    pub toggle_history: Option<Hotkey>,
    pub next_character: Option<Hotkey>,
    pub prev_character: Option<Hotkey>,
    /// Switch to the next profile in the list.
    pub next_profile: Option<Hotkey>,
    /// Switch to one profile, by its id. Global like every hotkey here (a
    /// profile never brings or takes these); one entry per saved profile.
    pub profiles: Vec<ProfileHotkey>,
    /// Keys handled by the input window itself, not registered system-wide.
    pub input: InputKeys,
}

/// The hotkey that switches to a saved profile.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileHotkey {
    /// The profile's file stem (profiles/<id>.json).
    pub id: String,
    /// Its name as last seen, for the Hotkeys page.
    pub name: String,
    pub hotkey: Option<Hotkey>,
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self {
            summon: Some(Hotkey::bare("F8")),
            quick_input: Some(Hotkey::bare("F9")),
            continue_: None,
            skip: None,
            clear: None,
            toggle_mode: None,
            toggle_overlays: None,
            show_chat: None,
            reply_latest: None,
            quick_phrases: None,
            toggle_lock: None,
            replay: None,
            toggle_history: None,
            next_character: None,
            prev_character: None,
            next_profile: None,
            profiles: Vec::new(),
            input: InputKeys::default(),
        }
    }
}

impl HotkeySettings {
    pub(crate) fn global_slots_mut(&mut self) -> [&mut Option<Hotkey>; 16] {
        [
            &mut self.summon,
            &mut self.quick_input,
            &mut self.continue_,
            &mut self.skip,
            &mut self.clear,
            &mut self.toggle_mode,
            &mut self.toggle_overlays,
            &mut self.show_chat,
            &mut self.reply_latest,
            &mut self.quick_phrases,
            &mut self.toggle_lock,
            &mut self.replay,
            &mut self.toggle_history,
            &mut self.next_character,
            &mut self.prev_character,
            &mut self.next_profile,
        ]
    }
}

macro_rules! define_input_keys {
    ($count:expr; $( $(#[$meta:meta])* $variant:ident => $field:ident : $default:expr ),+ $(,)?) => {
        /// The input window's own keys. `None` leaves the key to the editor.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        #[serde(default)]
        pub struct InputKeys {
            $(pub $field: Option<Hotkey>,)+
        }

        impl Default for InputKeys {
            fn default() -> Self {
                Self {
                    $($field: $default,)+
                }
            }
        }

        /// An action of the input window's own keys.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum InputKey {
            $($(#[$meta])* $variant,)+
        }

        impl InputKey {
            pub const ALL: [Self; $count] = [$(Self::$variant,)+];

            pub fn default_binding(self) -> Option<Hotkey> {
                match self {
                    $(Self::$variant => $default,)+
                }
            }

            pub fn get(self, keys: &InputKeys) -> Option<&Hotkey> {
                match self {
                    $(Self::$variant => keys.$field.as_ref(),)+
                }
            }

            pub fn slot(self, keys: &mut InputKeys) -> &mut Option<Hotkey> {
                match self {
                    $(Self::$variant => &mut keys.$field,)+
                }
            }
        }
    };
}

define_input_keys!(
    16;
    Send => send: Some(Hotkey::bare("Enter")),
    Newline => newline: Some(Hotkey::shift("Enter")),
    RecallPrev => recall_prev: Some(Hotkey::ctrl("ArrowUp")),
    RecallNext => recall_next: Some(Hotkey::ctrl("ArrowDown")),
    Hide => hide: Some(Hotkey::bare("Escape")),
    Bold => bold: Some(Hotkey::ctrl("KeyB")),
    Italic => italic: Some(Hotkey::ctrl("KeyI")),
    Underline => underline: Some(Hotkey::ctrl("KeyU")),
    /// Select the chat message above the selected one (the newest first).
    ChatPrev => chat_prev: Some(Hotkey::alt("ArrowUp")),
    /// Select the one below; past the newest, select none.
    ChatNext => chat_next: Some(Hotkey::alt("ArrowDown")),
    /// Reply to the selected chat message, or the newest.
    Reply => reply: Some(Hotkey::alt("KeyR")),
    /// Mention the selected message's author, or the newest's.
    Mention => mention: Some(Hotkey::alt("KeyM")),
    CancelReply => cancel_reply: None,
    /// Show live chat in the sidebar, or put it away.
    ToggleChat => toggle_chat: None,
    /// Open the sidebar's quick phrases, or fold them away.
    Phrases => phrases: None,
    Stickers => stickers: None,
);

impl InputKeys {
    /// The action bound to `chord`, if any.
    pub fn matching(&self, chord: &Hotkey) -> Option<InputKey> {
        InputKey::ALL
            .into_iter()
            .find(|k| k.get(self) == Some(chord))
    }
}

impl InputKey {
    /// Why `chord` may not be this key's.
    pub fn refusal(self, chord: &Hotkey) -> Option<Refusal> {
        chord.refusal_local(self == Self::Send)
    }

    /// Whether this key acts on live chat (Settings › Hotkeys lists these
    /// apart).
    pub fn is_chat(self) -> bool {
        matches!(
            self,
            Self::ChatPrev
                | Self::ChatNext
                | Self::Reply
                | Self::Mention
                | Self::CancelReply
                | Self::ToggleChat
        )
    }
}
