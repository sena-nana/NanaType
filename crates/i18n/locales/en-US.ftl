## App
app-name = NanaType
app-tagline = Typewriter for VTubers
history-count = { $count ->
    [one] { $count } line
   *[other] { $count } lines
}

## Window titles
window-input = Input
input-management = Stream & session
dock-tab-commands = Commands
dock-stickers = Stickers
dock-add-region = Add region
dock-close-region = Close current region
dock-more-regions = More regions
dock-wait = Wait 0.5s
window-display = Display
window-history = History
window-settings = Settings

## Tray
tray-mode = Input mode
tray-skin = Skin
tray-quit = Quit NanaType
tray-action-show-input = Show input window
tray-action-hide-input = Hide input window
tray-action-show-display = Show dialogue box
tray-action-hide-display = Hide dialogue box
tray-action-show-history = Show history
tray-action-hide-history = Hide history
tray-action-show-chat = Open chat panel
tray-action-hide-chat = Close chat panel
tray-action-lock = Lock overlays
tray-action-unlock = Unlock overlays
tray-action-continue = Continue
tray-action-sample = Play a sample line
tray-action-settings = Open settings…
tray-action-obs = Set up OBS…
tray-tooltip = NanaType — { $mode } · { $skin }
tray-tooltip-error = NanaType · ⚠ { $message }

## Input mode
mode-compose = Compose & send
mode-compose-hint = Write the line, then send it to play with the typewriter
mode-mirror = Live mirror
mode-mirror-hint = The display follows what you type
mode-compose-short = Compose
mode-mirror-short = Mirror

## Toolbar
tb-bold = Bold
tb-italic = Italic
tb-underline = Underline
tb-strike = Strikethrough
tb-color = Text color
tb-size = Size
tb-stroke = Outline
tb-shadow = Shadow
tb-stroke-none = Remove outline
tb-shadow-none = Remove shadow
tb-ruby = Ruby
tb-ruby-placeholder = Reading, e.g. かんじ
tb-ruby-apply = Add ruby
tb-ruby-remove = Remove ruby
tb-effects = Effects
tb-effect-shake = Shake
tb-effect-wave = Wave
tb-effect-jump = Jump
tb-effect-rainbow = Rainbow
tb-entrance-skin = Skin default
tb-entrance-none = None
tb-entrance-fade = Fade
tb-entrance-pop = Pop
tb-entrance-slide = Slide up
tb-pacing = Pacing
tb-wait-ms = Pause { $ms } ms
tb-speed-slow = Slower
tb-speed-fast = Faster
tb-speed-instant = Show instantly
tb-speed-reset = Normal speed
tb-pause = Wait for continue
tb-pause-auto = Pause, then continue
tb-page = New page
tb-insert = Insert
tb-sticker = Sticker
tb-sticker-search = Search stickers
tb-sticker-series = Sticker series
tb-sticker-all = All
tb-sticker-drop-hint = Drop folders here to add sticker packs
tb-sticker-drop-now = Release to import sticker folders
tb-sticker-importing = Importing { $name }…
tb-image = Image…
tb-sound = Sound effect
tb-expression = Expression
tb-expression-placeholder = Expression name, e.g. smile
tb-screenshake = Screen shake
tb-phrases = Quick phrases
tb-source = Markup source
tb-send = Send
tb-clear-format = Clear formatting
tb-more = More

## Editor
editor-placeholder = Type your line here…
input-plain-text = Plain text
input-skin-preview = Skin preview
editor-source-placeholder = Edit with [b]markup[/b], e.g. [shake]Whoa![/shake][wait=500]
editor-diagnostic = Character { $pos }: { $message }
marker-wait = wait { $ms }ms
marker-pause = continue
marker-pause-auto = wait { $ms }ms
marker-page = page
marker-sound = sound { $name }
marker-expression = expr { $name }
marker-portrait = portrait { $name }
marker-shake = shake
marker-vts = VTS { $name }
status-mirror = Mirroring
status-queued-chip = queued { $position }
status-sent = Sent
empty-phrases = No quick phrases yet
empty-stickers = No sticker folders yet
empty-open-assets = Add in Settings › Assets
empty-vts-hotkeys = No VTube Studio hotkeys yet

## Settings navigation
settings-general = General
settings-playback = Playback
settings-skins = Skins
settings-windows = Windows
settings-history = History
settings-hotkeys = Hotkeys
settings-output = Output / OBS
settings-integrations = Integrations
settings-assets = Assets
settings-about = About

## General
general-language = Language
general-language-system = System default
general-speaker = Character name
general-speaker-hint = Shown on the name plate and in history. Leave empty to hide the plate
general-avatar = Avatar
general-avatar-pick = Choose image…
general-avatar-clear = Remove
general-presence = Behavior
general-presence-resident = Always open
general-presence-summon = Summon with hotkey
general-presence-hint = In summon mode the input hides after sending or Esc
general-input = Input
general-mode = Input mode
general-send-key = Send key
general-send-key-enter = Enter
general-send-key-ctrl-enter = Ctrl+Enter
general-send-key-hint = The other combination starts a new line
general-autostart = Launch at login

