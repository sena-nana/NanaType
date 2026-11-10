//! The editor's keyboard commands. One table: what the key does here is what
//! the tooltip of its button says.

use nana_ui_platform::{KeyInput, KeyState};

use crate::session::Mode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Save,
    Export,
    Undo,
    Redo,
    /// Open find and replace over the source.
    Find,
    /// Close find and replace.
    Dismiss,
    /// The n-th workspace of the activity rail.
    Workspace(Mode),
    ToggleSide,
    ToggleProblems,
    Close,
    DebugStart,
    DebugStop,
}

/// The rail's order, as the number keys choose it.
pub const WORKSPACES: [Mode; 7] = Mode::ALL;

/// The command a key press asks for. `typing` is whether a text field has
/// the focus: it keeps undo and redo for its own text.
pub fn command(key: &KeyInput, typing: bool) -> Option<Command> {
    if key.state != KeyState::Pressed {
        return None;
    }
    let m = key.modifiers;
    let primary = if cfg!(target_os = "macos") {
        m.meta && !m.control
    } else {
        m.control && !m.meta
    };
    let name = key.logical.0.to_lowercase();
    if !primary {
        return match (name.as_str(), m.shift, m.alt || m.meta || m.control) {
            ("f5", false, false) => Some(Command::DebugStart),
            ("f5", true, false) => Some(Command::DebugStop),
            ("escape", false, false) => Some(Command::Dismiss),
            _ => None,
        };
    }
    if m.alt {
        return None;
    }
    match (name.as_str(), m.shift) {
        ("s", false) => Some(Command::Save),
        ("e", false) => Some(Command::Export),
        ("z", false) if !typing => Some(Command::Undo),
        ("z", true) | ("y", false) if !typing => Some(Command::Redo),
        ("f", false) => Some(Command::Find),
        ("b", false) => Some(Command::ToggleSide),
        ("j", false) => Some(Command::ToggleProblems),
        ("w", false) => Some(Command::Close),
        (digit, false) => digit
            .parse::<usize>()
            .ok()
            .and_then(|n| WORKSPACES.get(n.checked_sub(1)?))
            .map(|mode| Command::Workspace(*mode)),
        _ => None,
    }
}

/// `label` with the keys that do the same, as a tooltip shows it.
pub fn hinted(label: String, keys: &str) -> String {
    let keys = if cfg!(target_os = "macos") {
        keys.replace("Mod+", "⌘").replace("Shift+", "⇧")
    } else {
        keys.replace("Mod+", "Ctrl+")
    };
    format!("{label} ({keys})")
}

#[cfg(test)]
mod tests {
    use super::*;
    use nana_ui_platform::InputModifiers;

    fn press(name: &'static str, modifiers: InputModifiers) -> KeyInput {
        KeyInput {
            physical: nana_ui_platform::PhysicalKey(name.into()),
            logical: nana_ui_platform::LogicalKey(name.into()),
            state: KeyState::Pressed,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn the_primary_modifier_chooses_commands_and_typing_keeps_its_own_undo() {
        let primary = InputModifiers::primary();
        assert_eq!(command(&press("s", primary), true), Some(Command::Save));
        assert_eq!(command(&press("z", primary), false), Some(Command::Undo));
        assert_eq!(command(&press("z", primary), true), None);
        assert_eq!(
            command(&press("2", primary), false),
            Some(Command::Workspace(Mode::Files))
        );
        assert_eq!(command(&press("s", InputModifiers::default()), false), None);
        assert_eq!(
            command(&press("F5", InputModifiers::default()), false),
            Some(Command::DebugStart)
        );
    }
}
