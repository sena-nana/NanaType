//! Messages into the host program. Views, background threads and the tray all
//! speak this one type; [`crate::app::App::update`] is the only place that
//! changes state.

use nanatype_audio::AudioStatus;
use nanatype_bridge::BridgeStatus;
use nanatype_core::Cmd;
use nanatype_playback::CueKind;
use nanatype_settings::{Hotkey, InputKey, Settings};

use crate::windows::WindowKind;

/// An edit applied to the settings, then saved and propagated.
pub type SettingsEdit = Box<dyn FnOnce(&mut Settings) + Send>;

/// Global (system-wide) shortcut actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GlobalAction {
    Summon,
    Continue,
    Skip,
    Clear,
    ToggleMode,
    ToggleOverlays,
    /// Quick phrase `n` (Settings › Assets), inserted or sent like its menu
    /// item. Not in [`Self::ALL`]: phrases come and go with the settings.
    Phrase(usize),
}

impl GlobalAction {
    /// The fixed actions, in the order the Hotkeys page lists them.
    pub const ALL: [Self; 6] = [
        Self::Summon,
        Self::Continue,
        Self::Skip,
        Self::Clear,
        Self::ToggleMode,
        Self::ToggleOverlays,
    ];

    pub fn label_key(self) -> &'static str {
        match self {
            Self::Summon => "hotkeys-summon",
            Self::Continue => "hotkeys-continue",
            Self::Skip => "hotkeys-skip",
            Self::Clear => "hotkeys-clear",
            Self::ToggleMode => "hotkeys-toggle-mode",
            Self::ToggleOverlays => "hotkeys-toggle-overlays",
            Self::Phrase(_) => "assets-phrase-hotkey",
        }
    }

    /// Every action a chord can be bound to: the fixed ones, then one per
    /// quick phrase.
    pub fn all_in(s: &Settings) -> impl Iterator<Item = Self> + '_ {
        Self::ALL
            .into_iter()
            .chain((0..s.assets.phrases.len()).map(Self::Phrase))
    }

    pub fn get(self, s: &Settings) -> Option<&Hotkey> {
        let h = &s.hotkeys;
        match self {
            Self::Summon => h.summon.as_ref(),
            Self::Continue => h.continue_.as_ref(),
            Self::Skip => h.skip.as_ref(),
            Self::Clear => h.clear.as_ref(),
            Self::ToggleMode => h.toggle_mode.as_ref(),
            Self::ToggleOverlays => h.toggle_overlays.as_ref(),
            Self::Phrase(i) => s.assets.phrases.get(i)?.hotkey.as_ref(),
        }
    }

    /// The binding a fresh install has.
    pub fn default_binding(self) -> Option<Hotkey> {
        match self {
            Self::Phrase(_) => None,
            _ => Self::get(self, &Settings::default()).cloned(),
        }
    }

    pub fn set(self, s: &mut Settings, value: Option<Hotkey>) {
        if let Self::Phrase(i) = self {
            if let Some(p) = s.assets.phrases.get_mut(i) {
                p.hotkey = value;
            }
            return;
        }
        let h = &mut s.hotkeys;
        *match self {
            Self::Summon => &mut h.summon,
            Self::Continue => &mut h.continue_,
            Self::Skip => &mut h.skip,
            Self::Clear => &mut h.clear,
            Self::ToggleMode => &mut h.toggle_mode,
            Self::ToggleOverlays => &mut h.toggle_overlays,
            Self::Phrase(_) => unreachable!("handled above"),
        } = value;
    }
}

/// Anything a chord can be bound to: a system-wide hotkey, or one of the
/// input window's own keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Shortcut {
    Global(GlobalAction),
    Input(InputKey),
}

