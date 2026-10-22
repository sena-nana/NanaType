# Strings only NanaType shows (feature `app`).

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
input-skin-preview = Skin preview
editor-diagnostic = Character { $pos }: { $message }
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
settings-platforms = Live platforms
settings-integrations = Integrations
settings-assets = Assets
settings-about = About

## General
general-language = Language
general-language-system = System default
general-speaker = Character name
general-speaker-hint = Leave empty to hide the name plate
general-avatar = Avatar
general-avatar-pick = Choose image…
general-avatar-clear = Remove
general-presence = Behavior
general-presence-resident = Always open
general-presence-summon = Summon with hotkey
general-input = Input
general-mode = Input mode
general-autostart = Launch at login
general-updates = Updates
general-check-updates = Check for updates automatically
general-update-proxy = Custom proxy
general-update-proxy-hint = Optional https:// proxy prefix. Downloads are still verified by signature

## Playback
playback-speed = Typing speed
playback-typing = Typewriter effect
playback-punct = Punctuation pauses
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
playback-queue-replace-short = Replace
playback-queue-finish-short = Finish first
playback-queue-queue-short = Queue
playback-reduced = Reduce motion
playback-stagger = Mirror character delay
playback-sound = Typing sounds
sound-pack = Sound pack
sound-pack-hint = Replaces the skin's typing, line and continue sounds; the skin keeps its other effects
sound-pack-skin = Skin sounds
sound-pack-unavailable = { $id } (not installed)
sound-pack-import = Import sound pack
sound-pack-folder = Open packages folder
sound-pack-missing = Sound pack { $id } is unavailable; playing the skin's sounds
playback-audio = Sound
playback-volume = Master volume
playback-volume-typing = Typing volume
playback-volume-effects = Effects volume
playback-keystroke = Keystroke sounds
playback-keystroke-hint = Key sounds as you type in the input window; Enter and Backspace sound different
playback-keystroke-scheme = Keystroke sound set
playback-keystroke-scheme-mechanical = Mechanical keyboard
playback-keystroke-scheme-bubble = Bubbles
playback-volume-keystroke = Keystroke volume
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
skins-broken = Cannot load
skins-by = by { $author }
skins-version = Version { $version }
skins-overrides = Personalize
skins-override-name = Name plate color
skins-override-box = Box color
skins-override-accent = Accent color
skins-override-overflow = When text overflows
skins-override-overflow-skin = As the skin sets
skins-override-overflow-clip = Cut off
skins-override-overflow-scroll = Scroll along
skins-override-overflow-page = Page (wait, then clear)
skins-override-page-auto = Turn full pages after
skins-override-page-auto-hint = 0 waits for the continue key
skins-override-reset = Reset to skin defaults
general-appearance = Appearance
skins-interface = Interface
general-appearance-system = Follow system
general-appearance-light = Light
general-appearance-dark = Dark
skins-hot-reload = Reload when files change
skins-advanced-effects = Advanced skin effects
skins-play-effects = Play effects
skins-stop-effects = Stop effects
skins-installed = Installed “{ $name }”
skins-replaced = Updated “{ $name }” from { $previous }
skins-import-failed = The skin could not be imported
skins-fallback = Skin “{ $id }” could not load; using the default skin
skins-preview = Preview
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
overlay-menu-skin = Switch skin
overlay-menu-adjust-skin = Adjust skin…
overlay-menu-lock = Lock window
overlay-menu-reset = Restore default position and size
overlay-menu-close = Close window
overlay-menu-continue = Continue playback
overlay-menu-skip = Skip current playback
overlay-menu-clear-display = Clear display
overlay-menu-confirm-clear = Confirm clear

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
history-hide-latest-hint = The line still on the display joins the history once it leaves
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
hotkeys-global = Global hotkeys
hotkeys-phrases = Quick phrases
hotkeys-phrases-empty = No quick phrases yet
hotkeys-edit-phrases = Edit quick phrases
hotkeys-reset = Restore default
hotkeys-unsafe-local = Keys that type text need Ctrl, Alt or Shift
hotkeys-recall-prev = Previous line
hotkeys-recall-next = Next line

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
int-vts-hint = Switch expressions and press hotkeys on [expr] and [vts] markers
int-vts-port = Port
int-vts-authorize = Request access
int-vts-map = Expression mapping
int-vts-map-expression = Marker
int-vts-map-hotkey = VTS hotkey
int-vts-map-auto = Auto (same name)
int-vts-refresh = Refresh hotkeys
int-vts-lipsync = Lip sync
int-vts-mouth-param = Mouth parameter
int-vts-mouth-amp = Mouth opening
int-vts-reset = Reset expressions
int-vts-reset-line-end = When the line leaves the box
int-vts-reset-next-line = When the next line starts
int-vts-reset-off = Never
int-osc = OSC
int-osc-hint = Send to VNyan, Warudo, VSeeFace and others
int-osc-host = Host
int-osc-port = Port
int-osc-prefix = Address prefix
int-ws = WebSocket broadcast
int-ws-hint = Broadcast lines and expressions on a local port
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

