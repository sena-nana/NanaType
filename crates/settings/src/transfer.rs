//! Moving settings between installs, resetting them, and named snapshots of
//! them (profiles).
//!
//! An export is a JSON envelope (see [`Envelope`]) around the settings
//! document, so an import goes through the same migrations as
//! `settings.json`. Secrets live in the system's secret store and never
//! reach settings.json; the few credentials that do (VTube Studio's plugin
//! token, the 彗星号 page token, a proxy's `user:password@`) are left out of
//! exports as well, and an import keeps this install's.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::geometry::GeometryStore;
use crate::migrate::{MigrateError, migrate};
use crate::schema::{SETTINGS_SCHEMA, Settings};

/// The `app` of every export.
pub const EXPORT_APP: &str = "NanaType";
/// The envelope's own version, apart from the settings schema inside it.
pub const EXPORT_FORMAT: u32 = 1;
/// Avatars larger than this stay behind (and are counted as missing).
pub const MAX_AVATAR_BYTES: u64 = 4 * 1024 * 1024;
/// The folder beside settings.json that holds the profiles.
pub const PROFILE_DIR: &str = "profiles";
/// Where the settings before the last import, reset or restore are kept.
pub const PREVIOUS_FILE: &str = "settings.previous.json";

/// Never exported, and kept from this install on import: credentials that
/// sit in settings.json, and what belongs to this computer (plugin consent,
/// devices, GPU and log choices, update reminders, the active profile).
const LOCAL: &[&str] = &[
    "/enabled_plugins",
    "/disabled_packages",
    "/plugin_approvals",
    "/integrations/vts/token",
    "/update/skipped_version",
    "/update/last_check",
    "/diagnostics",
    "/performance",
    "/sound/device",
    "/speech/device",
    "/general/profile",
];

/// Exported only when integrations are included: services, their hosts,
/// ports and channels.
const INTEGRATIONS: &[&str] = &[
    "/integrations",
    "/output/obs",
    "/assets/telegram",
    "/translate/base_url",
];

/// Proxies whose `user:password@` is dropped from exports.
const PROXIES: &[&str] = &[
    "/update/proxy",
    "/assets/telegram/proxy",
    "/translate/proxy",
];

/// What "reset to defaults" keeps: credentials, the language and the
/// finished first-run guide.
const KEPT_ON_RESET: &[&str] = &[
    "/integrations/vts/token",
    "/general/locale",
    "/general/onboarding_done",
    "/diagnostics/crash_seen",
];

/// What a profile holds: how a stream looks, sounds and reads — the
/// characters, playback, sounds, the skin and its adjustments, the overlay
/// windows (visibility, lock, size; not their positions on screen), the
/// history overlay and the quick phrases. Everything else is global: the
/// language, hotkeys, input behaviour, integrations and their endpoints,
/// outputs, speech input and text to speech (each character's voice
/// travels with the characters), translation (its provider, language and
/// where subtitles show; the API keys are in the system's secret store and
/// never exported), updates, autostart, plugins and devices.
pub const PROFILE_SCOPE: &[&str] = &[
    "/general/characters",
    "/general/character",
    "/playback",
    "/sound",
    "/skin",
    "/overlays",
    "/history",
    "/assets/phrases",
];

/// Inside [`PROFILE_SCOPE`] but global all the same: the output device, the
/// saved history, the settings window's own look and skin hot reload.
const PROFILE_EXCEPT: &[&str] = &[
    "/sound/device",
    "/history/archive",
    "/skin/chrome",
    "/skin/hot_reload",
];

/// What an export carries besides the settings themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportOptions {
    /// Window positions per monitor layout (geometry.json). They rarely fit
    /// another computer's monitors, so they stay behind unless asked for.
    pub window_positions: bool,
    /// Integration hosts, ports and channels.
    pub integrations: bool,
    /// The characters' avatar images, embedded in the file.
    pub avatars: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            window_positions: false,
            integrations: true,
            avatars: true,
        }
    }
}

/// A file carried inside an export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddedFile {
    /// The original file name; only its extension is used on import.
    pub name: String,
    /// Standard padded base64.
    pub data: String,
}

/// An exported settings file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    /// Always [`EXPORT_APP`].
    pub app: String,
    /// [`EXPORT_FORMAT`] when written.
    pub format: u32,
    /// The settings schema of `settings`.
    pub schema: u32,
    /// The NanaType version that wrote it.
    #[serde(default)]
    pub version: String,
    /// Unix milliseconds.
    #[serde(default)]
    pub exported_at: u64,
    #[serde(default)]
    pub includes: ExportOptions,
    /// The settings document, as in settings.json.
    pub settings: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<GeometryStore>,
    /// Character index → its avatar.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub avatars: BTreeMap<usize, EmbeddedFile>,
}