## Playback
playback-speed = Typing speed
playback-typing = Typewriter effect
playback-typing-hint = When off, each line appears at once
playback-punct = Punctuation pauses
playback-punct-hint = Pause briefly after commas, stops and ellipses
playback-entrance = Character entrance
playback-after = After the line
playback-flow = Line flow
playback-after-hold = Keep showing
playback-after-fade = Fade out
playback-after-clear = Clear
playback-after-delay = Stay for
playback-after-delay-value = { $seconds } s
playback-auto = Advance automatically
playback-auto-off = Off
playback-auto-value = After { $seconds } s
playback-queue = Sending while a line plays
playback-queue-replace = Replace immediately
playback-queue-finish = Finish the current line first
playback-queue-queue = Queue until continue
playback-queue-replace-short = Replace
playback-queue-finish-short = Finish first
playback-queue-queue-short = Queue
playback-reduced = Reduce motion
playback-reduced-hint = Turns off per-character motion and screen shake
playback-stagger = Mirror character delay
playback-sound = Typing sounds
playback-audio = Sound
playback-volume = Master volume
playback-volume-typing = Typing volume
playback-volume-effects = Effects volume
playback-device = Output device
playback-device-default = System default

## Skins
skins-active = Current
skins-library = Skin library
skins-import = Import skin…
skins-export = Export…
skins-duplicate = Duplicate to edit
skins-open-folder = Open folder
skins-delete = Delete
skins-delete-confirm = Delete the skin “{ $name }”? This cannot be undone.
skins-builtin = Built-in
skins-broken = Cannot load
skins-by = by { $author }
skins-version = Version { $version }
skins-overrides = Personalize
skins-override-name = Name plate color
skins-override-box = Box color
skins-override-accent = Accent color
skins-override-overflow = Long text
skins-override-overflow-hint = What the box does when a line is taller than it
skins-override-overflow-skin = As the skin sets
skins-override-overflow-clip = Cut off
skins-override-overflow-scroll = Scroll along
skins-override-overflow-page = Page (wait, then clear)
skins-override-page-auto = Turn full pages after
skins-override-page-auto-hint = In page mode; 0 waits for the continue key
skins-override-reset = Reset to skin defaults
general-appearance = Appearance
skins-interface = Interface
general-appearance-system = Follow system
general-appearance-light = Light
general-appearance-dark = Dark
skins-hot-reload = Reload when files change
skins-advanced-effects = Advanced skin effects
skins-advanced-effects-hint = Enable textures, brushwork and lighting; static textures remain with reduced motion
skins-play-effects = Play effects
skins-stop-effects = Stop effects
skins-hot-reload-hint = For skin authors
skins-installed = Installed “{ $name }”
skins-replaced = Updated “{ $name }” from { $previous }
skins-import-failed = The skin could not be imported
skins-fallback = Skin “{ $id }” could not load; using the default skin
skins-preview = Preview
skins-preview-name = Nana
skins-filter-all = All
skins-tag-light = Light
skins-tag-dark = Dark
skins-tag-classic = Classic
skins-tag-streaming = Streaming
skins-tag-game = Game
skins-tag-retro = Retro
skins-tag-cute = Cute
skins-tag-elegant = Elegant
skins-tag-minimal = Minimal

## Windows
windows-input = Input
windows-display = Display
windows-history = History
windows-visible = Show
windows-lock = Interaction
windows-lock-interactive = Interactive
windows-on-top = Always on top
windows-vertical-text = Vertical text
windows-background = Capture background
windows-background-transparent = Transparent
windows-background-chroma = Chroma key
windows-reset = Reset position
windows-size = Size
windows-size-skin = Follow skin

## Overlay context menu
overlay-menu-skin = Switch skin (all windows)
overlay-menu-adjust-skin = Adjust skin…
overlay-menu-lock = Lock window (click-through; unlock from tray or Settings)
overlay-menu-reset = Restore default position and size
overlay-menu-close = Close window
overlay-menu-continue = Continue playback
overlay-menu-skip = Skip current playback
overlay-menu-clear-display = Clear display
overlay-menu-confirm-clear = Confirm clear
overlay-menu-clear-history-peer = Clear history (unavailable during a session)

## History
history-max = Lines kept
history-visible = Lines shown
history-fade = Fade out
history-avatar = Show avatar
history-name = Show name
history-time = Show time
history-sync = Appears
history-sync-submit = When sent
history-sync-reveal = Typing along
history-sync-complete = When fully shown
history-hide-latest = Hide the newest line
history-hide-latest-hint = The line still on the display window joins the history once the box clears or the next line replaces it, like Echo-Live
history-hide-latest-skin = As the skin says
history-hide-latest-always = Hide
history-hide-latest-never = Don't hide
history-clear = Clear history
history-clear-confirm = Clear all history of this session?
history-session = Session history

## Hotkeys
hotkeys-summon = Summon input
hotkeys-continue = Continue
hotkeys-skip = Skip line
hotkeys-clear = Clear display
hotkeys-toggle-mode = Switch input mode
hotkeys-toggle-overlays = Show/hide overlays
hotkeys-record = Record
hotkeys-clear-binding = Clear
hotkeys-ok = Registered
hotkeys-conflict-os = Used by another program
hotkeys-conflict-dup = Same as “{ $action }”
hotkeys-unsafe = A single key would block normal typing; add Ctrl, Alt or Shift
hotkeys-badge-dup = Duplicate
hotkeys-badge-conflict = Conflict
hotkeys-badge-unsafe = Unsafe
hotkeys-click-to-set = Click to set
hotkeys-press-keys = Press keys · Esc to cancel
hotkeys-local = Input window shortcuts

