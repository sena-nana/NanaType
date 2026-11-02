[简体中文](../updates.md) · [English](updates.md) · [日本語](../ja/updates.md)

# Checking for updates

NanaType gets new versions from GitHub Releases. The Steam edition doesn't include the update check described here (the `updates` feature isn't compiled in) and is updated by SteamPipe instead; see [Steam edition](steam.md). Files may be downloaded through third-party proxies: trust depends only on the signature and digest, never on where the download came from.

## How it works

1. The release workflow (`.github/workflows/release.yml`, triggered by pushing a `vX.Y.Z` tag) packages the Windows installer and portable build and the macOS (Apple silicon) dmg and zip, generates `latest.json` (platform, type, size and SHA-256 of each file) and its Ed25519 signature `latest.json.sig`, and creates a draft Release.
2. The client (`crates/update`) requests `releases/latest/download/latest.json` and its signature from GitHub, the built-in proxies and any proxy you entered, all at once, verifies them with the built-in public key, takes the result with the newest version, and orders the sources by how quickly they responded.
3. While downloading, the SHA-256 is computed as the file is written; if the size or digest doesn't match, the next source is tried. The file is handed to the system to open only after it passes verification.

The client never contacts `api.github.com`, so any service that can proxy file downloads from `github.com` works. Error pages, stale caches or tampered files returned by a proxy are all rejected; version numbers from several sources are compared at once, so an old manifest cached by a proxy can't hide a newer version.

## Proxies

Proxies use the prefix form: `<proxy>https://github.com/sena-nana/NanaType/releases/...`. The built-in list lives in `crates/update/src/mirror.rs`; measured in 2026-10 (from a test location outside mainland China):

| Proxy | Release files | Notes |
| --- | --- | --- |
| gh-proxy.com | Works; SHA-256 matches the original | Cloudflare back end |
| ghfast.top | Works; matches | |
| ghproxy.net | Works; matches | Best for files under 100 MB |
| gh.llkk.cc, mirror.ghproxy.com | Unreachable | Not included |
| jsDelivr | Doesn't support Release files | Not included |

These are all free services run by individuals and may throttle, change domains or shut down, so they are used only as transport. **Settings › General › Updates › Custom proxy** lets you add one more prefix starting with `https://`, which is tried first.

## Checking and installing

With **Check for updates automatically** on, NanaType checks in the background about 10 seconds after startup, at most once every 24 hours. When a new version is found you're notified, and in **Settings › About** you can read the notes, **Skip this version** or **Download**. Downloads go to `updates/` in the cache directory and are cleared on the next launch.

- Windows installed version (the program directory contains `unins000.exe`): runs `setup.exe`; NanaType quits first.
- Windows portable version: opens the folder containing the zip, and you unzip and replace the files yourself.
- macOS: opens the dmg. There are no builds for Intel Macs yet.

## Signing key

```bash
cargo run -p nanatype-update --example keygen
```

Put `public` into `PUBLIC_KEY` in `crates/update/src/sign.rs`; store `secret` as the repository secret `NANATYPE_UPDATE_KEY` and keep an offline backup. Never commit it. The workflow generates and signs the manifest with `--example manifest` and fails if the secret and public key don't match.

Rotation: old versions only trust the old public key, so first publish a version signed with the old key that embeds the new public key, then replace the secret. If the private key leaks, anyone can sign manifests; rotate immediately and tell users to download the new version manually from GitHub.

## Releasing

1. Change `workspace.package.version` in the root `Cargo.toml` and commit.
2. `git tag vX.Y.Z && git push origin vX.Y.Z` (the tag must match the version).
3. Review the draft Release, then publish it. `releases/latest` only points to it once it's officially published; tags containing `-` are marked as pre-releases and aren't offered to the stable version.