/// An export ready to write.
#[derive(Debug)]
pub struct Exported {
    pub envelope: Envelope,
    /// Avatars asked for that could not be read or were too large.
    pub missing_avatars: usize,
}

/// `settings` as an export file. Reads the avatar files when asked to.
pub fn export(
    settings: &Settings,
    geometry: &GeometryStore,
    options: ExportOptions,
    version: &str,
    now: u64,
) -> Exported {
    let mut doc = serde_json::to_value(settings).unwrap_or_else(|_| Value::Object(Map::new()));
    for path in LOCAL {
        remove(&mut doc, path);
    }
    if !options.integrations {
        for path in INTEGRATIONS {
            remove(&mut doc, path);
        }
    }
    for path in PROXIES {
        if let Some(Value::String(url)) = doc.pointer_mut(path) {
            *url = without_userinfo(url);
        }
    }
    let mut avatars = BTreeMap::new();
    let mut missing_avatars = 0;
    if options.avatars {
        for (i, c) in settings.general.characters.iter().enumerate() {
            let Some(path) = c.avatar.as_deref().map(Path::new) else {
                continue;
            };
            let read = std::fs::metadata(path)
                .ok()
                .filter(|m| m.is_file() && m.len() <= MAX_AVATAR_BYTES)
                .and_then(|_| std::fs::read(path).ok());
            match read {
                Some(bytes) => {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
                    avatars.insert(i, EmbeddedFile { name, data });
                }
                None => missing_avatars += 1,
            }
        }
    }
    Exported {
        envelope: Envelope {
            app: EXPORT_APP.into(),
            format: EXPORT_FORMAT,
            schema: SETTINGS_SCHEMA,
            version: version.into(),
            exported_at: now,
            includes: options,
            settings: doc,
            geometry: options.window_positions.then(|| geometry.clone()),
            avatars,
        },
        missing_avatars,
    }
}

/// `scheme://user:password@host` without `user:password@`.
fn without_userinfo(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_owned();
    };
    let authority_end = rest.find('/').unwrap_or(rest.len());
    match rest[..authority_end].rfind('@') {
        Some(at) => format!("{scheme}://{}", &rest[at + 1..]),
        None => url.to_owned(),
    }
}

/// Why a file cannot be imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// Not JSON, or settings that do not read.
    Unreadable(String),
    /// JSON, but not NanaType settings.
    NotSettings,
    /// Settings from a newer NanaType (their schema).
    TooNew(u32),
    /// An envelope from a newer NanaType (its format).
    FormatTooNew(u32),
}

/// A checked, migrated import waiting for confirmation.
#[derive(Debug, Clone)]
pub struct Incoming {
    /// The settings document, migrated to [`SETTINGS_SCHEMA`].
    pub settings: Value,
    /// The schema it was written in.
    pub from_schema: u32,
    pub includes: ExportOptions,
    /// Unix milliseconds; `None` for a bare settings.json.
    pub exported_at: Option<u64>,
    pub version: String,
    pub geometry: Option<GeometryStore>,
    pub avatars: BTreeMap<usize, EmbeddedFile>,
}

/// Check and migrate an export, or a bare settings.json.
pub fn parse_import(text: &str) -> Result<Incoming, ImportError> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| ImportError::Unreadable(e.to_string()))?;
    let obj = value.as_object().ok_or(ImportError::NotSettings)?;
    let mut incoming = if obj.contains_key("app") {
        if obj.get("app").and_then(Value::as_str) != Some(EXPORT_APP) {
            return Err(ImportError::NotSettings);
        }
        let format = obj.get("format").and_then(Value::as_u64).unwrap_or(0) as u32;
        if format > EXPORT_FORMAT {
            return Err(ImportError::FormatTooNew(format));
        }
        let envelope: Envelope =
            serde_json::from_value(value).map_err(|e| ImportError::Unreadable(e.to_string()))?;
        if !envelope.settings.is_object() {
            return Err(ImportError::NotSettings);
        }
        Incoming {
            settings: envelope.settings,
            from_schema: 0,
            includes: envelope.includes,
            exported_at: Some(envelope.exported_at),
            version: envelope.version,
            geometry: envelope.geometry,
            avatars: envelope.avatars,
        }
    } else if obj.contains_key("schema") || obj.contains_key("general") {
        // A settings.json copied by hand carries everything it has.
        Incoming {
            settings: value,
            from_schema: 0,
            includes: ExportOptions {
                window_positions: false,
                integrations: true,
                avatars: false,
            },
            exported_at: None,
            version: String::new(),
            geometry: None,
            avatars: BTreeMap::new(),
        }
    } else {
        return Err(ImportError::NotSettings);
    };
    incoming.from_schema = match migrate(&mut incoming.settings) {
        Ok(from) => from,
        Err(MigrateError::TooNew(schema)) => return Err(ImportError::TooNew(schema)),
        Err(MigrateError::NotAnObject) => return Err(ImportError::NotSettings),
    };
    serde_json::from_value::<Settings>(incoming.settings.clone())
        .map_err(|e| ImportError::Unreadable(e.to_string()))?;
    Ok(incoming)
}