## Output
output-display = Display
output-history = History
output-spout = Spout2 output
output-syphon = Syphon output
output-spout-name = Sender name
output-spout-copy = Copy name
output-spout-size = Output resolution
output-spout-size-window = Match window
output-spout-straight = Straight alpha
output-spout-straight-hint = Turn on if the receiver has no premultiplied alpha mode
output-status-active = Sending
output-status-unavailable = Unavailable: { $reason }
output-spout-unsupported = Spout2 is Windows-only; use window capture
output-syphon-missing = Syphon.framework was not found
output-syphon-publish = Could not publish the frame
output-sample = Play a sample line
output-sample-line = [b]Hello![/b] This is a NanaType [wave]sample line[/wave].[wait=300][shake]Can you see it?[/shake]
output-obs-title = Set up OBS
output-obs-window = You can also use “Window Capture” (method: Windows 10 1903 and up); the display window background is transparent.
output-obs-mac = Or use “macOS Screen Capture” and pick the display window.

## Integrations
int-vts = VTube Studio
int-vts-hint = Turn on VTube Studio expressions and press hotkeys as [expr=name] and [vts …] markers are reached
int-vts-port = Port
int-vts-authorize = Request access
int-vts-authorize-hint = Then allow the NanaType plugin inside VTube Studio
int-vts-map = Expression mapping
int-vts-map-expression = Marker
int-vts-map-hotkey = VTS hotkey
int-vts-map-auto = Auto (same name)
int-vts-refresh = Refresh hotkeys
int-vts-lipsync = Lip sync
int-vts-lipsync-hint = Move the model's mouth while text is typing
int-vts-mouth-param = Mouth parameter
int-vts-mouth-param-hint = Input parameter driven while talking (MouthOpen by default)
int-vts-mouth-amp = Mouth opening
int-vts-reset = Reset expressions
int-vts-reset-hint = Turn off the expressions a line turned on
int-vts-reset-line-end = When the line leaves the box
int-vts-reset-next-line = When the next line starts
int-vts-reset-off = Never
int-osc = OSC
int-osc-hint = Send OSC messages to VNyan, Warudo, VSeeFace and others
int-osc-host = Host
int-osc-port = Port
int-osc-prefix = Address prefix
int-ws = WebSocket broadcast
int-ws-hint = Broadcast line and expression events (JSON) on a local port
int-ws-port = Port
int-ws-clients = { $count ->
    [one] { $count } client connected
   *[other] { $count } clients connected
}
int-status-off = Off
int-status-connecting = Connecting…
int-status-connected = Connected
int-status-needs-token = Needs access
int-status-error = Error: { $message }

int-off-hint = Turn this on to set up the connection
output-off-hint = Turn this on to set its name and size
## Assets
telegram-stickers = Telegram stickers
telegram-hint = Search and download packs through Telegram's official API. First login requires an API ID and API Hash from my.telegram.org; reconnect to restore a saved session. Supports WebP, TGS and WebM, with offline playback after import.
telegram-api-id = API ID
telegram-api-hash = API Hash
telegram-phone = Phone number (with + country code)
telegram-proxy = Download proxy
telegram-proxy-hint = Leave empty for a direct connection. Supports SOCKS5 with authentication (a proxy user and password are saved in plain text in the settings). Reconnect after changing it; login, search and downloads all use this proxy.
telegram-connect = Connect / log in
telegram-code = Telegram login code
telegram-password = Two-step verification password
telegram-verify = Verify
telegram-working = Connecting or processing request…
telegram-offline = Not connected to Telegram
telegram-code-sent = Login code sent. Check Telegram or SMS.
telegram-password-required = This account requires a two-step verification password.
telegram-ready = Logged in. Search and download sticker packs.
telegram-query = Keywords, pack name or t.me/addstickers link
telegram-search = Search packs
telegram-download = Download by link / name
telegram-no-results = No packs found. Try other keywords or paste a pack link.
telegram-logout = Log out and clear session
telegram-source = Telegram source
telegram-count = { $count } stickers
assets-stickers = Sticker packs
assets-stickers-hint = Download a preset or add a local folder, including Echo-Live packs. Images and animations are cached during import for offline use.
stickers-preset-pixel = Sheep's pixel heads (static)
stickers-preset-animated = Sheep's animated emotes
stickers-not-downloaded = Not downloaded
stickers-downloading = Downloading { $done } / { $total }
stickers-processing = Preparing cache { $done } / { $total }
stickers-ready = Ready · { $count } stickers
stickers-failed = Failed: { $message }
stickers-repository = GitHub repository
stickers-download = Download
stickers-retry = Retry
assets-add-folder = Add folder…
assets-remove = Remove
assets-remove-folder-confirm = Remove this folder from the stickers?
assets-phrases = Quick phrases
assets-phrase-label = Label
assets-phrase-markup = Content (markup allowed)
assets-phrase-send = Send directly
assets-phrase-add = Add phrase
assets-phrase-remove-confirm = Delete this phrase?
assets-phrase-hotkey = Hotkey

## About
about-build = Build { $build }
about-logs = Logs folder
about-data = Data folder
about-repository = Official repository
about-bilibili = Official Bilibili
about-disclaimer = Third-party notice
about-disclaimer-body = NanaType is an independent third-party tool. It is not affiliated with, endorsed by or authorised by Bilibili, Douyin or any other platform or its operators. Names and trademarks belong to their respective owners.
about-custom-skins = Custom skins
about-custom-skins-body = If you need a custom skin, you can contact the relevant creators or the developer, who offers a basic customisation service.
about-contact = Contact the developer
about-open = Open
about-open-link = Open link
about-expand = Expand
about-collapse = Collapse
about-disclaimer-unofficial = Unofficial software
about-disclaimer-unofficial-hint = Not affiliated with Bilibili, Douyin or other platforms
about-licenses = Open-source licences

