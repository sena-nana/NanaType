//! Messages into the host program. Views, background threads and the tray all
//! speak this one type; [`crate::app::App::update`] is the only place that
//! changes state.

use nanatype_audio::AudioStatus;
use nanatype_bridge::BridgeStatus;
use nanatype_core::Cmd;
use nanatype_playback::CueKind;
use nanatype_settings::{Hotkey, InputDockTab, InputKey, Refusal, Settings};

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
    ShowChat,
    ReplyLatest,
    QuickPhrases,
    ToggleLock,
    Replay,
    ToggleHistory,
    NextCharacter,
    PrevCharacter,
    NextProfile,
    /// Quick phrase `n` (Settings › Assets), inserted or sent like its menu
    /// item. Not in [`Self::ALL`]: phrases come and go with the settings.
    Phrase(usize),
    /// Switch to character `n` (Settings › General). Not in [`Self::ALL`].
    Character(usize),
}

impl GlobalAction {
    /// The fixed actions, in the order the Hotkeys page lists them.
    pub const ALL: [Self; 15] = [
        Self::Summon,
        Self::ShowChat,
        Self::ReplyLatest,
        Self::QuickPhrases,
        Self::Continue,
        Self::Skip,
        Self::Clear,
        Self::Replay,
        Self::ToggleMode,
        Self::ToggleOverlays,
        Self::ToggleHistory,
        Self::ToggleLock,
        Self::NextCharacter,
        Self::PrevCharacter,
        Self::NextProfile,
    ];

    /// Whether the action brings the input window forward. On Windows the
    /// hotkey thread raises it, while it holds the foreground right.
    pub fn raises_input(self) -> bool {
        matches!(
            self,
            Self::Summon | Self::ShowChat | Self::ReplyLatest | Self::QuickPhrases
        )
    }

