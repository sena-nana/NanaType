//! Pictures NanaUI is allowed to load.
//!
//! NanaUI reads a local `url(...)` picture only when the file canonicalizes
//! under one directory, the image URL base (`nana_ui::set_background_image_url_base`;
//! the first call wins for the whole process, and without one it is the
//! working directory). NanaType makes its data folder that base. Skin artwork
//! uses immutable content URLs there so in-place edits invalidate image caches.
//! Pictures outside that base (sticker folders, chat avatars saved in the cache
//! folder, files the user picked) are copied once into
//! `<data>/media/<hash>.<ext>` and the copy's URL is handed out instead.
//!
//! The copy is named after the source's canonical path, size and modified
//! time, so an edited picture gets a new name (and a new URL, which NanaUI's
//! texture cache has not seen). Copies nobody used for [`Limits::max_age`]
//! go, and the oldest go first when the folder grows past
//! [`Limits::max_total`]; active copies stay pinned until explicitly retired.

use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::assets::file_url;

/// NanaUI refuses local pictures larger than this (`MAX_LOCAL_URL_BYTES`).
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// The folder under the base that holds the copies.
pub const DIR: &str = "media";

/// Extensions copied into the mirror; anything else outside the base is
/// not a picture NanaType shows.
const PICTURE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"];

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_file: u64,
    pub max_total: u64,
    pub max_age: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_file: MAX_FILE_BYTES,
            max_total: 256 * 1024 * 1024,
            max_age: Duration::from_secs(30 * 24 * 3600),
        }
    }
}

#[derive(Debug)]
pub struct Media {
    /// Canonical base (as `fs::canonicalize` spells it, for comparisons).
    base: PathBuf,
    /// Canonical `<base>/media`.
    dir: PathBuf,
    limits: Limits,
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    /// Bytes in `dir`, as of the last prune plus copies since.
    total: u64,
    /// Copies handed out this session; pinned until explicitly retired.
    used: HashSet<String>,
}

impl Media {
    /// A mirror for `base`, creating `base` and its media folder.
    pub fn new(base: &Path, limits: Limits) -> std::io::Result<Self> {
        fs::create_dir_all(base.join(DIR))?;
        let base = fs::canonicalize(base)?;
        let dir = fs::canonicalize(base.join(DIR))?;
        let media = Self {
            base,
            dir,
            limits,
            state: Mutex::new(State::default()),
        };
        media.prune();
        Ok(media)
    }

    /// The base to hand NanaUI: canonical, in its plain spelling.
    pub fn base(&self) -> PathBuf {
        plain(&self.base)
    }

    /// The folder holding the copies.
    pub fn dir(&self) -> PathBuf {
        plain(&self.dir)
    }

    /// A `file://` URL NanaUI will load for the picture at `path`, or `None`
    /// when it cannot be shown. With `root` (the folder the user chose: a
    /// sticker folder, a skin), `path` must stay inside it after symlinks and
    /// junctions are resolved.
    pub fn url(&self, path: &Path, root: Option<&Path>) -> Option<String> {
        self.path(path, root).map(|p| file_url(&p))
    }

    /// Materialize packaged artwork inside NanaUI's allowed image base.
    /// Content keys keep hot reloads and application updates out of stale caches.
    pub fn bytes_url(&self, path: &str, bytes: &[u8]) -> Option<String> {
        let ext = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
        if !PICTURE_EXTS.contains(&ext.as_str())
            || bytes.is_empty()
            || bytes.len() as u64 > self.limits.max_file
        {
            return None;
        }
        let name = format!("embedded-b3-{}.{ext}", blake3::hash(bytes));
        self.cache(&name, bytes.len() as u64, || Some(bytes.to_vec()))
            .map(|path| file_url(&path))
    }

