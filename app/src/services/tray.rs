//! The notification-area icon and its menu. Labels follow the UI language
//! and say what a click does ("Show input window" / "Hide input window");
//! the tooltip says the mode, the skin and whether overlays are locked, or
//! the latest error. The image carries the same state at a glance (see
//! [`Badge`]). Menu clicks arrive on the UI thread's message loop and are
//! forwarded as [`Msg`]s.

use std::cell::{Cell, RefCell};

use nanatype_i18n::{I18n, Locale};
use nanatype_settings::{InputMode, Lock, Settings};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::msg::{GlobalAction, Msg};
use crate::windows::WindowKind;

const ID_INPUT: &str = "nt.input";
const ID_DISPLAY: &str = "nt.display";
const ID_HISTORY: &str = "nt.history";
const ID_CHAT: &str = "nt.chat";
const ID_LOCK: &str = "nt.lock";
const ID_CONTINUE: &str = "nt.continue";
const ID_SAMPLE: &str = "nt.sample";
const ID_COMPOSE: &str = "nt.mode.compose";
const ID_MIRROR: &str = "nt.mode.mirror";
const ID_SETTINGS: &str = "nt.settings";
const ID_OBS: &str = "nt.obs";
const ID_QUIT: &str = "nt.quit";
const ID_SPEECH: &str = "nt.speech";
const SKIN_PREFIX: &str = "nt.skin:";

/// Windows keeps 128 UTF-16 units of a tooltip, terminator included.
const TOOLTIP_MAX: usize = 120;

/// What the menu's labels and the tooltip were last built from.
#[derive(Debug, Clone, PartialEq)]
pub struct TrayState {
    pub speech_active: bool,
    pub locale: Locale,
    pub input: bool,
    pub display: bool,
    pub history: bool,
    pub chat: bool,
    pub locked: bool,
    /// The continue hotkey's label, if one is bound.
    pub continue_key: Option<String>,
    pub tooltip: String,
    /// What the icon itself shows.
    pub badge: Badge,
}

/// State drawn on the tray image: a pink dot while mirroring, the mark
/// greyed while every overlay is hidden, a small padlock while overlays are
/// locked. Composed at runtime over the app's own icon at the tray's size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Badge {
    pub mirroring: bool,
    pub hidden: bool,
    pub locked: bool,
}

pub struct Tray {
    speech: MenuItem,
    icon: TrayIcon,
    /// The plain tray image the badges are drawn over.
    base: image::RgbaImage,
    /// The badges the image currently shows.
    drawn: Cell<Badge>,
    input: MenuItem,
    display: MenuItem,
    history: MenuItem,
    chat: MenuItem,
    lock: MenuItem,
    continue_: MenuItem,
    sample: MenuItem,
    mode: Submenu,
    compose: CheckMenuItem,
    mirror: CheckMenuItem,
    skins_menu: Submenu,
    skins: Vec<(String, CheckMenuItem)>,
    settings: MenuItem,
    obs: MenuItem,
    quit: MenuItem,
    shown: RefCell<Option<TrayState>>,
}

/// The tray image: on Windows the size of a small icon at the system DPI
/// (16 at 100 %, 20 at 125 %, 24 at 150 %, 32 at 200 %), picked from the
/// app's multi-size icon so the shell need not scale it; on macOS the 64 px
/// PNG, which stays sharp when the status item draws it at 18 pt; elsewhere
/// the 32 px PNG.
fn base_image() -> Result<image::RgbaImage, String> {
    if let Some(img) = sized_image() {
        return Ok(img);
    }
    decode(include_bytes!("../../assets/tray-32.png"))
}

fn decode(png: &[u8]) -> Result<image::RgbaImage, String> {
    Ok(image::load_from_memory(png)
        .map_err(|e| e.to_string())?
        .into_rgba8())
}

fn to_icon(img: image::RgbaImage) -> Result<Icon, String> {
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).map_err(|e| e.to_string())
}

#[cfg(windows)]
fn sized_image() -> Option<image::RgbaImage> {
    use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
    use windows::Win32::UI::WindowsAndMessaging::SM_CXSMICON;
    // SAFETY: plain queries without pointers.
    let px = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem()) };
    let px = u32::try_from(px).ok().filter(|p| *p > 0).unwrap_or(16);
    image_at(px)
}

/// Menu-bar icons are drawn at 18pt. A 64px bitmap stays sharp at 2x.
#[cfg(target_os = "macos")]
fn sized_image() -> Option<image::RgbaImage> {
    decode(include_bytes!("../../assets/tray-64.png")).ok()
}

