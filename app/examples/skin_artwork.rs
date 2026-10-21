//! Bake procedural skin artwork through the production restricted renderer.
//! `cargo run -p nanatype --example skin_artwork -- <output-directory>`
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
                manifest.push_str(&format!("\n[shaders.artwork]\nsource = \"{name}/artwork.wgsl\"\nintensity = 1\nperiod_s = 16\n"));
                if name == "parchment" {
                    manifest.push_str("textures = [\"parchment/ornament.png\"]\n");
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
            let ui = Ui::new(
                Arc::new(Default::default()),
                skin.clone(),
                nanatype_i18n::Locale::ZhCn,
            );
            ui.display.update(|v| Arc::make_mut(v).empty = false);
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
            let artwork = shader_effects::view(
                *ui,
                Source::Display,
                &skin,
                "artwork",
                Surface::default(),
                ShaderTarget::Artwork,
            )
            .expect("artwork");
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
            let mut session = RuntimeAgentSession::new(doc, w, h).unwrap();
            session.set_gpu_renderers(sweep::renderers());
            session.set_clear(Some([0.; 4]));
            let _ = session.screenshot_rgba().unwrap();
            let started = Instant::now();
            while shader_effects::cache_stats().pending > 0
                && started.elapsed() < Duration::from_secs(20)
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(
                shader_effects::cache_stats().pending,
                0,
                "compiler timed out"
            );
            let notices = shader_effects::take_notices();
            assert!(notices.is_empty(), "{name}: {notices:?}");
            let folder = out.join(name);
            std::fs::create_dir_all(&folder).unwrap();
            session.screenshot_png(folder.join("still.png")).unwrap();
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
