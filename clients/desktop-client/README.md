<div align="center">

### Gateterm Desktop (`clients/desktop-client`)

**A native GPU-accelerated terminal workbench: integrated virtual title bar, N-ary equal split panes, multi-server remote connections, and native AI Coding Agent awareness.**

<sub>Pure Rust · GPU rendering on Zed's `gpui` + `gpui-component` · VT core from `alacritty_terminal` · Powered by `gateterm-protocol`</sub>

<br />

[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-orange)](#)
[![Renderer](https://img.shields.io/badge/GPU-Zed%20gpui%20%2B%20Alacritty-3FDD8C)](#)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%C2%B7%20macOS%20%C2%B7%20Linux-blue)](#)
[![License](https://img.shields.io/badge/license-MIT-blue)](../../LICENSE.md)

<sub>English · [简体中文](README-cnzh.md)</sub>

</div>

---

## Why a Native Desktop Client

Gateterm's core boundary is that **a background `gateterm-server` owns your PTY processes and screen state — not a window or browser tab**. Building on that foundation, `clients/desktop-client` draws inspiration from [`l0ng-ai/tty7`](https://github.com/l0ng-ai/tty7) and `clients/web` to deliver a **zero-browser-overhead native GPU desktop workbench**:

- **Integrated GPUI Virtual Title Bar (`TitleBar`)** — Uses `gpui-component`'s `TitleBar::title_bar_options()` for a frameless/integrated title bar with no redundant horizontal tab strip. The left sidebar header keeps `gateterm`, Session History, and `+` (New Session), while the title bar houses the sidebar toggle, the active session breadcrumb (in a dedicated window-drag region), and a clean icon-only toolbar on the top right (in-terminal search, horizontal/vertical equal splits, multi-pane zoom, Structured Chat View toggle, and Settings).
- **GPU-Accelerated Terminal Rendering** — Custom `TerminalElement` built on Zed's `gpui` and `alacritty_terminal` batch-renders background quads, ligature-disabled monospace glyphs (with automatic CJK wide-character cell spanning), text selections, search highlights, and cursor shapes directly in the GPU scene.
- **N-ary Equal Split Panes (`PaneTree`)** — Splitting repeatedly along the same axis (e.g., clicking vertical or horizontal split twice) automatically flattens into sibling nodes and divides space into equal `1/N` shares (2 panes = 1/2 each, 3 panes = 1/3 each, 4 panes = 1/4 each), with zero per-pane header chrome when unsplit.
- **Provider-Grouped New Session Popup (`New Session Modal`)** — Matches `clients/web`: clicking `+` opens a filterable popover grouped by `provider` (`built-in`, `config`, `iTerm2`, `Windows Terminal`), displaying `name` and `detail · cwd` for one-click launch.
- **Out-of-the-Box Local Server + Multi-Server Remote Connections** — Automatically supervises an in-process `gateterm-server` on launch (with automatic port fallback), while letting you connect to any number of remote `gateterm-server` hosts (including pasting `http://host:port/#token=...` URLs directly).
- **Native AI Coding Agent Awareness** — Tracks live status for Claude Code, Codex, Gemini CLI, OpenCode, pi, Grok, and Antigravity CLI, surfaces a top permission approval banner when an agent blocks on a tool call, and switches between the raw VT terminal and a Structured Chat View in one click.

## What's Inside

| Module | Capabilities |
| :--- | :--- |
| **GPUI Virtual Title Bar (`TitleBar`)** | Integrated OS window controls · Dedicated window drag region (never blocks button clicks) · Top-right icon toolbar (Toggle Sidebar, In-Terminal Search, Split Right/Down, Zoom Split Pane, Toggle Chat View, Settings) |
| **N-ary Equal Split Panes (`PaneTree`)** | <kbd>Ctrl+Shift+D</kbd> split right equally · <kbd>Ctrl+Shift+E</kbd> split down equally · Consecutive splits along the same axis divide space into `1/N` equal shares · <kbd>Ctrl+Shift+Z</kbd> zoom/unzoom active pane · <kbd>Ctrl+Tab</kbd> cycle pane focus |
| **In-Terminal Scrollback Search (`Ctrl+F`)** | <kbd>Ctrl+F</kbd> opens a compact floating search bar in the top-right corner · Case-insensitive live search across scrollback history and viewport · Highlights all matches and the active match on the terminal canvas · <kbd>Enter</kbd> / <kbd>Shift+Enter</kbd> jumps between matches and auto-scrolls |
| **Grouped New Session Popup (`+`)** | Aligned with `clients/web` · Instant keyword filtering · Grouped by `Built-in` / `Custom` / `iTerm2` / `Windows Terminal` showing command details and working directories · Click to launch |
| **Live AI Agent Collaboration** | Per-session live status badges (`Waiting` / `Working` / `Thinking` / `Error` / `Idle`) in the sidebar · Top tool-permission approval banner (**Allow** / **Deny** / click badge to focus session) |
| **Structured Chat View (`Chat View`)** | <kbd>Ctrl+Shift+M</kbd> toggles between the VT terminal screen and a structured message stream · Renders `Thinking` blocks, `ToolUse` inputs, and `ToolResult` outputs · Bottom prompt input sends instructions directly to the terminal |
| **Agent Session History & Resume** | <kbd>Ctrl+Shift+H</kbd> browses host-side Claude Code / Codex / Gemini / pi / Grok session records · Filter by agent and search keywords · Preview full transcripts on the right · One-click resume in the original working directory |
| **GPU Terminal & Cross-Platform Input** | Full IME preedit overlay support · Accurate Windows ConPTY `win32-input-mode` key encoding (preserving `Shift+Enter`, `Ctrl+J`, and `Alt+Arrow` modifiers) · SGR mouse reporting passthrough and drag-to-select copy |
| **Themes, Fonts & Hook Management** | <kbd>Ctrl+,</kbd> includes 5 built-in color palettes (`2026 Dark` / `2026 Light` / `Tokyo Night` / `Catppuccin Mocha` / `Solarized Dark`, driving both UI chrome and 16-color ANSI palette) · Live font family, size, and line-height controls · One-click Agent Hook install/uninstall |

## Quick Start

```powershell
cd clients/desktop-client

# Skip rebuilding clients/web if web static assets are already built or not needed:
$env:GATETERM_SKIP_WEB_BUILD="1"

# Launch the native GPU desktop client
cargo run --release
```

## Keyboard Shortcuts

| Shortcut | Action |
| :--- | :--- |
| <kbd>Ctrl+Shift+N</kbd> / <kbd>Cmd+N</kbd> | Open the provider-grouped **New Session** launcher popup |
| <kbd>Ctrl+Shift+T</kbd> / <kbd>Cmd+T</kbd> | Create a new default shell session on the active server |
| <kbd>Ctrl+B</kbd> / <kbd>Cmd+B</kbd> | Collapse / expand the left sidebar |
| <kbd>Ctrl+F</kbd> / <kbd>Cmd+F</kbd> | Open / close the floating **In-Terminal Scrollback Search** bar |
| <kbd>Ctrl+Shift+D</kbd> / <kbd>Cmd+D</kbd> | Split active pane right equally (new terminal session) |
| <kbd>Ctrl+Shift+E</kbd> / <kbd>Cmd+Shift+D</kbd> | Split active pane down equally (new terminal session) |
| <kbd>Ctrl+Shift+Z</kbd> / <kbd>Cmd+Shift+Enter</kbd> | Zoom / unzoom the active split pane |
| <kbd>Ctrl+Tab</kbd> | Cycle focus across split panes in the active workspace |
| <kbd>Ctrl+Shift+M</kbd> | Toggle between **Terminal View** and **Structured Agent Chat View** |
| <kbd>Ctrl+Shift+H</kbd> | Open the **Agent Session History & Resume** modal |
| <kbd>Ctrl+,</kbd> / <kbd>Cmd+,</kbd> | Open the **Desktop Settings** modal |
| <kbd>Ctrl+Shift+W</kbd> | Close the active split pane or session view |
| <kbd>Ctrl+C</kbd> (with selection) / <kbd>Ctrl+Shift+C</kbd> | Copy selected terminal text |
| <kbd>Ctrl+V</kbd> / <kbd>Ctrl+Shift+V</kbd> / Right-click | Paste clipboard text (automatically respects Bracketed Paste mode) |

## Directory Structure

```text
clients/desktop-client/
├── Cargo.toml                   # Standalone Cargo workspace reusing crates/{protocol,client-core,server,cloud-client}
├── src/
│   ├── main.rs                  # GPUI entry point, virtual TitleBar configuration, keybindings & window initialization
│   ├── gpui_tokio.rs            # Bridge between GPUI and the Tokio async runtime
│   ├── config.rs                # Desktop config persistence (desktop-client.toml) & #token=... URL parser
│   ├── local_server.rs          # In-process gateterm-server lifecycle supervisor (LocalServerSupervisor)
│   ├── theme.rs                 # 5 terminal 16-color ANSI palettes synchronized with gpui-component themes
│   ├── i18n.rs                  # Bilingual English / Simplified Chinese localization
│   ├── client/
│   │   ├── ws.rs                # Multiplexed binary WebSocket client + exponential Backoff + ResumeTracker
│   │   ├── api.rs               # REST API client (Launchers / Agent Sessions / Pending / Chat / History / Hooks)
│   │   └── mod.rs               # Agent state priority merging & incremental structured chat window state
│   ├── terminal/
│   │   ├── element.rs           # Custom GPU terminal element (TerminalElement with search & selection highlights)
│   │   ├── input.rs             # xterm / modifyOtherKeys / ConPTY win32-input-mode keyboard encoder
│   │   └── view.rs              # TerminalView entity (selection, scrollback search, IME, mouse reporting & clipboard)
│   └── ui/
│       ├── app.rs               # GatetermWorkbench main view (virtual TitleBar, collapsible sidebar & floating search)
│       ├── pane.rs              # WorkspaceTab & N-ary equal split tree (PaneTree / PaneNode)
│       ├── new_session_modal.rs # Provider-grouped filterable New Session popover aligned with clients/web
│       ├── pending_banner.rs    # AI Agent tool permission approval banner
│       ├── chat_view.rs         # Structured AI Agent Chat View with Thinking/Tool blocks
│       ├── history_modal.rs     # Agent Session History search, transcript preview & one-click resume modal
│       ├── server_dialog.rs     # Add / edit remote server connection modal
│       └── settings_modal.rs    # Appearance, font, embedded server, AI Agent Hooks & Cloud status modal
```