    /// The file NanaUI should read for `path`: itself when it is under the
    /// base, else its copy in the mirror.
    pub fn path(&self, path: &Path, root: Option<&Path>) -> Option<PathBuf> {
        if looks_network(path) {
            return None;
        }
        let canonical = fs::canonicalize(path).ok()?;
        let meta = fs::metadata(&canonical).ok()?;
        if !meta.is_file() {
            return None;
        }
        if let Some(root) = root {
            let root = fs::canonicalize(root).ok()?;
            if !canonical.starts_with(&root) {
                return None;
            }
        }
        if canonical.starts_with(&self.base) {
            return Some(plain(&canonical));
        }
        if meta.len() > self.limits.max_file {
            return None;
        }
        let ext = canonical
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .filter(|e| PICTURE_EXTS.contains(&e.as_str()))?;
        let name = format!(
            "{}.{ext}",
            mirror_key(&canonical, meta.len(), modified(&meta))
        );
        self.cache(&name, meta.len(), || {
            let mut bytes = Vec::with_capacity(meta.len() as usize);
            fs::File::open(&canonical)
                .ok()?
                .take(self.limits.max_file + 1)
                .read_to_end(&mut bytes)
                .ok()?;
            // Reject files changed since the cache key was computed.
            (bytes.len() as u64 == meta.len()).then_some(bytes)
        })
    }

    fn cache(
        &self,
        name: &str,
        len: u64,
        read: impl FnOnce() -> Option<Vec<u8>>,
    ) -> Option<PathBuf> {
        let target = self.dir.join(name);
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if fs::metadata(&target).is_ok_and(|m| m.is_file() && m.len() == len) {
            if state.used.insert(name.to_owned()) {
                touch(&target);
            }
            return Some(plain(&target));
        }
        let bytes = read()?;
        let partial = self.dir.join(format!("{name}.part"));
        let written = fs::create_dir_all(&self.dir)
            .and_then(|()| fs::write(&partial, &bytes))
            .and_then(|()| fs::rename(&partial, &target));
        if let Err(e) = written {
            let _ = fs::remove_file(&partial);
            crate::debug_log(&format!("media cache {name}: {e}"));
            return None;
        }
        state.used.insert(name.to_owned());
        state.total += len;
        if state.total > self.limits.max_total {
            self.prune_locked(&mut state);
        }
        Some(plain(&target))
    }

    /// Drop copies unused for `max_age`, then the oldest until the folder
    /// fits `max_total`. Copies handed out this session stay.
    pub fn prune(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        self.prune_locked(&mut state);
    }

    /// Obsolete skin revisions can leave the session pin set once none of
    /// the current views/catalogue references their URLs. Deletion still
    /// follows the normal age/size policy, outside the transition itself.
    pub fn retire_urls(&self, previous: impl IntoIterator<Item = String>, keep: &HashSet<String>) {
        let prefix = format!("{}/", file_url(&self.dir()));
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        for url in previous {
            if !keep.contains(&url)
                && let Some(name) = url.strip_prefix(&prefix)
            {
                state.used.remove(name);
            }
        }
    }

    fn prune_locked(&self, state: &mut State) {
        let now = SystemTime::now();
        let age = |t: SystemTime| now.duration_since(t).unwrap_or(Duration::ZERO);
        let mut files: Vec<(String, u64, SystemTime)> = Vec::new();
        for entry in fs::read_dir(&self.dir).into_iter().flatten().flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let mtime = meta.modified().unwrap_or(now);
            if name.ends_with(".part") {
                if age(mtime) > Duration::from_secs(3600) {
                    let _ = fs::remove_file(entry.path());
                }
                continue;
            }
            files.push((name, meta.len(), mtime));
        }
        files.sort_by_key(|f| f.2);
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        for (name, len, mtime) in files {
            if state.used.contains(&name) {
                continue;
            }
            if (age(mtime) > self.limits.max_age || total > self.limits.max_total)
                && fs::remove_file(self.dir.join(&name)).is_ok()
            {
                total -= len;
            }
        }
        state.total = total;
    }
}

static MEDIA: OnceLock<Media> = OnceLock::new();

