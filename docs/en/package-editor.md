[简体中文](../package-editor.md) · [English](package-editor.md) · [日本語](../ja/package-editor.md)

# Package editor

> **Purpose:** Author, preview, validate, sign, and export NanaType packages with the standalone NanaType Editor.
>
> **On this page:** [Workspace](#layout) · [Open a project](#opening-a-project) · [Edit and preview](#files) · [Sign packages](#protection--signing) · [Save and export](#saving-and-exporting)


Tracks [#6](https://github.com/sena-nana/NanaType/issues/6); for the format and boundaries see [Unified package design](../package-design.md#编辑器与非目标).

The editor is a standalone program, **NanaType Editor** (`editor/`, built and installed separately).

It shares the skin renderer, UI components and plugin host in `crates/frame` with NanaType, but neither contains the other's code, strings or assets: NanaType has no editing entry point, and the editor has no chat, multiplayer, stickers, outputs or hotkeys.

The editor works with internal drafts and with directory projects that are written back in place. Editing never changes the current skin, installed packages, plugin consents or running plugins. Changes take effect only after you export a `.ntpkg` and import it in NanaType under "Settings → Plugins & packages".

## Layout

Once a project is open, the window is a canvas-centred workspace (minimum 960×600 logical pixels, initially 1440×900):

- **Toolbar**: the package name (click it for the project menu: "Revert to last save", "Close"), package type, and "Undo"/"Redo". Workspaces that show the canvas also have the layout target ("Base layout", "Landscape", "Portrait", "Breakpoint rule"), viewport size presets and the "Display"/"History"/"Input" views. On the right are the "Show source beside the canvas" or "Show live preview" toggle, export and save. Only Save uses the accent colour, and only while there are unsaved changes.
- **Activity bar**: "Design", "Files", "Sounds", "Data and pipelines", "Lua"; at the bottom, "Package" and "Protection & signing". Entries for capabilities the project lacks are hidden (a skin-only package has no Lua, for example). Clicking the current entry again collapses or expands the left panel.
- **Left panel / centre / right inspector**: these change with the workspace. Both side panels can be resized by dragging, and their widths are kept when you switch workspaces. When the window is narrower than 1280 logical pixels, the left panel starts collapsed.
- **Bottom drawer**: the problem list (click to jump to the file and line) and the Lua debug log. Collapsed by default; open it from the status bar.
- **Status bar**: problem count, debug log entry, project origin, and save, validation and signing status.

Colours follow the system light/dark setting; the canvas area always uses a neutral background.

## Data directory

The editor reads and writes NanaType's data directory (Windows `%APPDATA%\dev.sena-nana.nanatype`, macOS `~/Library/Application Support/dev.sena-nana.nanatype`); the start page shows the current path:

- `packages/`: packages installed in NanaType. Read-only; used to edit copies, duplicate packages and bundle dependencies.
- `package-drafts/`: manually saved drafts, with separate auto-recovery snapshots under `recovery/`.
- Signing keys (a DPAPI file or the keychain item `dev.sena-nana.nanatype.package-signing`). Keys created earlier in NanaType keep working.
- `config/settings.json`: read-only. By default the preview uses the package's own defaults; personalization is applied only after you explicitly turn on "Simulate NanaType personalization", and it is never written back to the project.

Portable NanaType keeps its data in `data/` next to the program; point the editor at it with `nanatype-editor --data <that directory>`.

The portable editor itself also uses the `data/` next to its program. `nanatype-editor <file.ntpkg or project directory>` on the command line opens a project directly; launching again hands the absolute path to the window that is already open, going through the same unsaved-changes prompt.

NanaType and the editor can run at the same time.

## Opening a project

The start page lists New, Open, Drafts and the packages installed in NanaType:

- **New**: choose "Skin only", "Resources only", "Lua only", "Sound pack only" or "Mixed", and enter a package ID. The templates pass validation as-is.
- **Open project directory…**: choose a root directory containing `package.toml`; saving writes back to that directory. Installed package directories cannot be opened as writable projects. Directory scans skip version-control directories and reject symbolic links.
- **Open .ntpkg…**: damaged packages can also be opened so you can repair them. If signature verification fails, all local files are read-only.
- **Edit** (installed user packages): reads a copy from the install directory; the install directory itself is never modified.
- **Copy as new package** (any package, built-in ones included): creates a derived copy with a new ID, strips the original signature, and writes `derived_from`.

After installing a new package in NanaType, click "Reload" to refresh the list. Changing a known package ID requires "Copy as new package" (a derived copy), and source edits follow the same rule. Before deriving, any uncommitted forms must be dealt with, and everything goes through the same unsaved-changes prompt.

## Files

The "Files" workspace has a file tree on the left. A lock icon marks signature-protected files, and dependency packages under `dependencies/` are read-only. The top of the tree offers import, new / move / replace and delete. The centre shows the selected file, and a live preview can stay open on the right. Text files open in an editor with line numbers:

- Lua, TOML and WGSL support indenting and toggling comments.
- Search is case-insensitive and supports previous, next, replace and replace all; each file keeps its own cursor, selection and scroll position.
- TOML and JSON can be formatted, keeping comments and key order intact.

Binary files show their format (detected from content), size, BLAKE3 digest, and the source and license from the data declaration.

Files can be created, moved, renamed, replaced, and imported several at a time.

The target path field sets the directory for new or renamed files; imports go to the directory of the file selected when the import started (files at the root import into `assets/` by default).

Renaming shows the affected references and updates recognised declaration paths automatically; strings inside Lua/WGSL are only flagged for manual updating, and protected references block the operation.

Images can be shown on a transparency checkerboard with any declared nine-slice guides; fonts show sample text; audio shows its duration and offers play, stop and volume — the audio device is opened only when you explicitly play.

Imported assets keep their file names. The host checks the path, the size, and that the extension matches the content — a `.png` must really be a PNG, for example.

Full validation runs 300 ms after you stop typing, keeping only one background job and folding it into the latest version.

On errors the last valid preview stays visible and is marked stale.

Syntax diagnostics for sources over 64 KiB and the pipeline list of large skins are updated in the background, while the typing path keeps only lightweight checks.

Full validation follows the same path as import and runs in the background, covering dependencies, signatures, skins, data and the script entry.

Clicking an item in the problem list jumps to the corresponding file and line.

## Package

The form can edit the following fields; when writing back to `package.toml`, everything else, including comments, is preserved:

- Localized names and descriptions, tags, cover, minimum version, base skin
- Version, author, creation date, licensee, license
- Capabilities
- Dependencies, one `package-id@exact-version` per line, selectable from installed packages. Missing versions, duplicate IDs and invalid versions are reported line by line; nothing is silently ignored.

Turning on the skin or Lua capability adds the starter files. Dependencies are bundled into the draft from installed packages.

Declared data and processing pipelines are added, edited and removed with forms in the "Data and pipelines" workspace:

- **Data** (`[data.<id>]` in `package.toml`): ID, file path, format, format version, source, license, and JSON-only constraints (a JSON object; leave empty for no constraints). The file must already be in the package. Renaming also updates `[lua] reads`.
- Live diagnostics, the format action and the editing mode of declared data follow `format`: text data in a file named `.json` is not reported as invalid JSON, and JSON data in a `.txt` file can still be formatted.
- **Pipelines** (`[pipelines.<name>]` in `skin.toml`): name, processor, each input port, and parameters. Entries also list the output names and types of the registered processor. Inputs take a data ID (a dependency's data is written `pkg:package-id/data-id`), or `pipeline-name.output` to feed from another pipeline; parameters take one `name = value` per line.

Uncommitted form content (including invalid input) is kept in the project session, so switching tabs does not lose it, and it counts toward the unsaved-changes prompt and recovery snapshots. These inputs must be applied or discarded before saving or exporting.

Before writing, `package.toml` is parsed strictly and pipelines are checked against the registered processors (port types, cycles, depth, parameter ranges, whether referenced data is declared and its format matches, and whether materials lose their source).

If the edit introduces new errors, nothing is written and a message explains why.

Writes modify the file in place, preserving other keys and comments.

When `package.toml` or `skin.toml` is signature-protected, the corresponding section is read-only.

Pipelines that cannot be parsed are still listed, with a note that they must be fixed in the source.

## Protection & signing

The first time you sign, choose "Use this computer's signing key"; if there is no key yet, one is created. The private key is kept only in the system's protected storage — the keychain on macOS, DPAPI on Windows. It never goes into packages, logs or plugins. Signing is not supported on other platforms yet.

Tick the files to protect and sign; `package.toml` is always protected. After signing:

- Protected files are read-only, and the file list is fixed.
- After editable files are modified, the signature still truthfully shows as valid.
- Packages signed with this computer's key can have their signature removed, then be modified and signed again.
- Packages signed with another key can only be modified after copying them as a derived copy.

A signature only shows that the content is unchanged since signing; it does not prove who the author is.

## Preview

The preview is the canvas in the centre of the "Design" workspace.

Above the canvas are the playback mode ("Still / stop", "Play", "Reduced motion", "Advanced effects off"), "Replay", zoom and "Fit", background, and "Canvas settings" (custom logical width and height, "Simulate NanaType personalization"), plus the layout layer currently in effect; below it are the "Sample speaker" and test text.

The toolbar selects the viewport preset ("Landscape", "Portrait", "Narrow", "Default size") and the view.

Still, play, reduced motion, advanced effects off and replay all use a private Stage, which stops refreshing once you leave the canvas.

Once a skin passes validation, the draft is drawn with the real renderer, and you can switch between three views:

- **Display**, **History**: the same as the skin preview in Settings.
- **Input**: the input box as it looks when skin preview is on in the input window (`Source::Draft` of `dialogue`), holding a private sample draft. The sample editor cannot take focus, does not accept input and never sends messages.

The preview uses the same `nanatype_frame::ui::preview::stage` as NanaType's skin page, and each sample is drawn on its own private `Stage`. The live preview on the right of the "Files" workspace uses this canvas too.

## Properties and canvas

The left side of the "Design" workspace holds the **Layers**: the tables declared in `skin.toml` (including inherited defaults) listed by hierarchy, with the dialogue box first and array tables (such as decorations) listed by index.

Below them are the "Preview parameters", which change only the test values.

Selecting a layer shows that table's values in the **Inspector** on the right and selects the parts it draws on the canvas (name tag, portrait, continue mark, decorations); clicking a part on the canvas also selects its layer.

The inspector shows one value per row.

Labels of locally declared values use the accent colour, inherited values a secondary colour.

Numbers can be adjusted by dragging (one drag commits a single undo step), colours have a picker, and other values are typed in and confirmed with "Apply". "Restore inheritance" removes the local declaration, and the link icon opens the parameter binding.

Sub-tables are listed at the end of the rows and can be entered level by level; "Add field" offers parameter, group, preset and responsive templates.

**Advanced skins** treat the source as authoritative. "Show source beside the canvas" in the toolbar splits the centre into the canvas and the `skin.toml`/WGSL source, and selecting a layer scrolls the source to its table. "This table's TOML" at the bottom of the inspector shows the whole block of that table (and its sub-tables) in the source; editing it in place replaces only that block, keeping everything else and all comments verbatim.

Tables without their own header tell you to edit them in the enclosing table.

Shaders, materials, parameters, groups and presets all appear as layers.

The canvas supports selecting single elements, moving/resizing decorations, and moving the name tag, portrait and continue mark. Adding, duplicating, deleting and reordering decorations is at the top of the layers panel. Arrow keys move, Shift increases the step, and Esc or losing window focus cancels a drag; one drag commits a single undo record.

The edit target can be "Base layout", "Landscape", "Portrait" or "Breakpoint rule".

Properties show local overrides and inherited values, "Restore inheritance" removes the local declaration, and the preview shows the order in which the active layers are applied.

The first override of an array copies the full effective array of the current layer.

Layout geometry comes from the real render nodes, gestures are converted according to the preview zoom, and percentage lengths are preserved.

Before a drag is committed, the actual resolved result is compared; if the selected layer is overridden by another responsive layer, the write is refused with a prompt to switch targets.

A parameter binding must be removed explicitly before the value can become a constant; shapes inside WGSL are still edited in the source.

Breakpoint-rule layouts keep the runtime's literal-value restrictions: if an inherited array contains parameter bindings, committing it to a rule layer is rejected by full validation and the original is kept. Landscape and portrait layouts can keep parameter bindings.

Local declaration edits use `toml_edit`, preserving other fields, comments, unknown fields and parameter placeholders. While the source has a syntax error, the corresponding visual operations are paused, and you can keep fixing it on the source page. The Sounds page reuses the declared-property editor and asset playback. There is no shader node graph or Lua breakpoint debugger.

## Lua debugging

Debugging follows these rules:

- Background full validation uses a separate Wasm worker process that only compiles the Lua; it never executes the source, so no script trust is needed.
- Before actually running, read the risk notice and tick "I reviewed the current source and trust it to run in the debug process"; only then can you "Start".
- The script runs in the same kind of isolated Wasm worker process as real plugins, under the same sandbox and limits; the worker is obtained by the editor relaunching itself with `--plugin-worker`.
- Starting and reclaiming the system process happens in the background; a failed start shows an error, and results from before a stop or edit are ignored.
- The debugger sends only the test events you choose (`line_start`, `line_end`, `speaking`, `chat_message` or custom JSON), and supports replaying an editable event sequence and filtering the log.
- Returned views and commands appear only in the debug panel.
- Editing any file stops the script and revokes trust; scripts never run automatically.
- Infinite loops, views over budget and syntax errors all end with an error and never freeze the UI.
- Error line numbers are mapped back to the entry file and can be clicked to jump there.

## Saving and exporting

| Action | Behaviour |
|---|---|
| Save | Drafts are written to `package-drafts/`, directory projects back to their source directory. The source does not need to pass validation first, but pending forms must be dealt with |
| Revert to last save | Discards all changes made since |
| Close / quit / open another project | Always asks to save, discard or cancel; after saving, the requested action runs, and any new edits made meanwhile stay unsaved |
| Discard draft | Requires a second confirmation; deletes only the draft and never affects installed packages |
| Export .ntpkg… | Bundles dependencies and runs full validation; on failure no file is written |

Drafts, recovery snapshots and exports are written as a temporary file in the same directory followed by an atomic replace; if a write fails, the old file is kept.

Saving, asset reads, full-validation digests and key access run in the background, and results carry the project identity and version.

Global undo/redo covers source, forms, files, signing and the canvas, and signing and protection status are restored along with content.

A recovery snapshot is written 2 seconds after you stop editing, without overwriting the manual save point; on the next launch, the start page lets you recover or discard it. Snapshots of directory projects keep the manual save point, the source path and the latest disk baseline separately, and external changes are re-checked after recovery.

The directory watcher coalesces bursts of events.

Unmodified files follow the disk, edits to different files are merged, and conflicts on the same file offer keep local, accept disk, or save a copy.

The baseline is re-checked before saving, and overwriting is blocked while conflicts are unresolved.

Multi-file saves use a log and backups in `directory-transactions/` under the app data directory, and an interrupted save is recovered the next time the project opens; if recovery conflicts with newer external changes, the log is kept and its path is reported for manual handling.

On macOS the package declares the `.ntpkg` document type and handles Finder open events; on Windows the installer registers an "Open with" entry without replacing the user's default app. The command line also accepts directory paths.

## Verification

```sh
cargo test --locked -p nanatype-theme --features editor --lib
cargo test --locked -p nanatype-editor --lib --test package_editor --test editor_reliability --test editor_authoring_ui -- --test-threads=1
cargo run --locked -p nanatype-editor --example editor_perf
scripts/check-product-split.sh
cargo run -p nanatype-editor
```

GPU UI tests run serially and produce screenshots under `target/package-editor/` in Chinese, English and Japanese at 960/1280/1440/1920px, covering the Design, Files, Sounds, Lua, Data and pipelines, Package and signing workspaces.

Reliability tests cover save versions, stale results, recovery of invalid forms, undoing signatures, local declarations and reference operations, directory conflicts and interrupted saves; the 28 built-in skins reuse the theme library's loading, responsive snapshot and round-trip tests.

The performance example records the CPU time of edit + view under a fixed large-source / large-asset load.

It cannot replace native input latency, GPU preview updates or drag frame times.

Native macOS/Windows file pickers, quitting, IME, DPI 1/1.5/2, watching, audio, keychain/DPAPI, system open events and the final installers still need separate acceptance; passing automation does not mean platform acceptance is done.
