//! Typing completion in the input editor: `@` names a recent chatter, `/`
//! picks a marker. The editor's text up to the caret decides what is being
//! completed; accepting replaces that token with a mention or a marker.

use std::{ops::Range, sync::Arc};

use nana_ui::runtime::{TextCompletion, TextCompletionEdit};
use nanatype_i18n::Locale;
use nanatype_model::{
    ChatUser, Continuous, ContinuousKind, Document, Emphasis, EntranceKind, Inline, Marker,
    Mention, Pace, VtsAction,
};

use crate::chat::Platform;
use crate::msg::{MarkupEdit, Tool};
use crate::ui::I18N;

/// At most this many rows are shown; the list scrolls through the rest.
pub const MAX_ITEMS: usize = 8;
/// At most this many rows are offered.
const MAX_MATCHES: usize = 32;
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
    /// Remove the token and apply a span effect at the caret, as its
    /// toolbar button does: what is typed next carries it.
    Tool(Tool),
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
    Speed,
    /// A rate or a fixed `Nms` per character.
    Rate,
    Name,
    Volume,
    Intensity,
    /// `on` or `off`.
    Active,
    Value,
}

impl Arg {
    fn hint(self, locale: Locale) -> String {
        let (key, required) = match self {
            Self::Ms => ("complete-arg-ms", false),
            Self::Amp => ("complete-arg-amp", false),
            Self::Speed => ("complete-arg-speed", false),
            Self::Rate => ("complete-arg-rate", true),
            Self::Name => ("complete-arg-name", true),
            Self::Volume => ("complete-arg-volume", false),
            Self::Intensity => ("complete-arg-intensity", false),
            Self::Active => ("complete-arg-active", false),
            Self::Value => ("complete-arg-value", true),
        };
        let name = I18N.t_in(locale, key);
        if required {
            format!("<{name}>")
        } else {
            format!("[{name}]")
        }
    }
}

/// A `/` command: a markup tag, with its arguments in order.
struct Command {
    name: &'static str,
    /// What it does, shown beside its name.
    about: &'static str,
    args: &'static [Arg],
}

const fn command(name: &'static str, about: &'static str, args: &'static [Arg]) -> Command {
    Command { name, about, args }
}

const LOOP: &[Arg] = &[Arg::Amp, Arg::Speed];

const COMMANDS: &[Command] = &[
    command("wait", "cheat-wait", &[Arg::Ms]),
    command("pause", "cheat-pause", &[Arg::Ms]),
    command("page", "cheat-page", &[]),
    command("em", "complete-about-em", &[Arg::Ms]),
    command("speed", "complete-about-speed", &[Arg::Rate]),
    command("instant", "tb-speed-instant", &[]),
    command("shake", "tb-effect-shake", LOOP),
    command("wave", "tb-effect-wave", LOOP),
    command("jump", "tb-effect-jump", LOOP),
    command("rainbow", "tb-effect-rainbow", LOOP),
    command("pulse", "tb-effect-pulse", LOOP),
    command("flicker", "tb-effect-flicker", LOOP),
    command("fade", "tb-entrance-fade", &[Arg::Ms]),
    command("pop", "tb-entrance-pop", &[Arg::Ms]),
    command("slide", "tb-entrance-slide", &[Arg::Ms]),
    command("screenshake", "complete-about-shake", &[Arg::Amp, Arg::Ms]),
    command("se", "cheat-se", &[Arg::Name, Arg::Volume]),
    command("expr", "cheat-expr", &[Arg::Name, Arg::Intensity]),
    command("portrait", "complete-portrait", &[Arg::Name]),
    command("vts-hotkey", "complete-about-vts-hotkey", &[Arg::Name]),
    command(
        "vts-expr",
        "complete-about-vts-expr",
        &[Arg::Name, Arg::Active],
    ),
    command(
        "vts-param",
        "complete-about-vts-param",
        &[Arg::Name, Arg::Value, Arg::Ms],
    ),
];

/// Values the list offers before one is typed.
const WAITS: [&str; 3] = ["250", "500", "1000"];
const SPEEDS: [&str; 3] = ["0.5", "2", "40ms"];