#[cfg(not(any(windows, target_os = "macos")))]
fn sized_image() -> Option<image::RgbaImage> {
    None
}

/// The app icon's image that best fits `px` (from the multi-size `.ico`).
pub fn image_at(px: u32) -> Option<image::RgbaImage> {
    decode(ico_png(include_bytes!("../../assets/nanatype.ico"), px)?).ok()
}

/// The Nana highlight pink of the mirroring dot.
const DOT: [u8; 3] = [232, 106, 146];
/// The padlock's ink: the Nana text navy.
const LOCK: [u8; 3] = [27, 47, 75];

/// `base` with `badge` drawn on it. Shapes are laid out on a 16-unit grid
/// scaled to the image, each with a 1-unit white keyline so it reads on a
/// light or dark taskbar, and anti-aliased by 4×4 supersampling.
pub fn badged(base: &image::RgbaImage, badge: Badge) -> image::RgbaImage {
    let mut img = base.clone();
    if badge.hidden {
        // Hidden overlays: the mark in grey at about half strength.
        for p in img.pixels_mut() {
            let [r, g, b, a] = p.0;
            let y = (0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b))
                .round()
                .clamp(0.0, 255.0) as u8;
            p.0 = [y, y, y, (f32::from(a) * 0.55).round() as u8];
        }
    }
    let u = img.width() as f32 / 16.0;
    if badge.locked {
        // A padlock in the bottom-left corner: a body and a shackle arch.
        let (cx, cy) = (4.0 * u, 8.0 * u);
        let (outer, inner) = (2.6 * u, 1.3 * u);
        let leg = (outer + inner) / 2.0;
        let lock = move |grow: f32| {
            move |x: f32, y: f32| {
                let body = x >= 0.5 * u - grow
                    && x <= 7.5 * u + grow
                    && y >= 10.0 * u - grow
                    && y <= 15.5 * u + grow;
                let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
                let arch = y <= cy && d <= outer + grow && d >= (inner - grow).max(0.0);
                let legs = y > cy
                    && y < 10.5 * u
                    && ((x - (cx - leg)).abs() <= (outer - inner) / 2.0 + grow
                        || (x - (cx + leg)).abs() <= (outer - inner) / 2.0 + grow);
                body || arch || legs
            }
        };
        paint(&mut img, lock(u), [255, 255, 255]);
        paint(&mut img, lock(0.0), LOCK);
    }
    if badge.mirroring {
        // A pink dot in the bottom-right corner.
        let (c, r) = (12.0 * u, 3.0 * u);
        let dot = move |grow: f32| {
            move |x: f32, y: f32| (x - c).powi(2) + (y - c).powi(2) <= (r + grow).powi(2)
        };
        paint(&mut img, dot(u), [255, 255, 255]);
        paint(&mut img, dot(0.0), DOT);
    }
    img
}

