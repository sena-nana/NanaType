//! Input routing, keybindings, hotkey capture, and dictation fencing.

use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use nana_ui::RuntimeProgramContext;
use nana_ui_platform::{InputPayload, KeyState, WindowId};
use nanatype_settings::{Hotkey, InputKey, InputKeys};

use crate::app::App;
use crate::msg::{EditorMsg, Msg, Shortcut, SpeechMsg, WindowMsg, input_message};
use crate::services::sounds::Keystroke;
use crate::ui::Ui;
use crate::windows::WindowKind;

const KEYSTROKE_DEDUP: Duration = Duration::from_millis(40);

pub(super) fn is_modifier(code: &str) -> bool {
    matches!(
        code,
        "ShiftLeft"
            | "ShiftRight"
            | "ControlLeft"
            | "ControlRight"
            | "AltLeft"
            | "AltRight"
            | "MetaLeft"
            | "MetaRight"
    )
}

/// The quick bar's own key ahead of its field: Esc hides it.
pub(super) fn quick_key(key: &nana_ui::KeyInput) -> Option<Msg> {
    (key.state == KeyState::Pressed && chord_of(key) == Hotkey::bare("Escape"))
        .then_some(Msg::QuickHide)
}

/// Send from the quick bar when the configured send key is not plain Enter.
pub(super) fn quick_send_key(key: &nana_ui::KeyInput, ui: Ui) -> Option<Msg> {
    if key.state != KeyState::Pressed {
        return None;
    }
    let chord = chord_of(key);
    if chord == Hotkey::bare("Enter") {
        return None;
    }
    let send = ui.settings.with_untracked(|s| s.hotkeys.input.send.clone());
    (send.as_ref() == Some(&chord)).then(|| Msg::QuickSend(ui.quick_text.get_untracked()))
}

/// Input window key handler registered on the rich editor and markup source.
pub(super) fn key_policy(
    post: RuntimeProgramContext<Msg>,
    ui: Ui,
    input_keys: Arc<RwLock<InputKeys>>,
    source: bool,
) -> impl FnMut(&nana_ui::KeyInput) -> bool + Send + 'static {
    move |key| {
        let completing = ui.completion.with_untracked(Option::is_some);
        let action = {
            let keys = input_keys.read().unwrap_or_else(|e| e.into_inner());
            input_key(key, source, completing, &keys)
        };
        let mutates_draft = match &action {
            Some((msg, _)) => !matches!(
                msg,
                Msg::Editor(EditorMsg::Complete(_)) | Msg::Chat(crate::msg::ChatMsg::Select(_))
            ),
            None => key_may_edit_draft(key),
        };
        if mutates_draft {
            ui.draft_fence.with_untracked(|f| f.bump_version());
            if ui.speech.with_untracked(|s| s.phase.active()) {
                post.dispatch(Msg::Speech(SpeechMsg::Edit));
            }
        }
        let Some((message, consumed)) = action else {
            return false;
        };
        post.dispatch(message);
        consumed
    }
}

/// Update `ui.draft_fence` synchronously for IME composition events while a draft
/// editor is focused. Returns true when active dictation should pause.
pub(super) fn fence_speech_composition(
    ui: Ui,
    payload: &InputPayload,
    draft_focused: bool,
) -> bool {
    if !draft_focused {
        ui.draft_fence.with_untracked(|f| f.set_composing(false));
        return false;
    }
    let InputPayload::Composition(comp) = payload else {
        return false;
    };
    let active_edit = ui.draft_fence.with_untracked(|f| match comp {
        nana_ui_platform::CompositionInput::Start
        | nana_ui_platform::CompositionInput::Update { .. }
        | nana_ui_platform::CompositionInput::DeleteSurrounding { .. } => {
            f.set_composing(true);
            true
        }
        nana_ui_platform::CompositionInput::Commit(_) => {
            f.bump_version();
            f.set_composing(false);
            true
        }
        _ => {
            f.set_composing(false);
            false
        }
    });
    active_edit && ui.speech.with_untracked(|s| s.phase.active())
}

