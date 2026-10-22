//! Typing completion in the input editor: `@` names a recent chatter, `/`
//! picks a marker. The editor's text up to the caret decides what is being
//! completed; accepting replaces that token with a mention or a marker.

use std::{ops::Range, sync::Arc};

use nanatype_i18n::Locale;
use nanatype_model::{ChatUser, Inline, Marker, Mention};

use crate::chat::Platform;
use crate::ui::I18N;

/// At most this many rows are offered.
pub const MAX_ITEMS: usize = 8;
/// A token longer than this is prose, not something to complete.
const MAX_TOKEN: usize = 32;
/// What an inline object stands as in the rich editor's text.
const OBJECT: char = '\u{FFFC}';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// `@`: a chatter.
    Mention,
    /// `/`: a marker.
    Command,
}

/// The `@…` or `/…` before the caret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub trigger: Trigger,
    /// Byte range from the trigger character to the caret.
    pub range: Range<usize>,
    /// What follows the trigger.
    pub needle: String,
}

/// The token the caret ends, if any. A trigger counts at the start of a line
/// or after a space or an object, so `a/b` and `me@host` stay prose. A
/// mention is one word; a command may take one argument (`/se laugh`).
pub fn token(text: &str, caret: usize) -> Option<Token> {
    let before = text.get(..caret)?;
    let mut spaces = 0;
    for (i, c) in before.char_indices().rev() {
        if caret - i > MAX_TOKEN {
            return None;
        }
        let trigger = match c {
            '@' => Some(Trigger::Mention),
            '/' => Some(Trigger::Command),
            _ => None,
        };
        if let Some(trigger) = trigger {
            let starts = before[..i]
                .chars()
                .next_back()
                .is_none_or(|p| p.is_whitespace() || p == OBJECT);
            if starts {
                let needle = &before[i + c.len_utf8()..];
                if needle.starts_with(char::is_whitespace)
                    || (trigger == Trigger::Mention && spaces > 0)
                {
                    return None;
                }
                return Some(Token {
                    trigger,
                    range: i..caret,
                    needle: needle.to_owned(),
                });
            }
            continue;
        }
        if c == '\n' || c == OBJECT {
            return None;
        }
        if c.is_whitespace() {
            spaces += 1;
            if spaces > 1 {
                return None;
            }
        }
    }
    None
}

/// One row of the list.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub label: String,
    pub detail: String,
    /// What accepting the row inserts.
    pub insert: Inline,
}

/// Where `needle` is found in `hay`: 0 for a prefix, 1 inside, else none.
fn rank(hay: &str, needle: &str) -> Option<u8> {
    let hay = hay.to_lowercase();
    match hay.find(needle) {
        Some(0) => Some(0),
        Some(_) => Some(1),
        None => None,
    }
}

/// The chatters matching `needle`, from the newest message back, each once.
pub fn users<'a>(
    newest_first: impl Iterator<Item = (Platform, &'a ChatUser)>,
    needle: &str,
    locale: Locale,
) -> Vec<Item> {
    let needle = needle.to_lowercase();
    let mut seen: Vec<(Platform, &str)> = Vec::new();
    let mut ranked: Vec<(u8, Item)> = Vec::new();
    for (platform, user) in newest_first {
        if user.name.trim().is_empty() {
            continue;
        }
        let key = if user.uid.is_empty() {
            &user.name
        } else {
            &user.uid
        };
        if seen.contains(&(platform, key.as_str())) {
            continue;
        }
        seen.push((platform, key.as_str()));
        if let Some(r) = rank(&user.name, &needle) {
            ranked.push((
                r,
                Item {
                    label: format!("@{}", user.name),
                    detail: I18N.t_in(locale, platform.label_key()),
                    insert: Inline::Mention(Mention::new(user.clone())),
                },
            ));
        }
    }
    finish(ranked)
}

/// The markers matching `needle`: by markup tag (`wait`, `se laugh`) or by
/// the chip's own label.
pub fn commands(skin: &nanatype_theme::Skin, needle: &str, locale: Locale) -> Vec<Item> {
    let needle = needle.to_lowercase();
    let mut all: Vec<(String, Marker)> = vec![
        ("wait".into(), Marker::Wait { ms: 500 }),
        ("pause".into(), Marker::Pause { auto_ms: None }),
        ("page".into(), Marker::Page),
        (
            "screenshake".into(),
            Marker::ScreenShake {
                amp: skin.effects.screen_shake_px,
                ms: 300,
            },
        ),
    ];
    all.extend(skin.sounds.se.keys().map(|name| {
        (
            format!("se {name}"),
            Marker::Sound {
                sound: name.clone(),
                gain: 1.0,
            },
        )
    }));
    all.extend(skin.dialogue.portrait.variants.keys().map(|name| {
        (
            format!("expr {name}"),
            Marker::Expression {
                name: name.clone(),
                intensity: None,
            },
        )
    }));
    let ranked = all
        .into_iter()
        .filter_map(|(tag, marker)| {
            let detail = crate::ui::input::marker_label(&marker, locale);
            let r = rank(&tag, &needle).or_else(|| rank(&detail, &needle).map(|_| 1))?;
            Some((
                r,
                Item {
                    label: format!("/{tag}"),
                    detail,
                    insert: Inline::Marker(marker),
                },
            ))
        })
        .collect();
    finish(ranked)
}