## Onboarding
onboard-welcome = Welcome to NanaType
onboard-welcome-body = A few steps and the typewriter is ready for your stream.
onboard-language = Choose a language
onboard-speaker = Your character name
onboard-skin = Choose a skin
onboard-hotkeys = Hotkeys
onboard-hotkeys-body = Press { $summon } to summon the input and { $continue } to continue.
onboard-obs = Connect OBS
onboard-next = Next
onboard-back = Back
onboard-skip = Skip
onboard-done = Get started

## Common
common-cancel = Cancel
common-apply = Apply
common-reset = Reset
common-copied = Copied
common-ms = { $ms } ms
common-skin-default = Skin default

## Errors and notices
err-settings-quarantined = The settings file was damaged. It was saved to { $path } and defaults are in use
err-settings-newer = These settings come from a newer NanaType; changes made now will not be saved
err-audio = Audio device unavailable: { $message }
err-image = The image could not be read: { $message }
err-spout = Spout output failed: { $message }
err-syphon = Syphon output failed: { $message }
# Bilibili
window-chat = Chat
chat-reply = Reply
chat-at-short = Mention
chat-style-compact = Compact
chat-style-history = Bubbles
chat-panel-style = Chat panel style
chat-replying = Replying to { $name }
chat-reply-cancel = Cancel reply
chat-clear = Clear
chat-filter-text = Comments
chat-filter-super-chat = Super Chats
chat-filter-gift = Gifts
chat-filter-member = Memberships
chat-empty = Waiting for chat…
chat-not-configured = No live room connected yet
chat-gift = sent { $gift } ×{ $count }
chat-member = became { $guard } ×{ $count } { $unit }
chat-guard-governor = Governor
chat-guard-admiral = Admiral
chat-guard-captain = Captain
chat-min-gift = Minimum gift value
chat-min-gift-hint = In yuan; free gifts are worth 0
chat-status-retrying = Disconnected, retrying: { $message }
history-chat = Live chat
history-chat-show = Show live chat in history
history-chat-show-hint = Comments, Super Chats, gifts and memberships from Bilibili join your lines in the history list
history-show-quote = Show reply quotes
history-show-quote-hint = A line that answers a comment shows the comment above it
history-role-host = Host
history-role-viewer = Viewer
int-bilibili-open-panel = Open the chat panel
int-bilibili-quote-display = Quote replies on the dialogue box

## wave3-input
tb-cheatsheet = Markup cheat sheet
tb-stroke-white = White outline
tb-stroke-black = Black outline
tb-stroke-accent = Accent outline
tb-shadow-drop = Drop
tb-shadow-glow = Glow
cheat-markers = Markers
cheat-shortcuts = Shortcuts
cheat-wait = Wait this many ms, then go on
cheat-pause = Stop until you continue
cheat-page = Start a new page
cheat-expr = Switch the portrait's expression
cheat-se = Play a sound effect
cheat-sticker = Insert a sticker
cheat-at = Mention a viewer
cheat-newline = New line
cheat-recall = Recall a sent line
cheat-hide = Hide the input window
## end wave3-input

## wave3-settings
settings-group-input = Input
settings-group-look = Look
settings-group-stream = Stream
settings-group-platforms = Platforms
settings-general-sub = Language, your name and how the input window behaves
settings-playback-sub = How lines type out, move on and sound
settings-skins-sub = Pick a look for the dialogue box and make it yours
settings-windows-sub = Show, lock and reset the display, history and input windows
settings-history-sub = What the history window keeps and shows
settings-hotkeys-sub = System-wide shortcuts that also work in a game or OBS
settings-output-sub = Send the display and history to OBS
settings-integrations-sub = Live chat, VTube Studio and other apps
settings-assets-sub = Stickers and quick phrases for the input window
settings-about-sub = Version, folders and links
output-obs-step-plugin = Install the Spout2 plugin for OBS
output-obs-step-enable = First enable the display output in NanaType and wait until it is ready:
output-obs-enable = Enable display output
output-obs-enabled = Display output enabled
output-obs-step-source = Add a “Spout2 Capture” source and pick this sender:
output-obs-step-alpha = Set Composite mode to “Premultiplied Alpha”
output-obs-step-syphon = Add a “Syphon Client” source (included with OBS on macOS)
output-obs-step-syphon-source = Pick this sender:
output-obs-other = Other ways
int-vts-authorized = Authorized
int-vts-map-empty = Connect and authorize to list your VTS hotkeys
skins-more = More actions
## end wave3-settings

## wave4-skins
skins-view-grid = Cards
skins-view-list = List
windows-lock-locked-short = Locked
windows-lock-tip = Interactive (default): left-drag to move, use the wheel to scale, or drag an edge to resize. Locked: mouse input passes through to the window below.
windows-vertical-text-tip = Only changes the writing direction; the skin's landscape or portrait layout still follows the window's shape.
windows-layout-hint = The display, partner display, and history windows can be moved and scaled by default. Left-drag anywhere inside to move; scroll up to enlarge or down to shrink while keeping the aspect ratio. Right-click to close, switch or adjust the shared skin, toggle always on top, lock, or restore the default layout. The display also offers playback controls; history offers visibility options and a clear confirmation. Position and size are saved automatically. Locked windows pass mouse input through; unlock from the tray or Settings. Closing a window keeps enabled OBS outputs running.
windows-background-tip = Transparent keeps the alpha for OBS. Chroma key fills the window green, for a capture that has no alpha.
windows-reset-tip = The reset button puts the window back where the skin places it, at the skin's size
windows-input-na = “—” does not apply: the input window is always interactive and is not captured.
history-window-link = Window & capture
history-window-link-hint = Show, lock and size the history window, or send it to OBS
## end wave4-skins