/// Draw `color` over `img` wherever `inside(x, y)` holds, the coverage of
/// each pixel measured at 4×4 sample points.
fn paint(img: &mut image::RgbaImage, inside: impl Fn(f32, f32) -> bool, color: [u8; 3]) {
    for (x, y, p) in img.enumerate_pixels_mut() {
        let hits = (0..16u8)
            .filter(|i| {
                let sx = x as f32 + (f32::from(i % 4) + 0.5) / 4.0;
                let sy = y as f32 + (f32::from(i / 4) + 0.5) / 4.0;
                inside(sx, sy)
            })
            .count();
        if hits == 0 {
            continue;
        }
        let a = hits as f32 / 16.0;
        let [r, g, b, da] = p.0;
        let da = f32::from(da) / 255.0;
        let out_a = a + da * (1.0 - a);
        let blend = |s: u8, d: u8| {
            ((f32::from(s) * a + f32::from(d) * da * (1.0 - a)) / out_a)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        p.0 = [
            blend(color[0], r),
            blend(color[1], g),
            blend(color[2], b),
            (out_a * 255.0).round() as u8,
        ];
    }
}

/// The PNG image of an `.ico` that best fits `px`: the smallest at least
/// that large, or else the largest. Only PNG-compressed entries count.
fn ico_png(ico: &[u8], px: u32) -> Option<&[u8]> {
    let u16_at = |at: usize| Some(u16::from_le_bytes(ico.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_le_bytes(ico.get(at..at + 4)?.try_into().ok()?));
    if u16_at(0)? != 0 || u16_at(2)? != 1 {
        return None;
    }
    let count = usize::from(u16_at(4)?);
    let mut entries: Vec<(u32, &[u8])> = (0..count)
        .filter_map(|i| {
            let at = 6 + i * 16;
            let size = match *ico.get(at)? {
                0 => 256,
                n => u32::from(n),
            };
            let len = usize::try_from(u32_at(at + 8)?).ok()?;
            let offset = usize::try_from(u32_at(at + 12)?).ok()?;
            let data = ico.get(offset..offset.checked_add(len)?)?;
            data.starts_with(b"\x89PNG").then_some((size, data))
        })
        .collect();
    entries.sort_by_key(|(size, _)| *size);
    entries
        .iter()
        .find(|(size, _)| *size >= px)
        .or(entries.last())
        .map(|(_, data)| *data)
}

fn toggle_msg(kind: WindowKind) -> Msg {
    Msg::ToggleWindow(kind)
}

fn menu_message(id: &str) -> Option<Msg> {
    if let Some(message) = super::overlay_menu::message(id) {
        return Some(message);
    }
    Some(match id {
        ID_INPUT => toggle_msg(WindowKind::Input),
        ID_DISPLAY => toggle_msg(WindowKind::Display),
        ID_HISTORY => toggle_msg(WindowKind::History),
        ID_CHAT => toggle_msg(WindowKind::Chat),
        ID_LOCK => Msg::Edit(Box::new(|s: &mut Settings| {
            let locked = s.overlays.display.lock == Lock::Locked;
            let lock = if locked {
                Lock::Interactive
            } else {
                Lock::Locked
            };
            s.overlays.display.lock = lock;
            s.overlays.peer_display.lock = lock;
            s.overlays.history.lock = lock;
        })),
        ID_CONTINUE => Msg::Hotkey(GlobalAction::Continue),
        ID_SAMPLE => Msg::PlaySample,
        ID_COMPOSE => Msg::Edit(Box::new(|s: &mut Settings| {
            s.general.input_mode = InputMode::Compose
        })),
        ID_MIRROR => Msg::Edit(Box::new(|s: &mut Settings| {
            s.general.input_mode = InputMode::Mirror
        })),
        ID_SETTINGS => Msg::OpenSettings,
        ID_OBS => Msg::SettingsPage(crate::ui::settings::Page::Output),
        ID_QUIT => Msg::Quit,
        ID_SPEECH => Msg::SpeechToggle,
        other => {
            let skin = other.strip_prefix(SKIN_PREFIX)?.to_owned();
            Msg::Edit(Box::new(move |s: &mut Settings| s.skin.id = skin))
        }
    })
}

impl Tray {
    /// Build on the UI thread. `post` forwards menu actions.
    pub fn build(post: impl Fn(Msg) + Send + Sync + Clone + 'static) -> Result<Self, String> {
        // One process-wide handler serves both tray and overlay native menus.
        // Install before fallible tray setup so right-click still works if the
        // notification-area icon cannot be created.
        let menu_post = post.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Some(message) = menu_message(event.id.0.as_str()) {
                menu_post(message);
            }
        }));
        let item = |id: &str| MenuItem::with_id(id, "", true, None);
        let check = |id: &str| CheckMenuItem::with_id(id, "", true, false, None);
        let input = item(ID_INPUT);
        let display = item(ID_DISPLAY);
        let history = item(ID_HISTORY);
        let chat = item(ID_CHAT);
        let lock = item(ID_LOCK);
        let continue_ = item(ID_CONTINUE);
        let sample = item(ID_SAMPLE);
        let compose = check(ID_COMPOSE);
        let mirror = check(ID_MIRROR);
        let mode = Submenu::new("", true);
        mode.append_items(&[&compose, &mirror])
            .map_err(|e| e.to_string())?;
        let skins_menu = Submenu::new("", true);
        let settings = item(ID_SETTINGS);
        let obs = item(ID_OBS);
        let quit = item(ID_QUIT);
        let speech = item(ID_SPEECH);
        let menu = Menu::new();
        menu.append_items(&[
            &input,
            &display,
            &history,
            &chat,
            &lock,
            &PredefinedMenuItem::separator(),
            &continue_,
            &sample,
            &speech,
            &PredefinedMenuItem::separator(),
            &mode,
            &skins_menu,
            &PredefinedMenuItem::separator(),
            &settings,
            &obs,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .map_err(|e| e.to_string())?;
        let base = base_image()?;
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("NanaType")
            .with_icon(to_icon(base.clone())?)
            .with_menu_on_left_click(false)
            .build()
            .map_err(|e| e.to_string())?;

        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                post(Msg::ShowWindow(WindowKind::Input));
            }
        }));
        Ok(Self {
            speech,
            icon,
            base,
            drawn: Cell::new(Badge::default()),
            input,
            display,
            history,
            chat,
            lock,
            continue_,
            sample,
            mode,
            compose,
            mirror,
            skins_menu,
            skins: Vec::new(),
            settings,
            obs,
            quit,
            shown: RefCell::new(None),
        })
    }

    /// Labels that do not depend on state.
    pub fn relabel(&self, i18n: &I18n, locale: Locale) {
        let t = |k: &str| i18n.t_in(locale, k);
        self.sample.set_text(t("tray-action-sample"));
        self.mode.set_text(t("tray-mode"));
        self.compose.set_text(t("mode-compose"));
        self.mirror.set_text(t("mode-mirror"));
        self.skins_menu.set_text(t("tray-skin"));
        self.settings.set_text(t("tray-action-settings"));
        self.obs.set_text(t("tray-action-obs"));
        self.quit.set_text(t("tray-quit"));
        // State labels are in the old language until the next `show`.
        self.shown.replace(None);
    }

    /// Check marks that follow the settings.
    pub fn sync(&self, s: &Settings) {
        self.compose
            .set_checked(s.general.input_mode == InputMode::Compose);
        self.mirror
            .set_checked(s.general.input_mode == InputMode::Mirror);
        for (id, item) in &self.skins {
            item.set_checked(*id == s.skin.id);
        }
    }

    /// Show/hide labels and the tooltip; only what changed is rewritten.
    pub fn show(&self, i18n: &I18n, state: TrayState) {
        let mut shown = self.shown.borrow_mut();
        let old = shown.as_ref();
        let t = |k: &str| i18n.t_in(state.locale, k);
        let changed = |f: fn(&TrayState) -> bool| old.is_none_or(|o| f(o) != f(&state));
        let label = |on: bool, show: &str, hide: &str| t(if on { hide } else { show });
        if changed(|s| s.speech_active) {
            self.speech.set_text(t(if state.speech_active {
                "speech-stop"
            } else {
                "speech-start"
            }));
        }
        if changed(|s| s.input) {
            self.input.set_text(label(
                state.input,
                "tray-action-show-input",
                "tray-action-hide-input",
            ));
        }
        if changed(|s| s.display) {
            self.display.set_text(label(
                state.display,
                "tray-action-show-display",
                "tray-action-hide-display",
            ));
        }
        if changed(|s| s.history) {
            self.history.set_text(label(
                state.history,
                "tray-action-show-history",
                "tray-action-hide-history",
            ));
        }
        if changed(|s| s.chat) {
            self.chat.set_text(label(
                state.chat,
                "tray-action-show-chat",
                "tray-action-hide-chat",
            ));
        }
        if changed(|s| s.locked) {
            self.lock.set_text(label(
                state.locked,
                "tray-action-lock",
                "tray-action-unlock",
            ));
        }
        if old.is_none_or(|o| o.continue_key != state.continue_key) {
            let name = t("tray-action-continue");
            self.continue_.set_text(match &state.continue_key {
                // A tab right-aligns the shortcut in a Windows menu.
                Some(key) if cfg!(windows) => format!("{name}\t{key}"),
                Some(key) => format!("{name} ({key})"),
                None => name,
            });
        }
        if old.is_none_or(|o| o.tooltip != state.tooltip) {
            let _ = self.icon.set_tooltip(Some(truncate(&state.tooltip)));
        }
        // Redraw the image only when a badge changes.
        if self.drawn.get() != state.badge
            && let Ok(icon) = to_icon(badged(&self.base, state.badge))
            && self.icon.set_icon(Some(icon)).is_ok()
        {
            self.drawn.set(state.badge);
        }
        *shown = Some(state);
    }

    /// Rebuild the skin submenu: `(id, display name)`.
    pub fn set_skins(&mut self, skins: &[(String, String)], active: &str) {
        for (_, item) in self.skins.drain(..) {
            let _ = self.skins_menu.remove(&item);
        }
        for (id, name) in skins {
            let item = CheckMenuItem::with_id(
                format!("{SKIN_PREFIX}{id}"),
                name,
                true,
                id == active,
                None,
            );
            let _ = self.skins_menu.append(&item);
            self.skins.push((id.clone(), item));
        }
    }
}

