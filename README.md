<div align="center">

# Gateterm

**A cross-platform terminal & AI Agent workbench: persistent sessions, seamless resume, native agent awareness.**

<sub>Pure Rust server & protocol · GPU Desktop (`clients/desktop-native`) · Web (`clients/web`) · Native Android (`clients/android`)</sub>

<br />

[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-orange)](#)
[![Desktop](https://img.shields.io/badge/Desktop-GPUI%20%2B%20Alacritty-3FDD8C)](clients/desktop-native)
[![Clients](https://img.shields.io/badge/Clients-Desktop%20%C2%B7%20Web%20%C2%B7%20Android-blue)](#)
[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE.md)

<sub>English · [简体中文](README-cnzh.md)</sub>

<br />

![Gateterm Banner](docs/assets/banner.png)

</div>

---

## Why

**A background server owns your shells, PTY processes, and screen state — not a window or browser tab.**

- **Memory-frugal & persistent** — A tiny single-binary daemon hosts your sessions. Quit the desktop app, close the browser tab, or switch away on your phone — your shells and coding agents keep running.
- **Native GPU Desktop Workbench (`clients/desktop-native`)** — Built with Rust, `gpui`, and `alacritty_terminal`: integrated virtual title bar, embedded local server out of the box, multi-server remote connections, N-ary equal split panes, and in-terminal scrollback search.
- **Agent-aware** — First-class integration with Claude Code, Codex, Gemini CLI, OpenCode, pi, Grok, and Antigravity CLI: live status badges, remote tool permission approval, Structured Chat View, and past session browsing & resume.
- **Take over from any device** — Start an agent in the desktop workbench or browser on your workstation, then check its progress or approve a tool call from the native Android client or Gateterm Cloud notifications.

## Release builds & screenshots

Each [GitHub Release](../../releases) publishes four prebuilt packages so you can pick the deployment model that fits each machine:

| Release Package | Platforms | What It Includes | Best For |
| :--- | :--- | :--- | :--- |
| **`gateterm-desktop-native`** | Windows (`x86_64` `.zip`)<br/>macOS (`aarch64` `.app.tar.gz`) | Native GPU terminal workbench (`gpui` + `alacritty_terminal`) + embedded local `gateterm-server` + system tray residency | Primary desktop terminal on Windows & macOS: native GPU rendering, N-ary equal split panes, and managing both local and remote `gateterm-server` nodes in one window |
| **`gateterm-desktop-web`** | Windows (`x86_64` `.zip`)<br/>macOS (`aarch64` `.app.tar.gz`) | Lightweight system-tray server host + native settings window (LAN QR pairing, AI hook installer, Cloud sign-in) + embedded Web UI | Browser-first workflows or turning a Windows/macOS workstation into a background host for Web and Android clients |
| **`gateterm-server`** | Linux (`x86_64`, `aarch64`)<br/>Windows (`x86_64`)<br/>macOS (`aarch64`) | Headless single-binary daemon with the Web client (`clients/web`) compiled in | Linux desktops, remote servers, cloud devboxes, and NAS — access directly in any browser or connect from `gateterm-desktop-native` / `gateterm-android` |
| **`gateterm-android`** | Android (`arm64-v8a` `.apk`) | Native Jetpack Compose client with QR pairing, multi-server connections, and agent-adaptive shortcut bar | Monitoring coding agents on the go, approving tool calls (`Allow` / `Deny`), and resuming terminal sessions from your phone |

### 1. Native GPU Desktop Workbench (`gateterm-desktop-native`)

Starts an embedded local `gateterm-server` automatically on launch, renders terminals natively via GPU, and connects to any number of remote `gateterm-server` hosts from the left sidebar:

![gateterm-desktop-native workbench](docs/assets/desktop-native.png)

### 2. Tray Server Host (`gateterm-desktop-web`) & Embedded Web Client (`gateterm-server`)

`gateterm-desktop-web` sits quietly in the system tray with a native settings window for one-scan LAN/Tailscale QR pairing, AI Agent hook installation, and Gateterm Cloud notifications, while `gateterm-server` (and `gateterm-desktop-web`) serves the full Web terminal directly to any browser:

| Tray Server Host Settings (`gateterm-desktop-web`) | Embedded Web Client (`clients/web` / `gateterm-server`) |
| :---: | :---: |
| ![gateterm-desktop-web settings](docs/assets/desktop-web.png) | ![gateterm web client](docs/assets/web-client.png) |

## What's inside

| Area | Capabilities |
| :--- | :--- |
| **GPU Desktop Workbench (`clients/desktop-native`)** | Native GPU rendering · Integrated virtual title bar & toolbar · Embedded `gateterm-server` started automatically · Multi-server remote connections · Equal horizontal/vertical split panes · In-terminal scrollback search · 5 built-in color palettes · System tray residency & QR pairing |
| **Tray Server Host (`clients/desktop-web`)** | Lightweight tray-resident server process for Windows & macOS · Native settings window with QR pairing, AI hook installer, and Cloud sign-in · Open terminals in any browser or mobile client |
| **Agent Awareness & Remote Approval** | Per-session live brand icons & status badges (`Waiting` / `Working` / `Thinking` / `Error` / `Idle`) · Top approval banner when an agent blocks on a tool call (**Allow** / **Deny** / focus session) · One-click hook management |
| **Structured Chat View (`Chat View`)** | Switch any active agent session between its terminal screen and a structured message stream · Renders `Thinking` blocks, `ToolUse` inputs, and `ToolResult` outputs |
| **Agent Session History & Resume** | Browse host-side Claude Code, Codex, pi, Grok, and Antigravity records (including sessions started outside Gateterm) · Filter by agent, search titles/directories, preview transcripts, and resume in one click |
| **Smart Launchers** | Clicking **+** opens a ready-to-launch menu discovering default shells, custom `config.toml` entries, macOS **iTerm2** profiles, and **Windows Terminal** profiles |
| **Web & Native Android Clients** | `clients/web` (bundled directly into the server binary) · `clients/android` (Kotlin + Jetpack Compose client with QR server pairing, agent-adaptive shortcut bar, and background keepalive) |
| **Gateterm Cloud (Optional)** | Pushes mobile notifications when an agent enters `Waiting` or completes a turn · Zero terminal data or access tokens transit the relay |

## Supported agents

Installing an agent's hook under **Settings → AI Agent Hooks** unlocks live status, remote tool approval, Structured Chat View, and past session resume:

| Agent | Live Status | Remote Approval (`Allow`/`Deny`) | Structured Chat View | Past Session Resume |
| :--- | :---: | :---: | :---: | :---: |
| **Claude Code** | ✓ | ✓ | ✓ | ✓ |
| **Codex** | ✓ | ✓ | ✓ | ✓ |
| **Gemini CLI** | ✓ | ✓ | ✓ | ✓ |
| **OpenCode** | ✓ | ✓ | ✓ | ✓ |
| **pi** | ✓ | — | ✓ | ✓ |
| **Grok (`grok`)** | ✓ | — | ✓ | ✓ |
| **Antigravity (`agy`)** | ✓ | — | ✓ | ✓ |

## Philosophy

A memory-frugal Swiss Army knife — and it stays that way:

- **Small**: single static binary, one process, minimal resident footprint.
- **Restrained**: the minimum feature set that meets the need, done well.
- **Not all-in-one**: window management belongs to clients; anything a hosted shell can already do (editors, file tools) will not be rebuilt into the server.

## Quick start

### Option 1: Native GPU Desktop Workbench (`clients/desktop-native`)

`clients/desktop-native` automatically starts an embedded local `gateterm-server` in-process and opens an interactive terminal tab out of the box, while also letting you connect to remote `gateterm-server` hosts:

```sh
cd clients/desktop-native
GATETERM_SKIP_WEB_BUILD=1 cargo run --release
```

### Option 2: Tray Server Host (`clients/desktop-web`, Windows / macOS)

Run the lightweight tray-resident server host on Windows or macOS, and open terminals in the browser or mobile app:

```sh
cargo run -p gateterm-desktop-web
```

### Option 3: Standalone Daemon (`gateterm-server`)

```sh
cargo run -p gateterm-server
# Prints a ready-to-open URL on startup:
#     http://127.0.0.1:7683/#token=<random token>
```

### Android client (`clients/android`)

Build with JDK 17 and Android SDK 36, or open `clients/android` in Android Studio:

```sh
cd clients/android
./gradlew :app:assembleDebug :app:testDebugUnitTest
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

Use `gradlew.bat` on Windows. Scan the QR code from Desktop Settings or paste the server access URL to connect.

## Roadmap

1. **More terminal capability, more clients.** Deepen core terminal features across the GPU Desktop client (`clients/desktop-native`), Web, and Android, and bring the same protocol to iOS and other platforms.
2. **Agent integration.** Extend live status, remote approval, chat, and session resume across clients and supported agents.
3. **No file manager.** File management is very unlikely to ever be added — that's what the shell in your session is for.

---

<div align="center">
<sub>

Built on Rust, [`gpui`](https://github.com/zed-industries/zed), and [`alacritty_terminal`](https://github.com/alacritty/alacritty) · [MIT License](LICENSE.md)

</sub>
</div>