## wave4-onboarding
onboard-step = Step { $step } of { $total }
onboard-skin-hint = Pick a look for the dialogue box; the preview shows it live. The Skins page has the rest.
onboard-mode-body = Choose how your lines reach the display. You can switch any time from the input window.
onboard-obs-hint = Show the display in your stream through Spout2
onboard-reopen = You can open this guide again from About.
about-setup-guide = Setup guide
playback-reduced-system-on = System setting: on
playback-reduced-system-off = System setting: off
## end wave4-onboarding

## wave5-fonts
skins-fallback-font = Fallback font
## end wave5-fonts

## wave5-fonts-licences
about-fonts = Open-source fonts
about-fonts-hint = Fonts bundled for the built-in skins, with their full licences
about-fonts-author = By { $author }
about-fonts-version = Version: { $version }
about-fonts-license-other = Licence below
about-design-echo = The Vanilla, Glass, Bubble and Void skins reference designs from sheep-realms/Echo-Live, by Sheep-realms and contributors. NanaType implements and adapts these styles in its local skin format.
about-design-upstream-license = Upstream licensing
about-license-project = Original code: MIT. Inherited material keeps its licence. The Bilibili component and combined app also include CC BY-NC 4.0 material, subject to attribution and NonCommercial conditions.
about-source-api-collect = Original Bilibili protocol documentation by SocialSisterYi and contributors. Adapted live protocol descriptions and WBI material retain CC BY-NC 4.0 attribution and NonCommercial conditions. The original repository is unavailable; a mirror is linked.
about-source-douyin = WebSocket signing and HTTP a_bogus ported from revision 0e506f3; connection parameters reference the same revision. Copyright (c) 2025 今晚打打.
about-source-repository = Source repository
about-source-mirror = Documentation mirror
## end wave5-fonts-licences
## round2-skins
skins-fallback-font-missing = { $family } not installed
## end round2-skins
## round2-status
link-fault-refused = The other program is not running (connection refused)
link-fault-timeout = Timed out
link-fault-closed = The other side closed the connection
link-fault-port-in-use = Port { $port } is used by another program
link-fault-unresolved = Cannot find host { $host }
link-fault-silent = No data for too long
link-fault-remote = The other side said: { $message }
link-fault-remote-empty = The other side refused the request
output-status-native-unavailable = The graphics card cannot share the output texture
output-status-failed = The output picture could not be produced
spout-error-backend = Spout could not start
spout-error-interop = The picture cannot be shared between graphics devices
spout-error-format = The picture's format or size is not supported
spout-error-publish = The picture could not be sent
spout-error-name = The sender name is not valid
err-tray = Tray icon unavailable: { $message }
skins-export-failed = The skin could not be exported: { $message }
skins-delete-failed = The skin could not be deleted: { $message }
## end round2-status

## Chat interaction
chat-at-action = Mention
chat-latest = Back to latest
chat-inserted = Inserted @{ $name }
chat-queued = Queued · { $count }
chat-room-owner = Room owner
chat-moderator = Moderator

# Two-person sessions
peer-title = Two-person session
peer-expand = Expand
peer-collapse = Collapse
peer-off = Offline
peer-connecting = Connecting…
peer-waiting = Waiting for partner
peer-reconnecting = Reconnecting…
peer-direct = Direct
peer-relay = Relay
peer-error = Connection failed
peer-create = Create session
peer-join = Join
peer-leave = Leave
peer-copy = Copy invitation
peer-invitation = Paste your partner's invitation
peer-relay-placeholder = Custom relay URL (empty for public relays)
peer-own-controls = You and your partner have separate display windows. Shortcuts control only your own track.
peer-paused = Wait for the session to reconnect before sending or controlling playback. You can keep editing.

# Independent partner output
window-peer-display = Partner display
windows-peer-display = Partner
output-peer-display = Partner display
peer-window-toggle = Partner window
output-name-conflict = Output names must be unique. Choose a different name for each output.

output-peer-hint = Separate partner output during a session; keeps sending while hidden.

tb-effect-pulse = Pulse
tb-effect-flicker = Flicker

# Editor context menu
edit-undo = Undo
edit-redo = Redo
edit-cut = Cut
edit-copy = Copy
edit-paste = Paste
edit-select-all = Select all