    pub fn label_key(self) -> &'static str {
        match self {
            Self::Summon => "hotkeys-summon",
            Self::Continue => "hotkeys-continue",
            Self::Skip => "hotkeys-skip",
            Self::Clear => "hotkeys-clear",
            Self::ToggleMode => "hotkeys-toggle-mode",
            Self::ToggleOverlays => "hotkeys-toggle-overlays",
            Self::ShowChat => "hotkeys-show-chat",
            Self::ReplyLatest => "hotkeys-reply-latest",
            Self::QuickPhrases => "hotkeys-quick-phrases",
            Self::ToggleLock => "hotkeys-toggle-lock",
            Self::Replay => "hotkeys-replay",
            Self::ToggleHistory => "hotkeys-toggle-history",
            Self::NextCharacter => "hotkeys-next-character",
            Self::PrevCharacter => "hotkeys-prev-character",
            Self::NextProfile => "hotkeys-next-profile",
            Self::Phrase(_) => "assets-phrase-hotkey",
            Self::Character(_) => "general-character-hotkey",
        }
    }

    /// Every action a chord can be bound to: the fixed ones, then one per
    /// quick phrase and one per character.
    pub fn all_in(s: &Settings) -> impl Iterator<Item = Self> + '_ {
        Self::ALL
            .into_iter()
            .chain((0..s.assets.phrases.len()).map(Self::Phrase))
            .chain((0..s.general.characters.len()).map(Self::Character))
    }

    /// What an action does when it is just a message: everything but the
    /// actions that raise the input window and the playback keys.
    pub fn message(self) -> Option<Msg> {
        Some(match self {
            Self::ToggleLock => Msg::Edit(Box::new(|s: &mut Settings| s.overlays.toggle_lock())),
            Self::Replay => Msg::Core(Cmd::Replay),
            Self::ToggleHistory => Msg::Window(WindowMsg::Toggle(WindowKind::History)),
            Self::NextCharacter => {
                Msg::Edit(Box::new(|s: &mut Settings| s.general.cycle_character(1)))
            }
            Self::PrevCharacter => {
                Msg::Edit(Box::new(|s: &mut Settings| s.general.cycle_character(-1)))
            }
            Self::Character(i) => Msg::Edit(Box::new(move |s: &mut Settings| {
                s.general.select_character(i)
            })),
            Self::NextProfile => Msg::Profile(ProfileMsg::Next),
            _ => return None,
        })
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
            Self::ShowChat => h.show_chat.as_ref(),
            Self::ReplyLatest => h.reply_latest.as_ref(),
            Self::QuickPhrases => h.quick_phrases.as_ref(),
            Self::ToggleLock => h.toggle_lock.as_ref(),
            Self::Replay => h.replay.as_ref(),
            Self::ToggleHistory => h.toggle_history.as_ref(),
            Self::NextCharacter => h.next_character.as_ref(),
            Self::PrevCharacter => h.prev_character.as_ref(),
            Self::NextProfile => h.next_profile.as_ref(),
            Self::Phrase(i) => s.assets.phrases.get(i)?.hotkey.as_ref(),
            Self::Character(i) => s.general.characters.get(i)?.hotkey.as_ref(),
        }
    }

    /// The binding a fresh install has.
    pub fn default_binding(self) -> Option<Hotkey> {
        match self {
            Self::Phrase(_) | Self::Character(_) => None,
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
        if let Self::Character(i) = self {
            if let Some(c) = s.general.characters.get_mut(i) {
                c.hotkey = value;
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
            Self::ShowChat => &mut h.show_chat,
            Self::ReplyLatest => &mut h.reply_latest,
            Self::QuickPhrases => &mut h.quick_phrases,
            Self::ToggleLock => &mut h.toggle_lock,
            Self::Replay => &mut h.replay,
            Self::ToggleHistory => &mut h.toggle_history,
            Self::NextCharacter => &mut h.next_character,
            Self::PrevCharacter => &mut h.prev_character,
            Self::NextProfile => &mut h.next_profile,
            Self::Phrase(_) | Self::Character(_) => unreachable!("handled above"),
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
                InputKey::ChatPrev => "hotkeys-chat-prev",
                InputKey::ChatNext => "hotkeys-chat-next",
                InputKey::Reply => "hotkeys-reply",
                InputKey::Mention => "hotkeys-mention",
                InputKey::CancelReply => "chat-reply-cancel",
                InputKey::ToggleChat => "hotkeys-toggle-chat",
                InputKey::Phrases => "hotkeys-phrases-panel",
                InputKey::Stickers => "hotkeys-stickers-panel",
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

    /// Why `hotkey` may not be bound here: it would swallow typing, take a
    /// chord the system or editor uses, or take the send key's Enter.
    pub fn refusal(self, hotkey: &Hotkey) -> Option<Refusal> {
        match self {
            Self::Global(_) => hotkey.refusal_global(false),
            Self::Input(k) => k.refusal(hotkey),
        }
    }

    /// What a refusal says, for this kind of shortcut.
    pub fn refusal_key(self, refusal: Refusal) -> &'static str {
        match (refusal, self) {
            (Refusal::Types, Self::Global(_)) => "hotkeys-unsafe",
            (Refusal::Types, Self::Input(_)) => "hotkeys-unsafe-local",
            (Refusal::Reserved, _) => "hotkeys-reserved",
            (Refusal::SendOnly, _) => "hotkeys-send-only",
        }
    }
}

/// What an input window key does.
pub fn input_message(key: InputKey) -> Msg {
    match key {
        InputKey::Send => Msg::Core(Cmd::Send),
        InputKey::Newline => Msg::Editor(EditorMsg::InsertNewline),
        InputKey::RecallPrev => Msg::Core(Cmd::Recall(-1)),
        InputKey::RecallNext => Msg::Core(Cmd::Recall(1)),
        InputKey::Hide => Msg::Window(WindowMsg::HideInput),
        InputKey::Bold => Msg::Editor(EditorMsg::Tool(Tool::Bold)),
        InputKey::Italic => Msg::Editor(EditorMsg::Tool(Tool::Italic)),
        InputKey::Underline => Msg::Editor(EditorMsg::Tool(Tool::Underline)),
        InputKey::ChatPrev => Msg::Chat(ChatMsg::Select(-1)),
        InputKey::ChatNext => Msg::Chat(ChatMsg::Select(1)),
        InputKey::Reply => Msg::Chat(ChatMsg::MentionSelected { reply: true }),
        InputKey::Mention => Msg::Chat(ChatMsg::MentionSelected { reply: false }),
        InputKey::CancelReply => Msg::Chat(ChatMsg::ClearReply),
        InputKey::ToggleChat => Msg::Chat(ChatMsg::Toggle),
        InputKey::Phrases => Msg::Chat(ChatMsg::DockTab(InputDockTab::Phrases)),
        InputKey::Stickers => Msg::Chat(ChatMsg::DockTab(InputDockTab::Stickers)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileTarget {
    /// Character `n`'s avatar.
    Avatar(usize),
    InsertImage,
    ImportSkin,
    ExportSkin,
    StickerFolder,
    SpeechModel,
    /// Save the session open in Settings › History as this format.
    ExportHistory(crate::archive::Format),
    ExportSettings,
    ImportSettings,
}

pub enum Msg {
    PeerStart {
        invitation: Option<String>,
        password: String,
    },
    /// Open the Steam overlay and invite friends into the room being hosted.
    PeerInviteFriends,
    PeerLeave,
    PeerPanel(bool),
    PeerEvent(u64, nanatype_p2p::Event),
    /// A session command (send, continue, draft changed…).
    Core(Cmd),
    /// Change settings.
    Edit(SettingsEdit),
    Hotkey(GlobalAction),
    /// Start recording a chord for an action; `None` stops recording.
    RecordHotkey(Option<Shortcut>),
    /// A chord was captured while recording.
    HotkeyCaptured(Hotkey),
    PickFile(FileTarget),
    OpenFolder(std::path::PathBuf),
    /// Package recent logs for a bug report (About page).
    ExportDiagnostics,
    DiagnosticsExported(Result<std::path::PathBuf, String>),
    /// Open a web page in the default browser.
    OpenUrl(&'static str),
    CopyText(String),
    PlaySample,
    VtsAuthorize,
    VtsRefresh,
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
    /// The skin step's entrance has started; mount its preview when that
    /// entrance is over. A newer hold drops the one it replaces.
    HoldOnboardPreview,
    /// Mount the skin step's preview, unless a newer hold replaced `generation`.
    RevealOnboardPreview {
        generation: u64,
    },
    HotkeyStatus(std::collections::HashMap<GlobalAction, crate::services::hotkeys::HotkeyState>),
    /// The input window's native handle (for the summon hotkey).
    InputHwnd(isize),
    /// The quick input bar's native handle (for the summon hotkey).
    QuickHwnd(isize),
    /// Send the quick bar's line as plain text and hide the bar.
    QuickSend(String),
    /// Hide the quick bar (Esc, or it lost focus), dropping its line.
    QuickHide,
    /// Show a message in the status line.
    Notice {
        text: String,
        error: bool,
    },
    /// A host callback changed UI signals outside a program update.
    UiChanged,
    Quit,
    // Each domain's messages, handled by its own module in `app/`.
    Speech(SpeechMsg),
    Tts(TtsMsg),
    Translate(TranslateMsg),
    Package(PackageMsg),
    Editor(EditorMsg),
    Window(WindowMsg),
    Skin(SkinMsg),
    Sticker(StickerMsg),
    Chat(ChatMsg),
    Twitch(TwitchMsg),
    Blive(BliveMsg),
    Obs(ObsMsg),
    #[cfg(feature = "updates")]
    Update(UpdateMsg),
    Workshop(WorkshopMsg),
    History(HistoryMsg),
    Backup(BackupMsg),
    Profile(ProfileMsg),
}

pub enum SpeechMsg {
    Toggle,
    Resume,
    Edit,
    Event(u64, nanatype_speech::Event),
    Tick(u64, u64),
    Refresh,
    Inventory(nanatype_speech::Inventory),
    ModelDownload,
    ModelCancel,
    ModelDelete,
    ModelProgress(u8),
    ModelDone(Result<(), String>),
}

pub enum TtsMsg {
    Event(nanatype_tts::Event),
    /// List the installed voices again.
    Refresh,
    /// Say a sample in a character's voice (`None`: the default voice).
    Test(Option<usize>),
}

pub enum TranslateMsg {
    Reply(nanatype_translate::Reply),
    /// Keep a provider's API key in the system's secret store; empty
    /// forgets it.
    Key(nanatype_settings::TranslateProvider, String),
    /// Translate a sample line with the current settings.
    Test,
}

pub enum PackageMsg {
    RequestActivation(String),
    ConfirmActivation,
    CancelActivation,
    DisablePlugin(String),
    Refresh,
    Remove(String),
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
    Listed {
        ticket: u64,
        rows: Vec<crate::plugins::PackageRow>,
    },
    /// A package archive was inspected and installed off the UI thread.
    Installed(
        Result<
            (
                nanatype_theme::InstallOutcome,
                Box<nanatype_theme::SkinRegistry>,
            ),
            Vec<nanatype_theme::SkinDiagnostic>,
        >,
    ),
    /// A package export finished off the UI thread.
    Exported(Result<(), String>),
    /// A fresh snapshot for the current activation review or confirmation request.
    ReviewReady {
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
}

pub enum EditorMsg {
    /// Replace the editor's markup source (source mode).
    Markup(MarkupEdit),
    /// A formatting action from the toolbar or a shortcut.
    Tool(Tool),
    /// A raw editor command (scripted demos; phrase insertion).
    Command(nana_ui::runtime::RichEditCommand),
    /// The rich editor's document changed.
    Changed(nana_ui_core::RichText),
    /// The rich editor's selection moved (byte offsets).
    Selection(std::ops::Range<usize>),
    /// The `@` / `/` completion list.
    Complete(Complete),
    /// The rich editor found no text to paste (try an image).
    Paste,
    Context {
        source: bool,
        x: f32,
        y: f32,
        keyboard: bool,
    },
    Action {
        source: bool,
        action: nana_ui::runtime::TextEditAction,
    },
    /// Switch between the rich editor and the markup source.
    ToggleSource,
    InsertPhrase(usize),
    /// Break the line at the caret (the input window's newline key).
    InsertNewline,
    /// An image pasted into the editor (straight RGBA).
    PasteImage {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
}

pub enum WindowMsg {
    Toggle(WindowKind),
    Show(WindowKind),
    OverlayAction(WindowKind, crate::services::overlay_menu::Action),
    HideInput,
    Reset(WindowKind),
    OpenSettings,
    SettingsPage(crate::ui::settings::Page),
    /// The live-platforms page, open at this platform.
    SettingsPlatform(crate::chat::Platform),
    /// A second instance asked us to come forward.
    SecondInstance,
    /// The monitor layout was read.
    Displays(Vec<nanatype_settings::MonitorInfo>),
}

pub enum SkinMsg {
    Delete(String),
    Export(String),
    Duplicate(String),
    /// A file of the active (folder) skin changed and hot reload is on.
    FilesChanged,
    /// Background layout materials finished; re-project the views using them.
    LayoutMaterialsReady,
    ScanReady {
        ticket: u64,
        result: Result<Box<crate::services::watch::PreparedSkins>, String>,
    },
    /// Prepare only the selected preview; static selection must not start motion.
    PreparePreview {
        id: String,
        play: bool,
    },
    /// A skin preview finished preparing in the background.
    PreviewReady {
        ticket: u64,
        result: Result<Box<crate::ui::PreparedPreview>, String>,
    },
    SavePersonalization {
        id: String,
        profile: nanatype_settings::SkinPersonalization,
    },
    /// Skin sounds finished decoding.
    SoundsLoaded {
        skin: String,
        /// The sound pack layered over the skin's sounds.
        pack: Option<String>,
        bank: crate::services::sounds::Bank,
        errors: Vec<String>,
    },
}

pub enum StickerMsg {
    /// A sticker file changed on disk.
    AssetsChanged,
    Event(crate::stickers::Event),
    Download(crate::stickers::Preset),
    Retry(std::path::PathBuf),
    Remove(std::path::PathBuf),
    Repository(std::path::PathBuf),
    ImportFolders(Vec<std::path::PathBuf>),
    Telegram(crate::stickers::telegram::Command),
    TelegramEvent(crate::stickers::telegram::Event),
}

pub enum ChatMsg {
    /// A chat message with stable references; pictures may still be loading.
    Message(std::sync::Arc<crate::chat::ChatMessage>),
    /// A referenced picture is now in the local cache. Never adds messages.
    AssetReady(nanatype_model::AssetRef),
    /// Quote the message when `reply` is true; otherwise insert its author
    /// label at the caret.
    Mention { seq: u64, reply: bool },
    /// Move the chat selection by this many messages: up from none selects
    /// the newest, down past the newest selects none.
    Select(i32),
    /// [`Self::Mention`] for the selected message, or the newest.
    MentionSelected { reply: bool },
    /// Drop the pending reply.
    ClearReply,
    /// Empty the chat panel.
    Clear,
    /// The input sidebar's panel, chat pin or fold changed: show it (the
    /// sidebar does at once), save it, and update the tray.
    Dock(nanatype_settings::InputDockLayout),
    /// Show live chat in the input sidebar, or put it away.
    Toggle,
    /// Bring the input window forward with live chat in view.
    Show,
    /// Open this sidebar panel, or fold the sidebar when it is open.
    DockTab(InputDockTab),
}

pub enum TwitchMsg {
    Event(nanatype_twitch::SessionEvent),
    Login,
    CancelLogin,
    /// Open twitch.tv/activate for the pending device code.
    OpenActivation,
    Refresh,
    Logout,
    SendToggle,
    SendConfirm(bool),
    Retry(nanatype_model::LineId),
    SkipFailed(nanatype_model::LineId),
}

pub enum BliveMsg {
    /// The third-party (blivechat protocol) connection reported.
    Event(nanatype_blive::BliveEvent),
    /// The 彗星号 relay reported.
    Relay(nanatype_blive::RelayStatus),
    /// Keep this blivechat 身份码 in the system's secret store; empty forgets it.
    AuthCode(String),
    /// Make a new 彗星号 address; pages using the old one are refused.
    ResetCometToken,
}

pub enum ObsMsg {
    /// The obs-websocket connection reported.
    Event(nanatype_obs::ObsEvent),
    /// Keep this obs-websocket password in the system's secret store; empty
    /// forgets it.
    Password(String),
    /// Give this window's OBS source a bounding box at its drawn size.
    FitBounds(crate::windows::WindowKind),
}

#[cfg(feature = "updates")]
pub enum UpdateMsg {
    /// The update checker reported.
    Event(crate::services::updater::Event),
    Check,
    Download,
    Cancel,
    /// Open the downloaded installer (or its folder).
    Install,
    /// Stop reminding about the release on offer.
    Skip,
    OpenNotes,
}

pub enum WorkshopMsg {
    /// The Steam Workshop reported.
    Event(nanatype_steam::Event),
    /// Browse the Workshop for this kind of content.
    Browse(crate::workshop::Tag),
    /// Open a subscribed item's Workshop page.
    Open(u64),
    Unsubscribe(u64),
    /// Download a subscribed item again.
    Download(u64),
}

/// Saved history (Settings › History).
pub enum HistoryMsg {
    /// The saved sessions, newest first, listed off the UI thread.
    Listed(Vec<crate::archive::Session>),
    /// Show this session's lines; `None` closes the viewer.
    Open(Option<String>),
    Loaded {
        name: String,
        records: Result<Vec<crate::archive::Record>, String>,
    },
    /// List a page of the open session's earlier lines.
    Earlier,
    /// Ask where to save the open session.
    Export(crate::archive::Format),
    Exported(Result<std::path::PathBuf, String>),
    Delete(String),
    OpenFolder,
}

/// Settings export, import, reset and the backup kept before them
/// (Settings › General).
pub enum BackupMsg {
    /// Ask where to save an export with the page's options.
    Export,
    /// The export's path and how many avatars stayed behind.
    Exported(Result<(std::path::PathBuf, usize), String>),
    /// Ask which file to import.
    Import,
    /// A chosen file was read, checked and migrated off the UI thread.
    Checked(Result<Box<nanatype_settings::Incoming>, nanatype_settings::ImportError>),
    /// Apply the checked import (`true`) or drop it.
    Confirm(bool),
    /// The import's avatars are written; apply its settings.
    Unpacked(Box<Settings>, Option<Box<nanatype_settings::GeometryStore>>),
    /// Everything back to defaults (asked first on the page).
    Reset,
    /// Back to the settings before the last import, reset or restore.
    Restore,
    Restored(Result<Box<Settings>, String>),
}

/// Named snapshots of the profile-scoped settings (Settings › General).
pub enum ProfileMsg {
    /// Save the current settings as a new profile with this name.
    SaveAs(String),
    /// Save the current settings into this profile.
    Update(String),
    Switch(String),
    /// Switch to the profile after the active one in the list.
    Next,
    Rename {
        id: String,
        name: String,
    },
    Duplicate(String),
    Delete(String),
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
    /// Toggle a looping effect; at other values it replaces the same kind.
    Effect(nanatype_model::Continuous),
    /// Toggle bold, spaced, one-character-at-a-time emphasis.
    Emphasis(nanatype_model::Emphasis),
    /// An entrance; `None` takes the skin's duration.
    Entrance(nanatype_model::EntranceKind, Option<u16>),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn edited(msg: Option<Msg>, mut s: Settings) -> Settings {
        let Some(Msg::Edit(edit)) = msg else {
            panic!("a settings edit")
        };
        edit(&mut s);
        s
    }

    #[test]
    fn new_actions_dispatch_their_messages() {
        assert!(matches!(
            GlobalAction::Replay.message(),
            Some(Msg::Core(Cmd::Replay))
        ));
        assert!(matches!(
            GlobalAction::ToggleHistory.message(),
            Some(Msg::Window(WindowMsg::Toggle(WindowKind::History)))
        ));
        let locked = edited(GlobalAction::ToggleLock.message(), Settings::default());
        let o = &locked.overlays;
        assert!(
            [o.display.lock, o.peer_display.lock, o.history.lock]
                .iter()
                .all(|l| *l == nanatype_settings::Lock::Locked)
        );
        assert_eq!(o.input.lock, nanatype_settings::Lock::Interactive);
        let unlocked = edited(GlobalAction::ToggleLock.message(), locked);
        assert_eq!(unlocked.overlays, Settings::default().overlays);

        let mut s = Settings::default();
        s.general.characters.push(nanatype_settings::Character {
            name: "Two".into(),
            ..Default::default()
        });
        let s = edited(GlobalAction::NextCharacter.message(), s);
        assert_eq!(s.general.character, 1);
        let s = edited(GlobalAction::NextCharacter.message(), s);
        assert_eq!(s.general.character, 0, "wraps around");
        let s = edited(GlobalAction::PrevCharacter.message(), s);
        assert_eq!(s.general.character, 1);
        let s = edited(GlobalAction::Character(0).message(), s);
        assert_eq!(s.general.character, 0);
        assert!(matches!(
            GlobalAction::NextProfile.message(),
            Some(Msg::Profile(ProfileMsg::Next))
        ));
        // Raising and playback actions are the app's own.
        assert!(GlobalAction::Summon.message().is_none());
        assert!(GlobalAction::Continue.message().is_none());
    }

    #[test]
    fn character_hotkeys_bind_like_the_fixed_ones() {
        let mut s = Settings::default();
        s.general.characters.push(Default::default());
        let chord = Hotkey::ctrl("Digit2");
        Shortcut::Global(GlobalAction::NextCharacter).bind(&mut s, Some(chord.clone()));
        Shortcut::Global(GlobalAction::Character(1)).bind(&mut s, Some(chord.clone()));
        assert_eq!(s.general.characters[1].hotkey.as_ref(), Some(&chord));
        assert_eq!(GlobalAction::NextCharacter.get(&s), None, "taken over");
        assert!(GlobalAction::all_in(&s).any(|a| a == GlobalAction::Character(1)));
        for action in GlobalAction::ALL {
            assert_eq!(
                action.default_binding(),
                action.get(&Settings::default()).cloned()
            );
        }
    }
}