/// Prefix matches first, each group in its given order.
fn finish(mut ranked: Vec<(u8, Item)>) -> Vec<Item> {
    ranked.sort_by_key(|(r, _)| *r);
    ranked
        .into_iter()
        .map(|(_, item)| item)
        .take(MAX_ITEMS)
        .collect()
}

/// The open list: what it completes and which row is chosen.
#[derive(Debug, Clone, PartialEq)]
pub struct Completion {
    pub token: Token,
    pub items: Arc<[Item]>,
    pub selected: usize,
}

impl Completion {
    /// Move the choice by `delta` rows, wrapping around.
    pub fn move_by(&mut self, delta: i32) {
        let n = self.items.len() as i64;
        if n > 0 {
            self.selected = (self.selected as i64 + delta as i64).rem_euclid(n) as usize;
        }
    }
}

/// The app's completion state.
#[derive(Debug, Default)]
pub struct Completer {
    pub open: Option<Completion>,
    /// The rich editor's caret; none while a range is selected.
    pub caret: Option<usize>,
    /// Where the token starts that Esc or accepting closed the list on; it
    /// stays closed there until another token starts.
    pub dismissed: Option<usize>,
}

impl Completer {
    pub fn view(&self) -> Option<Arc<CompletionView>> {
        self.open.as_ref().map(|c| {
            Arc::new(CompletionView {
                trigger: c.token.trigger,
                items: c.items.clone(),
                selected: c.selected,
            })
        })
    }
}

/// What the popup shows.
#[derive(Debug, Clone, PartialEq)]
pub struct CompletionView {
    pub trigger: Trigger,
    pub items: Arc<[Item]>,
    pub selected: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> Option<Token> {
        token(text, text.len())
    }

    fn user(uid: &str, name: &str) -> ChatUser {
        ChatUser {
            uid: uid.into(),
            name: name.into(),
            avatar: None,
        }
    }

    #[test]
    fn a_trigger_counts_at_a_word_start_only() {
        assert_eq!(at("@").unwrap().needle, "");
        let t = at("hi @Nan").unwrap();
        assert_eq!(
            (t.trigger, t.range, t.needle.as_str()),
            (Trigger::Mention, 3..7, "Nan")
        );
        assert_eq!(at("x\u{FFFC}/wa").unwrap().trigger, Trigger::Command);
        assert!(at("me@host").is_none());
        assert!(at("a/b").is_none());
        assert!(at("\u{FFFC}wa").is_none());
        assert!(at("/wait\nnext").is_none());
        // The caret inside the token completes only what precedes it.
        assert_eq!(token("@abc", 2).unwrap().needle, "a");
    }

    #[test]
    fn a_command_takes_one_argument_and_a_mention_none() {
        assert_eq!(at("/se la").unwrap().needle, "se la");
        assert!(at("/se la x").is_none());
        assert!(at("@a b").is_none());
        assert!(at("/ wait").is_none());
        assert!(at(&format!("/{}", "x".repeat(40))).is_none());
    }

    #[test]
    fn chatters_are_listed_once_newest_first_prefix_matches_ahead() {
        let a = user("1", "Alice");
        let b = user("2", "Bob");
        let a2 = user("1", "Alice");
        let mal = user("3", "Malice");
        let blank = user("4", " ");
        let chat = [
            (Platform::Twitch, &mal),
            (Platform::Twitch, &a2),
            (Platform::Twitch, &b),
            (Platform::Twitch, &a),
            (Platform::Twitch, &blank),
        ];
        let all = users(chat.into_iter(), "", Locale::EnUs);
        let names: Vec<_> = all.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(names, ["@Malice", "@Alice", "@Bob"]);
        let names: Vec<_> = users(chat.into_iter(), "ALI", Locale::EnUs)
            .into_iter()
            .map(|i| i.label)
            .collect();
        assert_eq!(names, ["@Alice", "@Malice"]);
        // The same uid on another platform is someone else.
        let other = [(Platform::Twitch, &a), (Platform::Youtube, &a2)];
        assert_eq!(users(other.into_iter(), "", Locale::EnUs).len(), 2);
    }

    #[test]
    fn markers_match_by_tag_or_label() {
        let skin = nanatype_theme::Skin::default();
        let wait = commands(&skin, "wa", Locale::EnUs);
        assert_eq!(wait[0].label, "/wait");
        assert_eq!(wait[0].insert, Inline::Marker(Marker::Wait { ms: 500 }));
        assert!(commands(&skin, "", Locale::EnUs).len() >= 4);
        assert!(commands(&skin, "zzz", Locale::EnUs).is_empty());
        let page = commands(&skin, "翻", Locale::ZhCn);
        assert!(page.iter().any(|i| i.label == "/page"));
    }

    #[test]
    fn the_choice_wraps() {
        let item = Item {
            label: String::new(),
            detail: String::new(),
            insert: Inline::Marker(Marker::Page),
        };
        let mut c = Completion {
            token: at("/").unwrap(),
            items: Arc::from(vec![item.clone(), item.clone(), item]),
            selected: 0,
        };
        c.move_by(-1);
        assert_eq!(c.selected, 2);
        c.move_by(2);
        assert_eq!(c.selected, 1);
    }
}
