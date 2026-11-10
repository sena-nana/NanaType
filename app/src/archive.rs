//! Saved history: one JSON Lines file per session under the data folder's
//! `history/`, named by the session's first saved line
//! (`2026-11-02_213005.jsonl`). A line is appended as it reaches the history,
//! so a crash loses at most the line being written, and a stream that runs
//! past midnight stays one file (and one subtitle track).
//!
//! Everything here is plain file and text work; the app runs it off the UI
//! thread.

use std::path::{Path, PathBuf};

use nanatype_model::{HistoryItem, Inline, SourceKind};
use serde::{Deserialize, Serialize};

/// The folder under the data folder.
pub const DIR: &str = "history";
const EXT: &str = "jsonl";

/// Who said a saved line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// The streamer.
    #[serde(rename = "self")]
    Own,
    /// A co-op partner in a P2P room.
    Partner,
    /// Viewer chat or a paid event from a live platform.
    Chat,
}

/// One saved line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// Unix milliseconds.
    pub at: u64,
    pub speaker: String,
    pub source: Source,
    /// The live platform of a chat line (`twitch`, `blivechat`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    pub text: String,
    /// The line's markup when its plain-text projection would lose content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markup: Option<String>,
    /// Its translation, when one arrived. A translation that arrives after
    /// the line was written follows as an [`Amend`] line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation: Option<String>,
}

/// A later addition to a record already in the file: its translation.
/// Builds that do not know it skip the line, as they skip a torn one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Amend {
    pub amend: Key,
    pub translation: String,
}

/// Which record an [`Amend`] is for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Key {
    pub at: u64,
    pub speaker: String,
}

impl Amend {
    /// The amendment as one line of the file.
    pub fn line(&self) -> Vec<u8> {
        let mut bytes = serde_json::to_vec(self).unwrap_or_default();
        bytes.push(b'\n');
        bytes
    }
}

/// Amendment lines start with this (serde writes fields in order).
const AMEND_PREFIX: &str = "{\"amend\":";

/// Whether the plain-text projection would lose document content or styling.
///
/// Typed lines have always carried markup. Chat and event lines need it too
/// when they contain an inline asset (or any other non-plain inline), otherwise
/// loading an archive can never recover the media reference from `text`.
fn doc_needs_markup(item: &HistoryItem) -> bool {
    if item.kind == SourceKind::Typewriter {
        return true;
    }
    item.doc.paragraphs.iter().any(|paragraph| {
        paragraph.align.is_some()
            || paragraph.inlines.iter().any(|inline| match inline {
                Inline::Text(run) => {
                    !run.style.is_plain() || !run.effects.is_none() || !run.pace.is_inherit()
                }
                Inline::Ruby(_) | Inline::Atom(_) | Inline::Mention(_) | Inline::Marker(_) => true,
            })
    })
}

impl Record {
    /// A history row as saved; `partner` for a co-op partner's line.
    pub fn of(item: &HistoryItem, partner: bool) -> Self {
        let typed = item.kind == SourceKind::Typewriter;
        Self {
            at: item.at,
            speaker: item.author.clone(),
            source: match (typed, partner) {
                (false, _) => Source::Chat,
                (true, true) => Source::Partner,
                (true, false) => Source::Own,
            },
            platform: (!typed).then(|| item.source.0.clone()),
            text: item.doc.plain_text(),
            markup: doc_needs_markup(item).then(|| nanatype_markup::serialize(&item.doc)),
            translation: None,
        }
    }

    pub fn translated(mut self, translation: Option<&str>) -> Self {
        self.translation = translation.map(str::to_owned);
        self
    }

    /// The record as one line of its file.
    pub fn line(&self) -> Vec<u8> {
        let mut bytes = serde_json::to_vec(self).unwrap_or_default();
        bytes.push(b'\n');
        bytes
    }
}

/// A saved session, as the viewer lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    /// The file stem, `2026-11-02_213005`.
    pub name: String,
    pub lines: usize,
}

impl Session {
    /// `2026-11-02 21:30`, from the name.
    pub fn title(&self) -> String {
        let (date, time) = self.name.split_once('_').unwrap_or((&self.name, ""));
        match (time.get(0..2), time.get(2..4)) {
            (Some(h), Some(m)) => format!("{date} {h}:{m}"),
            _ => date.to_owned(),
        }
    }
}

