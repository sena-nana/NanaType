//! The two native scenes exported to OBS must never contain the other track.

mod common;

use nanatype::{
    p2p::SessionUi,
    ui::{Ui, dialogue},
};
use nanatype_core::{Cmd, Core};
use nanatype_i18n::Locale;
use nanatype_settings::Settings;
use std::{path::Path, sync::Arc};

fn render(ui: Ui, remote: bool, path: &Path) -> image::RgbaImage {
    nanatype::clock::capture_at(100_000, || render_at(ui, remote, path))
}

fn render_at(ui: Ui, remote: bool, path: &Path) -> image::RgbaImage {
    let mut session = common::session()
        .id(if remote { 6 } else { 2 })
        .themed(false)
        .mount(796, 236, move || {
            if remote {
                nana_ui::runtime::view::IntoView::into_any(dialogue::peer_window(ui))
            } else {
                nana_ui::runtime::view::IntoView::into_any(dialogue::display_window(ui))
            }
        });
    session.screenshot_png(path).unwrap();
    image::open(path).unwrap().into_rgba8()
}

fn pixels(img: &image::RgbaImage, color: [u8; 3]) -> usize {
    img.pixels()
        .filter(|p| p[3] > 200 && p.0[..3].iter().zip(color).all(|(a, b)| a.abs_diff(b) <= 6))
        .count()
}

#[test]
fn separate_scenes_keep_self_and_partner_content_when_roles_change() {
    let skin = common::skin("nanatype.halo");
    let settings = Settings::default();
    let ui = Ui::new(Arc::new(settings.clone()), skin.clone(), Locale::ZhCn);
    let mut own = Core::new(settings.clone(), skin.clone());
    let mut partner = Core::new(settings, skin);
    own.handle(
        Cmd::SendMarkup("[color=#d00050]SELF SELF SELF[/color]".into()),
        0,
    );
    own.handle(Cmd::Skip, 1);
    partner.handle(
        Cmd::SendMarkup("[color=#0080d0]PARTNER PARTNER[/color]".into()),
        0,
    );
    partner.handle(Cmd::Skip, 1);
    ui.display.set(Arc::new(own.display().clone()));
    ui.peer_display.set(Arc::new(partner.display().clone()));
    ui.now.set(100_000);
    ui.peer_now.set(100_000); // The host advances each display clock independently.
    let out =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/p2p-output-tests");
    std::fs::create_dir_all(&out).unwrap();
    for host in [true, false] {
        ui.peer.set(Arc::new(SessionUi {
            active: true,
            host,
            ..Default::default()
        }));
        let own_image = render(ui, false, &out.join(format!("self-{host}.png")));
        let partner_image = render(ui, true, &out.join(format!("partner-{host}.png")));
        assert!(pixels(&own_image, [208, 0, 80]) > 100, "self text missing");
        assert_eq!(
            pixels(&own_image, [0, 128, 208]),
            0,
            "partner leaked into own source"
        );
        assert!(
            pixels(&partner_image, [0, 128, 208]) > 100,
            "partner text missing"
        );
        assert_eq!(
            pixels(&partner_image, [208, 0, 80]),
            0,
            "self leaked into partner source"
        );
    }
    ui.peer.set(Arc::new(SessionUi::default()));
    let inactive = render(ui, true, &out.join("inactive.png"));
    assert_eq!(
        pixels(&inactive, [0, 128, 208]),
        0,
        "old partner content remained after leave"
    );
}
