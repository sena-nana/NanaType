[简体中文](../logging.md) · [English](logging.md) · [日本語](../ja/logging.md)

# Logging

NanaType and NanaType Editor have a single logging pipeline: NanaUI's `nana-diagnostics`. It handles collection, buffering, writing to disk, rotation, crash snapshots and export; NanaType decides in `crates/diag` (`nanatype-diag`) what to record, how much to keep, and what must never be written.

```text
任意线程 ──event!/report!/metric!──▶ 线程本地环形缓冲（满了就丢，不阻塞）
                                          │
                              nana-diagnostics 后台线程
                               ├─ 飞行记录器：最近 60 秒，内存中
                               └─ 批量写入 ──▶ 会话日志 .nlog
依赖库的 log / tracing ──bridge──▶ external.* 事件（限速、脱敏）
panic ──NanaUI 的 panic hook──▶ Fatal 事件 + 崩溃快照
```

(Any thread writes `event!`/`report!`/`metric!` into a thread-local ring buffer that drops on overflow and never blocks. A `nana-diagnostics` background thread keeps a 60-second in-memory flight recorder and batch-writes the `.nlog` session log. Dependencies' `log` / `tracing` output is bridged into rate-limited, redacted `external.*` events. Panics go through NanaUI's panic hook to a Fatal event plus a crash snapshot.)

The producer path does no file I/O, takes no locks and never waits; where no runtime is installed (for example in a plugin worker process) each call site is just one atomic read.

## File locations

| Platform | Session logs | Crash snapshots |
| --- | --- | --- |
| Windows | `%LOCALAPPDATA%\dev.sena-nana.nanatype\Logs` | `…\Crash` |
| macOS | `~/Library/Logs/dev.sena-nana.nanatype` | `…/Crash` |
| Linux | `$XDG_STATE_HOME/dev.sena-nana.nanatype/logs` | `…/crash` |
| Portable | `<程序目录>/data/logs` | `<程序目录>/data/crash` |

(`<程序目录>` is the program directory.) The editor uses its own app ID (`dev.sena-nana.nanatype-editor`). A single file rotates past 16 MiB; the log directory is capped at 128 MiB, 32 files and 14 days in total, and at most 16 crash snapshots are kept. **Settings › About › Logs folder** opens the directory directly.

`.nlog` is a binary format; view it with NanaUI's `nana-nlog`:

```bash
cargo install --git https://github.com/sena-nana/NanaUI nana-diagnostics --bin nana-nlog
```

```bash
nana-nlog text <文件.nlog> --redact-home
```

## Levels and switches

| Situation | Recorded (flight recorder) | Written to disk |
| --- | --- | --- |
| Default (release) | Debug and above | Warn and above, faults, markers, metric snapshots |
| **Settings › About › Detailed logs** | Debug and above | Everything |
| `NANATYPE_DEBUG=1` | Trace and above | Everything |
| Debug build | Debug and above | Everything |

The runtime always records Debug, so a crash snapshot contains the last minute of debug context; the switches only decide how much goes into the session log. The setting is read once, read-only, before any window opens, via `nanatype_settings::peek_diagnostics` (no migration, no quarantining of corrupt files); changes take effect after a restart. The editor follows the same switch in NanaType's settings.

