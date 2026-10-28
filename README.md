# Gateterm

![Gateterm Banner](docs/assets/banner.png)

English | [中文](README-cnzh.md)

A tiny, memory-frugal terminal server: a daemon hosts pty sessions that survive
disconnects, and clients attach over a WebSocket binary protocol with seamless
resume. It is not a tmux wrapper — it extracts the kernel of what tmux does
(process + IO + screen-state hosting) and gives window management back to the
client.

## Why

This project is inspired by tmux: a hosted, always-on console is something you
end up needing constantly. Jupyter's built-in terminal already nails the
experience — open a browser, get a live shell that survives the tab — but it is
deeply bound to Jupyter itself. What was missing is an **extremely lightweight,
memory-frugal standalone terminal server**.

That is gateterm's entire positioning. And unlike tmux, its GUI is not a pure
in-terminal UI: sessions are exposed through a client-neutral wire protocol, so
you can operate them from a browser or the native Android client.

## Philosophy

A memory-frugal Swiss Army knife — and it stays that way:

- **Small**: single static binary, one process, minimal resident footprint.
- **Restrained**: the minimum feature set that meets the need, done well.
- **Not all-in-one**: window management belongs to clients; anything a hosted
  shell can already do (editors, file tools) will not be rebuilt into the
  server.

## Quick start

```sh
# Requires Node/npm: the server's build.rs builds clients/web automatically
# (set GATETERM_SKIP_WEB_BUILD=1 to skip it)
cargo run -p gateterm-server
# The startup log prints a ready-to-open URL (Jupyter-style; a fresh random
# token is generated on every start):
#     http://127.0.0.1:7683/#token=<random token>
```

See [DESIGN.md](DESIGN.md#standalone-server) for authentication, configuration, launchers, network access and update behavior.

## Desktop app (Windows / macOS)

Run the tray-resident server host on Windows or macOS, and open terminals in the browser or mobile app:

```sh
cargo run -p gateterm-desktop
```

See [DESIGN.md](DESIGN.md#tray-server-host) for settings, QR pairing, session lifecycle, chat access and hook management.

## Android client

A native Kotlin + Jetpack Compose client for connecting to Gateterm servers. See [DESIGN.md](DESIGN.md#android-client) for connection, keyboard and terminal behavior.

Build with JDK 17 and Android SDK 36, or open `clients/android` in Android Studio:

```sh
cd clients/android
./gradlew :app:assembleDebug :app:testDebugUnitTest
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

Use `gradlew.bat` on Windows. After installation, add the server address and
token, or import the server access URL from the clipboard.

## Agent session history

See [DESIGN.md](DESIGN.md#agent-session-history) for browsing, resuming and indexing agent sessions.

## Gateterm Cloud (optional)

Sign in to Gateterm Cloud on your server and Android client to receive agent questions, permission requests, and completion notifications on your phone.

## Architecture

Wire protocol: [`docs/protocol.md`](docs/protocol.md) — the normative spec, and
the checklist any protocol change has to pass. Background and rationale:
`docs/superpowers/specs/2026-08-16-gateterm-design.md`.

- `crates/protocol` — frame and message definitions (single source of truth)
- `crates/server` — the daemon: pty hosting, server-side grid, WebSocket
- `crates/client-core` — reusable logic for Rust clients
- `crates/desktop` — tray-resident GUI with the server embedded (Windows / macOS)
- `clients/web` — reference client (TypeScript + xterm.js)
- `clients/android` — native Android terminal client (Kotlin + Jetpack Compose)

## Roadmap

gateterm is currently an **experimental demo**, with browser and native Android
clients available.

1. **More terminal capability, more clients.** Deepen core terminal features,
   continue improving Android, and bring the same protocol to iOS and other platforms.
2. **Agent integration.** Extend live status, remote approval, chat and session resume across clients and supported agents.
3. **No file manager.** File management is very unlikely to ever be added —
   that's what the shell in your session is for.

## Known limitations

See [DESIGN.md](DESIGN.md#known-limitations) for terminal resync limitations.

## License

[MIT](LICENSE.md)
