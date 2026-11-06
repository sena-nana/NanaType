[简体中文](../obs-spout.md) · [English](obs-spout.md) · [日本語](../ja/obs-spout.md)

# OBS / Spout output

> **Purpose:** Send NanaType windows to OBS over Spout2 on Windows and tune output performance.
>
> **On this page:** [Sender lifecycle](#sender-lifecycle) · [Performance settings](#running-alongside-games) · [Debug example](#debug-example)


On Windows, the display, the partner display and the history can each send a shared GPU texture over Spout2, which OBS receives with the Spout2 plugin's *Spout2 Capture*. For setup, see the OBS section of the [usage guide](usage.md); this page describes the sender lifecycle and performance settings.

## Sender lifecycle

- **Frame handoff**: NanaUI's DX12 render output is copied to the Spout shared surface through a D3D11 bridge device and handed off with a single shared fence; the next ready is not signaled until the previous release has completed. All outputs on the same device generation and the same adapter share one D3D11 device and immediate context, while each sender owns its own registration, shared surface, imported texture and fence, so tearing down one output doesn't affect another.
- **Dynamic pixels**: WGSL effects and shine sweeps declare that their pixels change over time (NanaUI `SceneGpuRenderer::content_version`), so even when the text is still, the natively retained output never reuses a stale frame.
- **Quick reopen (5 seconds)**: when an output is turned off, a transparent frame is sent first and the output is paused, keeping the sender for up to 5 seconds. Reopening with the same name, size and alpha reuses the original sender, shared texture pool and fence, and republishes the current text. A rename, a change of size or alpha, a name already in use, or a new device generation all prevent reuse; if publishing the transparent frame fails, the sender is torn down right away so the old text doesn't stay frozen in OBS. Each output has at most one sender waiting for reuse, with at most three across the whole app; repeated settings changes or hot reloads don't extend the deadline.
- **Cold reconnect (30 seconds)**: once the 5 seconds run out, the sender is unregistered and its image resources are released, keeping only the device without images or fences; the device is released 30 seconds after the last output is closed. When an output is enabled, the sender is created during the app update phase, and if the fixed size is already known a transparent surface is published first, so OBS discovery runs in parallel with rendering the first frame.
- **SDK fix**: the Spout SDK enumerates every display mode each time a sender is constructed (about 20 ms per output). At build time, the SDK copy in `OUT_DIR` replaces this with a single `ENUM_CURRENT_SETTINGS` query, falling back to 60 Hz on failure; if the original function changes, the build fails and requires an explicit review.
- Cleanup reuses the existing timer thread and a single cleanup task, with no per-frame polling; quitting the app releases every sender.


## Running alongside games

When a game or encoder saturates the GPU / CPU, you can turn these on in **Settings › Output / OBS › Performance** (Windows only):

- **Raise GPU priority**: sets the whole process's GPU scheduling priority to High and turns off execution speed throttling; if any system control is refused, everything falls back to Normal and the hint shows **Not in effect**.
- **Raise CPU priority**: raises only the threads that frames wait on (the UI thread that draws every window and sends Spout, and the timer thread that drives the per-character rhythm) to above normal; chat, download, decoding and other threads stay under normal system scheduling.
- **CPU affinity**: makes those threads prefer performance cores (chosen by the EfficiencyClass the system reports, not guessed from core numbers). On CPUs without hybrid cores it shows **Not needed**.

All three are off by default; the settings are saved under `performance` and reapplied automatically after a restart.

**Render GPU** in the same section picks the graphics card used for rendering (default **Automatic**).

When sharing textures over Spout, overhead is lowest when NanaType and OBS run on the same card.

The GPU device is only created at startup, so changes take effect after a restart: at startup the selected name is written to `WGPU_ADAPTER_NAME`, and if the selected card no longer exists it falls back to automatic selection; if `WGPU_ADAPTER_NAME` is already set externally, the external value wins.

Cards are matched by name, so two cards with the same name can't be told apart.

## Debug example

`cargo build -p nanatype --locked --example native_outputs` builds a debug entry point that delegates directly to the real `Host` (Windows debug / DX12 only), uses the real Spout sender and watches the shared fence from a monitor thread. Before running it, point `NANATYPE_NATIVE_PROBE` at a fresh, isolated directory; it doesn't read the regular install directory, and the observer is not included in release builds.