Dependencies log through `log` (and tracing's `log` feature). By default only Warn and above is taken, Info with detailed logs on; `hyper`, `reqwest`, `ureq`, `rustls`, `h2` and `tungstenite` are limited to Error because their lower levels contain request details. Each crate gets at most one line per second, and the number of dropped lines is recorded in the next line's `suppressed` field.

## Privacy rules

Logs must be safe to attach to a bug report by default, so sensitive content is never written in the first place:

- **Never logged**: cookies (`SESSDATA`, `bili_jct` and so on), tokens, authorisation codes, passwords, proxy credentials; chat message text, user names, avatars; the lines the user types; room numbers, channel names, video IDs; audio device names.
- **May be logged**: status codes, error codes, counts, durations, byte counts, window or source indices, boolean switches, skin IDs, plugin IDs.
- Event fields can only be numbers or booleans (at most 4 per event), so they can never carry a string out.
- Free text may appear only in fault messages, and only through `nanatype_diag::report!`. It calls `redact::text`: URLs keep only scheme, host and path; values such as `token=`, `Cookie:` and `Authorization: Bearer` become `***`; the home directory becomes `~`; the text is truncated to 512 bytes. Redaction is a safety net, not a licence: don't format strings that might contain personal information into messages.
- Every error notice the user sees (`App::notice(…, true)`) is also logged as `app.error_notice`, so notice text must not splice in chat content or account details either.

## Event catalogue

Each crate declares its events as `static EventDescriptor` in its own `diag` module and lists them in `EVENTS`; `app/tests/diag_catalog.rs` checks that IDs and names are unique and that names start with their domain's prefix. Domains are allocated centrally in `crates/diag/src/domains.rs`:

| Domain | Prefix | Contents |
| --- | --- | --- |
| `0x0100` | `app.` | Lifecycle, settings loading, windows, error notices, translation failures (kind and HTTP status, never the line), and hotkey registration, trigger, and activation results; IDs from 100 up are the shared `nanatype-frame` part |
| `0x0101` | `settings.` | Reserved |
| `0x0102` | `output.` | Spout/Syphon, window output state, VTS/OSC/WebSocket bridges |
| `0x0103` | `chat.` | Shared parts such as chat images |
| `0x0107` | `twitch.` | Connection (1), connection error (2), account (3), account failure (4), warning (5), send failure (6), received chat event count (metric 50) |
| `0x0108` | `p2p.` | Endpoints, relay online, resource transfer |
| `0x0109` | `plugins.` | Worker ready time, failures (with the tail of stderr), script errors |
| `0x010A` | `plugin_log.` | Lines plugins write with `nana.log` |
| `0x010B` | `update.` | Per-source check result and duration, downloads, version parse failures |
| `0x010C` `0x010D` `0x010F` | `steam.` `stickers.` `audio.` | Why Steam is unavailable, sticker preview and upload failures, audio device open and loss |
| `0x010E` | `skin.` | Reserved; skin loading and shader errors are currently logged as `app.error_notice` |
| `0x0110` | `editor.` | The editor |
| `0x01F0` | `external.` | Dependency logs |

Connection and account states share one encoding: connection `0` off, `1` connecting, `2` connected, `3` disconnected or stream ended, `4` error, `5` login required; account `0` restoring, `1` signed out, `2` waiting for QR scan or verification, `3` ready, `4` expired, `5` failed.

### Adding an event

```rust
use nanatype_diag::domains::P2P;
use nanatype_diag::{EventDescriptor, FieldDescriptor, Severity};

pub static RELAY_SWITCHED: EventDescriptor = EventDescriptor::new(
    P2P, 5, "p2p.relay_switched", Severity::Info, &[FieldDescriptor::u64("ms")],
);
```

```rust
nanatype_diag::event!(crate::diag::RELAY_SWITCHED, ms = took.as_millis() as u64);
nanatype_diag::report!(crate::diag::ONLINE_TIMEOUT; "{error}");
```

Then add it to the crate's `EVENTS`. On hot paths, count with `metric!` or rate-limit with `Throttle`; for milestones use `nanatype_diag::marker` (fixed text only). The session header's `edition`, `features`, `os`, `locale`, `skin` and `verbose` are written by `set_session_info`.

## Plugins

Plugin workers are separate processes and do not install the diagnostics runtime. The host records each worker's ready time, timeouts and protocol errors; when a worker exits abnormally it attaches the last few lines of its stderr (Windows AppContainer workers have no stderr). Lines a plugin script writes with `nana.log(level, message)` come back over the protocol and the host logs them as `[plugin ID] message`; see the [plugin docs](plugins.md) for the rules.

## Diagnostics package

**Settings › About › Diagnostics package › Export** runs in the background and:

1. Writes a snapshot of the current flight recorder and flushes pending writes to disk;
2. Exports the 5 most recent sessions and 5 snapshots as text (`.txt`) and JSON Lines (`.jsonl`), with the home directory replaced by `~` and no raw `.nlog` included;
3. Adds `manifest.json` and `summary.json` (version, build number, edition, OS, language, skin ID, which platforms and outputs are enabled (booleans only), plugin count, and the detailed-logs switch);
4. Zips it as `dev.sena-nana.nanatype-diagnostics-<时间>.zip` (`<时间>` is a timestamp) in the log directory and opens that directory.

The package stays on your machine; whether to attach it to a bug report is up to you. If the previous run crashed (there is a panic snapshot newer than `diagnostics.crash_seen`), the next launch reminds you once.