## Assets
telegram-stickers = Telegram stickers
telegram-hint = Get an API ID and API Hash at my.telegram.org
telegram-api-id = API ID
telegram-api-hash = API Hash
telegram-phone = Phone number (with + country code)
telegram-proxy = Download proxy
telegram-proxy-hint = Leave empty to connect directly. SOCKS5 user and password are saved in plain text
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
about-update = Updates
update-check = Check for updates
update-checking = Checking for updates…
update-up-to-date = NanaType is up to date
update-current = Current version v{ $version }
update-available = Version v{ $version } is available
update-available-hint = { $size } download
update-notes = Release notes
update-skip = Skip this version
update-download = Download
update-downloading = Downloading v{ $version }
update-downloading-hint = { $percent }%
update-cancel = Cancel
update-ready = v{ $version } is downloaded and verified
update-ready-installer = The installer replaces this version; NanaType quits first
update-ready-dmg = Open the disk image and drag NanaType into Applications
update-ready-folder = Unzip it and replace the current program folder
update-install = Install
update-open = Open
update-retry = Try again
update-toast = NanaType v{ $version } is out. Download it from Settings › About
update-error-unreachable = Could not reach an update source
update-error-rejected = Release information failed its signature check and was ignored
update-error-no-asset = The new version has no download for this system
update-error-download = The download failed or did not match the published checksum
update-error-other = Update failed
about-logs = Logs folder
about-data = Data folder
about-repository = Official repository
about-bilibili = Official Bilibili
about-disclaimer = Third-party notice
about-disclaimer-body = NanaType is an independent third-party tool. It is not affiliated with, endorsed by or authorised by Bilibili, Douyin or any other platform or its operators. Names and trademarks belong to their respective owners.
about-custom-skins = Custom skins
about-custom-skins-body = Contact the developer for a custom skin
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
chat-open-panel = Open chat panel
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
chat-min-gift = Minimum gift value (¥)
chat-status-retrying = Disconnected, retrying: { $message }
history-chat = Live chat
history-chat-show = Show live chat in history
history-show-quote = Show reply quotes
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
cheat-complete = Complete a viewer after @ or a marker after /
complete-arg-ms = ms
complete-arg-amp = amplitude
complete-arg-name = name
complete-arg-volume = volume
complete-arg-intensity = intensity
complete-portrait = Switch the portrait without broadcasting
complete-about-shake = Shake the whole dialogue box
complete-wait = Wait { $ms } ms, then go on
complete-pause-auto = Stop, then go on after { $ms } ms
complete-shake = Shake the box by { $amp } for { $ms } ms
complete-volume = Play the sound at volume { $vol }
complete-intensity = Switch the expression at intensity { $x }

## wave3-settings
settings-group-input = Input
settings-group-look = Look
settings-group-stream = Stream
output-obs-step-plugin = Install the Spout2 plugin for OBS
output-obs-step-enable = Enable the display output:
output-obs-enable = Enable display output
output-obs-enabled = Display output enabled
output-obs-step-source = Add a “Spout2 Capture” source and pick this sender:
output-obs-step-alpha = Set Composite mode to “Premultiplied Alpha”
output-obs-step-syphon = Add a “Syphon Client” source in OBS
output-obs-step-syphon-source = Pick this sender:
output-obs-other = Other ways
int-vts-authorized = Authorized
int-vts-map-empty = Connect and authorize to list your VTS hotkeys
skins-more = More actions