# Bilibili direct connection and authenticated delivery
settings-bilibili = Bilibili Live
settings-bilibili-sub = Account, live chat and typewriter delivery
bili-account = Bilibili account
bili-login = Sign in with QR code
bili-manage = Manage Bilibili Live
bili-login-needed = Sign in to Bilibili first
bili-restoring = Checking your session…
bili-expired = Session expired. Scan to sign in again.
bili-login-failed = Sign-in unavailable
bili-qr-waiting = Scan with the Bilibili app
bili-qr-scanned = Scanned. Confirm on your phone.
bili-qr-expired = QR code expired
bili-qr-refresh = Refresh QR code
bili-refresh = Refresh account
bili-logout = Sign out
bili-sync-profile = Use Bilibili name and avatar
bili-sync-profile-hint = Keep your profile updated; turn off to restore your custom identity
bili-identity-hint = Your name and avatar are synced from Bilibili. Manage this in Bilibili Live.
bili-room = Target live room
bili-room-placeholder = Blank uses your own room
bili-room-hint = Receive and send in the same room; short room IDs are accepted
bili-own-room = Use my live room
bili-connect = Connect
bili-receive = Receive live chat
bili-send-section = Typewriter delivery
bili-send-switch = Send to chat
bili-send-hint = Only submitted lines are sent. Off at each application start.
bili-send-title = Enable sending to live chat?
bili-send-warning = Submitted lines will also be sent to this room. Images and stickers are unavailable. Text styling and performances become plain text in chat; long lines are split automatically. Local performances are preserved.
bili-enable = Enable sending
bili-images-unavailable = Remove images and stickers and keep readable text before sending to chat.
bili-panel = Chat panel
bili-queued = Queued
bili-sending = Sending
bili-submitted = Accepted by Bilibili
bili-failed = Failed
bili-uncertain = Result unknown; do not resend blindly
bili-cancelled = Remaining delivery cancelled
bili-delivery-failed = Chat delivery did not complete
bili-copy-remaining = Copy unconfirmed text
bili-retry = Retry unsent parts
bili-skip-failed = Skip this line and continue
bili-dedup-unconfirmed = Some messages have no identifier; echo deduplication is unconfirmed
bili-invalid-room = Enter a positive live room ID, or leave blank to use your own room.
bili-results = Delivery history
bili-queue-full = The chat queue is full. This line is shown locally only.

# Douyin live chat
settings-douyin = Douyin Live
settings-douyin-sub = Receive Douyin live chat using native Rust.
dy-room = Live room
dy-room-hint = Enter a live.douyin.com URL or room number, then connect.
dy-enabled = Receive chat
dy-native = Native Rust
dy-connect = Connect / reconnect
dy-open-panel = Open chat panel
dy-account = Account session
dy-account-hint = Public rooms can be received anonymously. Cookies are saved only after account verification; receiving chat does not confirm sign-in.
dy-logout = Sign out and clear credentials
dy-import = Verify and import Cookie
dy-cookie-placeholder = Paste the complete Cookie (optional)
dy-off = Disconnected
dy-connecting = Connecting; waiting for live messages
dy-connected = Connected
dy-disconnected = Disconnected; click reconnect
dy-error = Connection failed
dy-account-ready = Sign-in verified
dy-account-out = Signed out
dy-account-unverified = Sign-in not yet verified
dy-account-expired = Session expired
dy-invalid-room = Enter a valid Douyin live-room URL or number.
dy-storage-error = Protected system storage is unavailable. Credentials were not saved.
dy-clear-error = Could not clear account credentials. Please retry.
dy-room-offline = This room is offline.
dy-invalid-frame = Skipped a live message that could not be decoded.
dy-no-messages = No verifiable live messages were received.
dy-native-error = Native connection failed. Check the network, room status or Cookie, then reconnect.
dy-diamonds = { $count } Douyin coins

tb-inherit = Follow skin
tb-mixed = Mixed
tb-size-relative = Relative size
tb-size-pixels = Pixel size
tb-apply-relative = Apply multiplier
tb-apply-pixels = Apply pixels
tb-font = Font
tb-font-body = Body
tb-font-name = Name
tb-font-accent = Accent
tb-font-family = Enter font family name
tb-align = Paragraph alignment
tb-align-start = Align left
tb-align-center = Center
tb-align-end = Align right
tb-current-value = Current: { $value }

# Skin preview editor
skin-edit-font = Body font
skin-edit-font-presets = Font presets
skin-edit-size = Body size (8–200 px)
skin-edit-line = Line height (1–3 ×)
skin-edit-color = Text color
skin-edit-width = Width (160–8192 px)
skin-edit-height = Height (96–8192 px)
skin-edit-test-text = Sample text (preview only)
skin-edit-sample-reset = Reset sample text
skin-edit-reset = Restore skin defaults
skin-edit-discard = Discard changes
skin-edit-invalid = Enter valid numbers and a hex color. Leave fields empty for skin defaults.
skin-edit-font-missing = This font is unavailable. A fallback font will be used.
skin-edit-draft-hint = Changes appear only in this preview. Save updates this skin without switching the active skin. Empty fields use skin defaults.
skin-edit-unavailable = This skin cannot be previewed right now.
skin-edit-sample = Hello, NanaType! Test wrapping, font size and spacing. 你好，世界！0123456789
    Second line: resize the window to inspect the actual layout.
skin-edit-history-sample = Hello, NanaType! Welcome to the stream.
    A longer history message to check wrapping, names and timestamps in a narrow window. 你好！0123456789
    Third message: customize the font, color and line height separately.
skin-edit-long-name = A long speaker name for narrow windows
skin-edit-save = Save
skin-edit-saved = Saved to this skin.

settings-packages = Plugins & packages
settings-packages-sub = Manage skins, shared resources and functional plugins
packages-import = Import package
packages-refresh = Reload
packages-installed = Installed
packages-none = No packages yet. Import one to add skins, resources or features.
packages-dependencies = Dependencies
packages-remove = Remove
packages-access = Plugins cannot access the network, user files or external programs.
packages-kind-skin = Skin
packages-kind-resources = Shared resources
packages-kind-function = Functional plugin