impl Incoming {
    /// The settings after the import: what the file carries over `current`,
    /// keeping this install's credentials and computer-bound state, and its
    /// integrations when the file has none.
    pub fn merge(&self, current: &Settings) -> Settings {
        let here = to_value(current);
        let mut next = self.settings.clone();
        let mut keep: Vec<&str> = LOCAL.to_vec();
        if !self.includes.integrations {
            keep.extend(INTEGRATIONS);
        }
        for path in keep {
            graft(&mut next, &here, path);
        }
        from_value(next)
    }

    /// Write the embedded avatars into `dir` and point their characters at
    /// them. Returns how many were written.
    pub fn unpack_avatars(
        &self,
        dir: &Path,
        settings: &mut Settings,
        now: u64,
    ) -> std::io::Result<usize> {
        let mut written = 0;
        for (i, file) in &self.avatars {
            let Some(character) = settings.general.characters.get_mut(*i) else {
                continue;
            };
            let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&file.data) else {
                continue;
            };
            if bytes.len() as u64 > MAX_AVATAR_BYTES {
                continue;
            }
            let ext = Path::new(&file.name)
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase)
                .filter(|e| ["png", "jpg", "jpeg", "gif", "webp", "bmp"].contains(&e.as_str()))
                .unwrap_or_else(|| "png".into());
            std::fs::create_dir_all(dir)?;
            let path = dir.join(format!("imported-{now}-{i}.{ext}"));
            std::fs::write(&path, bytes)?;
            character.avatar = Some(path.display().to_string());
            written += 1;
        }
        Ok(written)
    }
}

/// The settings sections that differ, by their key in settings.json.
pub fn changed_sections(before: &Settings, after: &Settings) -> Vec<&'static str> {
    let (a, b) = (to_value(before), to_value(after));
    [
        "general",
        "playback",
        "sound",
        "speech",
        "tts",
        "translate",
        "skin",
        "overlays",
        "history",
        "hotkeys",
        "output",
        "integrations",
        "assets",
        "update",
        "enabled_plugins",
        "disabled_packages",
    ]
    .into_iter()
    .filter(|k| a.get(k) != b.get(k))
    .collect()
}

impl Settings {
    /// A new installation's settings. Older files and recovery keep the
    /// historical defaults; only a fresh start shows both sides of the
    /// conversation in the history.
    pub fn fresh() -> Self {
        let mut s = Self::default();
        s.history.chat.enabled = true;
        s
    }

    /// Everything back to [`Self::fresh`], but for credentials, the language
    /// and the finished first-run guide.
    pub fn reset_from(&self) -> Self {
        let here = to_value(self);
        let mut next = to_value(&Self::fresh());
        for path in KEPT_ON_RESET {
            graft(&mut next, &here, path);
        }
        from_value(next)
    }
}

/// A named snapshot of the profile-scoped settings ([`PROFILE_SCOPE`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    /// Its file stem in [`PROFILE_DIR`].
    pub id: String,
    pub name: String,
    /// The scoped sections, settings-shaped and at [`SETTINGS_SCHEMA`].
    pub settings: Value,
}

/// A profile on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileFile {
    pub name: String,
    /// Unix milliseconds.
    #[serde(default)]
    pub saved_at: u64,
    /// The scoped sections, with their `schema`.
    pub settings: Value,
}

impl Profile {
    /// A profile of `settings` as they are now.
    pub fn capture(id: String, name: String, settings: &Settings) -> Self {
        Self {
            id,
            name,
            settings: profile_sections(settings),
        }
    }

