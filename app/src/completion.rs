//! Typing completion in the input editor: `@` names a recent chatter, `/`
//! picks a marker. The editor's text up to the caret decides what is being
//! completed; accepting replaces that token with a mention or a marker.

use std::{ops::Range, sync::Arc};

use nana_ui::runtime::{TextCompletion, TextCompletionEdit};
use nanatype_i18n::Locale;
use nanatype_model::{ChatUser, Document, Inline, Marker, Mention};

use crate::chat::Platform;
use crate::ui::I18N;

/// At most this many rows are offered.
pub const MAX_ITEMS: usize = 8;
/// A token longer than this is prose, not something to complete.
const MAX_TOKEN: usize = 48;
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
/// mention is one word; a command takes arguments (`/screenshake 8 400`).
pub fn token(text: &str, caret: usize) -> Option<Token> {
    let before = text.get(..caret)?;
    let mut spaced = false;
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
                    || (trigger == Trigger::Mention && spaced)
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
        spaced |= c.is_whitespace();
    }
    None
}

/// What accepting a row does.
#[derive(Debug, Clone, PartialEq)]
pub enum Insert {
    /// Replace the token with a mention or a marker.
    Inline(Inline),
    /// Replace the token with text and go on completing: a command that
    /// needs an argument.
    Text(String),
}

/// One row of the list.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub label: String,
    /// The arguments a command takes, beside its name.
    pub note: String,
    pub detail: String,
    pub insert: Insert,
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
                    label: user.name.clone(),
                    note: String::new(),
                    detail: I18N.t_in(locale, platform.label_key()),
                    insert: Insert::Inline(Inline::Mention(Mention::new(user.clone()))),
                },
            ));
        }
    }
    finish(ranked)
}

/// A command's argument, for its hint.
#[derive(Debug, Clone, Copy)]
enum Arg {
    Ms,
    Amp,
    Name,
    Volume,
    Intensity,
}

impl Arg {
    fn hint(self, locale: Locale) -> String {
        let (key, required) = match self {
            Self::Ms => ("complete-arg-ms", false),
            Self::Amp => ("complete-arg-amp", false),
            Self::Name => ("complete-arg-name", true),
            Self::Volume => ("complete-arg-volume", false),
            Self::Intensity => ("complete-arg-intensity", false),
        };
        let name = I18N.t_in(locale, key);
        if required {
            format!("<{name}>")
        } else {
            format!("[{name}]")
        }
    }
}

/// A `/` command: a marker's markup tag, with its arguments in order.
struct Command {
    name: &'static str,
    /// What it does, shown beside its name.
    about: &'static str,
    args: &'static [Arg],
}

const COMMANDS: &[Command] = &[
    Command {
        name: "wait",
        about: "cheat-wait",
        args: &[Arg::Ms],
    },
    Command {
        name: "pause",
        about: "cheat-pause",
        args: &[Arg::Ms],
    },
    Command {
        name: "page",
        about: "cheat-page",
        args: &[],
    },
    Command {
        name: "screenshake",
        about: "complete-about-shake",
        args: &[Arg::Amp, Arg::Ms],
    },
    Command {
        name: "se",
        about: "cheat-se",
        args: &[Arg::Name, Arg::Volume],
    },
    Command {
        name: "expr",
        about: "cheat-expr",
        args: &[Arg::Name, Arg::Intensity],
    },
    Command {
        name: "portrait",
        about: "complete-portrait",
        args: &[Arg::Name],
    },
];

/// Waits the list offers before one is typed.
const WAITS: [&str; 3] = ["250", "500", "1000"];

impl Command {
    fn hint(&self, locale: Locale) -> String {
        let hints: Vec<String> = self.args.iter().map(|a| a.hint(locale)).collect();
        hints.join(" ")
    }

    /// What the command inserts with no arguments; none when it needs one.
    fn default(&self, skin: &nanatype_theme::Skin) -> Option<Marker> {
        match self.name {
            "wait" => Some(Marker::Wait { ms: 500 }),
            "pause" => Some(Marker::Pause { auto_ms: None }),
            "page" => Some(Marker::Page),
            "screenshake" => Some(Marker::ScreenShake {
                amp: skin.effects.screen_shake_px,
                ms: 300,
            }),
            _ => None,
        }
    }

