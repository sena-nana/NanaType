[简体中文](../usage.md) · [English](usage.md) · [日本語](../ja/usage.md)

# User Guide

Everything about day-to-day use: streaming platforms, OBS, sessions with friends, stickers and skins. For a feature overview, see the [README](../../README.en.md).

## Basics

1. On launch the input window appears near the bottom of the screen; after that it remembers wherever you move and resize it. Closing the input window hides it to the tray and keeps your draft; press F8 to bring it back. The display and history windows are transparent overlays.
2. Type your line straight into the input window and press **Enter** to send (Shift+Enter for a new line; you can switch to Ctrl+Enter in settings). Ctrl+↑ / ↓ recalls lines you have already sent, and Esc hides the window.
3. Select text and use the toolbar (or Ctrl+B / I / U) to make it bold, color it, add an outline, ruby, effects or rhythm. With nothing selected, the setting applies to whatever you type next. Pauses, page breaks, sounds, expressions and other markers appear as thin vertical bars between characters and take up no layout width, so line wrapping in the input window matches the display exactly.
4. The `</>` button on the toolbar switches to the markup source; both views edit the same line.
5. Global hotkeys: **F8** shows the input window, **F9** continues (past a pause / skips the current segment).
6. From the tray menu you can show or hide each window, lock the overlays, switch modes and skins, and open settings. The tray icon also shows the current state: a pink dot while mirroring, grayed out when both the display and history are hidden, and a small padlock while the overlays are locked.

The display, the partner display and the history window can be dragged with the left mouse button and scaled proportionally with the scroll wheel. Their right-click menu lets you close the window, switch the global skin or open skin adjustments, toggle **Always on top**, lock the window, and **Restore default position and size**. Your own display can also continue playback, skip what is currently playing and clear the display; the history window can toggle avatars, names and timestamps, and clear this session's history via **Clear history › Confirm clear** (unavailable during a session with friends). A locked window lets the mouse pass through; unlock it from the tray or **Settings › Windows**. Closing a window only hides it — any enabled Spout2 / Syphon output keeps running, and native context menus never end up in the shared-texture output.

Input modes:

Both editing modes support scrolling with the wheel / trackpad and PageUp / PageDown navigation, with Shift to extend the selection. The context menu offers undo, redo, cut, copy, paste and select all. While an IME is composing, Enter, Esc and the arrow keys go to the IME first.

- **Compose & send**: finish writing, then send; the display plays it back at typewriter pace.
- **Live mirror**: the display follows your typing in real time; when you send, the line goes into history.

### Twitch chat and sending lines

1. Open **Settings › Twitch**, enter a channel name or `https://www.twitch.tv/channelname`, and turn on **Receive chat**. Reading chat does not require signing in.
2. To send lines, click **Sign in with Twitch**, open twitch.tv/activate and enter the code shown on the page. The token is kept in OS-protected storage (Windows DPAPI / macOS Keychain).
3. Turn on **Send lines to Twitch chat** and confirm; from then on, lines you send are posted to the channel as you. Long lines are split at 500 characters; if sending fails, the queue pauses so you can retry or skip.

For protocol, sign-in and rate-limit details, see [Twitch integration](twitch.md).

### Third-party chat: blivechat and Comet (彗星号)

Open **Settings › Third-party chat**, turn it on and choose a source:

- **blivechat**: run blivechat (default `127.0.0.1:12450`), enter your Bilibili Open Platform identity code and click **Save and connect**, or use a room number instead. The identity code is kept in OS-protected storage (Windows DPAPI / macOS Keychain) and never written to the settings file.
- **彗星号 (Comet)**: add the NanaType plugin to your Comet canvas and paste the connection address shown on the settings page into the plugin's settings. The plugin page lives in [`integrations/comet/`](../../integrations/comet/); for how to upload it and how it works, see [Comet integration](comet.md).

Third-party chat messages show up in the chat panel, and @-mentions, replies and mixing into history work the same way. With no live room at hand, you can try blivechat mode against the mock server:

```bash
cargo run -p nanatype-blive --example mock_blivechat
```

