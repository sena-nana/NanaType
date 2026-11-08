//! One folder pipeline for local imports and downloaded presets.
pub mod cache;
pub mod definition;
mod formats;
mod literal;
pub mod playback;
pub mod telegram;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use crossbeam_channel::{Sender, unbounded};

use cache::CachedImage;
use definition::Definition;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Pixel,
    Animated,
}
impl Preset {
    pub const ALL: [Self; 2] = [Self::Pixel, Self::Animated];
    pub fn id(self) -> &'static str {
        match self {
            Self::Pixel => "echo-pixel-head",
            Self::Animated => "echo-emotelab",
        }
    }
    pub fn title_key(self) -> &'static str {
        match self {
            Self::Pixel => "stickers-preset-pixel",
            Self::Animated => "stickers-preset-animated",
        }
    }
    pub fn repository(self) -> &'static str {
        match self {
            Self::Pixel => "https://github.com/sheep-realms/Echo-Live",
            Self::Animated => {
                "https://github.com/sheep-realms/Echo-Live-Sheep-realms-EmoteLab-Extension"
            }
        }
    }
    fn raw(self) -> &'static str {
        match self {
            Self::Pixel => "https://raw.githubusercontent.com/sheep-realms/Echo-Live/master",
            Self::Animated => {
                "https://raw.githubusercontent.com/sheep-realms/Echo-Live-Sheep-realms-EmoteLab-Extension/main"
            }
        }
    }
    fn definition_path(self) -> &'static str {
        match self {
            Self::Pixel => "res/data/emoji.js",
            Self::Animated => "main.js",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    NotDownloaded,
    Downloading(usize, usize),
    Processing(usize, usize),
    Ready,
    Failed(String),
}
impl Phase {
    pub fn busy(&self) -> bool {
        matches!(self, Self::Downloading(..) | Self::Processing(..))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PackRow {
    pub root: PathBuf,
    pub title: String,
    pub author: String,
    pub license: String,
    pub count: usize,
    pub preset: Option<Preset>,
    pub repository: Option<String>,
    pub phase: Phase,
}

#[derive(Debug, Clone)]
pub struct Prepared {
    pub definition: Definition,
    pub images: Vec<Arc<CachedImage>>,
}

#[derive(Debug, Clone)]
pub enum Event {
    Progress {
        root: PathBuf,
        generation: u64,
        phase: Phase,
    },
    Done {
        root: PathBuf,
        generation: u64,
        result: Result<Prepared, String>,
    },
}

struct Job {
    root: PathBuf,
    generation: u64,
    download: Option<Preset>,
    cancelled: Arc<AtomicBool>,
}
struct Pending {
    generation: u64,
    cancelled: Arc<AtomicBool>,
    download: bool,
}

pub struct Manager {
    pub rows: Vec<PackRow>,
    pub loaded: HashMap<PathBuf, Prepared>,
    jobs: HashMap<PathBuf, Pending>,
    next: u64,
    tx: Sender<Job>,
}

/// Canonical identity, with the Win32 long-path prefix removed. Missing preset
/// directories still have a stable absolute identity before their first download.
pub fn folder(path: &Path) -> PathBuf {
    let path = path.canonicalize().unwrap_or_else(|_| {
        if path.is_absolute() {
            path.into()
        } else {
            std::env::current_dir().unwrap_or_default().join(path)
        }
    });
    #[cfg(windows)]
    {
        let spelling = path.to_string_lossy();
        if let Some(unc) = spelling.strip_prefix(r"\\?\UNC\") {
            PathBuf::from(format!(r"\\{unc}"))
        } else {
            PathBuf::from(spelling.trim_start_matches(r"\\?\"))
        }
    }
    #[cfg(not(windows))]
    {
        path
    }
}

/// Shared by the folder picker and platform drops. Only directories are
/// accepted; canonical aliases do not enqueue another import.
pub fn add_folders(dirs: &mut Vec<String>, paths: &[PathBuf]) {
    let mut known: HashSet<_> = dirs.iter().map(|d| folder(Path::new(d))).collect();
    for path in paths.iter().filter(|p| p.is_dir()) {
        let root = folder(path);
        if known.insert(root.clone()) {
            dirs.push(root.display().to_string());
        }
    }
}

impl Manager {
    pub fn new(data: &Path, post: impl Fn(Event) + Send + 'static) -> Self {
        let root = data.join("media/stickers");
        let (tx, rx) = unbounded::<Job>();
        std::thread::Builder::new()
            .name("nanatype-stickers".into())
            .spawn(move || {
                // Off the UI thread: deleting a previous generation can be slow.
                let cache = cache::open(&root);
                for job in rx {
                    if job.cancelled.load(Ordering::Relaxed) {
                        continue;
                    }
                    let progress = |phase| {
                        post(Event::Progress {
                            root: job.root.clone(),
                            generation: job.generation,
                            phase,
                        })
                    };
                    let result = (|| {
                        if let Some(preset) = job.download {
                            download(preset, &job.root, &job.cancelled, &progress)?;
                        }
                        let definition = definition::load(&job.root)?;
                        let mut images = Vec::new();
                        for (i, entry) in definition.entries.iter().enumerate() {
                            if job.cancelled.load(Ordering::Relaxed) {
                                return Err("Cancelled".into());
                            }
                            progress(Phase::Processing(i, definition.entries.len()));
                            let source =
                                definition::contained(&job.root, &job.root.join(&entry.path))?;
                            images.push(
                                cache::prepare(&source, &cache, &job.cancelled)
                                    .map_err(|e| format!("{}: {e}", entry.path.display()))?,
                            );
                        }
                        cache::prune(&cache)?;
                        Ok(Prepared { definition, images })
                    })();
                    if !job.cancelled.load(Ordering::Relaxed) {
                        post(Event::Done {
                            root: job.root,
                            generation: job.generation,
                            result,
                        });
                    }
                }
            })
            .expect("spawn sticker worker");
        let rows = Preset::ALL
            .into_iter()
            .map(|preset| PackRow {
                root: folder(&data.join("stickers").join(preset.id())),
                title: String::new(),
                author: "Sheep-realms".into(),
                license: "CC BY-NC-SA 4.0".into(),
                count: 0,
                preset: Some(preset),
                repository: Some(preset.repository().into()),
                phase: Phase::NotDownloaded,
            })
            .collect();
        Self {
            rows,
            loaded: HashMap::new(),
            jobs: HashMap::new(),
            next: 0,
            tx,
        }
    }

    pub fn sync(&mut self, dirs: &[String]) {
        let desired: HashSet<_> = dirs.iter().map(|d| folder(Path::new(d))).collect();
        let remove: Vec<_> = self
            .rows
            .iter()
            .filter(|r| {
                !desired.contains(&r.root)
                    && !matches!(r.phase, Phase::NotDownloaded)
                    && !self.jobs.get(&r.root).is_some_and(|j| j.download)
            })
            .map(|r| r.root.clone())
            .collect();
        for root in remove {
            self.remove(&root);
        }
        for root in desired {
            if !self.rows.iter().any(|r| r.root == root) {
                self.rows.push(PackRow {
                    title: root
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    root: root.clone(),
                    author: String::new(),
                    license: String::new(),
                    count: 0,
                    preset: None,
                    repository: None,
                    phase: Phase::NotDownloaded,
                });
            }
            if self
                .rows
                .iter()
                .any(|r| r.root == root && r.phase == Phase::NotDownloaded)
            {
                self.start(&root, None);
            }
        }
    }

    pub fn start(&mut self, root: &Path, download: Option<Preset>) {
        let root = folder(root);
        if self.jobs.contains_key(&root) {
            return;
        }
        let Some(row) = self.rows.iter_mut().find(|r| r.root == root) else {
            return;
        };
        row.phase = if download.is_some() {
            Phase::Downloading(0, 0)
        } else {
            Phase::Processing(0, 0)
        };
        self.next += 1;
        let cancelled = Arc::new(AtomicBool::new(false));
        self.jobs.insert(
            root.clone(),
            Pending {
                generation: self.next,
                cancelled: cancelled.clone(),
                download: download.is_some(),
            },
        );
        let _ = self.tx.send(Job {
            root,
            generation: self.next,
            download,
            cancelled,
        });
    }

    pub fn rebuild(&mut self, dirs: &[String]) {
        self.sync(dirs);
        for root in dirs
            .iter()
            .map(|d| folder(Path::new(d)))
            .collect::<HashSet<_>>()
        {
            self.cancel(&root);
            self.start(&root, None);
        }
    }

    fn cancel(&mut self, root: &Path) {
        if let Some(job) = self.jobs.remove(root) {
            job.cancelled.store(true, Ordering::Relaxed);
        }
    }

    pub fn remove(&mut self, root: &Path) {
        let root = folder(root);
        self.cancel(&root);
        self.loaded.remove(&root);
        self.rows.retain_mut(|r| {
            if r.root != root {
                return true;
            }
            if r.preset.is_some() {
                r.phase = Phase::NotDownloaded;
                r.count = 0;
                true
            } else {
                false
            }
        });
    }

    /// Stale generations (including a removed/reimported directory) never publish.
    pub fn accept(&mut self, event: Event) -> Option<PathBuf> {
        let (root, generation) = match &event {
            Event::Progress {
                root, generation, ..
            }
            | Event::Done {
                root, generation, ..
            } => (root, *generation),
        };
        if !self
            .jobs
            .get(root)
            .is_some_and(|j| j.generation == generation)
        {
            return None;
        }
        let row = self.rows.iter_mut().find(|r| &r.root == root)?;
        match event {
            Event::Progress { phase, .. } => {
                row.phase = phase;
                None
            }
            Event::Done { root, result, .. } => {
                self.jobs.remove(&root);
                match result {
                    Ok(prepared) => {
                        row.phase = Phase::Ready;
                        row.count = prepared.images.len();
                        row.title = prepared.definition.title.clone();
                        row.author = prepared.definition.author.clone();
                        row.license = prepared.definition.license.clone();
                        if prepared.definition.repository.is_some() {
                            row.repository = prepared.definition.repository.clone();
                        }
                        self.loaded.insert(root.clone(), prepared);
                        Some(root)
                    }
                    Err(error) => {
                        row.phase = Phase::Failed(error);
                        None
                    }
                }
            }
        }
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        for job in self.jobs.values() {
            job.cancelled.store(true, Ordering::Relaxed);
        }
    }
}

fn encoded(path: &str) -> String {
    path.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

fn get(agent: &ureq::Agent, url: &str, limit: u64) -> Result<Vec<u8>, String> {
    agent
        .get(url)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .with_config()
        .limit(limit)
        .read_to_vec()
        .map_err(|e| e.to_string())
}

fn download(
    preset: Preset,
    root: &Path,
    cancelled: &AtomicBool,
    progress: &impl Fn(Phase),
) -> Result<(), String> {
    // A complete previous download is still a normal folder import and goes
    // through the exact same validation/preprocessing as a user-selected folder.
    if let Ok(d) = definition::load(root)
        && d.entries
            .iter()
            .all(|e| definition::contained(root, &root.join(&e.path)).is_ok())
    {
        return Ok(());
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .user_agent(concat!("NanaType/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    download_from(preset, root, preset.raw(), &agent, cancelled, progress)
}

fn download_from(
    preset: Preset,
    root: &Path,
    raw: &str,
    agent: &ureq::Agent,
    cancelled: &AtomicBool,
    progress: &impl Fn(Phase),
) -> Result<(), String> {
    let parent = root.parent().ok_or("Missing sticker installation parent")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    // stage is a sibling of the managed preset folder, never a user directory.
    let stage = parent.join(format!("{}.part", preset.id()));
    if stage.exists() {
        fs::remove_dir_all(&stage).map_err(|e| e.to_string())?;
    }
    fs::create_dir(&stage).map_err(|e| e.to_string())?;
    let result = (|| {
        let source = String::from_utf8(get(
            agent,
            &format!("{raw}/{}", preset.definition_path()),
            definition::MAX_DEFINITION,
        )?)
        .map_err(|e| e.to_string())?;
        let lang = if preset == Preset::Pixel {
            let bytes = get(
                agent,
                &format!("{raw}/lang/zho-Hans.js"),
                definition::MAX_DEFINITION,
            )?;
            Some(literal::read(
                &String::from_utf8(bytes).map_err(|e| e.to_string())?,
                '{',
            )?)
        } else {
            None
        };
        let mut d = definition::echo(
            &source,
            preset == Preset::Animated,
            lang.as_ref(),
            Some(if preset == Preset::Pixel {
                "sp"
            } else {
                "sprel"
            }),
        )?;
        d.repository = Some(preset.repository().into());
        let total = d.entries.len() + 1;
        let mut total_bytes = 0usize;
        for (i, entry) in d.entries.iter().enumerate() {
            if cancelled.load(Ordering::Relaxed) {
                return Err("Cancelled".into());
            }
            progress(Phase::Downloading(i + 1, total));
            let bytes = get(
                agent,
                &format!(
                    "{raw}/{}",
                    encoded(&entry.path.to_string_lossy().replace('\\', "/"))
                ),
                cache::MAX_SOURCE,
            )?;
            total_bytes += bytes.len();
            if total_bytes > 128 * 1024 * 1024 {
                return Err("Sticker download exceeds 128 MiB".into());
            }
            image::guess_format(&bytes).map_err(|e| e.to_string())?;
            let dest = stage.join(&entry.path);
            fs::create_dir_all(dest.parent().ok_or("Missing image parent")?)
                .map_err(|e| e.to_string())?;
            fs::write(dest, bytes).map_err(|e| e.to_string())?;
        }
        // Preserve the upstream source definition and licensing, while the
        // normalized manifest selects just this preset's image registry.
        fs::write(stage.join("echo-source.js"), source).map_err(|e| e.to_string())?;
        let license = get(agent, &format!("{raw}/LICENSE"), definition::MAX_DEFINITION)?;
        fs::write(stage.join("LICENSE"), license).map_err(|e| e.to_string())?;
        fs::write(
            stage.join("ATTRIBUTION.txt"),
            format!(
                "{}\n{}\nImages: {}\n{}\n",
                d.title,
                d.author,
                d.license,
                preset.repository()
            ),
        )
        .map_err(|e| e.to_string())?;
        fs::write(
            stage.join(definition::MANIFEST),
            serde_json::to_vec_pretty(&d).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if cancelled.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        // Incomplete earlier installs are retained until the new one is ready.
        // Never replace an arbitrary directory supplied by the user.
        if root.exists() {
            return Err("Incomplete preset folder exists; import it locally or move it aside before retrying".into());
        }
        fs::rename(&stage, root).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(stage);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    const EXT: &str = r#"extensionManager.load({
        meta:{name:'sheep_realms_emotelab',title:{text:'Test'},project_url:'https://github.com/sheep-realms/test'},
        register_hook:{loaded:[{registry:'echolive:emoji',value:{pack:{
            meta:{namespace:'sprel',title:{text:'Test'},author:{name:'Sheep'},license:{name:'CC BY-NC-SA 4.0'}},
            path:{images:'extensions/sheep_realms_emotelab/emoji/'},
            image:{size:{height:{value:'4em'}}},content:[{name:'laugh',path:'001.gif'}]
        }}}]}
    });"#;

    fn scratch(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("nanatype-stickers-{name}-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn gif_bytes() -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut writer = image::codecs::gif::GifEncoder::new(&mut bytes);
        writer
            .set_repeat(image::codecs::gif::Repeat::Infinite)
            .unwrap();
        for color in [[255, 0, 0, 255], [0, 255, 0, 255]] {
            writer
                .encode_frame(image::Frame::from_parts(
                    image::RgbaImage::from_pixel(2, 2, image::Rgba(color)),
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(40, 1),
                ))
                .unwrap();
        }
        drop(writer);
        bytes
    }

    fn server(replies: Vec<(u16, Vec<u8>)>) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let thread = std::thread::spawn(move || {
            for (status, bytes) in replies {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = [0; 8192];
                let _ = stream.read(&mut request);
                write!(
                    stream,
                    "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    bytes.len()
                )
                .unwrap();
                stream.write_all(&bytes).unwrap();
            }
        });
        (url, thread)
    }

    #[test]
    fn download_and_local_import_share_definition_and_cache() {
        let data = scratch("download");
        let root = data.join("echo-emotelab");
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        let (url, thread) = server(vec![
            (200, EXT.as_bytes().to_vec()),
            (200, gif_bytes()),
            (200, b"License".to_vec()),
        ]);
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(3)))
            .build()
            .into();
        download_from(
            Preset::Animated,
            &root,
            &url,
            &agent,
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap();
        thread.join().unwrap();
        let preset = definition::load(&root).unwrap();
        let local = data.join("local");
        fs::create_dir_all(local.join("emoji")).unwrap();
        fs::write(local.join("main.js"), EXT).unwrap();
        fs::write(local.join("emoji/001.gif"), gif_bytes()).unwrap();
        let imported = definition::load(&local).unwrap();
        assert_eq!(preset.entries[0].lib, imported.entries[0].lib);
        assert_eq!(preset.entries[0].name, imported.entries[0].name);
        assert_eq!(preset.entries[0].height_em, imported.entries[0].height_em);
        let a = cache::prepare(
            &root.join(&preset.entries[0].path),
            &data.join("cache"),
            &AtomicBool::new(false),
        )
        .unwrap();
        let b = cache::prepare(
            &local.join(&imported.entries[0].path),
            &data.join("cache"),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(a.dir, b.dir);
        assert_eq!(a.delays, b.delays);
        assert!(root.join("ATTRIBUTION.txt").is_file());
        let _ = fs::remove_dir_all(data);
    }

    #[test]
    fn missing_download_never_publishes_and_retry_succeeds() {
        let data = scratch("retry");
        let root = data.join("echo-emotelab");
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(3)))
            .build()
            .into();
        let (url, thread) = server(vec![
            (200, EXT.as_bytes().to_vec()),
            (404, b"missing".to_vec()),
        ]);
        assert!(
            download_from(
                Preset::Animated,
                &root,
                &url,
                &agent,
                &AtomicBool::new(false),
                &|_| {}
            )
            .is_err()
        );
        thread.join().unwrap();
        assert!(!root.exists());
        assert!(!data.join("echo-emotelab.part").exists());
        let (url, thread) = server(vec![
            (200, EXT.as_bytes().to_vec()),
            (200, gif_bytes()),
            (200, b"License".to_vec()),
        ]);
        download_from(
            Preset::Animated,
            &root,
            &url,
            &agent,
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap();
        thread.join().unwrap();
        assert!(definition::load(&root).is_ok());
        let _ = fs::remove_dir_all(data);
    }

    #[test]
    fn manager_deduplicates_cancels_and_rejects_stale_generations() {
        let data = scratch("manager");
        let mut manager = Manager::new(&data, |_| {});
        let root = manager.rows[1].root.clone();
        manager.start(&root, None);
        let generation = manager.next;
        manager.start(&root, None);
        assert_eq!(manager.next, generation);
        manager.remove(&root);
        assert!(
            manager
                .accept(Event::Done {
                    root: root.clone(),
                    generation,
                    result: Err("late error".into())
                })
                .is_none()
        );
        assert_eq!(manager.rows[1].phase, Phase::NotDownloaded);
        let local = data.join("local");
        fs::create_dir_all(&local).unwrap();
        manager.sync(&[
            local.display().to_string(),
            local.join(".").display().to_string(),
        ]);
        assert_eq!(
            manager.rows.iter().filter(|r| r.preset.is_none()).count(),
            1
        );
        manager.remove(&folder(&local));
        assert!(manager.rows.iter().all(|r| r.preset.is_some()));
        let _ = fs::remove_dir_all(data);
    }

    #[test]
    #[ignore = "Downloads both upstream presets; run explicitly with network access"]
    fn upstream_presets_download_transcode_and_reopen_offline() {
        let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/sticker-presets");
        for preset in Preset::ALL {
            let root = data
                .join(format!("sources-{}", std::process::id()))
                .join(preset.id());
            download(preset, &root, &AtomicBool::new(false), &|p| {
                eprintln!("{}: {p:?}", preset.id())
            })
            .unwrap();
            let d = definition::load(&root).unwrap();
            assert!(!d.entries.is_empty());
            assert!(
                d.entries
                    .iter()
                    .any(|e| e.translations.contains_key("zh-CN")),
                "upstream localized titles must be preserved"
            );
            for entry in &d.entries {
                let image = cache::prepare(
                    &definition::contained(&root, &root.join(&entry.path)).unwrap(),
                    &data.join("media/stickers"),
                    &AtomicBool::new(false),
                )
                .unwrap();
                if preset == Preset::Animated {
                    assert!(image.animated());
                }
            }
            let reopened = definition::load(&root).unwrap();
            assert_eq!(reopened.entries.len(), d.entries.len());
            eprintln!(
                "{}: {} stickers ready offline",
                preset.id(),
                d.entries.len()
            );
        }
    }

    #[test]
    fn edited_sources_replace_caches_only_after_success() {
        let data = scratch("editing");
        let local = data.join("local");
        fs::create_dir_all(&local).unwrap();
        let image = local.join("hello.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))
            .save(&image)
            .unwrap();
        let (tx, rx) = unbounded();
        let mut manager = Manager::new(&data, move |event| {
            let _ = tx.send(event);
        });
        let dirs = vec![local.display().to_string()];
        let root = folder(&local);
        let finish = |manager: &mut Manager| {
            loop {
                let event = rx.recv_timeout(Duration::from_secs(10)).unwrap();
                let done = matches!(event, Event::Done { .. });
                manager.accept(event);
                if done {
                    break;
                }
            }
        };
        manager.sync(&dirs);
        finish(&mut manager);
        let before = manager.loaded[&root].images[0].key.clone();
        fs::write(&image, b"broken image").unwrap();
        manager.rebuild(&dirs);
        finish(&mut manager);
        assert!(matches!(
            manager.rows.iter().find(|r| r.root == root).unwrap().phase,
            Phase::Failed(_)
        ));
        assert_eq!(
            manager.loaded[&root].images[0].key, before,
            "the old valid revision stays usable"
        );
        image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 255, 0, 255]))
            .save(&image)
            .unwrap();
        manager.rebuild(&dirs);
        finish(&mut manager);
        assert_ne!(manager.loaded[&root].images[0].key, before);
        drop(manager);
        let _ = fs::remove_dir_all(data);
    }

    #[test]
    fn folder_picker_and_drops_share_directory_validation_and_deduplication() {
        let root = scratch("drop-folders");
        let a = root.join("a");
        let b = root.join("b");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        let file = root.join("not-a-folder.png");
        fs::write(&file, b"image").unwrap();
        let mut dirs = vec![a.display().to_string()];
        add_folders(
            &mut dirs,
            &[
                a.join("."),
                file,
                b.clone(),
                root.join("missing"),
                b.join("."),
            ],
        );
        assert_eq!(dirs.len(), 2);
        assert_eq!(folder(Path::new(&dirs[1])), folder(&b));
        let _ = fs::remove_dir_all(root);
    }
}
