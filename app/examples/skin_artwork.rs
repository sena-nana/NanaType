//! Bake procedural skin artwork through the production restricted renderer.
//! `cargo run -p nanatype --example skin_artwork -- <output-directory>`
//! `cargo run -p nanatype --example skin_artwork -- --package <dir> [--preset <id>] [--out <png>] [--at <ms>] [--scale <x>]`
//! bakes a package's own `artwork` shader at its region size, with a preset's
//! param values when given, into `<dir>/still.png` unless `--out` says where.
//! `--at` renders another moment of the loop than the still pose at 16 s;
//! `--scale` renders at that many device pixels per logical pixel.
//! Static images use identical geometry, palette, color conversion and seed.
use nana_ui::runtime::view::widget;
use nana_ui::runtime::{DocumentId, RuntimeDocument, Stack};
use nana_ui_core::{LengthSpec, PositionSpec};
use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession};
use nanatype::ui::{Ui, dialogue::Source, shader_effects, surface::Surface, sweep};
use nanatype_theme::{DirFiles, ShaderTarget, SkinOrigin};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--package") {
        return bake_package();
    }
    let out = PathBuf::from(std::env::args().nth(1).expect("output directory"));
    let sources = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../crates/theme/builtin");
    let requested: Vec<String> = std::env::args().skip(2).collect();
    let defaults = [
        "gothic",
        "celestial",
        "forest",
        "tidal",
        "lantern",
        "citrus",
    ];
    let names: Vec<&str> = if requested.is_empty() {
        defaults.to_vec()
    } else {
        requested.iter().map(String::as_str).collect()
    };
    for name in names {
        nanatype::clock::capture_at(16_000, || {
            let files = Arc::new(DirFiles::new(sources.join(name)));
            // Each built-in is a manifest within the shared artwork root.
            // Layer the selected manifest over those files without copying.
            let mut manifest =
                std::fs::read_to_string(sources.join(name).join("skin.toml")).unwrap();
            // Some designs deliberately have no runtime shader. Their WGSL
            // is an authoring tool only: load it for baking, never for playback
            // or export. Do not add an effect instance to the saved manifest.
            if !manifest.contains("[shaders.artwork]") {
                manifest.push_str("\n[shaders.artwork]\nsource = \"artwork.wgsl\"\nintensity = 1\nperiod_s = 16\n");
                if name == "parchment" {
                    manifest.push_str("textures = [\"ornament.png\"]\n");
                }
            }
            let manifest = manifest.into_bytes();
            let files = Arc::new(ArtworkFiles { files, manifest });
            let skin = nanatype_theme::load(
                files,
                SkinOrigin::Directory(sources.clone()),
                &(),
                env!("CARGO_PKG_VERSION"),
            )
            .expect("skin")
            .skin;
            // Classic plates keep their original margins and width fraction.
            // Bake at the real border-box size so the static fallback aligns
            // with the shader at the default window extent.
            let [w, h] = if defaults.contains(&name) {
                skin.dialogue.region.map(|v| v as u32)
            } else {
                let d = &skin.dialogue;
                let [mt, mr, mb, ml] = d.box_.margin;
                [
                    ((d.region[0] - ml - mr) * d.box_.width_fraction).round() as u32,
                    (d.region[1] - mt - mb).round() as u32,
                ]
            };
            let folder = out.join(name);
            std::fs::create_dir_all(&folder).unwrap();
            bake(skin.clone(), [w, h], 1., &folder.join("still.png"), name);
            if name == "gothic" {
                // The original plate samples a two-cell atlas. Responsive
                // native nodes need independent, aspect-preserving ornaments.
                let atlas = image::open(sources.join("gothic/ornament.png")).unwrap();
                let cell = atlas.width() / 2;
                for (index, part) in ["rose", "tracery"].into_iter().enumerate() {
                    atlas
                        .crop_imm(index as u32 * cell, 0, cell, atlas.height())
                        .save(folder.join(format!("{part}.png")))
                        .unwrap();
                }
            }
            println!("{name}: {w}x{h}");
        });
    }
}

#[derive(Debug)]
struct ArtworkFiles {
    files: Arc<DirFiles>,
    manifest: Vec<u8>,
}
impl nanatype_theme::SkinFiles for ArtworkFiles {
    fn read(&self, path: &str) -> Result<Vec<u8>, nanatype_theme::FileError> {
        if path == "skin.toml" {
            Ok(self.manifest.clone())
        } else {
            self.files.read(path)
        }
    }
    fn list(&self) -> Vec<String> {
        self.files.list()
    }
}