impl Shortcut {
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Global(a) => a.label_key(),
            Self::Input(k) => match k {
                InputKey::Send => "tb-send",
                InputKey::Newline => "cheat-newline",
                InputKey::RecallPrev => "hotkeys-recall-prev",
                InputKey::RecallNext => "hotkeys-recall-next",
                InputKey::Hide => "cheat-hide",
                InputKey::Bold => "tb-bold",
                InputKey::Italic => "tb-italic",
                InputKey::Underline => "tb-underline",
            },
        }
    }

    /// Every bindable action. A chord belongs to one of them only: a global
    /// hotkey takes its chord system-wide, so an input key sharing it would
    /// never fire.
    pub fn all_in(s: &Settings) -> impl Iterator<Item = Self> + '_ {
        GlobalAction::all_in(s)
            .map(Self::Global)
            .chain(InputKey::ALL.into_iter().map(Self::Input))
    }

    pub fn get(self, s: &Settings) -> Option<&Hotkey> {
        match self {
            Self::Global(a) => a.get(s),
            Self::Input(k) => k.get(&s.hotkeys.input),
        }
    }

    pub fn set(self, s: &mut Settings, value: Option<Hotkey>) {
        match self {
            Self::Global(a) => a.set(s, value),
            Self::Input(k) => *k.slot(&mut s.hotkeys.input) = value,
        }
    }

    /// Bind `value` here, taking it from whichever action had it.
    pub fn bind(self, s: &mut Settings, value: Option<Hotkey>) {
        if let Some(chord) = &value {
            let taken: Vec<Self> = Self::all_in(s)
                .filter(|o| *o != self && o.get(s) == Some(chord))
                .collect();
            for other in taken {
                other.set(s, None);
            }
        }
        self.set(s, value);
    }

    pub fn default_binding(self) -> Option<Hotkey> {
        match self {
            Self::Global(a) => a.default_binding(),
            Self::Input(k) => k.default_binding(),
        }
    }

    /// Whether `hotkey` may be bound here without swallowing typing.
    pub fn is_safe(self, hotkey: &Hotkey) -> bool {
        match self {
            Self::Global(_) => hotkey.is_safe_global(),
            Self::Input(_) => hotkey.is_safe_local(),
        }
    }
}