# Package provenance and explicit script consent
package-author = Author
package-created-at = Created on
package-licensed-to = Licensed to
package-license = License
package-unspecified = Not provided
package-signature-valid = Signature valid (author identity unverified)
package-protected-files = protected files
package-unsigned = Unsigned: source and integrity are not authenticated
package-integrity-failed = Integrity check failed; loading refused
packages-review-enable = Review source and request activation
packages-disable = Disable and revoke consent
packages-consent-required = This package content has no trusted-source confirmation and execution consent. Review it before enabling.
packages-changed-during-review = Package content changed during review. Cancel and request activation again.
packages-consent-title = Script plugin activation consent
packages-consent-risk = Enable only after confirming that the source is trustworthy. Scripts may display misleading content, misuse supplied data or consume resources. Isolation and limits do not guarantee absolute safety. A valid signature matches a public key; it does not verify the author’s identity.
packages-consent-data = The plugin receives playback events, dialogue, and chat authors and plain text. It may render in display, history, input content and plugin panels, and may query only the declared data listed below, as values. This version provides no network, filesystem, external program, clipboard or credential access APIs.
packages-content-digest = Content fingerprint covered by consent
packages-review-source = View complete script source
packages-source-preview-limit = The preview shows the first 16000 characters; the complete source is available. Consent covers these exact bytes and dependencies; changes require fresh consent.
packages-source-trusted = I reviewed the source, confirm it is trustworthy, and understand these risks
packages-consent-reads = Declared data it may read
packages-consent-reads-none = This plugin declares no data reads.
package-data-source = Source
package-data-license = License
package-data-protected = covered by the signature
package-data-unprotected = not covered by a signature
packages-consent-confirm = Consent and start this plugin
packages-consent-cancel = Cancel without granting new consent
packages-location = Package location
packages-script-entry = Script entry
packages-entry-protected = The script entry is covered by the signature.
packages-entry-unprotected = The script entry is unsigned or author-editable; review the current source.
pkgedit-title = Package editor
pkgedit-intro = Create, open or copy skin, resource and Lua packages. Changes stay in a draft: the current skin, installed packages and running plugins do not change until you export a .ntpkg and import it.
pkgedit-kind-skin = Skin only
pkgedit-kind-resources = Resources only
pkgedit-kind-lua = Lua only
pkgedit-kind-mixed = Mixed
pkgedit-id-placeholder = Package ID, e.g. my.package
pkgedit-new = New
pkgedit-open-file = Open .ntpkg…
pkgedit-drafts = Drafts
pkgedit-drafts-none = No saved drafts yet.
pkgedit-open = Open
pkgedit-discard = Discard draft
pkgedit-discard-question = Delete this draft permanently? Installed packages are not affected.
pkgedit-edit = Edit
pkgedit-copy = Copy as new package
pkgedit-origin-new = New project
pkgedit-origin-installed = Opened from installed package
pkgedit-origin-archive = Opened from file
pkgedit-origin-copy = Derived copy of
pkgedit-derived-from = Derived from
pkgedit-derived-unsigned = the original signature was removed; this copy does not speak for the original author
pkgedit-unsaved = Unsaved changes
pkgedit-saved-state = Draft saved
pkgedit-not-stored = Unchanged
pkgedit-validating = Checking in the background…
pkgedit-valid = No errors
pkgedit-errors = { $count ->
    [one] 1 error
   *[other] { $count } errors
}
pkgedit-save = Save draft
pkgedit-revert = Revert to last save
pkgedit-export = Export .ntpkg…
pkgedit-close = Close
pkgedit-close-unsaved = There are unsaved changes. Save them as a draft before closing?
pkgedit-close-save = Save and close
pkgedit-close-discard = Close without saving
pkgedit-tab-files = Files
pkgedit-tab-package = Package
pkgedit-tab-signing = Protection & signing
pkgedit-tab-preview = Preview
pkgedit-tab-debug = Lua debugger
pkgedit-import = Import asset…
pkgedit-remove = Remove file
pkgedit-remove-question = Remove this file from the draft?
pkgedit-read-only = Read-only
pkgedit-format = Format
pkgedit-search = Search in file
pkgedit-search-next = Next
pkgedit-search-none = No matches
pkgedit-no-file = Choose a file on the left.
pkgedit-binary = File details
pkgedit-file-format = Format
pkgedit-file-size = Size
pkgedit-file-dependency = From dependency (read-only)
pkgedit-data-declared = Declared as data
pkgedit-problems = Problems (click to locate)
pkgedit-no-problems = No problems found.
pkgedit-manifest-invalid = package.toml cannot be parsed; fix it under Files first.
pkgedit-manifest-protected = package.toml is protected by the signature and is read-only. Create a derived copy to change it.
pkgedit-version = Version
pkgedit-capabilities = Capabilities (enabling skin or Lua adds starter files)
pkgedit-dependencies-hint = Dependencies, one "package-id@exact-version" per line; applying bundles them from installed packages.
pkgedit-apply = Apply
pkgedit-data = Declared data
pkgedit-data-none = No data declared.
pkgedit-data-hint = Form edits are parsed strictly before they are written to package.toml, keeping comments and other keys; an edit that fails is not written. Full checks appear in the problem list.
pkgedit-pipelines = Pipelines
pkgedit-pipelines-none = No pipelines declared.
pkgedit-data-missing = file missing
pkgedit-data-add = Add data
pkgedit-data-id = Data ID
pkgedit-data-path = File path
pkgedit-data-format = Format
pkgedit-data-version = Format version
pkgedit-data-constraints = JSON constraints
pkgedit-data-constraints-hint = JSON constraints are optional and apply to JSON data only, for example {"{"}"type": "object", "required": ["items"]{"}"}. Leave empty for no limits.
pkgedit-entry-edit = Edit
pkgedit-entry-remove = Remove
pkgedit-entry-remove-question = Remove this declaration?
pkgedit-pipeline-add = Add pipeline
pkgedit-pipeline-name = Pipeline name
pkgedit-pipeline-processor = Processor
pkgedit-pipeline-inputs = Inputs: a data ID (pkg:package-id/data-id for a dependency), or pipeline.output to feed from another pipeline.
pkgedit-pipeline-params = Parameters, one name = number per line
pkgedit-pipeline-outputs = Outputs
pkgedit-pipeline-unknown = This processor is not registered; edit it in the source.
pkgedit-pipeline-broken = This declaration does not parse; edit it in skin.toml.
pkgedit-pipelines-no-skin = Pipelines live in skin.toml; enable the skin capability to declare them.
pkgedit-skin-protected = skin.toml is protected by the signature and is read-only. Create a derived copy to change it.
pkgedit-seal-note = A signature only shows that protected files match what was signed; it does not prove who the author is. Editable files may change without invalidating it.
pkgedit-key-load = Use this computer's signing key
pkgedit-key-none = This computer has no signing key yet.
pkgedit-key-create = Create signing key
pkgedit-key-fingerprint = Signing key fingerprint
pkgedit-key-note = The private key stays in the system's protected storage (macOS Keychain, Windows DPAPI). It never goes into a package, a log, the resource interface or a plugin process.
pkgedit-key-unavailable = The signing key is unavailable
pkgedit-protect = Choose the files to protect. package.toml is always protected; once signed, protected files are read-only and the file list is fixed.
pkgedit-sign = Sign
pkgedit-signed-own = Signed with this computer's key. Remove the signature to edit, then sign again.
pkgedit-signed-other = Signed with another key; protected files are read-only. To change them, create a derived copy without the original signature.
pkgedit-unseal = Remove signature to edit
pkgedit-derive = Create derived copy
pkgedit-derive-hint = A derived copy gets a new package ID, records its source and drops the signature.
pkgedit-preview-none = This project has no skin, or the skin has not passed its checks yet.
pkgedit-preview-note = The preview uses the real display and history renderers and only affects this draft.
pkgedit-preview-input-note = The input preview uses the real input box renderer with a private sample draft; the input window and its editor are not changed.
pkgedit-debug-no-lua = This project does not declare the Lua capability.
pkgedit-debug-risk = Debugging runs the draft's script in a separate sandboxed worker process. It only receives the test events you send, and its results only appear here, but a script can still show misleading content or use resources. Only run source you have reviewed. Editing any file stops the script and needs a new confirmation.
pkgedit-debug-trust = I reviewed the current source and trust it to run in the debug process
pkgedit-debug-start = Start
pkgedit-debug-stop = Stop
pkgedit-debug-clear = Clear results
pkgedit-debug-events = Send a test event
pkgedit-debug-send = Send
pkgedit-debug-invalid-json = An event must be a JSON object with a string "type" field.
pkgedit-debug-error = Script error
pkgedit-debug-output = Returned views
pkgedit-debug-waiting = Waiting for the script's view…
pkgedit-debug-idle = The script is not running.
pkgedit-debug-log = Log
pkgedit-saved = Draft saved
pkgedit-unsaved-open = The open project has unsaved changes. Save or close it first.
pkgedit-open-failed = Could not open the project
pkgedit-exported = Exported
pkgedit-export-failed = Export failed
pkgedit-debug-locate = Go to

