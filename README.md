<div align="center">

# Gateterm

**A cross-platform terminal & AI Agent workbench: persistent sessions, seamless resume, native agent awareness.**

<sub>Pure Rust server & protocol · GPU Desktop on Zed's `gpui` + `Alacritty` · Web (`xterm.js`) · Native Android</sub>

<br />

[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-orange)](#)
[![Desktop](https://img.shields.io/badge/Desktop-GPUI%20%2B%20Alacritty-3FDD8C)](clients/desktop-client)
[![Clients](https://img.shields.io/badge/Clients-Desktop%20%C2%B7%20Web%20%C2%B7%20Android-blue)](#)
[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE.md)

<sub>English · [简体中文](README-cnzh.md)</sub>

<br />

![Gateterm Banner](docs/assets/banner.png)

</div>

---

## Why

**A background server owns your shells, PTY processes, and screen state — not a window or browser tab.** Everything below follows from that boundary:

- **Memory-frugal & persistent** — A tiny single-binary daemon hosts your PTY sessions. Quit the desktop app, close the browser tab, or switch away on your phone — your shells and coding agents keep running.
- **Window management belongs to the client** — Not a tmux wrapper: Gateterm extracts the kernel of what tmux does (process + IO + screen-state hosting + byte-accurate `seq` ledger) and exposes it over a client-neutral binary WebSocket protocol.
- **Native GPU Desktop Workbench (`clients/desktop-client`)** — Inspired by [`tty7`](https://github.com/l0ng-ai/tty7) and built with Rust, `gpui-component`, and `alacritty_terminal`: integrated virtual `TitleBar`, embedded local server out of the box, multi-server remote management, N-ary equal split panes, and in-terminal scrollback search (<kbd>Ctrl+F</kbd>).
- **Agent-aware** — First-class integration with Claude Code, Codex, Gemini CLI, OpenCode, pi, Grok, and Antigravity CLI: live status badges, remote tool permission approval, read-only Structured Chat View, and past session browsing & resume.
- **Take over from any device** — Start an agent in the GPU desktop client or browser on your workstation, then check its progress or approve a tool call from the native Android client or Gateterm Cloud notifications.

## What's inside

| Area | Capabilities |
| :--- | :--- |
| **GPU Desktop Workbench (`clients/desktop-client`)** | Built on Zed's `gpui` + `alacritty_terminal` · Integrated virtual `TitleBar` with icon toolbar · Embedded `gateterm-server` started automatically · Multi-server connections · N-ary equal horizontal/vertical splits · <kbd>Ctrl+F</kbd> in-terminal scrollback search · 5 built-in color palettes · IME & Windows ConPTY `win32-input-mode` |
| **Agent Awareness & Remote Approval** | Per-session live status dots (`Waiting` / `Working` / `Thinking` / `Error` / `Idle`) · Top approval banner when an agent blocks on a tool call (**Allow** / **Deny** / focus session) · One-click hook install, uninstall & purge |
| **Structured Chat View (`Chat View`)** | <kbd>Ctrl+Shift+M</kbd> switches any agent session between its raw VT screen and a structured message stream · Renders `Thinking` blocks, `ToolUse` inputs, and `ToolResult` outputs · Read-only, never cached, never written |
| **Agent Session History & Resume** | <kbd>Ctrl+Shift+H</kbd> browses host-side Claude Code / Codex / pi / Grok / Antigravity records (including sessions started outside Gateterm) · Filter by agent, search titles/directories, preview transcripts, and resume in one click |
| **Smart Launchers** | Clicking **+** opens a ready-to-launch menu instead of a prompt · Automatically discovers default shells, `config.toml` entries, macOS **iTerm2** profiles, and **Windows Terminal** `settings.json` (read-only) |
| **Web & Native Android Clients** | `clients/web` (bundled directly into the server binary) · `clients/android` (native Kotlin + Jetpack Compose client with QR-code server pairing, agent-optimized two-row shortcut bar, and background foreground-service keepalive) |
| **Gateterm Cloud (Optional)** | Pushes mobile notifications when an agent enters `Waiting` or completes a turn · **Zero terminal data transits the relay** (no PTY bytes and no server access tokens ever leave your machine) |

## Supported agents

None of the agents are wrapped or proxied — the agent you start is the official CLI running in a standard PTY. Installing its hook under **Settings → AI Agent Hooks** unlocks live status, remote tool approval, Structured Chat View, and past session resume:

<details open>
<summary>Supported AI Coding Agents Matrix</summary>

| Agent | Live Status | Remote Approval (`Allow`/`Deny`) | Structured Chat View | Past Session Resume | Integration Mechanism |
| :--- | :---: | :---: | :---: | :---: | :--- |
| **Claude Code** | ✓ | ✓ | ✓ | ✓ | Automatic hooks in `~/.claude/settings.json` |
| **Codex** | ✓ | ✓ | ✓ | ✓ | Automatic hooks (review once with `/hooks` inside Codex) |
| **Gemini CLI** | ✓ | ✓ | ✓ | ✓ | Automatic hooks in `~/.gemini/settings.json` |
| **OpenCode** | ✓ | ✓ | ✓ | ✓ | Generated OpenCode plugin hooks |
| **pi** | ✓ | — | ✓ | ✓ | Generated extension at `~/.pi/agent/extensions/gateterm.ts` |
| **Grok (`grok`)** | ✓ | — | ✓ | ✓ | Hook config at `~/.grok/hooks/gateterm.json` |
| **Antigravity (`agy`)** | ✓ | — | ✓ | ✓ | `gateterm` entry in `~/.gemini/config/hooks.json` (includes thinking blocks) |

</details>

## Philosophy

A memory-frugal Swiss Army knife — and it stays that way:

- **Small**: single static binary, one process, minimal resident footprint.
- **Restrained**: the minimum feature set that meets the need, done well.
- **Not all-in-one**: window management belongs to clients; anything a hosted shell can already do (editors, file tools) will not be rebuilt into the server.

## Quick start

### Option 1: Native GPU Desktop Workbench (`clients/desktop-client`)

`clients/desktop-client` automatically starts an embedded local `gateterm-server` in-process and opens an interactive terminal tab out of the box, while also letting you connect to any number of remote `gateterm-server` hosts:

```sh
cd clients/desktop-client
# Set GATETERM_SKIP_WEB_BUILD=1 to skip rebuilding clients/web if only running the native GUI
GATETERM_SKIP_WEB_BUILD=1 cargo run --release
```

See [DESIGN.md](DESIGN.md#native-gpu-desktop-workbench) for keyboard shortcuts and [`clients/desktop-client/README.md`](clients/desktop-client/README.md) for client-specific notes.

### Option 2: Standalone Daemon (`gateterm-server`)

```sh
# Requires Node/npm: the server's build.rs builds clients/web automatically
# (set GATETERM_SKIP_WEB_BUILD=1 to skip it)
cargo run -p gateterm-server
# The startup log prints a ready-to-open URL (Jupyter-style; a fresh random
# token is generated on every start):
#     http://127.0.0.1:7683/#token=<random token>
```

See [DESIGN.md](DESIGN.md#standalone-server) for authentication, configuration, launchers, network access and update behavior.

## Tray Server Host (`gateterm-desktop`, Windows / macOS)

Run the tray-resident server host on Windows or macOS, and open terminals in the browser or mobile app:

```sh
cargo run -p gateterm-desktop
```

See [DESIGN.md](DESIGN.md#tray-server-host) for settings, QR pairing, session lifecycle, chat access and hook management.

## Android client (`clients/android`)

A native Kotlin + Jetpack Compose client for connecting to Gateterm servers. See [DESIGN.md](DESIGN.md#android-client) for connection, keyboard and terminal behavior.

Build with JDK 17 and Android SDK 36, or open `clients/android` in Android Studio:

```sh
cd clients/android
./gradlew :app:assembleDebug :app:testDebugUnitTest
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

Use `gradlew.bat` on Windows. After installation, add the server address and token, or import the server access URL from the clipboard.

## Agent session history

See [DESIGN.md](DESIGN.md#agent-session-history) for browsing, resuming and indexing agent sessions.

## Gateterm Cloud (optional)

Sign in to Gateterm Cloud on your server and Android client to receive agent questions, permission requests, and completion notifications on your phone.

## Architecture

Wire protocol: [`docs/protocol.md`](docs/protocol.md) — the normative spec, and the checklist any protocol change has to pass. Background and rationale: `docs/superpowers/specs/2026-08-16-gateterm-design.md`.

- `crates/protocol` — frame and message definitions (single source of truth)
- `crates/server` — the daemon: pty hosting, server-side grid, WebSocket
- `crates/client-core` — reusable logic for Rust clients (`ClientTerm`, `ResumeTracker`, `Backoff`)
- `crates/desktop` — tray-resident server host with native settings window (Windows / macOS)
- `clients/desktop-client` — native GPU-accelerated desktop terminal workbench (Rust + `gpui` + `gpui-component` + `alacritty_terminal`)
- `clients/web` — browser client (TypeScript + `xterm.js`)
- `clients/android` — native Android terminal client (Kotlin + Jetpack Compose)

## Roadmap

1. **More terminal capability, more clients.** Deepen core terminal features across the GPU Desktop client (`clients/desktop-client`), Web, and Android, and bring the same protocol to iOS and other platforms.
2. **Agent integration.** Extend live status, remote approval, chat and session resume across clients and supported agents.
3. **No file manager.** File management is very unlikely to ever be added — that's what the shell in your session is for.

## Known limitations

See [DESIGN.md](DESIGN.md#known-limitations) for terminal resync limitations.

---

<div align="center">
<sub>

Built on Rust, [`gpui`](https://github.com/zed-industries/zed), and [`alacritty_terminal`](https://github.com/alacritty/alacritty) · [MIT License](LICENSE.md)

</sub>
</div>