/// What an input window key does.
pub fn input_message(key: InputKey) -> Msg {
    match key {
        InputKey::Send => Msg::Core(Cmd::Send),
        InputKey::Newline => Msg::InsertNewline,
        InputKey::RecallPrev => Msg::Core(Cmd::Recall(-1)),
        InputKey::RecallNext => Msg::Core(Cmd::Recall(1)),
        InputKey::Hide => Msg::HideInput,
        InputKey::Bold => Msg::Tool(Tool::Bold),
        InputKey::Italic => Msg::Tool(Tool::Italic),
        InputKey::Underline => Msg::Tool(Tool::Underline),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileTarget {
    Avatar,
    InsertImage,
    ImportSkin,
    ExportSkin,
    StickerFolder,
}

pub enum Msg {
    RequestPluginActivation(String),
    ConfirmPluginActivation,
    CancelPluginActivation,
    DisablePlugin(String),
    RefreshPackages,
    RemovePackage(String),
    /// A running plugin's package folder (or a dependency's) changed on disk.
    PluginPackagesChanged,
    /// Fresh digests for running plugins, reviewed off the UI thread.
    PluginPackagesReviewed(Vec<(String, u64, Result<String, String>)>),
    /// Enabled plugins reviewed off the UI thread by sync `ticket`.
    PluginsReviewed {
        ticket: u64,
        reviews: crate::plugins::Reviews,
    },
    /// Package rows with verified signatures, from listing `ticket`.
    PackagesListed {
        ticket: u64,
        rows: Vec<crate::plugins::PackageRow>,
    },
    /// A package archive was inspected and installed off the UI thread.
    PackageInstalled(
        Result<
            (
                nanatype_theme::InstallOutcome,
                Box<nanatype_theme::SkinRegistry>,
            ),
            Vec<nanatype_theme::SkinDiagnostic>,
        >,
    ),
    /// A package export finished off the UI thread.
    PackageExported(Result<(), String>),
    /// A fresh snapshot for the current activation review or confirmation request.
    PluginReviewReady {
        ticket: u64,
        result: Result<std::sync::Arc<crate::plugins::ActivationReview>, String>,
    },
    PluginCommand {
        id: String,
        command: String,
    },
    PluginInput {
        id: String,
        event: serde_json::Value,
    },
    PluginResult {
        id: String,
        generation: u64,
        response: std::sync::Arc<nanatype_plugins::WorkerResponse>,
    },
    PeerStart {
        invitation: Option<String>,
        relay: String,
    },
    PeerLeave,
    PeerPanel(bool),
    PeerEvent(u64, nanatype_p2p::Event),
    /// A session command (send, continue, draft changed…).
    Core(Cmd),
    /// Change settings.
    Edit(SettingsEdit),
    /// Replace the editor's markup source (source mode).
    Markup(MarkupEdit),
    /// A formatting action from the toolbar or a shortcut.
    Tool(Tool),
    /// A raw editor command (scripted demos; phrase insertion).
    EditorCommand(nana_ui::runtime::RichEditCommand),
    /// The rich editor's document changed.
    EditorChanged(nana_ui_core::RichText),
    /// The rich editor's selection moved (byte offsets).
    EditorSelection(std::ops::Range<usize>),
    /// The `@` / `/` completion list.
    Complete(Complete),
    /// The rich editor found no text to paste (try an image).
    EditorPaste,
    EditorContext {
        source: bool,
        x: f32,
        y: f32,
        keyboard: bool,
    },
    EditorAction {
        source: bool,
        action: nana_ui::runtime::TextEditAction,
    },
    /// Switch between the rich editor and the markup source.
    ToggleSource,
    Hotkey(GlobalAction),
    /// Start recording a chord for an action; `None` stops recording.
    RecordHotkey(Option<Shortcut>),
    /// A chord was captured while recording.
    HotkeyCaptured(Hotkey),
    ToggleWindow(WindowKind),
    ShowWindow(WindowKind),
    OverlayAction(WindowKind, crate::services::overlay_menu::Action),
    HideInput,
    ResetWindow(WindowKind),
    OpenSettings,
    SettingsPage(crate::ui::settings::Page),
    /// The live-platforms page, open at this platform.
    SettingsPlatform(crate::chat::Platform),
    PickFile(FileTarget),
    DeleteSkin(String),
    ExportSkin(String),
    DuplicateSkin(String),
    OpenFolder(std::path::PathBuf),
    /// Open a web page in the default browser.
    OpenUrl(&'static str),
    CopyText(String),
    PlaySample,
    VtsAuthorize,
    VtsRefresh,
    InsertPhrase(usize),
    /// Break the line at the caret (the input window's newline key).
    InsertNewline,
    DismissNotice(u64),
    /// The tray stops showing error notice `id` in its tooltip.
    TrayErrorExpired(u64),
    /// A toast timed out or was closed; the host takes it out of its window.
    ToastDismissed(
        WindowKind,
        nana_ui::runtime::Entity<nana_ui::runtime::Toast>,
    ),
    /// Release paused output resources at their bounded deadline.
    OutputCleanup,
    /// A scheduled wake from the timer thread.
    Wake {
        epoch: u64,
        reason: nanatype_playback::WakeReason,
    },
    /// A visual or broadcast cue reached its time.
    Cue(CueKind),
    Audio(AudioStatus),
    Bridge(BridgeStatus),
    /// A sticker file changed on disk.
    AssetsChanged,
    Stickers(crate::stickers::Event),
    StickerDownload(crate::stickers::Preset),
    StickerRetry(std::path::PathBuf),
    StickerRemove(std::path::PathBuf),
    StickerRepository(std::path::PathBuf),
    StickerImportFolders(Vec<std::path::PathBuf>),
    Telegram(crate::stickers::telegram::Command),
    TelegramEvent(crate::stickers::telegram::Event),
    /// A file of the active (folder) skin changed and hot reload is on.
    SkinFilesChanged,
    /// Background layout materials finished; re-project the views using them.
    LayoutMaterialsReady,
    SkinScanReady {
        ticket: u64,
        result: Result<Box<crate::services::watch::PreparedSkins>, String>,
    },
    /// Prepare only the selected preview; static selection must not start motion.
    PrepareSkinPreview {
        id: String,
        play: bool,
    },
    SaveSkinPersonalization {
        id: String,
        profile: nanatype_settings::SkinPersonalization,
    },
    /// A second instance asked us to come forward.
    SecondInstance,
    /// Skin sounds finished decoding.
    SoundsLoaded {
        skin: String,
        /// The sound pack layered over the skin's sounds.
        pack: Option<String>,
        bank: crate::services::sounds::Bank,
        errors: Vec<String>,
    },
    /// The monitor layout was read.
    Displays(Vec<nanatype_settings::MonitorInfo>),
    HotkeyStatus(std::collections::HashMap<GlobalAction, crate::services::hotkeys::HotkeyState>),
    /// An image pasted into the editor (straight RGBA).
    PasteImage {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    /// The input window's native handle (for the summon hotkey).
    InputHwnd(isize),
    /// A chat message with stable references; pictures may still be loading.
    Chat(std::sync::Arc<crate::chat::ChatMessage>),
    /// A referenced picture is now in the local cache. Never adds messages.
    ChatAssetReady(nanatype_model::AssetRef),
    #[cfg(feature = "unofficial-chat")]
    /// The Bilibili connection changed.
    Bilibili(nanatype_bilibili::SessionEvent),
    #[cfg(feature = "unofficial-chat")]
    Douyin(nanatype_douyin::SessionEvent),
    #[cfg(feature = "unofficial-chat")]
    DouyinRefresh,
    #[cfg(feature = "unofficial-chat")]
    DouyinLogout,
    #[cfg(feature = "unofficial-chat")]
    DouyinImport(String),
    #[cfg(feature = "unofficial-chat")]
    Youtube(nanatype_youtube::SessionEvent),
    #[cfg(feature = "unofficial-chat")]
    /// Connect (again) to the configured YouTube stream.
    YoutubeRefresh,
    Twitch(nanatype_twitch::SessionEvent),
    TwitchLogin,
    TwitchCancelLogin,
    /// Open twitch.tv/activate for the pending device code.
    TwitchOpenActivation,
    TwitchRefresh,
    TwitchLogout,
    TwitchSendToggle,
    TwitchSendConfirm(bool),
    TwitchRetry(nanatype_model::LineId),
    TwitchSkipFailed(nanatype_model::LineId),
    #[cfg(feature = "unofficial-chat")]
    BilibiliLogin,
    #[cfg(feature = "unofficial-chat")]
    BilibiliCancelLogin,
    #[cfg(feature = "unofficial-chat")]
    BilibiliRefresh,
    #[cfg(feature = "unofficial-chat")]
    BilibiliLogout,
    #[cfg(feature = "unofficial-chat")]
    BilibiliOwnRoom,
    #[cfg(feature = "unofficial-chat")]
    BilibiliSendToggle,
    #[cfg(feature = "unofficial-chat")]
    BilibiliSendConfirm(bool),
    #[cfg(feature = "unofficial-chat")]
    BilibiliRetry(nanatype_model::LineId),
    #[cfg(feature = "unofficial-chat")]
    BilibiliSkipFailed(nanatype_model::LineId),
    /// The third-party (blivechat protocol) connection reported.
    Blivechat(nanatype_blive::BliveEvent),
    /// The 彗星号 relay reported.
    CometRelay(nanatype_blive::RelayStatus),
    /// Keep this blivechat 身份码 in the system's secret store; empty forgets it.
    BlivechatAuthCode(String),
    /// Make a new 彗星号 address; pages using the old one are refused.
    CometResetToken,
    /// Quote the message when `reply` is true; otherwise insert its author
    /// label at the caret.
    ChatMention {
        seq: u64,
        reply: bool,
    },
    /// Drop the pending reply.
    ClearReply,
    /// Empty the chat panel.
    ClearChat,
    #[cfg(feature = "updates")]
    /// The update checker reported.
    Update(crate::services::updater::Event),
    #[cfg(feature = "updates")]
    CheckUpdate,
    #[cfg(feature = "updates")]
    DownloadUpdate,
    #[cfg(feature = "updates")]
    CancelUpdate,
    #[cfg(feature = "updates")]
    /// Open the downloaded installer (or its folder).
    InstallUpdate,
    #[cfg(feature = "updates")]
    /// Stop reminding about the release on offer.
    SkipUpdate,
    #[cfg(feature = "updates")]
    OpenUpdateNotes,
    /// The Steam Workshop reported.
    Workshop(nanatype_steam::Event),
    /// Browse the Workshop for this kind of content.
    WorkshopBrowse(crate::workshop::Tag),
    /// Open a subscribed item's Workshop page.
    WorkshopOpen(u64),
    WorkshopUnsubscribe(u64),
    /// Download a subscribed item again.
    WorkshopDownload(u64),
    /// Show a message in the status line.
    Notice {
        text: String,
        error: bool,
    },
    /// A host callback changed UI signals outside a program update.
    UiChanged,
    Quit,
}

/// A toolbar or shortcut action on the draft. Applied to the rich editor's
/// selection, or as markup in source mode.
#[derive(Debug, Clone, PartialEq)]
pub enum Tool {
    Bold,
    Italic,
    Underline,
    Strike,
    Color(nanatype_model::Rgba8),
    /// Relative size, fixed pixels, or None to inherit the skin.
    Size(Option<nanatype_model::Size>),
    Font(Option<nanatype_model::FontSlot>),
    Align(Option<nanatype_model::ParagraphAlign>),
    Stroke(Option<nanatype_model::Stroke>),
    Shadow(Option<nanatype_model::Shadow>),
    Ruby(String),
    Effect(nanatype_model::ContinuousKind),
    Entrance(nanatype_model::EntranceKind),
    Pace(nanatype_model::Pace),
    /// Insert a marker or a picture at the caret.
    Insert(nanatype_model::Inline),
    ClearFormat,
}

/// The `@` / `/` completion list's keys and clicks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Complete {
    /// Fill in this row, or the chosen one.
    Accept(Option<usize>),
    /// Move the choice by rows.
    Move(i32),
    /// Close the list until another token starts.
    Dismiss,
}

/// Toolbar edits of the markup editor.
pub enum MarkupEdit {
    /// The user typed: the editor's whole text and selection (byte offsets).
    Typed {
        text: String,
        selection: (usize, usize),
    },
    /// Wrap the selection in `open` … `close` (or insert both at the caret).
    Wrap {
        open: String,
        close: String,
    },
    /// Insert at the caret, replacing the selection.
    Insert(String),
    /// Remove every tag inside the selection.
    Align(Option<nanatype_model::ParagraphAlign>),
    ClearFormat,
}
