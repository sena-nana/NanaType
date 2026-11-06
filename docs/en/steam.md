[简体中文](../steam.md) · [English](steam.md) · [日本語](../ja/steam.md)

# Steam edition

> **Purpose:** Build, package, publish, and operate the Steam edition of NanaType.
>
> **On this page:** [Edition differences](#steam-edition) · [Workshop](#workshop) · [Packaging](#packaging) · [Upload and release](#uploading-and-going-live)


The Steam edition and the GitHub release share the same code and are told apart by Cargo features:

| | GitHub release (default) | Steam edition |
| --- | --- | --- |
| Build | `cargo build -p nanatype` | `cargo build -p nanatype --no-default-features --features builtin-fonts,steam` |
| Updates | Checks GitHub Releases (`updates`, see [Checking for updates](updates.md)) | Updated by SteamPipe; contains no update-check code |
| Workshop | — | The Skins and Playback pages access subscribed skins and sound packs |

`steam` cannot be enabled together with `updates` (a `compile_error!` in `app/src/lib.rs`). `scripts/check-product-split.sh` and CI check that the Steam edition doesn't link `nanatype-update` and that other builds don't link Steamworks.

## Connection sources

Twitch and blivechat / 彗星号 (Comet) are available in both editions. The Steam edition contains no GitHub update-check code; blivechat's room number and identity code modes are both kept.

## Configuration shared with the GitHub edition

Both editions use the same app ID (`dev.sena-nana.nanatype`) and bundle ID, so settings, the package directory, cache, logs and credentials live in the same places. The Steam package uses nana-packager's `steam` backend and doesn't write the portable marker, so user data never ends up in the Steam install directory. Both editions share the single-instance lock and can't run at the same time.

- Launch at login writes the same startup entry (Windows `HKCU\…\Run\NanaType`, macOS LaunchAgent); whichever edition set it last wins.
- Workshop content is not copied into `packages/`. If you select a Workshop skin or sound pack and then open the GitHub edition, it temporarily uses the default skin or the skin's own sounds without rewriting your settings; going back to the Steam edition restores them. A skin made from a Workshop skin with **Duplicate to edit** bundles the Workshop skin into its own `dependencies/`, so it works in both editions.

## Steam IDs and local configuration

The App ID and depot IDs are not committed to the repository:

1. Copy `.cargo/config.example.toml` to `.cargo/config.toml` (already in `.gitignore`) and fill in `[env]`:
   - `NANATYPE_STEAM_APP_ID`: the Steam App ID. Cargo passes it to `crates/steam/build.rs` at build time, which compiles it into the program.
   - `NANATYPE_STEAM_DEPOT_WINDOWS`, `NANATYPE_STEAM_DEPOT_MACOS`: written into the depot build scripts by the packaging scripts.
   - Optional `NANATYPE_STEAM_TAG_SKIN`, `NANATYPE_STEAM_TAG_SOUNDS`: the tags used by the Workshop browse buttons, defaulting to `Skin` and `Sound Pack`.
2. You can also set them directly as environment variables; in CI, provide them through secrets.

A release build that links Steam fails outright if the App ID is missing; a debug build falls back to Valve's test app Spacewar (480) with a warning. The program specifies the App ID through `Client::init_app`, so no `steam_appid.txt` is needed, and none is included when packaging.

## Development and debugging

- `cargo run -p nanatype --no-default-features --features builtin-fonts,steam` does not connect to Steam by default, and the Workshop shows as unavailable; it only connects when `NANATYPE_STEAM=1` is set and the Steam client is running.
- A release build started outside Steam relaunches itself through Steam (`restart_through_steam`, before the single-instance lock); if Steam isn't running it starts normally, just without the Workshop.
- The self-check run used for package validation (`NANA_PACKAGE_VALIDATE`) neither connects to Steam nor relaunches.

## Workshop

- Subscribed, installed items are handed to the package registry as read-only external package directories (`SkinRegistry::set_external`); the root of an item's folder must be a complete package (`package.toml` at the top level).
- When package IDs collide, built-in packages win, then user-installed packages, and finally the Workshop item with the smaller item ID; shadowed items show the reason in the subscription list.
- The subscription list shows download progress, waiting to download, not downloaded, unusable and conflict states, and lets you open the item page, download again or unsubscribe. After unsubscribing, the item disappears from the list and the registry immediately, and Steam deletes its files after the app quits; if it was selected as the skin or sound pack, the default is restored.
- Workshop packages can't be exported or deleted; on the **Plugins & packages** page, **Remove** becomes **Unsubscribe**. Lua feature packages still need to be reviewed and consented to one by one.
- **Browse Workshop** prefers to open the page for the matching tag in the Steam overlay; when the overlay is off, it hands `steam://openurl/` to the Steam client instead. The overlay attaches to every window, including the transparent display windows and the Spout/Syphon outputs, so before an official release, test whether it shows up in the stream picture and, if needed, disable the overlay in the Steamworks back end.

### Publishing items

In NanaType Editor, export a `.ntpkg` and unzip it into a folder, or use the package directory directly, then upload it with SteamCMD's `workshop_build_item`; the content root is the package directory. Tag skins with `Skin` and sound packs with `Sound Pack` (or whatever tags you set in your local configuration).

## Packaging

Requires `nana-packager` (as in the [release process](updates.md)) and the IDs in `.cargo/config.toml`.

```bash
pwsh scripts/package.ps1 -Steam
```

```bash
scripts/package-macos.sh --steam
```

- Both scripts build with the Steam feature, fill the IDs into `packaging/steam/nana-package.steam.toml` and write it to `target/steam/nana-package.toml`, then package and validate; no installer, zip or dmg is produced.
- On Windows, the `steam_api64.dll` shipped with steamworks-sys is placed next to `NanaType.exe`.
- On macOS, `libsteam_api.dylib` goes into `Contents/Frameworks`, and the executable's load path is rewritten and then ad-hoc re-signed, because `Contents/MacOS` in the bundle may contain only executables.
- License files are generated by `third-party-notices.ps1 -Steam` and `macos-licenses.py … nanatype-steam`, listing only the crates the Steam edition actually links.
- The output is in `dist/app/`; nana-packager additionally writes depot build scripts and file manifests to `dist/steam/` (`depot_build_<depot>.vdf`, `nana-steam-build.json`). Upload with the scripts below, not with that depot build script (it has no exclusion rules).

Upload credentials are managed only by SteamCMD; neither the repository nor the scripts read them.

## Uploading and going live

Requires [SteamCMD](https://partner.steamgames.com/doc/sdk/uploading) (on PATH, or specified via `STEAMCMD`; on macOS this is usually `steamcmd.sh`), a Steam build account with upload permission for the app, and the same `.cargo/config.toml` IDs as for packaging.

```bash
pwsh scripts/steam-upload.ps1 -Account <构建账号>
```

```bash
sh scripts/steam-upload.sh --account <构建账号>
```

- The account can also be given through the `STEAM_BUILD_ACCOUNT` environment variable. The script only passes the account name to `steamcmd +login <账号> +run_app_build <脚本> +quit`; SteamCMD itself asks for the password and the Steam Guard code and caches the login. The script neither receives nor stores the password.
- The script fills the IDs and absolute paths into `packaging/steam/app_build.vdf.in` and `depot_build.vdf.in` and writes them to `target/steampipe/` (`app_build_<app>.vdf`, `depot_build_<depot>.vdf`); build logs, manifests and chunk caches go to `target/steampipe/output/`. None of it is committed.
- Content roots: the Windows depot is `dist/app/NanaType` (the directory containing `NanaType.exe`), and the macOS depot is `dist/app` (containing `NanaType.app`). `steam_appid.txt`, `*.pdb` and `.DS_Store` are excluded from uploads.
- By default only the depot for the local platform is uploaded (both packaging scripts clear `dist/`, so a machine only has its own platform's output). `-Platform windows|macos|all` (`--platform`) chooses explicitly; `-WindowsContent` and `-MacosContent` (`--windows-content`, `--macos-content`) point to output copied from another machine. To upload both depots at once, copy the Windows `dist/app/NanaType` folder to the Mac and use `--platform all --windows-content <dir>`; don't do it the other way around by copying the `.app` to Windows, because the symlinks and metadata inside the `.app` bundle get lost.
- `-Preview` (`--preview`) only generates the file manifest and logs without uploading, which is useful for checking the exclusion rules.
- The scripts don't set a live branch (`SetLive` is empty): the `default` branch can't go live automatically. After uploading, find the new build in Steamworks under SteamPipe → Builds (`https://partner.steamgames.com/apps/builds/<App ID>`; its description is `NanaType <version> (<commit>) <platform>`), choose the "default" branch, click "Preview Change" to check the depots, then set it live.
- Uploading the two platforms separately produces two builds, each containing one depot. Before going live, confirm in "Preview Change" that the other depot won't be removed; the safest option is to go live with a single build that contains both depots.