    /// `current` with this profile's sections.
    pub fn apply(&self, current: &Settings) -> Settings {
        let here = to_value(current);
        let mut next = here.clone();
        for path in PROFILE_SCOPE {
            graft(&mut next, &self.settings, path);
        }
        for path in PROFILE_EXCEPT {
            graft(&mut next, &here, path);
        }
        from_value(next)
    }

    pub fn file(&self, now: u64) -> ProfileFile {
        ProfileFile {
            name: self.name.clone(),
            saved_at: now,
            settings: self.settings.clone(),
        }
    }

    pub fn path(dir: &Path, id: &str) -> PathBuf {
        dir.join(format!("{id}.json"))
    }
}

/// The profile-scoped part of `settings`, with its schema.
pub fn profile_sections(settings: &Settings) -> Value {
    let here = to_value(settings);
    let mut out = Value::Object(Map::new());
    out["schema"] = SETTINGS_SCHEMA.into();
    for path in PROFILE_SCOPE {
        graft(&mut out, &here, path);
    }
    for path in PROFILE_EXCEPT {
        remove(&mut out, path);
    }
    out
}

/// The profiles in `dir`, by name. A damaged file, or one from a newer
/// NanaType, is left alone and named in the second list.
pub fn load_profiles(dir: &Path) -> (Vec<Profile>, Vec<String>) {
    let mut profiles = Vec::new();
    let mut skipped = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (profiles, skipped);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(id) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| valid_profile_id(s))
        else {
            continue;
        };
        match read_profile(&path) {
            Ok((name, settings)) => profiles.push(Profile {
                id: id.to_owned(),
                name,
                settings,
            }),
            Err(e) => skipped.push(format!("{}: {e:?}", path.display())),
        }
    }
    profiles.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    (profiles, skipped)
}

fn read_profile(path: &Path) -> Result<(String, Value), ImportError> {
    let text = std::fs::read_to_string(path).map_err(|e| ImportError::Unreadable(e.to_string()))?;
    let file: ProfileFile =
        serde_json::from_str(&text).map_err(|e| ImportError::Unreadable(e.to_string()))?;
    let mut settings = file.settings;
    match migrate(&mut settings) {
        Ok(_) => {}
        Err(MigrateError::TooNew(schema)) => return Err(ImportError::TooNew(schema)),
        Err(MigrateError::NotAnObject) => return Err(ImportError::NotSettings),
    }
    serde_json::from_value::<Settings>(settings.clone())
        .map_err(|e| ImportError::Unreadable(e.to_string()))?;
    Ok((file.name, settings))
}

/// Profile ids are file stems: short, lowercase and safe on every system.
pub fn valid_profile_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A fresh profile id, not in `taken`.
pub fn new_profile_id(now: u64, taken: impl Fn(&str) -> bool) -> String {
    (0u64..)
        .map(|n| format!("p{:x}", now.wrapping_add(n)))
        .find(|id| !taken(id))
        .expect("an unused id")
}

fn to_value(s: &Settings) -> Value {
    serde_json::to_value(s).unwrap_or_else(|_| Value::Object(Map::new()))
}

/// Settings from a document known to deserialize, sanitized.
fn from_value(v: Value) -> Settings {
    let mut s: Settings = serde_json::from_value(v).unwrap_or_default();
    s.sanitize();
    s.schema = SETTINGS_SCHEMA;
    s
}

/// Make `dst` at `pointer` what `src` has there: copied when present,
/// removed when not.
fn graft(dst: &mut Value, src: &Value, pointer: &str) {
    match src.pointer(pointer) {
        Some(v) => set(dst, pointer, v.clone()),
        None => remove(dst, pointer),
    }
}

fn set(dst: &mut Value, pointer: &str, value: Value) {
    let mut at = dst;
    let mut keys = pointer.split('/').skip(1).peekable();
    while let Some(key) = keys.next() {
        if !at.is_object() {
            *at = Value::Object(Map::new());
        }
        let obj = at.as_object_mut().expect("an object");
        if keys.peek().is_none() {
            obj.insert(key.to_owned(), value);
            return;
        }
        at = obj.entry(key).or_insert_with(|| Value::Object(Map::new()));
    }
}

fn remove(dst: &mut Value, pointer: &str) {
    let Some((parent, key)) = pointer.rsplit_once('/') else {
        return;
    };
    let parent = if parent.is_empty() {
        Some(dst)
    } else {
        dst.pointer_mut(parent)
    };
    if let Some(Value::Object(obj)) = parent {
        obj.remove(key);
    }
}
