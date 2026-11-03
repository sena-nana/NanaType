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

/// The local display (`None`), or a partner seat's window.
fn render(ui: Ui, seat: Option<usize>, path: &Path) -> image::RgbaImage {
    nanatype::clock::capture_at(100_000, || render_at(ui, seat, path))
}

fn render_at(ui: Ui, seat: Option<usize>, path: &Path) -> image::RgbaImage {
    let mut session = common::session()
        .id(seat.map_or(2, |seat| 6 + seat as u64 * 2))
        .themed(false)
        .mount(796, 236, move || {
            if let Some(seat) = seat {
                nana_ui::runtime::view::IntoView::into_any(dialogue::peer_window(ui, seat))
            } else {
                nana_ui::runtime::view::IntoView::into_any(dialogue::display_window(ui))
            }
        });
    session.set_gpu_renderers(nanatype::ui::sweep::renderers());
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
    let mut partner = Core::new(settings, skin.clone());
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
        let own_image = render(ui, None, &out.join(format!("self-{host}.png")));
        let partner_image = render(ui, Some(0), &out.join(format!("partner-{host}.png")));
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
    let mut second = Core::new(Settings::default(), skin);
    second.handle(
        Cmd::SendMarkup("[color=#20a040]SECOND SECOND[/color]".into()),
        0,
    );
    second.handle(Cmd::Skip, 1);
    ui.peer_lanes[1]
        .display
        .set(Arc::new(second.display().clone()));
    ui.peer_seats.set(2);
    let first = render(ui, Some(0), &out.join("partner-seat-1.png"));
    let second_image = render(ui, Some(1), &out.join("partner-seat-2.png"));
    assert!(pixels(&first, [0, 128, 208]) > 40, "first partner missing");
    assert!(
        pixels(&second_image, [32, 160, 64]) > 40,
        "second partner missing"
    );
    assert_eq!(
        pixels(&first, [32, 160, 64]),
        0,
        "second partner leaked into the first seat's window"
    );
    assert_eq!(
        pixels(&second_image, [0, 128, 208]),
        0,
        "first partner leaked into the second seat's window"
    );
    for image in [&first, &second_image] {
        assert_eq!(
            pixels(image, [208, 0, 80]),
            0,
            "self leaked into a partner window"
        );
    }
    ui.peer_seats.set(0);
    ui.peer.set(Arc::new(SessionUi::default()));
    let inactive = render(ui, Some(0), &out.join("inactive.png"));
    assert_eq!(
        pixels(&inactive, [0, 128, 208]),
        0,
        "old partner content remained after leave"
    );
}