    /// The marker these arguments make, read by the markup parser so `/`
    /// accepts exactly what the tag does (`/wait 800ms`, `/se laugh 0.5`).
    fn marker(&self, args: &[&str], skin: &nanatype_theme::Skin) -> Option<Marker> {
        if args.is_empty() {
            return self.default(skin);
        }
        if args.len() > self.args.len()
            || args
                .iter()
                .any(|a| a.contains(['[', ']', '=', '"', '\\', ',']))
        {
            return None;
        }
        let tag = match (self.name, args) {
            ("screenshake", _) => format!("[screenshake={}]", args.join(",")),
            ("se", [name, vol]) => format!("[se={name} vol={vol}]"),
            ("expr", [name, x]) => format!("[expr={name} intensity={x}]"),
            (name, [value]) => format!("[{name}={value}]"),
            _ => return None,
        };
        let parsed = nanatype_markup::parse(&tag);
        if !parsed.diagnostics.is_empty() {
            return None;
        }
        match parsed.doc.paragraphs.as_slice() {
            [p] => match p.inlines.as_slice() {
                [Inline::Marker(m)] => Some(m.clone()),
                _ => None,
            },
            _ => None,
        }
    }

    /// The names the skin offers for the first argument.
    fn names(&self, skin: &nanatype_theme::Skin) -> Vec<String> {
        match self.name {
            "se" => skin.sounds.se.keys().cloned().collect(),
            "expr" | "portrait" => skin.dialogue.portrait.variants.keys().cloned().collect(),
            _ => Vec::new(),
        }
    }

    /// The row inserting this command with `args`, named by the word being
    /// completed, as an IDE lists the member after a dot.
    fn row(&self, args: &[&str], skin: &nanatype_theme::Skin, locale: Locale) -> Option<Item> {
        let marker = self.marker(args, skin)?;
        Some(Item {
            label: args.last().copied().unwrap_or(self.name).to_owned(),
            note: String::new(),
            detail: meaning(&marker, locale),
            insert: Insert::Inline(Inline::Marker(marker)),
        })
    }
}