packages-consent-reviewing = Reviewing plugin content…
packages-consent-confirming = Checking content and confirming consent…

# Third-party chat: blivechat and 彗星号
settings-blivechat = Third-party chat
settings-blivechat-sub = Receive live chat from blivechat or 彗星号 (Comet).
int-blive = Receive third-party chat
int-blive-hint = Comments, Super Chats, gifts and memberships appear in the chat panel, where you can mention or reply to viewers
int-blive-source = Source
int-blive-source-server = blivechat
int-blive-source-comet = 彗星号 (Comet)
int-blive-panel = Chat panel
int-blive-panel-hint = A window for you only; it is never captured
int-blive-open-panel = Open the chat panel
int-blive-section = blivechat
int-blive-room-kind = Room
int-blive-room-kind-hint = The auth code is recommended
int-blive-auth-code = Auth code
int-blive-auth-placeholder = Paste the auth code
int-blive-auth-save = Save and connect
int-blive-auth-saved = Auth code saved
int-blive-auth-clear = Clear
int-blive-auth-not-stored = The auth code could not be saved; enter it again next time
int-blive-room-id = Room number
int-blive-room-placeholder = Enter a room number
int-blive-connect = Connect
int-blive-host = blivechat host
int-blive-port = Port
int-comet-section = 彗星号 (Comet)
int-comet-address = Address
int-comet-address-hint = Paste it into the 彗星号 plugin's settings
int-comet-copy = Copy
int-comet-reset = Make a new one
int-comet-reset-question = The old address stops working and must be pasted again in 彗星号. Continue?
int-comet-waiting = Waiting for 彗星号
int-comet-connected = 彗星号 · Connected
int-comet-relay-error = Cannot receive 彗星号 chat: { $message }
chat-status-joined = Joined room { $room }
chat-status-no-room = Enter an auth code or a room number
chat-fatal = blivechat stopped: { $message }
chat-fatal-auth-code = The auth code is not valid; check it and enter it again
chat-fatal-too-many-retries = Too many retries; try again later
chat-fatal-too-many-connections = Too many clients connected; try again later