/// What Settings › History shows of the saved sessions.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct View {
    pub sessions: Vec<Session>,
    /// The session whose lines are shown.
    pub open: Option<String>,
    pub records: Vec<Record>,
    /// How many of the newest records the viewer lists; earlier ones come a
    /// page at a time.
    pub shown: usize,
}

impl View {
    /// Lines the viewer lists at first, and adds with each "earlier".
    pub const PAGE: usize = 500;

    /// Show `records` of session `name` from its newest page.
    pub fn open(&mut self, name: String, records: Vec<Record>) {
        self.open = Some(name);
        self.records = records;
        self.shown = Self::PAGE;
    }

    pub fn close(&mut self) {
        self.open = None;
        self.records.clear();
        self.shown = 0;
    }

    /// List one more page of earlier lines.
    pub fn earlier(&mut self) {
        self.shown = (self.shown + Self::PAGE).min(self.records.len());
    }

    /// The lines listed, oldest first.
    pub fn listed(&self) -> &[Record] {
        &self.records[self.hidden()..]
    }

    /// Earlier lines not listed yet.
    pub fn hidden(&self) -> usize {
        self.records.len().saturating_sub(self.shown)
    }
}

/// What a session exports as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Txt,
    Srt,
}

impl Format {
    pub fn ext(self) -> &'static str {
        match self {
            Self::Txt => "txt",
            Self::Srt => "srt",
        }
    }

    pub fn render(self, records: &[Record], offset: i64) -> String {
        match self {
            Self::Txt => txt(records, offset),
            Self::Srt => srt(records),
        }
    }
}

// ---- dates -------------------------------------------------------------

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// The inverse of [`civil`].
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from((m + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Local seconds since the epoch.
fn local_secs(unix_ms: u64, offset: i64) -> i64 {
    (unix_ms / 1000) as i64 + offset
}

/// The local day number of `unix_ms`.
fn local_day(unix_ms: u64, offset: i64) -> i64 {
    local_secs(unix_ms, offset).div_euclid(86_400)
}

/// `HH:MM:SS` local time.
fn clock(unix_ms: u64, offset: i64) -> String {
    let s = local_secs(unix_ms, offset).rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
}

/// The file stem of a session whose first line was said at `unix_ms`.
pub fn session_name(unix_ms: u64, offset: i64) -> String {
    let (y, m, d) = civil(local_day(unix_ms, offset));
    format!(
        "{y:04}-{m:02}-{d:02}_{}",
        clock(unix_ms, offset).replace(':', "")
    )
}

/// The local day a session file's name starts with.
fn day_of_name(name: &str) -> Option<i64> {
    let date = name.get(0..10)?;
    let mut parts = date.split('-');
    let y = parts.next()?.parse().ok()?;
    let m = parts.next()?.parse().ok()?;
    let d = parts.next()?.parse().ok()?;
    ((1..=12).contains(&m) && (1..=31).contains(&d)).then(|| days_from_civil(y, m, d))
}

// ---- files -------------------------------------------------------------

/// A session's file in `dir`. Names come from [`list`]; anything that could
/// leave the folder is refused.
pub fn path(dir: &Path, name: &str) -> Option<PathBuf> {
    (!name.is_empty() && !name.contains(['/', '\\', '.']))
        .then(|| dir.join(format!("{name}.{EXT}")))
}

/// Every record of a file's text, with later translations folded in. A
/// line cut short by a crash is skipped.
pub fn parse(text: &str) -> Vec<Record> {
    let mut records: Vec<Record> = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        if line.starts_with(AMEND_PREFIX) {
            let Ok(amend) = serde_json::from_str::<Amend>(line) else {
                continue;
            };
            if let Some(r) = records
                .iter_mut()
                .rev()
                .find(|r| r.at == amend.amend.at && r.speaker == amend.amend.speaker)
            {
                r.translation = Some(amend.translation);
            }
        } else if let Ok(record) = serde_json::from_str(line) {
            records.push(record);
        }
    }
    records
}

pub fn load(dir: &Path, name: &str) -> std::io::Result<Vec<Record>> {
    let path = path(dir, name).ok_or(std::io::ErrorKind::InvalidInput)?;
    Ok(parse(&std::fs::read_to_string(path)?))
}

/// The saved sessions, newest first.
pub fn list(dir: &Path) -> Vec<Session> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut sessions: Vec<Session> = entries
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension()? != EXT {
                return None;
            }
            let name = p.file_stem()?.to_str()?.to_owned();
            day_of_name(&name)?;
            let text = std::fs::read_to_string(&p).ok()?;
            Some(Session {
                name,
                lines: text
                    .lines()
                    .filter(|l| !l.trim().is_empty() && !l.starts_with(AMEND_PREFIX))
                    .count(),
            })
        })
        .collect();
    sessions.sort_by(|a, b| b.name.cmp(&a.name));
    sessions
}