/// What completing can draw its names from besides the skin.
#[derive(Debug, Clone, Copy, Default)]
pub struct Names<'a> {
    /// The connected VTube Studio model's hotkeys.
    pub vts_hotkeys: &'a [String],
}

/// Whether `name` is a span tag rather than a marker.
fn span(name: &str) -> bool {
    matches!(name, "em" | "speed" | "instant" | "fade" | "pop" | "slide")
        || ContinuousKind::from_key(name).is_some()
}

impl Command {
    fn hint(&self, locale: Locale) -> String {
        let hints: Vec<String> = self.args.iter().map(|a| a.hint(locale)).collect();
        hints.join(" ")
    }

    /// What the command does with no arguments; none when it needs one.
    fn default(&self, skin: &nanatype_theme::Skin) -> Option<Insert> {
        let marker = |m| Some(Insert::Inline(Inline::Marker(m)));
        match self.name {
            "wait" => marker(Marker::Wait { ms: 500 }),
            "pause" => marker(Marker::Pause { auto_ms: None }),
            "page" => marker(Marker::Page),
            "screenshake" => marker(Marker::ScreenShake {
                amp: skin.effects.screen_shake_px,
                ms: 300,
            }),
            "em" => Some(Insert::Tool(Tool::Emphasis(Emphasis::default()))),
            "instant" => Some(Insert::Tool(Tool::Pace(Pace::Instant))),
            name => {
                if let Some(kind) = ContinuousKind::from_key(name) {
                    return Some(Insert::Tool(Tool::Effect(Continuous::new(kind))));
                }
                let kind = EntranceKind::ALL
                    .into_iter()
                    .find(|k| *k != EntranceKind::None && k.key() == name)?;
                // The skin's duration, as the toolbar gives it.
                Some(Insert::Tool(Tool::Entrance(kind, None)))
            }
        }
    }

    /// The tag these arguments make, as one would write it.
    fn tag(&self, args: &[&str]) -> Option<String> {
        let q = |v: &str| crate::app::quote(v);
        Some(match (self.name, args) {
            ("screenshake", _) => format!("[screenshake={}]", args.join(",")),
            ("se", [name, vol]) => format!("[se={} vol={vol}]", q(name)),
            ("expr", [name, x]) => format!("[expr={} intensity={x}]", q(name)),
            ("vts-hotkey", [name]) => format!("[vts hotkey={}]", q(name)),
            ("vts-expr", [file]) => format!("[vts expr={}]", q(file)),
            ("vts-expr", [file, on]) => format!("[vts expr={} active={on}]", q(file)),
            ("vts-param", [name, value]) => format!("[vts param={} value={value}]", q(name)),
            ("vts-param", [name, value, ms]) => {
                format!("[vts param={} value={value} ms={ms}]", q(name))
            }
            ("vts-param", _) => return None,
            (name, [amp]) if ContinuousKind::from_key(name).is_some() => {
                format!("[{name} amp={amp}]x[/{name}]")
            }
            (name, [amp, speed]) if ContinuousKind::from_key(name).is_some() => {
                format!("[{name} amp={amp} speed={speed}]x[/{name}]")
            }
            (name, [value]) if span(name) => format!("[{name}={value}]x[/{name}]"),
            (name, [value]) => format!("[{name}={}]", q(value)),
            _ => return None,
        })
    }

    /// What these arguments make, read by the markup parser so `/` accepts
    /// exactly what the tag does (`/wait 800ms`, `/se laugh 0.5`).
    fn insert(&self, args: &[&str], skin: &nanatype_theme::Skin) -> Option<Insert> {
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
        let parsed = nanatype_markup::parse(&self.tag(args)?);
        if !parsed.diagnostics.is_empty() {
            return None;
        }
        let [p] = parsed.doc.paragraphs.as_slice() else {
            return None;
        };
        match p.inlines.as_slice() {
            [Inline::Marker(m)] => Some(Insert::Inline(Inline::Marker(m.clone()))),
            [Inline::Text(run)] => {
                let tool = if let Some(effect) = run.effects.continuous.first() {
                    Tool::Effect(*effect)
                } else if let Some(em) = run.effects.emphasis {
                    Tool::Emphasis(em)
                } else if let Some(e) = run.effects.entrance {
                    Tool::Entrance(e.kind, Some(e.ms))
                } else if !run.pace.is_inherit() {
                    Tool::Pace(run.pace)
                } else {
                    return None;
                };
                Some(Insert::Tool(tool))
            }
            _ => None,
        }
    }

