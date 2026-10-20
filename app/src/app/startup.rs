use super::demo_editor;
use crate::msg::Msg;
use crate::paths::Paths;
use crate::scheduler::Scheduler;
use crate::services::chat_assets::ChatAssets;
use crate::services::hotkeys::Hotkeys;
use crate::skin_theme;
use crate::ui::Ui;
use nana_ui::RuntimeProgramContext;
use nanatype_audio::{AudioEngine, AudioHandle};
use nanatype_bilibili::BilibiliHandle;
use nanatype_bridge::BridgeHandle;
use nanatype_i18n::Locale;
use nanatype_settings::{GeometryStore, Settings};
use std::sync::Arc;
use std::time::Duration;

/// Background services started with the app; each posts back into the program.
pub(super) struct Workers {
    pub scheduler: Scheduler,
    pub audio: AudioHandle,
    pub bridge: BridgeHandle,
    pub chat_assets: ChatAssets,
    pub bilibili: BilibiliHandle,
    pub hotkeys: Hotkeys,
    pub douyin: nanatype_douyin::DouyinHandle,
    pub blivechat: nanatype_blive::BliveHandle,
    pub comet_relay: nanatype_blive::RelayHandle,
    pub sticker_manager: crate::stickers::Manager,
    pub telegram: crate::stickers::telegram::Service,
}

impl Workers {
    pub fn start(
        paths: &Paths,
        settings: &Settings,
        post: impl Fn(Msg) + Clone + Send + Sync + 'static,
    ) -> Self {
        Self {
            scheduler: Scheduler::start(post.clone()),
            audio: AudioEngine::start(settings.sound.device.clone(), forward(&post, Msg::Audio)),
            bridge: BridgeHandle::new(forward(&post, Msg::Bridge)),
            chat_assets: ChatAssets::start(
                paths.blobs(),
                forward(&post, |m| Msg::Chat(Arc::new(m))),
                forward(&post, Msg::ChatAssetReady),
            ),
            bilibili: BilibiliHandle::new(paths.config.clone(), forward(&post, Msg::Bilibili)),
            hotkeys: Hotkeys::start(
                forward(&post, Msg::Hotkey),
                forward(&post, Msg::HotkeyStatus),
            ),
            douyin: nanatype_douyin::DouyinHandle::new(
                paths.config.clone(),
                forward(&post, Msg::Douyin),
            ),
            blivechat: nanatype_blive::BliveHandle::new(forward(&post, Msg::Blivechat)),
            comet_relay: nanatype_blive::RelayHandle::new(forward(&post, Msg::CometRelay)),
            sticker_manager: crate::stickers::Manager::new(
                &paths.data,
                forward(&post, Msg::Stickers),
            ),
            telegram: crate::stickers::telegram::Service::new(
                paths.data.clone(),
                forward(&post, Msg::TelegramEvent),
            ),
        }
    }
}

/// A callback posting each value wrapped in a message.
fn forward<T>(
    post: &(impl Fn(Msg) + Clone + Send + Sync + 'static),
    wrap: impl Fn(T) -> Msg + Send + Sync + 'static,
) -> impl Fn(T) + Send + Sync + 'static {
    let post = post.clone();
    move |value| post(wrap(value))
}

/// UI state seeded from settings, saved geometry and the platform.
pub(super) fn ui(
    ctx: &RuntimeProgramContext<Msg>,
    settings: &Settings,
    skin: &Arc<nanatype_theme::Skin>,
    locale: Locale,
    geometry: &GeometryStore,
    system_dark: bool,
) -> Ui {
    let ui = Ui::new(Arc::new(settings.clone()), skin.clone(), locale);
    let input_dock = geometry.input_dock.clone().unwrap_or_default().normalized();
    ui.input_dock_regions.set(input_dock.regions);
    ui.input_dock_selected.set(input_dock.selected);
    ui.input_sidebar_width
        .set(nanatype_settings::input_sidebar_width(
            geometry.input_sidebar_width,
        ));
    ui.chrome_dark
        .set(skin_theme::dark_mode(settings.skin.chrome, system_dark));
    ui.system_reduced_motion.set(ctx.reduced_motion());
    ui.reduced_motion
        .set(settings.playback.reduced_motion || ctx.reduced_motion());
    ui
}

/// Development switches: `NANATYPE_EXIT_AFTER` and (debug) `NANATYPE_DEMO`.
pub(super) fn dev_hooks(ctx: &RuntimeProgramContext<Msg>) {
    if let Some(ms) = std::env::var("NANATYPE_EXIT_AFTER")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
    {
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(ms));
            ctx.dispatch(Msg::Quit);
        });
    }
    if cfg!(debug_assertions) && std::env::var_os("NANATYPE_DEMO").is_some() {
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1500));
            ctx.dispatch(Msg::PlaySample);
            if std::env::var("NANATYPE_DEMO").as_deref() == Ok("editor") {
                demo_editor(&ctx);
            }
        });
    }
}