## wave4-skins
skins-view-grid = Cards
skins-view-list = List
windows-lock-locked-short = Locked
windows-lock-tip = Interactive: drag to move, scroll to scale. Locked: clicks pass through
windows-vertical-text-tip = Changes the writing direction only, not the skin layout
windows-background-tip = Chroma key fills the window green for captures without alpha
history-window-link = Window & capture

## wave4-onboarding
about-setup-guide = Setup guide
playback-reduced-system-on = System setting: on

## wave5-fonts
skins-fallback-font = Fallback font

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
about-source-key-sounds = The "Mechanical keyboard" keystroke sounds are jim-ph's recordings of a 1986 DOS mechanical keyboard on Freesound, dedicated to the public domain under CC0, as collected by Tickeys.
about-source-key-sounds-bubble = The "Bubbles" keystroke sounds are recordings by Glaneur de sons on Freesound, licensed under CC BY 3.0, as collected by Tickeys; NanaType ships them unmodified.
about-source-repository = Source repository
about-source-mirror = Documentation mirror

## round2-skins
skins-fallback-font-missing = { $family } not installed

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
peer-relay-placeholder = Custom relay URL (optional)
peer-paused = Wait for the session to reconnect before sending or controlling playback. You can keep editing.

# Independent partner output
window-peer-display = Partner display
windows-peer-display = Partner
output-peer-display = Partner display
peer-window-toggle = Partner window
output-name-conflict = Output names must be unique. Choose a different name for each output.
tb-effect-pulse = Pulse
tb-effect-flicker = Flicker
tb-emphasis = Emphasis

# Editor context menu
edit-undo = Undo
edit-redo = Redo
edit-cut = Cut
edit-copy = Copy
edit-paste = Paste
edit-select-all = Select all

# Bilibili direct connection and authenticated delivery
settings-bilibili = Bilibili Live
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
bili-identity-hint = Name and avatar come from your Bilibili account
bili-room = Target live room
bili-room-placeholder = Blank uses your own room
bili-own-room = Use my live room
bili-connect = Connect
bili-receive = Receive live chat
bili-send-section = Typewriter delivery
bili-send-switch = Send to chat
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
dy-room = Live room
dy-room-placeholder = live.douyin.com URL or room ID
dy-enabled = Receive chat
dy-connect = Connect / reconnect
dy-account = Account session
dy-account-hint = Public rooms need no sign-in; a Cookie is saved only once verified
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
dy-native-error = Connection failed. Check the network, room status or Cookie, then reconnect.
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
skin-edit-size = Body size
skin-edit-line = Line height
skin-edit-color = Text color
skin-edit-width = Width
skin-edit-height = Height
skin-edit-test-text = Sample text
skin-edit-sample-reset = Reset sample text
skin-edit-reset = Restore skin defaults
skin-edit-discard = Discard changes
skin-edit-font-missing = This font is unavailable. A fallback font will be used.
skin-edit-draft-hint = Preview only until you save it to this skin
skin-edit-unavailable = This skin cannot be previewed right now.
skin-edit-save = Save
skin-edit-saved = Saved to this skin.
skin-params-title = Skin options
skin-params-hint = Appearance options this skin provides. They apply to the whole skin.
skin-params-default = Default
skin-params-invalid-number = Enter a number
skin-params-out-of-range = Must be { $min }–{ $max }
skin-params-invalid-color = Use a #RRGGBB colour
skin-params-invalid-option = Not one of the choices
skin-params-text-too-long = At most { $max } characters
skin-params-text-empty = Can't be empty
skin-params-text-control = Can't contain line breaks or control characters
skin-params-contrast = { $foreground } on { $background } is { $ratio }:1; aim for at least { $min }:1
settings-packages = Plugins & packages
packages-import = Import package
packages-dependencies = Dependencies
packages-remove = Remove
packages-access = Plugins cannot access the network, user files or external programs.