For Comet mode, a script can stand in for the plugin page and push sample messages to the relay (requires Node 22+):

```bash
node scripts/comet_publish.mjs "ws://127.0.0.1:12451/comet?token=…"
```

### Sessions with friends

1. Steam must be running. Open the session entry from the bar at the bottom of the input window; the host clicks **Create session**. A room holds up to 8 people and can have a password; the password is never included in invites.
2. **Invite friend** opens the Steam invite dialog. Invited friends don't need to enter the password. If you copy an invite and hand it to someone else, they must enter the password before joining when the room is locked.
3. The **Display** always shows your own lines. The **Partner display** gives every other member a live panel in seating order, and offers invites and acceptance status while nobody has joined yet. The shared history contains everything anyone has sent, in the order confirmed by the host. Everyone keeps their own skin and window layout; live mirror reveals what you are typing, while compose & send only reveals what you have sent. F9, skip and clear display only affect you.
4. During a session the history window shows this session's lines. History from before the session and live-room chat are not shared. When you leave, your local history comes back; a session is not restored across a process restart. A session keeps at most the latest 1000 lines, trimmed further by message-size limits.
5. If you disconnect you can keep editing, but sending and manual playback controls are paused. Disconnecting frees your seat; the same person can reconnect while a seat is free, and lines already sent are kept. If the host leaves, the room ends.

Images, stickers, avatars, avatars inside quotes and the current portrait are distributed by content hash. Previews are relayed by the host; full images are fetched directly from whoever posted them, falling back to the host if a direct connection fails. Each resource is capped at 16 MiB; oversized or missing resources show a placeholder. Only your own lines drive character controls such as your local VTube Studio / OSC.

Connections go direct when Steam can manage it, otherwise through Steam's relay. With a direct connection, peers can see your network address.

### Connecting to OBS

- **Spout2**: turn on Spout2 output in **Settings › Output / OBS**. In OBS, install the Spout2 plugin, add *Spout2 Capture*, pick the sender name (default `NanaType Display` / `NanaType Partner` / `NanaType History`) and set the composite mode to *Premultiplied Alpha*.
- **Separate outputs**: in **Settings › Output / OBS**, enable **Display** and **Partner display** separately, then add two sources in OBS, choosing `NanaType Display` (you) and `NanaType Partner` (the others). Size, opacity and sender name are set per output; enabled sender names must all be different. The partner output only runs during a session, and keeps outputting even while its window is hidden. For the shared history, pick `NanaType History`.
- **Window capture**: *Window Capture* with the capture method *Windows 10 (1903 and up)*.
- **Syphon** (macOS): turn on Syphon output in **Settings › Output / OBS**. In OBS, add *Syphon Client*, pick the sender name (default `NanaType Display` / `NanaType Partner` / `NanaType History`) and set the composite mode to *Premultiplied Alpha*. You can also capture the display window with *macOS Screen Capture*. Spout2 is Windows-only.

Turning an output off and back on within 5 seconds reuses the original sender, and the OBS picture comes back in a median of about 15–30 ms. For sender lifecycles and measurements with real OBS, see [OBS / Spout output](obs-spout.md).

### Characters