/// Pure navigation/copy/escape keys never mutate the draft and leave dictation
/// uninterrupted; every other key press advances `draft_version`.
pub(super) fn key_may_edit_draft(key: &nana_ui::KeyInput) -> bool {
    if key.state != KeyState::Pressed || is_modifier(key.physical.0.as_ref()) {
        return false;
    }
    let chord = chord_of(key);
    let key_name = chord.key.as_str();
    let no_ctrl_alt_meta = !chord.ctrl && !chord.alt && !chord.meta;
    if no_ctrl_alt_meta && key_name == "Escape" {
        return false;
    }
    if !chord.alt
        && !chord.meta
        && matches!(
            key_name,
            "ArrowLeft"
                | "ArrowRight"
                | "ArrowUp"
                | "ArrowDown"
                | "Home"
                | "End"
                | "PageUp"
                | "PageDown"
        )
    {
        return false;
    }
    if (chord.ctrl || chord.meta)
        && !chord.alt
        && !chord.shift
        && matches!(key_name, "KeyC" | "KeyA")
    {
        return false;
    }
    true
}

pub(super) fn input_key(
    key: &nana_ui::KeyInput,
    source: bool,
    completing: bool,
    keys: &InputKeys,
) -> Option<(Msg, bool)> {
    if completing {
        if source && moves_caret(key) {
            // The markup field reports no caret moves: close the list rather
            // than leave it on a token the caret has left.
            return Some((
                Msg::Editor(EditorMsg::Complete(crate::msg::Complete::Dismiss)),
                false,
            ));
        }
        match completion_key(key) {
            // The markup field's own popup closes too.
            Some(message @ Msg::Editor(EditorMsg::Complete(crate::msg::Complete::Dismiss)))
                if source =>
            {
                return Some((message, false));
            }
            // It chooses and fills in itself, ahead of the bindings.
            Some(_) if source => return None,
            Some(message) => return Some((message, true)),
            None => {}
        }
    }
    input_shortcut(key, source, keys).map(|m| (m, true))
}

/// A key press as a chord.
pub(super) fn chord_of(key: &nana_ui::KeyInput) -> Hotkey {
    let m = key.modifiers;
    let logical = key.logical.0.as_ref();
    let mut chars = logical.chars();
    let name = match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphabetic() => format!("Key{}", c.to_ascii_uppercase()),
        (Some(c), None) if c.is_ascii_digit() => format!("Digit{c}"),
        (Some(' '), None) => "Space".to_owned(),
        (Some(_), Some(_)) if logical != "Unidentified" => logical.to_owned(),
        _ => key.physical.0.as_ref().to_owned(),
    };
    Hotkey {
        ctrl: m.control,
        shift: m.shift,
        alt: m.alt,
        meta: m.meta,
        key: name,
    }
}

fn editor_breaks_line(chord: &Hotkey) -> bool {
    chord.key == "Enter" && !chord.ctrl && !chord.alt && !chord.meta
}

fn completion_key(key: &nana_ui::KeyInput) -> Option<Msg> {
    use crate::msg::Complete;
    let m = key.modifiers;
    if key.state != KeyState::Pressed || m.control || m.alt || m.meta {
        return None;
    }
    let action = match (chord_of(key).key.as_str(), m.shift) {
        ("Tab", false) | ("Enter", false) => Complete::Accept(None),
        ("Tab", true) | ("ArrowUp", false) => Complete::Move(-1),
        ("ArrowDown", false) => Complete::Move(1),
        ("Escape", false) => Complete::Dismiss,
        _ => return None,
    };
    Some(Msg::Editor(EditorMsg::Complete(action)))
}

fn moves_caret(key: &nana_ui::KeyInput) -> bool {
    key.state == KeyState::Pressed
        && matches!(
            chord_of(key).key.as_str(),
            "ArrowLeft" | "ArrowRight" | "Home" | "End" | "PageUp" | "PageDown"
        )
}