# Package provenance and explicit script consent
package-unspecified = Not provided
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
packages-consent-confirm = Consent and start this plugin
packages-consent-cancel = Cancel without granting new consent
packages-location = Package location
packages-script-entry = Script entry
packages-entry-protected = The script entry is covered by the signature.
packages-entry-unprotected = The script entry is unsigned or author-editable; review the current source.
packages-consent-reviewing = Reviewing plugin content…
packages-consent-confirming = Checking content and confirming consent…

# Third-party chat: blivechat and 彗星号
settings-blivechat = Third-party chat
int-blive = Receive third-party chat
int-blive-source = Source
int-blive-source-server = blivechat
int-blive-source-comet = 彗星号 (Comet)
int-blive-section = blivechat
int-blive-room-kind = Room
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

# YouTube live chat
settings-youtube = YouTube Live
yt-target = Video or channel
yt-target-placeholder = https://www.youtube.com/@handle
yt-enabled = Receive chat
yt-connect = Connect / reconnect
yt-off = Disconnected
yt-resolving = Looking for the channel's live stream
yt-waiting = Not live yet; checking again every minute
yt-connecting = Opening the chat
yt-connected = Connected
yt-ended = The stream has ended
yt-disconnected = Disconnected; reconnecting
yt-error = Connection failed
yt-invalid-target = Enter a YouTube video or channel address, an @handle or a video id.
yt-protocol-changed = YouTube's chat page has changed and could not be read. Please update NanaType.
yt-redirected = YouTube asked for sign-in or consent instead of the chat. Try again later.
yt-network-error = Could not reach YouTube. Check the network, then reconnect.
chat-gift-memberships = gifted { $count } memberships

# Twitch live chat
settings-twitch = Twitch
tw-channel = Channel
tw-channel-placeholder = channel_name
tw-enabled = Receive chat
tw-off = Disconnected
tw-connecting = Connecting
tw-connected = Connected
tw-error = Connection failed; retrying
tw-invalid-channel = Enter a valid Twitch channel name.
tw-channel-unavailable = This channel does not exist or is unavailable.
tw-banned = Twitch refused to join this channel.
tw-storage-error = Protected system storage is unavailable. The sign-in was not saved.
tw-login-expired = The sign-in code expired. Start again.
tw-network-error = Could not reach Twitch. Check the network.
tw-account = Account
tw-signed-out = Not signed in
tw-restoring = Checking the saved sign-in
tw-pending = Waiting for authorization
tw-pending-hint = Open twitch.tv/activate, sign in and enter this code:
tw-open-activation = Open twitch.tv/activate
tw-expired = Sign-in expired
tw-login-failed = Sign-in failed
tw-login = Sign in with Twitch
tw-login-hint = Sign in to send your lines to chat
tw-refresh = Check again
tw-logout = Sign out
tw-send-section = Send to chat
tw-send-switch = Send lines to Twitch chat
tw-send-title = Send lines to Twitch chat?
tw-send-warning = Every line you send from now on is posted as you to this channel. Images are not sent.
tw-delivery-failed = Twitch send failed
tw-submitted = Accepted by Twitch

## Steam Workshop
workshop-browse = Browse Workshop
workshop-badge = Workshop
workshop-subscriptions = Workshop subscriptions
workshop-find = Find more in the Workshop
workshop-find-hint = Items you subscribe to in Steam download and appear here
workshop-none = No subscriptions yet
workshop-unavailable = The Workshop is unavailable: start Steam, then reopen NanaType
workshop-ready = Installed
workshop-downloading = Downloading { $percent }%
workshop-pending = Waiting to download
workshop-not-installed = Not downloaded
workshop-invalid = Cannot be used: { $message }
workshop-shadowed = Not used: { $id } is already installed
workshop-view = View in the Workshop
workshop-download = Download again
workshop-unsubscribe = Unsubscribe
workshop-unsubscribe-confirm = Unsubscribe from "{ $name }"? Steam deletes its files after you quit.
workshop-unsubscribe-failed = Could not unsubscribe: { $message }
sound-pack-workshop = { $name } (Workshop)
