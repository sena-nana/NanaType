//! Atomic JSON files. A write goes to a temporary sibling and is renamed over
//! the target, keeping the previous good file as `.bak`. A file that does not
//! parse is moved aside as `*.corrupt-<unix-ms>.json` and reported; it is
//! never silently replaced by defaults.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::migrate::{MigrateError, migrate};
use crate::schema::{DiagnosticsSettings, Settings};

#[derive(Debug)]
pub enum LoadNote {
    /// No file yet: first run.
    Fresh,
    Loaded,
    /// Upgraded from an older schema; save to persist the new shape.
    Migrated {
        from: u32,
    },
    /// The file was unreadable and moved aside; defaults are in use.
    Quarantined {
        moved_to: PathBuf,
        reason: String,
    },
    /// Written by a newer NanaType. Loaded read-only: saving is refused so
    /// the newer file is not downgraded.
    TooNew {
        schema: u32,
    },
}

#[derive(Debug)]
pub struct Loaded<T> {
    pub value: T,
    pub note: LoadNote,
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    if path.exists() {
        let _ = fs::copy(path, path.with_extension("json.bak"));
    }
    fs::rename(&tmp, path)
}

pub fn save_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let text = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    write_atomic(path, &text)
}

fn quarantine(path: &Path, reason: String) -> LoadNote {
    let stamp = nanatype_model::unix_millis();
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "settings".into());
    let moved_to = path.with_file_name(format!("{stem}.corrupt-{stamp}.json"));
    match fs::rename(path, &moved_to) {
        Ok(()) => LoadNote::Quarantined { moved_to, reason },
        Err(e) => LoadNote::Quarantined {
            moved_to: path.to_path_buf(),
            reason: format!("{reason} (could not move it aside: {e})"),
        },
    }
}

/// Load a plain JSON document with defaults for missing fields.
pub fn load_json<T: DeserializeOwned + Default>(path: &Path) -> Loaded<T> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Loaded {
                value: T::default(),
                note: LoadNote::Fresh,
            };
        }
        Err(e) => {
            return Loaded {
                value: T::default(),
                note: quarantine(path, e.to_string()),
            };
        }
    };
    match serde_json::from_str(&text) {
        Ok(value) => Loaded {
            value,
            note: LoadNote::Loaded,
        },
        Err(e) => Loaded {
            value: T::default(),
            note: quarantine(path, e.to_string()),
        },
    }
}

/// The logging settings in `path`, read without migrating, quarantining or
/// writing anything: the logs start before the settings are loaded properly.
pub fn peek_diagnostics(path: &Path) -> DiagnosticsSettings {
    #[derive(Default, serde::Deserialize)]
    #[serde(default)]
    struct Peek {
        diagnostics: DiagnosticsSettings,
    }
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Peek>(&text).ok())
        .unwrap_or_default()
        .diagnostics
}

/// Load settings from `path`, falling back to `legacy` (the v0 location) on
/// first run.
pub fn load_settings(path: &Path, legacy: Option<&Path>) -> Loaded<Settings> {
    let source = if path.exists() {
        path.to_path_buf()
    } else if let Some(legacy) = legacy.filter(|p| p.exists()) {
        legacy.to_path_buf()
    } else {
        // New installations show both sides of the conversation. Deserializing
        // older files and recovery still use the historical opt-in default.
        let mut value = Settings::default();
        value.history.chat.enabled = true;
        return Loaded {
            value,
            note: LoadNote::Fresh,
        };
    };
    let from_legacy = source != path;
    let text = match fs::read_to_string(&source) {
        Ok(t) => t,
        Err(e) => {
            return Loaded {
                value: Settings::default(),
                note: if from_legacy {
                    LoadNote::Fresh
                } else {
                    quarantine(path, e.to_string())
                },
            };
        }
    };
    let mut value: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            return Loaded {
                value: Settings::default(),
                note: if from_legacy {
                    LoadNote::Fresh
                } else {
                    quarantine(path, e.to_string())
                },
            };
        }
    };
    let from = match migrate(&mut value) {
        Ok(from) => from,
        Err(MigrateError::TooNew(schema)) => {
            let mut settings: Settings = serde_json::from_value(value).unwrap_or_default();
            settings.sanitize();
            return Loaded {
                value: settings,
                note: LoadNote::TooNew { schema },
            };
        }
        Err(MigrateError::NotAnObject) => {
            return Loaded {
                value: Settings::default(),
                note: quarantine(path, "settings are not a JSON object".into()),
            };
        }
    };
    match serde_json::from_value::<Settings>(value) {
        Ok(mut settings) => {
            settings.sanitize();
            settings.schema = crate::SETTINGS_SCHEMA;
            let note = if from < crate::SETTINGS_SCHEMA || from_legacy {
                LoadNote::Migrated { from }
            } else {
                LoadNote::Loaded
            };
            Loaded {
                value: settings,
                note,
            }
        }
        Err(e) => Loaded {
            value: Settings::default(),
            note: if from_legacy {
                LoadNote::Fresh
            } else {
                quarantine(path, e.to_string())
            },
        },
    }
}