pub(super) fn input_shortcut(
    key: &nana_ui::KeyInput,
    source: bool,
    keys: &InputKeys,
) -> Option<Msg> {
    if key.state != KeyState::Pressed {
        return None;
    }
    let chord = chord_of(key);
    match keys.matching(&chord) {
        Some(InputKey::Newline) if editor_breaks_line(&chord) => None,
        Some(action) => Some(input_message(action)),
        None if source && chord == Hotkey::ctrl("KeyV") => super::editor_bridge::clipboard_image(),
        None => None,
    }
}

/// Click for a key pressed or IME composition committed in the input window.
pub(super) fn keystroke_sound(
    app: &mut App,
    payload: &InputPayload,
    last_keystroke: &mut Option<(Instant, bool)>,
) {
    if !app.settings.sound.keystroke {
        return;
    }
    let now = Instant::now();
    let recent = |any: bool| {
        last_keystroke.is_some_and(|(at, key)| (any || !key) && now - at < KEYSTROKE_DEDUP)
    };
    let key = match payload {
        InputPayload::Key(key) => {
            if key.state != KeyState::Pressed
                || key.repeat
                || is_modifier(key.physical.0.as_ref())
                || recent(false)
            {
                return;
            }
            *last_keystroke = Some((now, true));
            Keystroke::of(key.logical.0.as_ref())
        }
        InputPayload::Composition(
            nana_ui_platform::CompositionInput::Update { .. }
            | nana_ui_platform::CompositionInput::Commit(_),
        ) => {
            if recent(true) {
                return;
            }
            *last_keystroke = Some((now, false));
            Keystroke::Normal
        }
        _ => return,
    };
    app.keystroke(key);
}

/// Process Esc-to-clear-search, hotkey recorder capture, and global hide-input
/// outside the editor. Returns `Some(needs_finish)` when consumed.
pub(super) fn handle_window_key(
    app: &mut App,
    id: WindowId,
    handled: bool,
    payload: &InputPayload,
) -> Option<bool> {
    let InputPayload::Key(key) = payload else {
        return Some(false);
    };
    if key.state != KeyState::Pressed {
        return Some(false);
    }
    if chord_of(key) == Hotkey::bare("Escape") {
        match WindowKind::from_id(id) {
            Some(WindowKind::History) if app.ui.history_search.with_untracked(Option::is_some) => {
                app.ui.history_search.set(None);
                return Some(true);
            }
            Some(WindowKind::Danmaku) if app.ui.danmaku_search.with_untracked(Option::is_some) => {
                app.ui.danmaku_search.set(None);
                return Some(true);
            }
            _ => {}
        }
    }
    if let Some(target) = app.ui.recording.get_untracked() {
        let code = key.physical.0.as_ref();
        if code == "Escape" {
            if let Ok(msg) = Msg::RecordHotkey(None).into_domain() {
                app.update(msg);
            }
            return Some(true);
        }
        if !is_modifier(code) {
            let hotkey = match target {
                Shortcut::Global(_) => {
                    let m = key.modifiers;
                    Hotkey {
                        ctrl: m.control,
                        shift: m.shift,
                        alt: m.alt,
                        meta: m.meta,
                        key: code.to_owned(),
                    }
                }
                Shortcut::Input(_) => chord_of(key),
            };
            if let Some(refusal) = target.refusal(&hotkey) {
                let text = app.ui.now_t(target.refusal_key(refusal));
                app.notice(text, true);
                return Some(true);
            }
            if let Ok(msg) = Msg::HotkeyCaptured(hotkey).into_domain() {
                app.update(msg);
            }
            return Some(true);
        }
    }
    if WindowKind::from_id(id) == Some(WindowKind::Input)
        && !handled
        && app.settings.hotkeys.input.hide.as_ref() == Some(&chord_of(key))
    {
        if let Ok(msg) = Msg::Window(WindowMsg::HideInput).into_domain() {
            app.update(msg);
        }
        return Some(true);
    }
    Some(false)
}