- **Settings › General › Characters** lists who your lines are said by. Each character has a name, an avatar, a name colour for the name plate (**Use skin colour** goes back to the skin's) and an optional hotkey that switches to it. Add, reorder and remove them; the last one cannot be removed. An empty name hides the name plate. The name and avatar of older versions become the first character.
- Switch with the character button in the input window's bottom bar (shown once there are two or more), the tray's **Character** submenu, or **Next character**, **Previous character** and each character's own key in **Settings › Hotkeys**. The current character is remembered. Each line records its speaker when sent, so a later switch or rename never changes the history, the saved sessions, what a partner sees or the `speaker` of `line_start` events.
- **Settings › Hotkeys** can also bind **Lock/unlock overlays**, **Replay the last line** (plays it on the display again, without sending it to live chat or adding a history row) and **Show/hide history**, all unbound by default. The tray's lock and **Replay the last line** items do the same.

### Text to speech

- **Settings › Voice › Text to speech › Read sent lines aloud** (off by default) reads each line with the operating system's own voices as it starts on the display: AVSpeechSynthesizer on macOS, SAPI on Windows. Nothing is downloaded and nothing leaves the computer; install more voices in the system settings, then choose **Refresh voices**.
- What is read is the line's text: ruby reads as its base, a mention as the viewer's name, and stickers, images and markers are left out. A line of only stickers or punctuation is not read.
- A new line cuts off the one being read. **Skip line** and **Clear display** stop reading; **Replay the last line** reads it again.
- Pick a **Default voice** and one per character (the list shows the interface language's voices first; **Voice language** shows others or all). **Rate** and **Volume** apply to every voice, and **Test** plays a sample.
- **Also read viewer chat** and **Also read co-op partners' lines** are off by default. They read with the default voice, after what is being said, and only chat that joins the history; a partner's line is read once, when it shows.
- While reading aloud is on, VTube Studio lip sync (and the `speaking` event) follows the voice instead of the typewriter.
- The switch, default voice, rate, volume and the two options are global; each character's voice travels with the characters in a profile.

### Translated subtitles

- **Settings › Integrations › Translated subtitles › Translate sent lines** (off by default) translates each line into the language under **Translate into** (English by default; any other goes in **Other language code**, such as `pt-BR`); the source language is detected. The translation fades in right under the last line of the current page when it arrives (the box always keeps a line free for it, so paged text never pushes it out), smaller and quieter, in the skin's fonts, outline and shadow. Typing and sending never wait for it, and a late translation of a line that has already been replaced never reaches the box (it still joins the history). Reduced motion shows it without the fade. In vertical text it is a column after (to the left of) the last one.
- **Service**: **OpenAI-compatible** covers OpenAI, DeepSeek (`https://api.deepseek.com/v1`) and a local Ollama (`http://localhost:11434/v1`) or LM Studio (`http://localhost:1234/v1`): fill in the **Endpoint** (up to `/v1`) and the **Model**; a server on this computer needs no key. The prompt asks for the translation only, short, with names kept, and passes the speaker as context. **DeepL** only needs a key; keys ending in `:fx` use the free API.
- **API keys** are kept in the system's secret store (Windows Credential Manager / macOS Keychain), never in the settings file or an export. **Proxy** left empty uses the system's; servers on this computer are always reached directly.
- **Which lines**: **Your lines** is on by default; **Co-op partners' lines** translates only lines a partner sends without a translation of their own (when a partner translates, theirs travels with the line); **Viewer chat** translates chat that joins the history.
- **Where they show**: **Show on the display** and **Show in the history** are separate. Saved history keeps translations either way: a `.txt` export puts them on the line under the text, an `.srt` export as the subtitle's second line. WebSocket and plugin events: `line_start` carries `translation` when it is already known (a replay or a repeated line); otherwise a `translation` event (`id`, `text`, `target`) follows when it arrives, and OSC sends `/translation`.
- The last 256 translations are kept in memory, so replays and repeated lines show theirs at once. A request times out after 8 seconds, and newer lines replace older ones still waiting. A failure never holds up sending: the badge on the settings page says why, and each kind of failure shows one notice (**Test** translates a sample line with the current settings).
- Translation settings are global; profiles do not change them.

### Saving and searching the history

- **Autosave**: sent lines (a partner's too, in a session with friends) are saved by default to `history/` in the data folder, one JSON Lines file per session (e.g. `2026-11-02_213005.jsonl`), appended line by line, so a crash loses at most the line being written. Viewer chat is saved only with **Settings › History › Saved history › Also save viewer chat** on (off by default), and only the chat that joins the history. **Keep for (days)** defaults to 30: older sessions are deleted at startup, and 0 keeps them forever. **Clear history** only clears the window, never the saved files.
- **Viewing and exporting**: **Settings › History › Saved sessions** lists each session's start time and line count. **View** shows its lines (time · name · text; the newest 500 first, and **Show … earlier lines** at the top loads the batch before, back to the session's start), which export as `.txt`, or as `.srt` subtitles timed from the session's first line, each shown until the next starts or for a reading time by its length. **Delete** removes the session's file after a confirmation; **Open folder** shows where they are kept.
- **Search**: while the history window is unlocked, the search button in its top corner opens a field that filters the log by text or name (case-insensitive). The button again, or Esc, closes it and clears the filter. While the history window's Spout2/Syphon output is on there is no search, so the output always shows the whole log.

### Profiles, export and import

- **Profiles**: **Settings › General › Profiles** saves the current settings under a name (say "Karaoke" or "Just chatting"); profiles can be switched, renamed, duplicated, overwritten with the current settings and deleted. Switch from the tray's **Profile** submenu too, or bind **Next profile** under **Settings › Hotkeys** (cycles in list order, unbound by default). Switching first saves changes made under the active profile back into it. Profiles live in the config folder as `profiles/<id>.json`, written atomically like `settings.json`.
- **What a profile holds**: the characters (with avatars, colours and hotkeys) and the active one, playback, sound (but not the output device), the skin and its adjustments (but not the Settings window's light/dark look or hot reload), the overlay windows' visibility, lock, always-on-top and size, the history window (but not saving the history), and quick phrases. Everything else is global: language, hotkeys, input behaviour, integrations with their addresses and channels, outputs, speech input, text to speech (each character's voice goes with the characters), updates, autostart, plugins and window positions on screen.
- **Export**: **Settings › General › Backup and transfer › Export settings** saves a `.json` file carrying the app name, settings schema, export time and NanaType version. It can include integration addresses and channels (on by default), character avatars (on by default; the images are embedded, and any over 4 MB or not found are left out and counted afterwards) and window positions (off by default: they are kept per monitor layout and rarely fit another computer). Sign-ins and passwords (Twitch, OBS, the blivechat code) live in the system keychain and are never exported; neither are the VTube Studio plugin token, the 彗星号 address token or a proxy's `user:password@`. Plugin approvals, audio devices, GPU and log options and the active profile belong to this computer and stay behind as well.
- **Import**: **Import settings** reads an exported file or a hand-copied `settings.json`. Files from older versions are upgraded through the settings migrations first; files from a newer NanaType are refused. A confirmation lists what will change and nothing is applied before it. The current settings are first backed up to `settings.previous.json`, and **Restore previous settings** undoes the import (pressing it again returns to the imported settings). A file without integrations keeps yours; your sign-ins are always kept. Embedded avatars are written to `avatars/` in the data folder.
- **Reset all to defaults**: after a confirmation, the settings go back to a new install's, keeping the language, the finished first-run guide and the sign-ins (keychain passwords are not touched and you stay signed in). The current settings are backed up first, so **Restore previous settings** undoes it. Window positions and profiles are not affected.

### Accessibility

- **Settings › General › Accessibility** changes only the app's own windows: Settings, the input window (with its history and chat panels) and the package editor. Skins, and what the display, partner and history windows show (Spout2 / Syphon output included), always stay as they are.
- **High contrast**: **Follow system**, **Off** or **On**. On, the interface switches to opaque black and white with text at 7:1 or more, borders, control outlines and focus indicators at 3:1 or more, and heavier rules. **Follow system** follows the Windows high-contrast theme; macOS does not report **Increase contrast** yet, so turn it on by hand there.
- **Interface size**: 80%–200% (100% by default) enlarges text and controls on top of the system's display scale, never twice. Windows' minimum sizes grow with it, and an open Settings or input window grows to fit; skin previews keep the size the output has. The package editor takes the new size the next time it opens.
- **Reduce motion** is under **Settings › Playback**; the system's reduce-motion preference also turns it on.
- Every control has a name a screen reader announces, and switches and sliders report their state. Tab / Shift+Tab move between controls, Enter or Space operates them, Esc closes dialogs and pop-up menus.
- These settings are global; profiles do not change them.

### Checking for updates

The Steam edition is updated by Steam and has no such option. The GitHub edition checks GitHub Releases for new versions from **Settings › About**, and **Settings › General › Updates** checks automatically once a day by default. A new installer is only opened after it passes verification. Downloads try GitHub and the built-in proxies (gh-proxy.com, ghfast.top, ghproxy.net) in parallel, and you can add one custom proxy. Proxies only carry the bytes: the version manifest is signed with Ed25519 and installers are checked against SHA-256. See [Checking for updates](updates.md).

## Line markup

Lines are stored as BBCode-style markup, shared by source mode, quick phrases and templates. See [Line markup](markup.md) for the full reference.

```text
[@auto=1500]大家好！[wait=400][b][color=#e86a92]今天[/color][/b]也来[wave]打字[/wave]啦～
[pause][shake amp=2]哇！[/shake][se=coin][expr=smile]
[ruby=かんじ]漢字[/ruby][sticker=cats/hello h=1.6]
```

## Sticker packs

In **Settings › Assets › Sticker packs** you can download the **Sheep's pixel heads (static)** and **Sheep's animated emotes** presets, or add a local image folder, an Echo-Live root folder (containing `res/data/emoji.js`) or an Echo-Live emote extension folder (containing `main.js`). Presets only go online when you click them; after that they use the same import and management flow as local folders.

On import, images, thumbnails and full GIF frame caches are prepared in the background; the list shows downloading, processing, ready or failed. Once done everything works offline, the source files stay untouched, and unchanged content reuses its cache after a restart. Changing an image or the emote definitions rebuilds the affected cache automatically; removing a pack only stops loading it and never deletes the original files.

In the input window's sticker panel, search by name or pack name and click to insert. The result is still saved as NanaType markup such as `[sticker=sp/happy h=1.35]` or `[sticker=sprel/laugh h=4]`. Animated stickers play in the preview, display, history and Spout / Syphon output; with reduced motion they show their first frame. This feature only reads Echo-Live's image emote definitions and never runs extension scripts.

The sticker area on the right of the input window has **All** and per-series sub-tabs along the bottom; the sticker grid scrolls on its own and search only filters the current series. Drag one or more folders onto the sticker area or the sticker tab at the top to add them quickly; dropping on the tab opens the area automatically, and imported packs are managed together with the settings page.

The preset images come from [Echo-Live](https://github.com/sheep-realms/Echo-Live) and the [Sheep-realms animated emote extension](https://github.com/sheep-realms/Echo-Live-Sheep-realms-EmoteLab-Extension). Authors and image licenses are shown in the list, and the download folder keeps the source, definitions and license.

**Settings › Assets › Telegram stickers** lets you sign in to a Telegram account and find sticker packs with the official search, or download a whole pack by entering its name or `https://t.me/addstickers/packname`. The first sign-in needs your own API ID and API Hash from [my.telegram.org](https://my.telegram.org), a phone number with country code, then the code you receive and your optional two-step verification password. Clicking **Connect / log in** again restores the local session without re-entering credentials.

The download proxy accepts `socks5://127.0.0.1:7890` and authenticated `socks5://user:password@host:port`; leave it empty to connect directly. Reconnect after changing it; sign-in, search, file downloads and cross-datacenter connections all use the same proxy. This setting only applies to Telegram.

Static WebP, TGS vector animations and VP9 WebM with transparency are supported. After downloading, packs go through the same parsing, pre-transcoding and registration as local folders, appear in the same sticker list and bottom series tabs, and are identified as `tg_packname/fileID`. Animations are converted to full PNG frame caches that work offline after a restart; animations beyond Telegram's size or duration limits, or beyond what the current renderer supports, report a clear error. Original files and their sources are kept, and the stickers' rights remain with their creators.

The API Hash, phone number, code and password are only used while signing in and are never written to the config. The authorized session is stored separately as `telegram.session` in the app data folder, encrypted with the current user's DPAPI on Windows and protected by user-only file permissions elsewhere; do not share this session file. **Log out and clear session** signs out and deletes the local session; downloaded stickers keep working offline.

## Skins

A skin is a folder containing `skin.toml`, or a `.ntpkg` archive, and can inherit from a built-in skin and only write the differences. See [Skin format](skins.md) for the format.

A sound pack is a package containing `sounds.toml`. Choose it in **Settings › Playback › Sound pack** to replace the skin's typing, opening and continue sounds; the remaining sounds still come from the skin. See [Sound packs](skins.md#sound-packs).
