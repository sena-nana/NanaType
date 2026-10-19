//! Where NanaType keeps its files.

use std::path::PathBuf;

pub const APP_ID: &str = "dev.sena-nana.nanatype";

#[derive(Debug, Clone)]
pub struct Paths {
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
    pub logs: PathBuf,
}

impl Paths {
    pub fn resolve() -> Self {
        if let Some(p) = nana_ui::NanaApplication::paths() {
            return Self {
                config: p.config().to_path_buf(),
                data: p.data().to_path_buf(),
                cache: p.cache().to_path_buf(),
                logs: p.logs().to_path_buf(),
            };
        }
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| PathBuf::from("."))
            .join(APP_ID);
        Self {
            config: base.join("config"),
            data: base.join("data"),
            cache: base.join("cache"),
            logs: base.join("logs"),
        }
    }

    pub fn settings(&self) -> PathBuf {
        self.config.join("settings.json")
    }

    pub fn geometry(&self) -> PathBuf {
        self.config.join("geometry.json")
    }

    pub fn packages(&self) -> PathBuf {
        self.data.join("packages")
    }

    /// Unfinished package editor projects.
    pub fn package_drafts(&self) -> PathBuf {
        self.data.join("package-drafts")
    }

    pub fn blobs(&self) -> PathBuf {
        self.cache.join("images")
    }

    /// Settings written by the first NanaType build.
    pub fn legacy_settings() -> Option<PathBuf> {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("NanaType").join("settings.json"))
    }

    pub fn ensure(&self) {
        for dir in [
            &self.config,
            &self.data,
            &self.cache,
            &self.logs,
            &self.packages(),
            &self.blobs(),
        ] {
            let _ = std::fs::create_dir_all(dir);
        }
    }
}