    /// The names offered for the first argument.
    fn names(&self, skin: &nanatype_theme::Skin, names: Names<'_>) -> Vec<String> {
        match self.name {
            "se" => skin.sounds.se.keys().cloned().collect(),
            "expr" | "portrait" => skin.dialogue.portrait.variants.keys().cloned().collect(),
            "vts-hotkey" => names.vts_hotkeys.to_vec(),
            _ => Vec::new(),
        }
    }

    /// Values offered before the only argument is typed.
    fn values(&self) -> &'static [&'static str] {
        match self.name {
            "wait" => &WAITS,
            "speed" => &SPEEDS,
            _ => &[],
        }
    }

    /// The row inserting this command with `args`, named by the word being
    /// completed, as an IDE lists the member after a dot.
    fn row(&self, args: &[&str], skin: &nanatype_theme::Skin, locale: Locale) -> Option<Item> {
        let insert = self.insert(args, skin)?;
        let detail = match &insert {
            Insert::Inline(Inline::Marker(m)) => meaning(m, locale),
            Insert::Tool(tool) => {
                effect_meaning(tool, locale).unwrap_or_else(|| I18N.t_in(locale, self.about))
            }
            _ => I18N.t_in(locale, self.about),
        };
        Some(Item {
            label: args.last().copied().unwrap_or(self.name).to_owned(),
            note: String::new(),
            detail,
            insert,
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
        Marker::Vts(VtsAction::Hotkey { .. }) => t("complete-about-vts-hotkey"),
        Marker::Vts(VtsAction::Expression { active: true, .. }) => t("complete-about-vts-expr"),
        Marker::Vts(VtsAction::Expression { active: false, .. }) => t("complete-vts-expr-off"),
        Marker::Vts(VtsAction::Param { value, ms: 0, .. }) => {
            tr("complete-vts-param", &[("value", f64::from(*value).into())])
        }
        Marker::Vts(VtsAction::Param { value, ms, .. }) => tr(
            "complete-vts-param-hold",
            &[("value", f64::from(*value).into()), ("ms", (*ms).into())],
        ),
    }
}

/// What an effect at other than its usual values does; none at those,
/// where the command's own words say it.
fn effect_meaning(tool: &Tool, locale: Locale) -> Option<String> {
    let tr = |k: &str, args: &[(&str, nanatype_i18n::Arg<'_>)]| I18N.tr_in(locale, k, args);
    Some(match tool {
        Tool::Effect(e) if e.amp == 1.0 && e.speed == 1.0 => return None,
        Tool::Effect(e) => tr(
            "complete-effect",
            &[
                ("amp", f64::from(e.amp).into()),
                ("speed", f64::from(e.speed).into()),
            ],
        ),
        Tool::Emphasis(em) if *em == Emphasis::default() => return None,
        Tool::Emphasis(em) => tr("complete-em", &[("ms", em.ms.into())]),
        Tool::Entrance(_, Some(ms)) => tr("complete-entrance", &[("ms", (*ms).into())]),
        Tool::Pace(Pace::Scale(x)) => tr("complete-speed", &[("x", f64::from(*x).into())]),
        Tool::Pace(Pace::FixedMs(ms)) => tr("complete-speed-ms", &[("ms", (*ms).into())]),
        _ => return None,
    })
}

/// The commands matching `needle`. A bare name lists the commands (by tag or
/// by what they do); after a space come its arguments: the names on offer
/// for a sound, an expression or a hotkey, or what the typed values make.
pub fn commands(
    skin: &nanatype_theme::Skin,
    names: Names<'_>,
    needle: &str,
    locale: Locale,
) -> Vec<Item> {
    let Some((head, rest)) = needle.split_once(char::is_whitespace) else {
        let needle = needle.to_lowercase();
        let ranked = COMMANDS
            .iter()
            .filter_map(|c| {
                let about = I18N.t_in(locale, c.about);
                let r = rank(c.name, &needle).or_else(|| rank(&about, &needle).map(|_| 1))?;
                let insert = c
                    .default(skin)
                    .unwrap_or_else(|| Insert::Text(format!("/{} ", c.name)));
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
    let offered = command.names(skin, names);
    if !offered.is_empty() && args.len() <= 1 {
        // A name: those on offer, then whatever was typed (an asset, a VTS
        // expression the skin does not draw).
        let typed = args.first().copied().unwrap_or("");
        let lower = typed.to_lowercase();
        let mut ranked: Vec<(u8, Item)> = offered
            .iter()
            .filter_map(|n| {
                let r = rank(n, &lower)?;
                Some((r, command.row(&[n.as_str()], skin, locale)?))
            })
            .collect();
        if !typed.is_empty() && !offered.iter().any(|n| n == typed) {
            ranked.extend(command.row(&[typed], skin, locale).map(|i| (2, i)));
        }
        return finish(ranked);
    }
    if args.is_empty() {
        let values = command.values();
        if values.is_empty() {
            return command.row(&[], skin, locale).into_iter().collect();
        }
        return values
            .iter()
            .filter_map(|v| command.row(&[v], skin, locale))
            .collect();
    }
    command.row(&args, skin, locale).into_iter().collect()
}

/// Prefix matches first, each group in its given order.
fn finish(mut ranked: Vec<(u8, Item)>) -> Vec<Item> {
    ranked.sort_by_key(|(r, _)| *r);
    ranked
        .into_iter()
        .map(|(_, item)| item)
        .take(MAX_MATCHES)
        .collect()
}

/// The open list: what it completes and which row is chosen.
#[derive(Debug, Clone, PartialEq)]
pub struct Completion {
    pub token: Token,
    pub items: Arc<[Item]>,
    pub selected: usize,
    /// The first row shown.
    pub first: usize,
}

impl Completion {
    /// The rows as the markup source's own popup shows them: each replaces
    /// the token in `source` with the tag it stands for.
    pub fn text_completions(&self, source: &str) -> Arc<[TextCompletion]> {
        let source: Arc<str> = source.into();
        self.items
            .iter()
            .map(|item| {
                let mut stops = Vec::new();
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
                    // The pair of tags, the caret between them.
                    Insert::Tool(tool) => match crate::app::markup_for(tool) {
                        Some(MarkupEdit::Wrap { open, close }) => {
                            stops.push(open.len()..open.len());
                            open + &close
                        }
                        _ => String::new(),
                    },
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
                        stops,
                    })
            })
            .collect()
    }

    /// Move the choice by `delta` rows, wrapping around; the rows shown
    /// follow it.
    pub fn move_by(&mut self, delta: i32) {
        let n = self.items.len() as i64;
        if n > 0 {
            self.selected = (self.selected as i64 + delta as i64).rem_euclid(n) as usize;
        }
        if self.selected < self.first {
            self.first = self.selected;
        } else if self.selected >= self.first + MAX_ITEMS {
            self.first = self.selected + 1 - MAX_ITEMS;
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
            let first = c.first.min(c.items.len());
            let shown = &c.items[first..(first + MAX_ITEMS).min(c.items.len())];
            Arc::new(CompletionView {
                trigger: c.token.trigger,
                start: c.token.range.start + 1 + word,
                first,
                items: shown.into(),
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
    /// Where the rows shown start among those offered.
    pub first: usize,
    /// The rows shown.
    pub items: Arc<[Item]>,
    /// The chosen row among those offered.
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

    fn tool(item: &Item) -> &Tool {
        match &item.insert {
            Insert::Tool(t) => t,
            other => panic!("not an effect: {other:?}"),
        }
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
        let wait = commands(&skin, Names::default(), "wa", Locale::EnUs);
        // Only the word being completed, IDE-style, and what it does.
        assert_eq!(wait[0].label, "wait");
        assert_eq!(wait[0].note, "[ms]");
        assert_eq!(wait[0].detail, I18N.t_in(Locale::EnUs, "cheat-wait"));
        let about: Vec<_> = commands(&skin, Names::default(), "", Locale::ZhCn)
            .into_iter()
            .map(|i| (i.label, i.detail))
            .collect();
        assert!(
            about.contains(&("se".into(), "播放音效".into())),
            "{about:?}"
        );
        assert!(about.contains(&("screenshake".into(), "震动整个对话框".into())));
        let value = |needle: &str| {
            let rows = commands(&skin, Names::default(), needle, Locale::ZhCn);
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
        assert_eq!(
            commands(&skin, Names::default(), "", Locale::EnUs).len(),
            COMMANDS.len()
        );
        assert!(commands(&skin, Names::default(), "zzz", Locale::EnUs).is_empty());
        let page = commands(&skin, Names::default(), "翻", Locale::ZhCn);
        assert!(page.iter().any(|i| i.label == "page"));
        // A command that needs a name fills in its own and waits for one.
        let se = commands(&skin, Names::default(), "se", Locale::EnUs);
        assert_eq!(se[0].label, "se");
        assert_eq!(se[0].insert, Insert::Text("/se ".into()));
        assert_eq!(se[0].note, "<name> [volume]");
    }

    #[test]
    fn arguments_make_the_marker_the_tag_would() {
        let skin = skin();
        let one = |needle: &str| {
            let rows = commands(&skin, Names::default(), needle, Locale::EnUs);
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
        let waits: Vec<_> = commands(&skin, Names::default(), "wait ", Locale::EnUs)
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
            assert!(
                commands(&skin, Names::default(), needle, Locale::EnUs).is_empty(),
                "{needle}"
            );
        }
    }

    #[test]
    fn a_name_lists_the_skins_then_what_was_typed() {
        let skin = skin();
        let labels = |needle: &str| -> Vec<String> {
            commands(&skin, Names::default(), needle, Locale::EnUs)
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

    /// The command for each kind of marker, so a new one fails to compile
    /// until `/` can make it.
    fn command_of(marker: &Marker) -> &'static str {
        match marker {
            Marker::Wait { .. } => "wait",
            Marker::Pause { .. } => "pause",
            Marker::Page => "page",
            Marker::Sound { .. } => "se",
            Marker::Expression { .. } => "expr",
            Marker::Portrait { .. } => "portrait",
            Marker::ScreenShake { .. } => "screenshake",
            Marker::Vts(VtsAction::Hotkey { .. }) => "vts-hotkey",
            Marker::Vts(VtsAction::Expression { .. }) => "vts-expr",
            Marker::Vts(VtsAction::Param { .. }) => "vts-param",
        }
    }

    #[test]
    fn every_performance_has_a_command() {
        let skin = skin();
        let one = |needle: &str| {
            let rows = commands(&skin, Names::default(), needle, Locale::EnUs);
            assert_eq!(rows.len(), 1, "{needle}: {rows:?}");
            rows[0].insert.clone()
        };
        let has = |name: &str| COMMANDS.iter().any(|c| c.name == name);
        for kind in ContinuousKind::ALL {
            assert!(has(kind.key()), "{kind:?}");
        }
        for kind in EntranceKind::ALL {
            assert!(kind == EntranceKind::None || has(kind.key()), "{kind:?}");
        }
        for name in ["em", "speed", "instant"] {
            assert!(has(name), "{name}");
        }
        // Each marker comes out of its own command.
        for needle in [
            "wait 800",
            "pause 2000",
            "screenshake 8",
            "se laugh",
            "expr smile",
            "portrait smile",
            "vts-hotkey Wave",
            "vts-expr Smile.exp3.json off",
            "vts-param MouthSmile 0.8 1500",
        ] {
            let Insert::Inline(Inline::Marker(m)) = one(needle) else {
                panic!("{needle}: not a marker");
            };
            assert!(needle.starts_with(command_of(&m)), "{needle}: {m:?}");
        }
    }

    #[test]
    fn span_commands_set_the_effect_the_tag_would() {
        let skin = skin();
        let one = |needle: &str| {
            let rows = commands(&skin, Names::default(), needle, Locale::EnUs);
            assert_eq!(rows.len(), 1, "{needle}: {rows:?}");
            tool(&rows[0]).clone()
        };
        let bare = |name: &str| {
            let rows = commands(&skin, Names::default(), name, Locale::EnUs);
            let row = rows.iter().find(|r| r.label == name).expect(name);
            row.insert.clone()
        };
        let wave = Continuous::new(ContinuousKind::Wave);
        assert_eq!(bare("wave"), Insert::Tool(Tool::Effect(wave)));
        assert_eq!(
            one("wave 2 0.5"),
            Tool::Effect(Continuous {
                amp: 2.0,
                speed: 0.5,
                ..wave
            })
        );
        assert_eq!(
            bare("em"),
            Insert::Tool(Tool::Emphasis(Emphasis::default()))
        );
        assert_eq!(one("em 200"), Tool::Emphasis(Emphasis { ms: 200 }));
        assert_eq!(
            bare("pop"),
            Insert::Tool(Tool::Entrance(EntranceKind::Pop, None))
        );
        assert_eq!(
            one("slide 120"),
            Tool::Entrance(EntranceKind::SlideUp, Some(120))
        );
        assert_eq!(bare("instant"), Insert::Tool(Tool::Pace(Pace::Instant)));
        // A speed needs one, and offers some.
        assert_eq!(bare("speed"), Insert::Text("/speed ".into()));
        let speeds: Vec<_> = commands(&skin, Names::default(), "speed ", Locale::EnUs)
            .into_iter()
            .map(|i| i.label)
            .collect();
        assert_eq!(speeds, ["0.5", "2", "40ms"]);
        assert_eq!(one("speed 40ms"), Tool::Pace(Pace::FixedMs(40)));
        assert_eq!(one("speed 0.5"), Tool::Pace(Pace::Scale(0.5)));
        let rows = commands(&skin, Names::default(), "speed 40ms", Locale::ZhCn);
        assert_eq!(rows[0].detail, "每字 40 毫秒");
        for needle in [
            "wave x",
            "wave 1 2 3",
            "em soon",
            "speed 0",
            "instant 1",
            "pop x",
        ] {
            assert!(
                commands(&skin, Names::default(), needle, Locale::EnUs).is_empty(),
                "{needle}"
            );
        }
    }

    #[test]
    fn hotkeys_come_from_vts() {
        let skin = skin();
        let hotkeys = ["Wave hand".to_owned(), "Cry".to_owned()];
        let names = Names {
            vts_hotkeys: &hotkeys,
        };
        let rows = commands(&skin, names, "vts-hotkey wa", Locale::EnUs);
        let labels: Vec<_> = rows.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["Wave hand", "wa"]);
        // A name with a space is quoted, so it reads back whole.
        assert_eq!(
            *marker(&rows[0]),
            Marker::Vts(VtsAction::Hotkey {
                hotkey: "Wave hand".into()
            })
        );
    }

    #[test]
    fn the_source_popup_puts_the_caret_between_the_tags() {
        let skin = skin();
        let items = commands(&skin, Names::default(), "wave 2", Locale::EnUs);
        let c = Completion {
            token: at("/wave 2").unwrap(),
            items: items.into(),
            selected: 0,
            first: 0,
        };
        let edit = c.text_completions("/wave 2")[0].edit.clone().unwrap();
        assert_eq!(edit.text, "[wave amp=2][/wave]");
        assert_eq!(edit.stops, vec![12..12]);
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
            first: 0,
        };
        c.move_by(-1);
        assert_eq!(c.selected, 2);
        c.move_by(2);
        assert_eq!(c.selected, 1);
    }

    #[test]
    fn the_rows_shown_follow_the_choice() {
        let skin = skin();
        let mut c = Completer {
            open: Some(Completion {
                token: at("/").unwrap(),
                items: commands(&skin, Names::default(), "", Locale::EnUs).into(),
                selected: 0,
                first: 0,
            }),
            ..Default::default()
        };
        let open = c.open.as_mut().unwrap();
        assert!(open.items.len() > MAX_ITEMS);
        open.move_by(MAX_ITEMS as i32);
        assert_eq!((open.selected, open.first), (MAX_ITEMS, 1));
        // Back up inside the window: it stays.
        open.move_by(-2);
        assert_eq!(open.first, 1);
        // Wrapping to the last row shows the end of the list.
        open.move_by(-(open.selected as i32 + 1));
        let n = open.items.len();
        assert_eq!((open.selected, open.first), (n - 1, n - MAX_ITEMS));
        let view = c.view().unwrap();
        assert_eq!(view.items.len(), MAX_ITEMS);
        assert_eq!(view.items.last(), c.open.as_ref().unwrap().items.last());
    }
}