/// Render `skin`'s `artwork` shader at `w`×`h` into the PNG at `path`.
fn bake(
    skin: Arc<nanatype_theme::Skin>,
    [w, h]: [u32; 2],
    scale: f32,
    path: &std::path::Path,
    name: &str,
) {
    let ui = Ui::new(
        Arc::new(Default::default()),
        skin.clone(),
        nanatype_i18n::Locale::ZhCn,
    );
    ui.display.update(|v| Arc::make_mut(v).empty = false);
    // Every artwork layer the skin stacks, in order; built-ins
    // bake their one `artwork` program.
    let mut ids: Vec<String> = skin
        .dialogue
        .shader_effects
        .iter()
        .filter(|e| e.target == ShaderTarget::Artwork)
        .map(|e| e.shader.clone())
        .collect();
    if ids.is_empty() {
        ids.push("artwork".into());
    }
    let artwork: Vec<_> = ids
        .iter()
        .map(|id| {
            shader_effects::view(
                *ui,
                Source::Display,
                &skin,
                id,
                Surface::default(),
                ShaderTarget::Artwork,
            )
            .expect("artwork")
        })
        .collect();
    let id = DocumentId::new(1).unwrap();
    let mut doc = RuntimeDocument::new(id);
    doc.context_mut()
        .mount_view_root(id, move || {
            widget(
                Stack::column(0.)
                    .width(LengthSpec::Px(w as f32))
                    .height(LengthSpec::Px(h as f32))
                    .with_layout(|l| l.position = PositionSpec::Relative),
            )
            .children(artwork)
        })
        .unwrap();
    let mut session = RuntimeAgentSession::new_scaled(doc, w, h, scale).unwrap();
    session.set_gpu_renderers(sweep::renderers());
    session.set_clear(Some([0.; 4]));
    let _ = session.screenshot_rgba().unwrap();
    let started = Instant::now();
    while shader_effects::cache_stats().pending > 0 && started.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        shader_effects::cache_stats().pending,
        0,
        "compiler timed out"
    );
    let notices = shader_effects::take_notices();
    assert!(notices.is_empty(), "{name}: {notices:?}");
    session.screenshot_png(path).unwrap();
}

fn bake_package() {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let dir = PathBuf::from(args.first().expect("package directory"));
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let preset = flag("--preset");
    let at = flag("--at").map_or(16_000, |v| v.parse().expect("--at <ms>"));
    let scale = flag("--scale").map_or(1., |v| v.parse().expect("--scale <x>"));
    let out = flag("--out").map_or_else(|| dir.join("still.png"), PathBuf::from);
    nanatype::clock::capture_at(at, || {
        let files = Arc::new(PackageFiles {
            files: Arc::new(DirFiles::new(&dir)),
        });
        let mut skin = nanatype_theme::load(
            files,
            SkinOrigin::Directory(dir.clone()),
            &(),
            env!("CARGO_PKG_VERSION"),
        )
        .unwrap_or_else(|e| panic!("{e:?}"))
        .skin;
        if let Some(id) = &preset {
            let values = skin
                .preset_values(id)
                .unwrap_or_else(|| panic!("no preset `{id}`"))
                .into_iter()
                .map(|(k, v)| (k, v.to_value()))
                .collect();
            let (resolved, warnings) =
                nanatype_theme::resolve_params(&skin, &values, env!("CARGO_PKG_VERSION"));
            assert!(warnings.is_empty(), "{warnings:?}");
            skin = Arc::new(resolved);
        }
        let size = skin.dialogue.region.map(|v| v as u32);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        bake(skin, size, scale, &out, "package");
        println!("{}: {}x{}", out.display(), size[0], size[1]);
    });
}

/// A package's files, with a stand-in for a `still.png` not baked yet.
#[derive(Debug)]
struct PackageFiles {
    files: Arc<DirFiles>,
}
impl nanatype_theme::SkinFiles for PackageFiles {
    fn read(&self, path: &str) -> Result<Vec<u8>, nanatype_theme::FileError> {
        match self.files.read(path) {
            Err(nanatype_theme::FileError::Missing(_)) if path == "still.png" => {
                let mut png = Vec::new();
                image::RgbaImage::new(1, 1)
                    .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                    .unwrap();
                Ok(png)
            }
            other => other,
        }
    }
    fn list(&self) -> Vec<String> {
        self.files.list()
    }
    fn root(&self) -> Option<&std::path::Path> {
        self.files.root()
    }
}