/// Delete sessions that started more than `keep_days` days before today;
/// `0` keeps everything. Returns how many went.
pub fn prune(dir: &Path, keep_days: u32, now_ms: u64, offset: i64) -> usize {
    if keep_days == 0 {
        return 0;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let oldest = local_day(now_ms, offset) - i64::from(keep_days) + 1;
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == EXT))
        .filter(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .and_then(day_of_name)
                .is_some_and(|day| day < oldest)
        })
        .filter(|p| std::fs::remove_file(p).is_ok())
        .count()
}

// ---- export ------------------------------------------------------------

/// `[21:30:05] Nana: line`, one per record; later lines of a multi-line
/// record are indented under the first.
pub fn txt(records: &[Record], offset: i64) -> String {
    let mut out = String::new();
    for r in records {
        let mut head = format!("[{}] ", clock(r.at, offset));
        if !r.speaker.is_empty() {
            head.push_str(&format!("{}: ", r.speaker));
        }
        let pad = " ".repeat(head.chars().count());
        for (i, line) in r.text.lines().enumerate() {
            out.push_str(if i == 0 { &head } else { &pad });
            out.push_str(line);
            out.push('\n');
        }
        if r.text.is_empty() {
            out.push_str(head.trim_end());
            out.push('\n');
        }
        // The translation follows under the text.
        for line in r.translation.iter().flat_map(|t| t.lines()) {
            out.push_str(&pad);
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// How long a line stays up when nothing follows it soon: a beat plus a
/// reading pace, within bounds.
pub fn reading_ms(text: &str) -> u64 {
    (1200 + 80 * text.chars().count() as u64).clamp(1500, 8000)
}

/// `HH:MM:SS,mmm` from milliseconds.
fn srt_time(ms: u64) -> String {
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

/// Subtitles timed from the session's first line: each shows until the next
/// one starts, or for its reading time when that comes first.
pub fn srt(records: &[Record]) -> String {
    let Some(origin) = records.first().map(|r| r.at) else {
        return String::new();
    };
    let mut out = String::new();
    for (i, r) in records.iter().enumerate() {
        let start = r.at.saturating_sub(origin);
        let mut end = start + reading_ms(&r.text);
        if let Some(next) = records.get(i + 1).map(|n| n.at.saturating_sub(origin))
            && next > start
        {
            end = end.min(next);
        }
        let speaker = if r.speaker.is_empty() {
            String::new()
        } else {
            format!("{}: ", r.speaker)
        };
        // A translation is the subtitle's second line.
        let translation = r
            .translation
            .as_deref()
            .map(|t| format!("\n{t}"))
            .unwrap_or_default();
        out.push_str(&format!(
            "{}\n{} --> {}\n{speaker}{}{translation}\n\n",
            i + 1,
            srt_time(start),
            srt_time(end),
            r.text
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use nanatype_model::SourceId;

    fn record(at: u64, speaker: &str, text: &str) -> Record {
        Record {
            at,
            speaker: speaker.into(),
            source: Source::Own,
            platform: None,
            text: text.into(),
            markup: None,
            translation: None,
        }
    }

    fn item(kind: SourceKind, source: &str, markup: &str) -> HistoryItem {
        HistoryItem {
            source: SourceId(source.into()),
            kind,
            line: None,
            author: "七七".into(),
            avatar: None,
            doc: nanatype_markup::parse(markup).doc,
            at: 1_762_000_000_123,
            uid: None,
            reply: None,
            event: None,
        }
    }

    #[test]
    fn records_round_trip_through_their_line() {
        let own = Record::of(
            &item(SourceKind::Typewriter, SourceId::LOCAL, "[b]你好[/b]\n世界"),
            false,
        );
        assert_eq!(own.source, Source::Own);
        assert_eq!(own.text, "你好\n世界");
        assert!(own.markup.as_deref().is_some_and(|m| m.contains("[b]你好")));
        let partner = Record::of(&item(SourceKind::Typewriter, "peer:b", "hi"), true);
        assert_eq!(partner.source, Source::Partner);
        let chat = Record::of(&item(SourceKind::Chat, "twitch", "gg"), false);
        assert_eq!(
            (chat.source, chat.platform.as_deref(), chat.markup.as_ref()),
            (Source::Chat, Some("twitch"), None)
        );
        let media_item = item(
            SourceKind::Chat,
            "twitch",
            "😀[emote=blob:hash alt=\"party 🎉\"]",
        );
        let media = Record::of(&media_item, false);
        assert_eq!(media.text, "😀party 🎉");
        let media_markup = media.markup.as_deref().expect("chat media markup");
        assert_eq!(nanatype_markup::parse(media_markup).doc, media_item.doc);
        let mut file = Vec::new();
        for r in [&own, &partner, &chat, &media] {
            let line = r.line();
            assert_eq!(line.iter().filter(|b| **b == b'\n').count(), 1);
            file.extend(line);
        }
        // A crash can leave half a line at the end.
        file.extend(br#"{"at":1,"speak"#);
        let text = String::from_utf8(file).unwrap();
        assert_eq!(parse(&text), vec![own.clone(), partner, chat, media]);
        let json = String::from_utf8(own.line()).unwrap();
        assert!(json.contains(r#""source":"self""#), "{json}");
        assert!(!json.contains("platform"), "{json}");
    }

    #[test]
    fn translations_join_their_records_and_older_files_still_read() {
        // A file from before translations: no field, still read.
        let old = r#"{"at":1,"speaker":"Nana","source":"self","text":"你好"}"#;
        let records = parse(old);
        assert_eq!(records[0].translation, None);
        assert!(
            !String::from_utf8(records[0].line())
                .unwrap()
                .contains("translation")
        );

        let inline = record(2, "Nana", "早上好").translated(Some("Good morning"));
        let mut file = String::from(old);
        file.push('\n');
        file.push_str(&String::from_utf8(inline.line()).unwrap());
        let late = Amend {
            amend: Key {
                at: 1,
                speaker: "Nana".into(),
            },
            translation: "Hello".into(),
        };
        let late = String::from_utf8(late.line()).unwrap();
        assert!(late.starts_with(AMEND_PREFIX), "{late}");
        file.push_str(&late);
        // One for a record that is not there changes nothing.
        file.push_str(r#"{"amend":{"at":9,"speaker":"x"},"translation":"lost"}"#);
        file.push('\n');
        let records = parse(&file);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].translation.as_deref(), Some("Hello"));
        assert_eq!(records[1].translation.as_deref(), Some("Good morning"));
        // A build without translations reads the amendment as nothing.
        assert!(serde_json::from_str::<Record>(late.trim()).is_err());

        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("2026-11-02_120000.jsonl"), &file).unwrap();
        assert_eq!(list(tmp.path())[0].lines, 2, "amendments are not lines");

        assert_eq!(
            txt(&records[1..], 0),
            "[00:00:00] Nana: 早上好\n                 Good morning\n"
        );
        assert_eq!(
            srt(&records[1..]),
            "1\n00:00:00,000 --> 00:00:01,500\nNana: 早上好\nGood morning\n\n"
        );
    }

    #[test]
    fn the_viewer_pages_back_through_a_whole_session() {
        let records: Vec<Record> = (0..1203).map(|i| record(i, "Nana", "hi")).collect();
        let mut view = View::default();
        view.open("s".into(), records);
        assert_eq!(view.listed().len(), View::PAGE);
        assert_eq!(view.listed().last().unwrap().at, 1202, "the newest first");
        assert_eq!(view.hidden(), 703);
        view.earlier();
        assert_eq!(view.hidden(), 203);
        view.earlier();
        assert_eq!(view.hidden(), 0);
        assert_eq!(view.listed().len(), 1203);
        assert_eq!(view.listed()[0].at, 0, "the whole session");
        view.earlier();
        assert_eq!(view.listed().len(), 1203);
        // Another session starts from its newest page again.
        view.open(
            "t".into(),
            (0..10).map(|i| record(i, "Nana", "hi")).collect(),
        );
        assert_eq!((view.listed().len(), view.hidden()), (10, 0));
        view.close();
        assert!(view.listed().is_empty());
    }

    #[test]
    fn dates_convert_both_ways() {
        for days in [-719_468, -1, 0, 59, 365 * 30 + 7, 20_394, 40_000] {
            let (y, m, d) = civil(days);
            assert_eq!(days_from_civil(y, m, d), days, "{y}-{m}-{d}");
        }
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_394), (2025, 11, 2));
        // 2026-11-02 21:30:05 UTC, seen from UTC+9 the next morning.
        let at = days_from_civil(2026, 11, 2) as u64 * 86_400_000 + 77_405_000;
        assert_eq!(session_name(at, 0), "2026-11-02_213005");
        assert_eq!(session_name(at, 9 * 3600), "2026-11-03_063005");
        assert_eq!(
            day_of_name("2026-11-02_213005"),
            Some(days_from_civil(2026, 11, 2))
        );
        assert_eq!(day_of_name("notes"), None);
        let s = Session {
            name: "2026-11-02_213005".into(),
            lines: 3,
        };
        assert_eq!(s.title(), "2026-11-02 21:30");
    }

    #[test]
    fn paths_stay_in_the_folder() {
        let dir = Path::new("history");
        assert_eq!(
            path(dir, "2026-11-02_213005"),
            Some(dir.join("2026-11-02_213005.jsonl"))
        );
        for bad in ["", "../settings", "a/b", "a\\b", "x.json"] {
            assert_eq!(path(dir, bad), None, "{bad}");
        }
    }

    #[test]
    fn pruning_keeps_the_last_days_and_other_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let today = days_from_civil(2026, 11, 2);
        let name = |days_ago: i64| {
            let (y, m, d) = civil(today - days_ago);
            format!("{y:04}-{m:02}-{d:02}_120000.jsonl")
        };
        for days_ago in [0, 1, 29, 30, 31, 400] {
            std::fs::write(dir.join(name(days_ago)), record(1, "a", "b").line()).unwrap();
        }
        std::fs::write(dir.join("notes.txt"), "mine").unwrap();
        std::fs::write(dir.join("2020-01-01_000000.txt"), "not a session").unwrap();
        let now = today as u64 * 86_400_000 + 3_600_000;
        assert_eq!(prune(dir, 0, now, 0), 0, "0 keeps everything");
        assert_eq!(prune(dir, 30, now, 0), 3);
        let left: Vec<String> = list(dir).into_iter().map(|s| s.name).collect();
        let stem = |d| name(d).trim_end_matches(".jsonl").to_owned();
        assert_eq!(left, vec![stem(0), stem(1), stem(29)]);
        assert!(dir.join("notes.txt").exists());
        assert!(dir.join("2020-01-01_000000.txt").exists());
        assert_eq!(list(dir)[0].lines, 1);
        assert_eq!(load(dir, &stem(0)).unwrap(), vec![record(1, "a", "b")]);
    }

    #[test]
    fn text_export_puts_each_line_after_its_time_and_speaker() {
        let records = [
            record(77_405_000, "Nana", "hello\nagain"),
            record(77_410_000, "", "…"),
        ];
        assert_eq!(
            txt(&records, 0),
            "[21:30:05] Nana: hello\n                 again\n[21:30:10] …\n"
        );
    }

    #[test]
    fn subtitles_run_from_the_first_line_until_the_next() {
        let base = 1_762_000_000_000;
        let records = [
            record(base, "Nana", "hi"),
            // Long after: the first stops at its reading time.
            record(base + 60_000, "Bob", "hello there"),
            // Soon after: the second stops when this starts.
            record(base + 61_000, "", "ok"),
        ];
        assert_eq!(
            srt(&records),
            "1\n00:00:00,000 --> 00:00:01,500\nNana: hi\n\n\
             2\n00:01:00,000 --> 00:01:01,000\nBob: hello there\n\n\
             3\n00:01:01,000 --> 00:01:02,500\nok\n\n"
        );
        assert_eq!(srt(&[]), "");
        assert_eq!(srt_time(3_723_004), "01:02:03,004");
        assert_eq!(reading_ms(&"字".repeat(200)), 8000);
    }
}