/// Make `data` NanaUI's image URL base and start the mirror in it. Call
/// once at startup, before any window draws a picture; later calls keep the
/// first mirror (NanaUI keeps its first base too).
pub fn init(data: &Path) -> Option<&'static Media> {
    if let Some(media) = MEDIA.get() {
        return Some(media);
    }
    let media = match Media::new(data, Limits::default()) {
        Ok(media) => media,
        Err(e) => {
            crate::debug_log(&format!("media mirror in {}: {e}", data.display()));
            return None;
        }
    };
    nana_ui::set_background_image_url_base(media.base());
    if effective_base().as_deref() != Some(media.base().as_path()) {
        crate::debug_log(&format!(
            "NanaUI image base is {:?}, not {}; pictures may not load",
            effective_base(),
            media.base().display()
        ));
    }
    Some(MEDIA.get_or_init(|| media))
}

/// The mirror [`init`] started, if any.
pub fn global() -> Option<&'static Media> {
    MEDIA.get()
}

/// [`Media::url`] through the mirror [`init`] started. Before `init` (unit
/// tests of views) it is the plain `file://` URL of `path`.
pub fn url(path: &Path, root: Option<&Path>) -> Option<String> {
    match MEDIA.get() {
        Some(media) => media.url(path, root),
        None => Some(file_url(path)),
    }
}

/// The directory NanaUI resolves relative picture URLs against right now.
pub fn effective_base() -> Option<PathBuf> {
    let probe = nana_ui::resolve_background_image_url("nanatype-base-probe")?;
    Path::new(&probe).parent().map(Path::to_path_buf)
}

fn modified(meta: &fs::Metadata) -> u128 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos())
}

fn touch(path: &Path) {
    if let Ok(file) = fs::File::options().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

/// 128-bit FNV-1a of the canonical path (case-folded on Windows, where
/// paths are), size and modified time: stable across runs and builds.
fn mirror_key(canonical: &Path, len: u64, mtime: u128) -> String {
    let mut path = plain(canonical).to_string_lossy().into_owned();
    if cfg!(windows) {
        path = path.replace('/', "\\").to_lowercase();
    }
    let key = format!("{path}\0{len}\0{mtime}");
    let a = fnv64(key.as_bytes(), 0xcbf2_9ce4_8422_2325);
    let b = fnv64(key.as_bytes(), 0x6c62_272e_07bb_0142);
    format!("{a:016x}{b:016x}")
}

fn fnv64(bytes: &[u8], seed: u64) -> u64 {
    let mut hash = seed;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// `\\?\C:\x` → `C:\x`: what `fs::canonicalize` returns on Windows, in the
/// spelling a `file://` URL can carry. Other paths are unchanged.
fn plain(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        let b = rest.as_bytes();
        if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
            return PathBuf::from(rest);
        }
    }
    path.to_path_buf()
}