/// What a marker does, in a few words.
fn meaning(marker: &Marker, locale: Locale) -> String {
    let t = |k: &str| I18N.t_in(locale, k);
    let tr = |k: &str, args: &[(&str, nanatype_i18n::Arg<'_>)]| I18N.tr_in(locale, k, args);
    match marker {
        Marker::Wait { ms } => tr("complete-wait", &[("ms", (*ms).into())]),
        Marker::Pause { auto_ms: None } => t("cheat-pause"),
        Marker::Pause { auto_ms: Some(ms) } => tr("complete-pause-auto", &[("ms", (*ms).into())]),
        Marker::Page => t("cheat-page"),
        Marker::ScreenShake { amp, ms } => tr(
            "complete-shake",
            &[("amp", f64::from(*amp).into()), ("ms", (*ms).into())],
        ),
        Marker::Sound { gain, .. } if *gain == 1.0 => t("cheat-se"),
        Marker::Sound { gain, .. } => tr("complete-volume", &[("vol", f64::from(*gain).into())]),
        Marker::Expression {
            intensity: Some(x), ..
        } => tr("complete-intensity", &[("x", f64::from(*x).into())]),
        Marker::Expression { .. } => t("cheat-expr"),
        Marker::Portrait { .. } => t("complete-portrait"),
        Marker::Vts(_) => crate::ui::input::marker_label(marker, locale),
    }
}

/// The markers matching `needle`. A bare name lists the commands (by tag or
/// by what they do); after a space come its arguments: the skin's names for
/// a sound or an expression, or the marker the typed values make.
pub fn commands(skin: &nanatype_theme::Skin, needle: &str, locale: Locale) -> Vec<Item> {
    let Some((head, rest)) = needle.split_once(char::is_whitespace) else {
        let needle = needle.to_lowercase();
        let ranked = COMMANDS
            .iter()
            .filter_map(|c| {
                let about = I18N.t_in(locale, c.about);
                let r = rank(c.name, &needle).or_else(|| rank(&about, &needle).map(|_| 1))?;
                let insert = match c.default(skin) {
                    Some(m) => Insert::Inline(Inline::Marker(m)),
                    None => Insert::Text(format!("/{} ", c.name)),
                };
                Some((
                    r,
                    Item {
                        label: c.name.to_owned(),
                        note: c.hint(locale),
                        detail: about,
                        insert,
                    },
                ))
            })
            .collect();
        return finish(ranked);
    };
    let head = head.to_lowercase();
    let Some(command) = COMMANDS.iter().find(|c| c.name == head) else {
        return Vec::new();
    };
    if command.args.is_empty() {
        return Vec::new();
    }
    let args: Vec<&str> = rest.split_whitespace().collect();
    let names = command.names(skin);
    if !names.is_empty() && args.len() <= 1 {
        // A name: the skin's, then whatever was typed (an asset, a VTS
        // expression the skin does not draw).
        let typed = args.first().copied().unwrap_or("");
        let lower = typed.to_lowercase();
        let mut ranked: Vec<(u8, Item)> = names
            .iter()
            .filter_map(|n| {
                let r = rank(n, &lower)?;
                Some((r, command.row(&[n.as_str()], skin, locale)?))
            })
            .collect();
        if !typed.is_empty() && !names.iter().any(|n| n == typed) {
            ranked.extend(command.row(&[typed], skin, locale).map(|i| (2, i)));
        }
        return finish(ranked);
    }
    if args.is_empty() {
        let rows: Vec<Item> = match command.name {
            "wait" => WAITS
                .iter()
                .filter_map(|ms| command.row(&[ms], skin, locale))
                .collect(),
            _ => command.row(&[], skin, locale).into_iter().collect(),
        };
        return rows;
    }
    command.row(&args, skin, locale).into_iter().collect()
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
    /// The rows as the markup source's own popup shows them: each replaces
    /// the token in `source` with the tag it stands for.
    pub fn text_completions(&self, source: &str) -> Arc<[TextCompletion]> {
        let source: Arc<str> = source.into();
        self.items
            .iter()
            .map(|item| {
                let text = match &item.insert {
                    Insert::Inline(inline) => {
                        let mut tag = nanatype_markup::serialize(&Document::from_inlines(vec![
                            inline.clone(),
                        ]));
                        // A space after a mention, so typing goes on as text.
                        if matches!(inline, Inline::Mention(_)) {
                            tag.push(' ');
                        }
                        tag
                    }
                    Insert::Text(text) => text.clone(),
                };
                TextCompletion::new(item.label.clone(), item.detail.clone())
                    .detail(item.note.clone())
                    .edit(TextCompletionEdit {
                        snippet: None,
                        variables: Default::default(),
                        source: source.clone(),
                        caret: self.token.range.end,
                        range: self.token.range.clone(),
                        text,
                        additional: Vec::new(),
                        stops: Vec::new(),
                    })
            })
            .collect()
    }

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
            // The word being completed: the name after the trigger, or
            // the argument after the last space.
            let needle = &c.token.needle;
            let word = needle
                .char_indices()
                .rfind(|(_, ch)| ch.is_whitespace())
                .map_or(0, |(i, ch)| i + ch.len_utf8());
            Arc::new(CompletionView {
                trigger: c.token.trigger,
                start: c.token.range.start + 1 + word,
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
    /// Where the word being completed starts in the editor's text: the
    /// list hangs there, its labels under the word.
    pub start: usize,
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
    fn a_command_takes_arguments_and_a_mention_none() {
        assert_eq!(at("/se la").unwrap().needle, "se la");
        assert_eq!(
            at("/screenshake 8 400").unwrap().needle,
            "screenshake 8 400"
        );
        assert!(at("@a b").is_none());
        assert!(at("/ wait").is_none());
        assert!(at(&format!("/{}", "x".repeat(50))).is_none());
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
        assert_eq!(names, ["Malice", "Alice", "Bob"]);
        let names: Vec<_> = users(chat.into_iter(), "ALI", Locale::EnUs)
            .into_iter()
            .map(|i| i.label)
            .collect();
        assert_eq!(names, ["Alice", "Malice"]);
        // The same uid on another platform is someone else.
        let other = [(Platform::Twitch, &a), (Platform::Youtube, &a2)];
        assert_eq!(users(other.into_iter(), "", Locale::EnUs).len(), 2);
    }

    fn marker(item: &Item) -> &Marker {
        match &item.insert {
            Insert::Inline(Inline::Marker(m)) => m,
            other => panic!("not a marker: {other:?}"),
        }
    }

    fn skin() -> nanatype_theme::Skin {
        let mut skin = nanatype_theme::Skin::default();
        for name in ["laugh", "clap"] {
            skin.sounds.se.insert(name.into(), format!("{name}.ogg"));
        }
        skin.dialogue
            .portrait
            .variants
            .insert("smile".into(), "smile.png".into());
        skin
    }

    #[test]
    fn commands_match_by_tag_or_what_they_do() {
        let skin = skin();
        let wait = commands(&skin, "wa", Locale::EnUs);
        // Only the word being completed, IDE-style, and what it does.
        assert_eq!(wait[0].label, "wait");
        assert_eq!(wait[0].note, "[ms]");
        assert_eq!(wait[0].detail, I18N.t_in(Locale::EnUs, "cheat-wait"));
        let about: Vec<_> = commands(&skin, "", Locale::ZhCn)
            .into_iter()
            .map(|i| (i.label, i.detail))
            .collect();
        assert!(
            about.contains(&("se".into(), "播放音效".into())),
            "{about:?}"
        );
        assert!(about.contains(&("screenshake".into(), "震动整个对话框".into())));
        let value = |needle: &str| {
            let rows = commands(&skin, needle, Locale::ZhCn);
            (rows[0].label.clone(), rows[0].detail.clone())
        };
        assert_eq!(
            value("wait 800"),
            ("800".into(), "停顿 800 毫秒后继续".into())
        );
        assert_eq!(
            value("se laugh 0.5"),
            ("0.5".into(), "以 0.5 的音量播放音效".into())
        );
        assert_eq!(value("se la"), ("laugh".into(), "播放音效".into()));
        assert_eq!(*marker(&wait[0]), Marker::Wait { ms: 500 });
        assert_eq!(commands(&skin, "", Locale::EnUs).len(), COMMANDS.len());
        assert!(commands(&skin, "zzz", Locale::EnUs).is_empty());
        let page = commands(&skin, "翻", Locale::ZhCn);
        assert!(page.iter().any(|i| i.label == "page"));
        // A command that needs a name fills in its own and waits for one.
        let se = commands(&skin, "se", Locale::EnUs);
        assert_eq!(se[0].label, "se");
        assert_eq!(se[0].insert, Insert::Text("/se ".into()));
        assert_eq!(se[0].note, "<name> [volume]");
    }

    #[test]
    fn arguments_make_the_marker_the_tag_would() {
        let skin = skin();
        let one = |needle: &str| {
            let rows = commands(&skin, needle, Locale::EnUs);
            assert_eq!(rows.len(), 1, "{needle}: {rows:?}");
            marker(&rows[0]).clone()
        };
        assert_eq!(one("wait 800"), Marker::Wait { ms: 800 });
        assert_eq!(one("WAIT 1200ms"), Marker::Wait { ms: 1200 });
        assert_eq!(
            one("pause 2000"),
            Marker::Pause {
                auto_ms: Some(2000)
            }
        );
        assert_eq!(one("pause "), Marker::Pause { auto_ms: None });
        assert_eq!(
            one("screenshake 8"),
            Marker::ScreenShake { amp: 8.0, ms: 300 }
        );
        assert_eq!(
            one("screenshake 8 450"),
            Marker::ScreenShake { amp: 8.0, ms: 450 }
        );
        assert_eq!(
            one("se laugh 0.5"),
            Marker::Sound {
                sound: "laugh".into(),
                gain: 0.5
            }
        );
        assert_eq!(
            one("expr smile 0.8"),
            Marker::Expression {
                name: "smile".into(),
                intensity: Some(0.8)
            }
        );
        // Offered values before one is typed.
        let waits: Vec<_> = commands(&skin, "wait ", Locale::EnUs)
            .into_iter()
            .map(|i| i.label)
            .collect();
        assert_eq!(waits, ["250", "500", "1000"]);
        // Bad values, extra arguments and markup characters make nothing.
        for needle in [
            "wait x",
            "wait 1 2",
            "screenshake a",
            "se laugh loud",
            "se a]b",
            "page ",
            "nope 1",
        ] {
            assert!(commands(&skin, needle, Locale::EnUs).is_empty(), "{needle}");
        }
    }

    #[test]
    fn a_name_lists_the_skins_then_what_was_typed() {
        let skin = skin();
        let labels = |needle: &str| -> Vec<String> {
            commands(&skin, needle, Locale::EnUs)
                .into_iter()
                .map(|i| i.label)
                .collect()
        };
        assert_eq!(labels("se "), ["clap", "laugh"]);
        assert_eq!(labels("se la"), ["laugh", "clap", "la"]);
        assert_eq!(labels("se laugh"), ["laugh"]);
        assert_eq!(labels("expr wink"), ["wink"]);
        assert_eq!(labels("portrait sm"), ["smile", "sm"]);
    }

    #[test]
    fn the_choice_wraps() {
        let item = Item {
            label: String::new(),
            note: String::new(),
            detail: String::new(),
            insert: Insert::Inline(Inline::Marker(Marker::Page)),
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
