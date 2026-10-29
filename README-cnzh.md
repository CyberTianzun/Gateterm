<div align="center">

# Gateterm

**跨平台终端与 AI Agent 工作台：会话常驻、多端无损续接、原生支持 Coding Agent。**

<sub>纯 Rust 服务端与协议核心 · GPU 原生桌面端 (`clients/desktop-native`) · Web 端 (`clients/web`) · Android 原生客户端 (`clients/android`)</sub>

<br />

[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-orange)](#)
[![Desktop](https://img.shields.io/badge/Desktop-GPUI%20%2B%20Alacritty-3FDD8C)](clients/desktop-native)
[![Clients](https://img.shields.io/badge/Clients-Desktop%20%C2%B7%20Web%20%C2%B7%20Android-blue)](#)
[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE.md)

<sub>[English](README.md) · 简体中文</sub>

<br />

![Gateterm Banner](docs/assets/banner.png)

</div>

---

## 为什么做这个

**shell、PTY 进程与屏幕状态由后台常驻的 `gateterm-server` 持有，而不是某一个窗口或网页标签。**

- **极致轻量与常驻** — 单一静态二进制守护进程托管会话；关掉桌面窗口、刷新浏览器或切走手机 App，后台任务与 Agent 照常运行。
- **原生 GPU 桌面工作台 (`clients/desktop-native`)** — 基于 Rust、`gpui` 与 `alacritty_terminal` 构建：沉浸式虚拟标题栏、内置本地服务端自动拉起、多远程节点管理、N 叉等分分屏与终端内回滚搜索。
- **AI Coding Agent 深度感知** — 原生支持 Claude Code、Codex、Gemini CLI、OpenCode、pi、Grok 与 Antigravity CLI：实时状态图标、远程工具调用权限审批、结构化对话视图（Chat View）以及宿主机历史会话检索与一键恢复。
- **全平台随时接管** — 在电脑上用原生 GPU 桌面端或浏览器开跑任务，出门用 Android 原生客户端或 Gateterm Cloud 推送随时查看状态、审批工具调用。

## 有什么 (`What's inside`)

| 模块 | 核心能力 |
| :--- | :--- |
| **原生 GPU 桌面工作台 (`clients/desktop-native`)** | 原生 GPU 渲染 · 虚拟标题栏与纯图标工具栏 · 内置本地 `gateterm-server` 开箱即用 · 多远程服务器并行连接 · 水平/垂直等分分屏 · 终端内回滚搜索 · 5 套内置主题调色板 · 系统托盘常驻与扫码配对 |
| **托盘服务端宿主 (`clients/desktop-web`)** | 面向 Windows 与 macOS 的极简托盘常驻进程 · 原生设置窗口（支持扫码配对、AI 钩子管理与 Cloud 登录） · 终端在浏览器或移动端打开 |
| **Agent 感知与远程审批** | 逐会话品牌图标与实时状态点 (`Waiting` / `Working` / `Thinking` / `Error` / `Idle`) · 工具调用阻塞时顶部弹出审批横幅（一键 **允许** / **拒绝** / 跳转会话） · 设置内一键安装/卸载钩子 |
| **结构化对话视图 (`Chat View`)** | 随时将运行 Agent 的会话在「终端画面」与「结构化消息流」间切换 · 独立渲染 `Thinking` 思考过程、`ToolUse` 工具参数与 `ToolResult` 结果 |
| **Agent 历史会话与恢复** | 检索宿主机已有的 Claude Code、Codex、pi、Grok 与 Antigravity 记录（含在 Gateterm 之外启动的对话） · 按 Agent 与关键词搜索、预览对话并一键在原目录恢复 |
| **智能启动菜单 (`Launchers`)** | 点击 **+** 直接列出可用启动项 · 自动扫描系统默认 Shell、`config.toml` 自定义项、macOS **iTerm2** Profile 与 **Windows Terminal** 配置 |
| **Web 与 Android 原生客户端** | `clients/web`（零配置内嵌于服务端二进制） · `clients/android`（Kotlin + Jetpack Compose 原生客户端，支持扫码配对、Agent 双行快捷键栏与后台保活） |
| **Gateterm Cloud（可选）** | 当 Agent 进入等待确认或任务完成时向手机发送实时通知 · 零终端数据与访问 Token 经过中继 |

## 支持的 Agent

通过「设置 → AI Agent 钩子」一键安装对应配置后，即可解锁实时状态、远程工具审批、结构化聊天视图与历史会话恢复：

| Agent | 实时状态感知 | 远程工具审批 (`Allow`/`Deny`) | 结构化对话视图 (`Chat View`) | 历史会话检索与恢复 (`Resume`) |
| :--- | :---: | :---: | :---: | :---: |
| **Claude Code** | ✓ | ✓ | ✓ | ✓ |
| **Codex** | ✓ | ✓ | ✓ | ✓ |
| **Gemini CLI** | ✓ | ✓ | ✓ | ✓ |
| **OpenCode** | ✓ | ✓ | ✓ | ✓ |
| **pi** | ✓ | — | ✓ | ✓ |
| **Grok (`grok`)** | ✓ | — | ✓ | ✓ |
| **Antigravity (`agy`)** | ✓ | — | ✓ | ✓ |

## 哲学

一把省内存的瑞士军刀，并且坚持只做一把瑞士军刀：

- **小巧**：服务端单一静态二进制、单进程、常驻内存占用极小；
- **克制**：用最小功能集满足需求，把它做好；
- **不做 all-in-one**：窗口管理属于客户端；被托管的 shell 本身能干的事（编辑器、文件工具），不会重复造进 server。

## 快速开始

### 方式一：原生 GPU 桌面工作台 (`clients/desktop-native`)

`clients/desktop-native` 启动时默认自动在进程内拉起内置本地 `gateterm-server` 并打开第一个终端会话，同时支持连接任意远程 `gateterm-server` 节点：

```sh
cd clients/desktop-native
GATETERM_SKIP_WEB_BUILD=1 cargo run --release
```

### 方式二：托盘服务端宿主 (`clients/desktop-web`，Windows / macOS)

在 Windows 或 macOS 上运行极简托盘常驻进程，通过浏览器或手机端打开终端：

```sh
cargo run -p gateterm-desktop-web
```

### 方式三：独立守护进程 (`gateterm-server`)

```sh
cargo run -p gateterm-server
# 启动日志会打印完整访问 URL，直接在浏览器打开或粘贴到客户端即可：
#     http://127.0.0.1:7683/#token=<本次随机 token>
```

### Android 客户端 (`clients/android`)

使用 JDK 17 和 Android SDK 36 构建，或用 Android Studio 打开 `clients/android`：

```sh
cd clients/android
./gradlew :app:assembleDebug :app:testDebugUnitTest
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

Windows 使用 `gradlew.bat`。安装后可通过桌面端设置中的二维码扫码配对，或从剪贴板导入访问链接。

## 路线图

1. **更多终端能力、更多客户端**：持续完善 GPU 桌面端（`clients/desktop-native`）、Web 与 Android 客户端，并把同一套协议带到 iOS 等平台；
2. **Agent 集成**：持续扩展各客户端与受支持 Agent 的实时状态、远程审批、结构化聊天与会话恢复能力；
3. **不做文件管理**：文件管理基本不可能加——那是会话里 shell 自己的事。

---

<div align="center">
<sub>

基于 Rust、[`gpui`](https://github.com/zed-industries/zed) 与 [`alacritty_terminal`](https://github.com/alacritty/alacritty) 构建 · [MIT 许可证](LICENSE.md)

</sub>
</div>