/// `text` cut to what a Windows tooltip keeps, with an ellipsis.
fn truncate(text: &str) -> String {
    if text.encode_utf16().count() <= TOOLTIP_MAX {
        return text.to_owned();
    }
    let mut out = String::new();
    let mut units = 0;
    for c in text.chars() {
        units += c.len_utf16();
        if units > TOOLTIP_MAX - 1 {
            break;
        }
        out.push(c);
    }
    out.push('…');
    out
}

/// The tooltip: `NanaType — mode · skin`, with a lock when overlays are
/// locked, or `NanaType · ⚠ message` while an error is the latest notice.
pub fn tooltip(
    i18n: &I18n,
    locale: Locale,
    mode: InputMode,
    skin: &str,
    locked: bool,
    error: Option<&str>,
) -> String {
    if let Some(message) = error {
        // One line: a multi-line error reads as its first line.
        let message = message.lines().next().unwrap_or_default();
        return i18n.tr_in(locale, "tray-tooltip-error", &[("message", message.into())]);
    }
    let mode = i18n.t_in(
        locale,
        match mode {
            InputMode::Compose => "mode-compose",
            InputMode::Mirror => "mode-mirror",
        },
    );
    let mut out = i18n.tr_in(
        locale,
        "tray-tooltip",
        &[("mode", mode.as_str().into()), ("skin", skin.into())],
    );
    if locked {
        out.push_str(" · 🔒");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_handler_keeps_tray_and_overlay_targets_separate() {
        assert!(matches!(
            menu_message(ID_DISPLAY),
            Some(Msg::ToggleWindow(WindowKind::Display))
        ));
        assert!(matches!(
            menu_message(ID_HISTORY),
            Some(Msg::ToggleWindow(WindowKind::History))
        ));
        assert!(matches!(
            menu_message("nt.overlay:peer_display:close"),
            Some(Msg::OverlayAction(
                WindowKind::PeerDisplay,
                super::super::overlay_menu::Action::Close
            ))
        ));
        let Some(Msg::Edit(edit)) = menu_message("nt.skin:nanatype.night") else {
            panic!("tray skin edit")
        };
        let mut settings = Settings::default();
        edit(&mut settings);
        assert_eq!(settings.skin.id, "nanatype.night");
        assert!(menu_message("nt.unknown").is_none());
    }

    #[test]
    fn icon_sizes_come_from_the_ico() {
        let ico = include_bytes!("../../assets/nanatype.ico");
        let size = |px| {
            let png = ico_png(ico, px).expect("png entry");
            image::load_from_memory(png).expect("decodes").width()
        };
        assert_eq!(size(16), 16);
        assert_eq!(size(20), 20);
        assert_eq!(size(24), 24);
        assert_eq!(size(30), 32);
        assert_eq!(size(48), 48);
        assert_eq!(size(512), 256);
    }

    #[test]
    fn tooltips_name_mode_skin_lock_and_errors() {
        let i18n = I18n::new(Locale::EnUs);
        let t = tooltip(&i18n, Locale::EnUs, InputMode::Mirror, "Halo", true, None);
        assert!(t.starts_with("NanaType — Live mirror · Halo"), "{t}");
        assert!(t.ends_with("🔒"), "{t}");
        let e = tooltip(
            &i18n,
            Locale::EnUs,
            InputMode::Compose,
            "Halo",
            false,
            Some("VTS refused\nmore"),
        );
        assert!(e.starts_with("NanaType · ⚠"), "{e}");
        assert!(e.contains("VTS refused") && !e.contains("more"), "{e}");
        let long = truncate(&"x".repeat(400));
        assert!(long.encode_utf16().count() <= TOOLTIP_MAX);
    }

    #[test]
    fn badges_draw_over_the_icon_at_every_size() {
        for px in [16, 20, 24, 32, 48] {
            let base = image_at(px).expect("icon");
            assert_eq!(base.width(), px);
            assert_eq!(badged(&base, Badge::default()), base, "{px}: plain");
            let at = |img: &image::RgbaImage, ux: f32, uy: f32| {
                let s = px as f32 / 16.0;
                img.get_pixel((ux * s) as u32, (uy * s) as u32).0
            };
            let dot = badged(
                &base,
                Badge {
                    mirroring: true,
                    ..Badge::default()
                },
            );
            assert_eq!(&at(&dot, 12.0, 12.0)[..3], &DOT, "{px}: dot");
            let lock = badged(
                &base,
                Badge {
                    locked: true,
                    ..Badge::default()
                },
            );
            assert_eq!(&at(&lock, 4.0, 13.0)[..3], &LOCK, "{px}: padlock");
            let grey = badged(
                &base,
                Badge {
                    hidden: true,
                    ..Badge::default()
                },
            );
            assert!(
                grey.pixels()
                    .zip(base.pixels())
                    .all(|(g, b)| g.0[0] == g.0[1] && g.0[1] == g.0[2] && g.0[3] <= b.0[3]),
                "{px}: greyed"
            );
        }
    }
}