/// A UNC or protocol-relative path: never opened, so a path from a
/// document cannot make Windows connect to a host.
fn looks_network(path: &Path) -> bool {
    let text = path.to_string_lossy().replace('\\', "/");
    if let Some(rest) = text.strip_prefix("//?/") {
        return rest.to_ascii_lowercase().starts_with("unc/");
    }
    text.starts_with("//")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "nanatype-media-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn mirror(s: &Scratch, limits: Limits) -> Media {
        Media::new(&s.0.join("data"), limits).unwrap()
    }

    // FNV-1a over `\stickers\a.png` / `/stickers/a.png`, `\0 42 \0 7`,
    // cross-checked with an independent implementation.
    #[cfg(windows)]
    const PINNED: &str = "a9832a77d72badac1a3cdba0ff2c8a47";
    #[cfg(not(windows))]
    const PINNED: &str = "c01620f935ab78fa02786dea320a765d";

    #[test]
    fn the_key_is_stable() {
        #[cfg(windows)]
        assert_eq!(
            mirror_key(Path::new(r"C:\Stickers\A.png"), 1, 2),
            mirror_key(Path::new(r"\\?\c:\stickers\a.PNG"), 1, 2),
            "Windows paths are case-insensitive"
        );
        // Pinned: a change renames every copy and orphans the old ones.
        let key = mirror_key(Path::new("/stickers/a.png"), 42, 7);
        assert_eq!(key, PINNED, "{key}");
        assert_ne!(key, mirror_key(Path::new("/stickers/a.png"), 43, 7));
        assert_ne!(key, mirror_key(Path::new("/stickers/a.png"), 42, 8));
        assert_ne!(key, mirror_key(Path::new("/stickers/b.png"), 42, 7));
        assert_eq!(fnv64(b"", 0xcbf2_9ce4_8422_2325), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv64(b"a", 0xcbf2_9ce4_8422_2325), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn embedded_images_cache_by_content_and_obey_media_limits() {
        let s = Scratch::new("embedded-images");
        let media = mirror(
            &s,
            Limits {
                max_file: 32,
                ..Default::default()
            },
        );
        let first = media.bytes_url("ink/stroke.png", b"first image").unwrap();
        assert!(first.starts_with(&file_url(&media.dir())));
        assert_eq!(
            first,
            media.bytes_url("renamed.png", b"first image").unwrap()
        );
        let changed = media.bytes_url("ink/stroke.png", b"other image").unwrap();
        assert_ne!(
            first, changed,
            "same-length changes invalidate the image cache"
        );
        assert_eq!(fs::read_dir(media.dir()).unwrap().count(), 2);
        assert!(media.bytes_url("empty.png", b"").is_none());
        assert!(media.bytes_url("large.png", &[0; 33]).is_none());
        assert!(media.bytes_url("wrong.exe", b"image").is_none());
    }

    #[test]
    fn inside_the_base_is_used_in_place() {
        let s = Scratch::new("inside");
        let m = mirror(&s, Limits::default());
        let skin = s.0.join("data").join("skins").join("x").join("frame.png");
        write(&skin, b"png");
        let got = m.path(&skin, Some(&s.0.join("data").join("skins").join("x")));
        assert_eq!(got, Some(plain(&fs::canonicalize(&skin).unwrap())));
        assert_eq!(fs::read_dir(m.dir()).unwrap().count(), 0, "nothing copied");
    }

    #[test]
    fn outside_the_base_is_copied_once_and_recopied_when_changed() {
        let s = Scratch::new("copy");
        let m = mirror(&s, Limits::default());
        let src = s.0.join("stickers").join("cat.PNG");
        write(&src, b"first");
        let a = m.path(&src, None).unwrap();
        assert!(a.starts_with(m.dir()));
        assert_eq!(a.extension().unwrap(), "png");
        assert_eq!(fs::read(&a).unwrap(), b"first");
        assert_eq!(m.path(&src, None).unwrap(), a, "same file, same copy");

        // A different size (and time) is a different picture.
        std::thread::sleep(Duration::from_millis(20));
        write(&src, b"second!");
        let b = m.path(&src, None).unwrap();
        assert_ne!(a, b);
        assert_eq!(fs::read(&b).unwrap(), b"second!");

        // Same size, new time: also new.
        std::thread::sleep(Duration::from_millis(20));
        write(&src, b"third!!");
        let c = m.path(&src, None).unwrap();
        assert_ne!(b, c);
        assert_eq!(fs::read(&c).unwrap(), b"third!!");
    }

    #[test]
    fn a_copy_removed_behind_our_back_is_made_again() {
        let s = Scratch::new("gone");
        let m = mirror(&s, Limits::default());
        let src = s.0.join("pics").join("a.webp");
        write(&src, b"abc");
        let a = m.path(&src, None).unwrap();
        fs::remove_file(&a).unwrap();
        assert_eq!(m.path(&src, None).unwrap(), a);
        assert!(a.is_file());
    }

    #[test]
    fn too_large_missing_and_non_pictures_are_refused() {
        let s = Scratch::new("refuse");
        let m = mirror(
            &s,
            Limits {
                max_file: 4,
                ..Limits::default()
            },
        );
        let big = s.0.join("pics").join("big.png");
        write(&big, b"12345");
        assert_eq!(m.path(&big, None), None);
        let ok = s.0.join("pics").join("ok.png");
        write(&ok, b"1234");
        assert!(m.path(&ok, None).is_some());
        let text = s.0.join("pics").join("notes.txt");
        write(&text, b"hi");
        assert_eq!(m.path(&text, None), None);
        assert_eq!(m.path(&s.0.join("pics").join("missing.png"), None), None);
        assert_eq!(m.path(&s.0.join("pics"), None), None, "a folder");
    }

    #[test]
    fn paths_cannot_climb_out_of_the_chosen_folder() {
        let s = Scratch::new("climb");
        let m = mirror(&s, Limits::default());
        let root = s.0.join("stickers");
        write(&root.join("in.png"), b"in");
        write(&s.0.join("secret.png"), b"secret");
        assert!(m.path(&root.join("in.png"), Some(&root)).is_some());
        let climb = root.join("..").join("secret.png");
        assert_eq!(m.path(&climb, Some(&root)), None);
        // Without a chosen folder (a file the user picked) it is just a file.
        assert!(m.path(&climb, None).is_some());
    }

    #[test]
    fn network_paths_are_never_opened() {
        assert!(looks_network(Path::new(r"\\server\share\a.png")));
        assert!(looks_network(Path::new("//server/share/a.png")));
        assert!(looks_network(Path::new(r"\\?\UNC\server\share\a.png")));
        assert!(!looks_network(Path::new(r"\\?\C:\a.png")));
        assert!(!looks_network(Path::new(r"C:\a.png")));
        assert!(!looks_network(Path::new("/home/a.png")));
        let s = Scratch::new("unc");
        let m = mirror(&s, Limits::default());
        assert_eq!(m.path(Path::new(r"\\localhost\c$\a.png"), None), None);
    }

    #[cfg(windows)]
    #[test]
    fn windows_spellings_of_one_file_share_one_copy() {
        let s = Scratch::new("forms");
        let m = mirror(&s, Limits::default());
        let src = s.0.join("My Pics").join("Cat #1.png");
        write(&src, b"cat");
        let canonical = fs::canonicalize(&src).unwrap();
        let text = src.to_string_lossy().into_owned();
        let forms = [
            src.clone(),
            PathBuf::from(text.replace('\\', "/")),
            PathBuf::from(text.to_uppercase()),
            PathBuf::from(text.to_lowercase()),
            canonical.clone(),
            PathBuf::from(format!(
                r"{}\..\My Pics\Cat #1.png",
                src.parent().unwrap().display()
            )),
        ];
        let first = m.path(&forms[0], None).unwrap();
        for form in &forms {
            assert_eq!(
                m.path(form, None).as_ref(),
                Some(&first),
                "{}",
                form.display()
            );
        }
        assert!(!first.to_string_lossy().starts_with(r"\\?\"));
        let url = m.url(&canonical, None).unwrap();
        assert!(url.starts_with("file:///"), "{url}");
        assert!(!url.contains('?'), "{url}");
        assert_eq!(
            plain(Path::new(r"\\?\C:\a\b.png")),
            PathBuf::from(r"C:\a\b.png")
        );
        assert_eq!(
            plain(Path::new(r"\\?\UNC\s\x\b.png")),
            PathBuf::from(r"\\?\UNC\s\x\b.png")
        );
    }

    /// A symlink (or junction) inside a chosen folder that points out of it
    /// is not followed; one pointing inside is fine.
    #[test]
    fn links_out_of_the_chosen_folder_are_not_followed() {
        let s = Scratch::new("links");
        let m = mirror(&s, Limits::default());
        let root = s.0.join("stickers");
        write(&root.join("in.png"), b"in");
        write(&s.0.join("outside").join("secret.png"), b"secret");
        let out_link = root.join("out.png");
        let in_link = root.join("alias.png");
        let dir_link = root.join("elsewhere");
        #[cfg(windows)]
        let made =
            std::os::windows::fs::symlink_file(s.0.join("outside").join("secret.png"), &out_link)
                .and_then(|()| std::os::windows::fs::symlink_file(root.join("in.png"), &in_link))
                .is_ok();
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(s.0.join("outside").join("secret.png"), &out_link)
            .and_then(|()| std::os::unix::fs::symlink(root.join("in.png"), &in_link))
            .is_ok();
        // A directory junction needs no privilege on Windows.
        #[cfg(windows)]
        let junction = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&dir_link)
            .arg(s.0.join("outside"))
            .output()
            .is_ok_and(|o| o.status.success());
        #[cfg(unix)]
        let junction = std::os::unix::fs::symlink(s.0.join("outside"), &dir_link).is_ok();
        if made {
            assert_eq!(m.path(&out_link, Some(&root)), None);
            assert!(m.path(&in_link, Some(&root)).is_some());
        } else {
            eprintln!("symlinks need privileges here; file links not tested");
        }
        assert!(junction, "could not make a directory link");
        assert_eq!(m.path(&dir_link.join("secret.png"), Some(&root)), None);
        // A chosen folder that is itself a link is followed: the user chose it.
        assert!(
            m.path(&dir_link.join("secret.png"), Some(&dir_link))
                .is_some()
        );
    }

    #[test]
    fn retired_skin_revisions_become_prunable_but_visible_aliases_stay_pinned() {
        let s = Scratch::new("skin-revisions");
        let m = mirror(
            &s,
            Limits {
                max_total: 5,
                ..Limits::default()
            },
        );
        let old = m.bytes_url("still.png", b"1111").unwrap();
        let new = m.bytes_url("still.png", b"2222").unwrap();
        m.retire_urls([old.clone(), new.clone()], &HashSet::from([new.clone()]));
        m.prune();
        let pinned = m.state.lock().unwrap();
        assert_eq!(pinned.used.len(), 1);
        drop(pinned);
        let names: Vec<_> = fs::read_dir(m.dir()).unwrap().flatten().collect();
        assert_eq!(names.len(), 1);
        assert_eq!(fs::read(names[0].path()).unwrap(), b"2222");
    }

    #[test]
    fn the_folder_is_capped_oldest_first_and_used_copies_stay() {
        let s = Scratch::new("cap");
        let m = mirror(
            &s,
            Limits {
                max_total: 10,
                ..Limits::default()
            },
        );
        // Three old copies from an earlier session, 4 bytes each.
        let old = |name: &str, secs: u64| {
            let p = m.dir().join(name);
            write(&p, b"1234");
            let f = fs::File::options().write(true).open(&p).unwrap();
            f.set_modified(SystemTime::now() - Duration::from_secs(secs))
                .unwrap();
            p
        };
        let oldest = old("a.png", 300);
        let middle = old("b.png", 200);
        let newest = old("c.png", 100);
        m.prune();
        assert!(!oldest.exists(), "12 > 10 bytes: the oldest goes");
        assert!(middle.exists() && newest.exists());

        // A copy made now pushes the next oldest out, never itself.
        let src = s.0.join("pics").join("new.png");
        write(&src, b"12345");
        let copy = m.path(&src, None).unwrap();
        assert!(copy.exists());
        assert!(!middle.exists(), "4 + 4 + 5 > 10: the oldest goes");
        assert!(newest.exists(), "4 + 5 fits");
        // Copies used this session survive even over the cap.
        let src2 = s.0.join("pics").join("new2.png");
        write(&src2, b"1234567890");
        let copy2 = m.path(&src2, None).unwrap();
        assert!(!newest.exists());
        assert!(copy.exists() && copy2.exists());
    }

    #[test]
    fn stale_copies_and_partial_files_expire() {
        let s = Scratch::new("age");
        let base = s.0.join("data");
        fs::create_dir_all(base.join(DIR)).unwrap();
        let aged = |name: &str, secs: u64| {
            let p = base.join(DIR).join(name);
            write(&p, b"x");
            let f = fs::File::options().write(true).open(&p).unwrap();
            f.set_modified(SystemTime::now() - Duration::from_secs(secs))
                .unwrap();
            p
        };
        let stale = aged("old.png", 31 * 24 * 3600);
        let fresh = aged("new.png", 60);
        let part = aged("x.png.part", 2 * 3600);
        let _m = Media::new(&base, Limits::default()).unwrap();
        assert!(!stale.exists());
        assert!(fresh.exists());
        assert!(!part.exists());
    }

    #[test]
    fn urls_escape_like_file_url() {
        let s = Scratch::new("url");
        let m = mirror(&s, Limits::default());
        let src = s.0.join("data").join("a b#c.png");
        write(&src, b"x");
        let url = m.url(&src, None).unwrap();
        assert!(url.ends_with("a%20b%23c.png"), "{url}");
    }
}
