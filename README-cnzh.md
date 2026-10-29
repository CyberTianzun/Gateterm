<div align="center">

# Gateterm

**跨平台终端与 AI Agent 工作台：会话常驻、多端无损续接、原生支持 Coding Agent。**

<sub>纯 Rust 服务端与协议核心 · GPU 原生桌面端基于 Zed 的 `gpui` + `Alacritty` · Web 端 (`xterm.js`) · Android 原生客户端</sub>

<br />

[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-orange)](#)
[![Desktop](https://img.shields.io/badge/Desktop-GPUI%20%2B%20Alacritty-3FDD8C)](clients/gateterm-desktop-web-native)
[![Clients](https://img.shields.io/badge/Clients-Desktop%20%C2%B7%20Web%20%C2%B7%20Android-blue)](#)
[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE.md)

<sub>[English](README.md) · 简体中文</sub>

<br />

![Gateterm Banner](docs/assets/banner.png)

</div>

---

## 为什么做这个

**shell、PTY 进程与屏幕状态由后台常驻的 `gateterm-server` 持有，而不是某一个窗口或网页标签。** 下面的所有能力都自然生长在这一条架构边界之上：

- **极致轻量与常驻** — 单一静态二进制守护进程托管 PTY 会话；关掉桌面窗口、刷新浏览器或切走手机 App，后台任务与 Agent 照常运行。
- **把窗口管理还给客户端** — 它不是 tmux 的包装，而是把 tmux 的内核（进程 + IO + 屏幕状态托管 + 精确字节序号 `seq` 账本）独立出来，通过统一的二进制 WebSocket 协议暴露给任意客户端。
- **原生 GPU 桌面工作台 (`clients/gateterm-desktop-web-native`)** — 参考 [`tty7`](https://github.com/l0ng-ai/tty7) 架构，使用 Rust + `gpui-component` + `alacritty_terminal` 构建：沉浸式 GPUI 虚拟标题栏、内置本地服务端自动拉起、多远程节点管理、N 叉等分分屏、终端内回滚搜索（<kbd>Ctrl+F</kbd>）。
- **AI Coding Agent 深度感知** — 原生感知 Claude Code、Codex、Gemini CLI、OpenCode、pi、Grok 与 Antigravity CLI：实时状态徽章、远程工具调用权限审批、只读结构化对话视图（Chat View）以及宿主机历史会话检索与一键恢复。
- **全平台随时接管** — 在电脑上用原生 GPU 桌面端或浏览器开跑任务，出门用 Android 原生客户端或 Gateterm Cloud 推送随时查看状态、审批工具调用。

## 有什么 (`What's inside`)

| 模块 | 核心能力 |
| :--- | :--- |
| **原生 GPU 桌面端 (`clients/gateterm-desktop-web-native`)** | 基于 Zed `gpui` + `alacritty_terminal` · 虚拟标题栏与纯图标工具栏 · 内置本地 `gateterm-server` 开箱即用 · 多远程服务器并行连接 · 水平/垂直 N 叉等分分屏 · <kbd>Ctrl+F</kbd> 终端内回滚搜索 · 5 套主题调色板 · IME 输入法与 ConPTY `win32-input-mode` 精确按键 |
| **Agent 感知与远程审批** | 逐会话实时状态点 (`Waiting` / `Working` / `Thinking` / `Error` / `Idle`) · 工具调用阻塞时顶部弹出审批横幅（一键 **允许** / **拒绝** / 跳转会话） · 在设置内一键安装/卸载/清理各 Agent 钩子 |
| **结构化对话视图 (`Chat View`)** | <kbd>Ctrl+Shift+M</kbd> 将正在运行 Agent 的终端切换为结构化消息流 · 独立渲染 `Thinking` 思考过程、`ToolUse` 工具参数与 `ToolResult` 结果 · 只读解析、不缓存、不落盘 |
| **Agent 历史会话与恢复** | <kbd>Ctrl+Shift+H</kbd> 检索宿主机已有的 Claude Code / Codex / pi / Grok / Antigravity 记录（含在 Gateterm 之外启动的对话） · 关键词与工作目录搜索 · 预览最近对话 · 一键在原目录恢复会话 |
| **智能启动菜单 (`Launchers`)** | 点击 **+** 直接列出可用启动项，无需手敲命令 · 自动只读扫描系统默认 Shell、`config.toml` 自定义项、macOS **iTerm2** Profile 与 **Windows Terminal** `settings.json` |
| **Web 与 Android 原生客户端** | `clients/web`（零配置内嵌于服务端二进制） · `clients/android`（Kotlin + Jetpack Compose 原生客户端，支持扫码添加服务器、针对 Agent 自动优化的双行快捷键栏与后台保活服务） |
| **Gateterm Cloud（可选）** | 当 Agent 进入「等待人工确认」或任务完成时向手机发送实时通知 · **零终端数据经过中继**（不转发 PTY 字节流、不接触本机访问 Token） |

## 支持的 Agent

Gateterm **不包装、不代理任何 Agent**——你在终端里启动的就是原汁原味的官方 CLI，运行在标准 PTY 中。通过「设置 → AI Agent 钩子」一键安装对应配置后，即可解锁实时状态、远程工具审批、结构化聊天视图与历史会话恢复：

<details open>
<summary>已支持的 AI Coding Agent 能力矩阵</summary>

| Agent | 实时状态感知 | 远程工具审批 (`Allow`/`Deny`) | 结构化对话视图 (`Chat View`) | 历史会话检索与恢复 (`Resume`) | 集成方式说明 |
| :--- | :---: | :---: | :---: | :---: | :--- |
| **Claude Code** | ✓ | ✓ | ✓ | ✓ | 自动配置 `~/.claude/settings.json` Hooks |
| **Codex** | ✓ | ✓ | ✓ | ✓ | 自动配置 Hooks（首次需在 Codex 中运行 `/hooks` 审核） |
| **Gemini CLI** | ✓ | ✓ | ✓ | ✓ | 自动配置 `~/.gemini/settings.json` Hooks |
| **OpenCode** | ✓ | ✓ | ✓ | ✓ | 自动生成 OpenCode 插件钩子 |
| **pi** | ✓ | — | ✓ | ✓ | 自动生成 `~/.pi/agent/extensions/gateterm.ts`（使用原会话文件恢复） |
| **Grok (`grok`)** | ✓ | — | ✓ | ✓ | 自动配置 `~/.grok/hooks/gateterm.json` |
| **Antigravity (`agy`)** | ✓ | — | ✓ | ✓ | 写入 `~/.gemini/config/hooks.json` 的 `gateterm` 条目（含思考块解析） |

</details>

## 哲学

一把省内存的瑞士军刀，并且坚持只做一把瑞士军刀：

- **小巧**：服务端单一静态二进制、单进程、常驻内存占用极小；
- **克制**：用最小功能集满足需求，把它做好；
- **不做 all-in-one**：窗口管理属于客户端；被托管的 shell 本身能干的事（编辑器、文件工具），不会重复造进 server。

## 快速开始

### 方式一：启动原生 GPU 桌面工作台 (`clients/gateterm-desktop-web-native`)

`clients/gateterm-desktop-web-native` 启动时默认自动在进程内拉起内置本地 `gateterm-server` 并打开第一个终端会话，同时支持连接任意远程 `gateterm-server` 节点：

```sh
cd clients/gateterm-desktop-web-native
# 若无需重新构建 web 前端静态资源，可设置 GATETERM_SKIP_WEB_BUILD=1 加速编译
GATETERM_SKIP_WEB_BUILD=1 cargo run --release
```

常用快捷键：
- <kbd>Ctrl+Shift+N</kbd> / <kbd>Cmd+N</kbd>：打开分组过滤的 **新建会话 (New Session)** 弹出菜单
- <kbd>Ctrl+Shift+T</kbd> / <kbd>Cmd+T</kbd>：新建默认终端会话
- <kbd>Ctrl+B</kbd> / <kbd>Cmd+B</kbd>：折叠 / 展开左侧边栏
- <kbd>Ctrl+F</kbd> / <kbd>Cmd+F</kbd>：打开终端内回滚搜索栏
- <kbd>Ctrl+Shift+D</kbd> / <kbd>Ctrl+Shift+E</kbd>：左右水平等分分屏 / 上下垂直等分分屏
- <kbd>Ctrl+Shift+Z</kbd>：最大化 / 还原当前活动窗格
- <kbd>Ctrl+Shift+M</kbd>：切换「VT 终端视图」与「Agent 结构化对话视图」
- <kbd>Ctrl+Shift+H</kbd>：浏览与恢复宿主机 Agent 历史会话

完整说明见 [`clients/gateterm-desktop-web-native/README-cnzh.md`](clients/gateterm-desktop-web-native/README-cnzh.md)（英文版：[`clients/gateterm-desktop-web-native/README.md`](clients/gateterm-desktop-web-native/README.md)）。

### 方式二：运行独立守护进程 (`gateterm-server`)

```sh
# 需要 Node/npm：server 的 build.rs 会自动构建 clients/web
# (设置 GATETERM_SKIP_WEB_BUILD=1 可跳过)
cargo run -p gateterm-server
# 启动日志会打印完整访问 URL(Jupyter 式,token 每次启动随机生成),直接点开即可:
#     http://127.0.0.1:7683/#token=<本次随机 token>
```

要固定 token（重启后旧页面免重新登录），用 `--token <值>`，或在配置文件里写 `token = "<值>"`。配置文件不会由 server 自动生成；需要时自行创建 `~/Library/Application Support/gateterm/config.toml`（Linux 为 `~/.config/gateterm/config.toml`），或用 `--config <路径>` 指定，字段均可省略：

```toml
listen = "127.0.0.1:7683"
token = "my-fixed-token"
# 多端同时 attach 同一个会话时,pty 只有一个尺寸,各端的诉求需要归并:
# "smallest"(默认,谁都不会看到被截断的画面)、"largest"、"latest"(跟随最近一端)
window_size = "smallest"

# 给「新建会话」菜单加几条自定义项。可选 —— 菜单本来就有默认 shell,
# 以及从 iTerm2 / Windows Terminal 里扫出来的 profile。
[[launcher]]
name = "prod ssh"
command = ["ssh", "prod01"]   # 直接是 argv,不是命令行字符串:不用猜切分规则
cwd = "~/work"                # 可选
```

### 新建会话菜单

点 **+** 弹出的是一个菜单，不是输入框。菜单内容由服务端的 provider 提供，常见情况下什么都不用配：

| provider | 读哪里 |
| --- | --- |
| 内置 | `$SHELL`(unix)/ `powershell.exe`、`%ComSpec%`(Windows) |
| 自定义 | 上面配置文件里的 `[[launcher]]` |
| iTerm2 | `com.googlecode.iterm2.plist` + `DynamicProfiles/`(macOS) |
| Windows Terminal | `settings.json`(商店版 / Preview / 便携版都找) |

第三方配置**只读不写**。只收「跑什么、在哪跑」跟默认不同的 profile —— 只改了配色的那些在这个菜单里是噪音。某个 provider 失败（没装、配置损坏、读不了）只是少几条并留一行日志，菜单照常能用。

想接一个新来源，在 `crates/server/src/launcher/` 里实现一个 `LauncherProvider` 即可。

默认只监听 127.0.0.1。要在其他设备访问，用命令行参数覆盖（优先于配置文件）：

```sh
cargo run -p gateterm-server -- --host 0.0.0.0 --port 9000
```

使用 `--host 0.0.0.0` 时，启动日志会列出回环地址及所有已连接网卡的 IPv4 访问链接（含 token），包括局域网和 VPN 地址。对外监听请自行保证网络层安全（Tailscale / 反向代理 + TLS）。

server 启动时及每隔 24 小时会在后台检查 GitHub 最新正式 Release。发现更高版本时，在控制台打印新版本号和 Release 页面链接；断网或检查失败不影响服务启动。

## 托盘服务端宿主 (`gateterm-desktop-web`，Windows / macOS)

除了完整渲染终端的 `clients/gateterm-desktop-web-native` 外，仓库还提供极简托盘宿主 `gateterm-desktop-web`：把同一套 server 内嵌进一个托盘常驻进程，配一个小巧的原生设置窗口（终端在浏览器或移动端打开）。

```sh
cargo run -p gateterm-desktop-web
```

桌面界面支持简体中文和 English。首次启动跟随操作系统显示语言：`zh` 语言使用中文，其他语言回退英文。可在「设置 → 界面语言」中选择语言或恢复「跟随系统」，保存后窗口和托盘立即切换。偏好保存在 `config.toml` 的 `[desktop].language`，可选值为 `auto`、`en`、`zh-CN`。

desktop 启动后及每隔 24 小时也会在后台检查最新正式 Release。启动时发现更新会弹出系统对话框，可选择「立刻更新」用默认浏览器打开 Release 页面，或「取消」继续使用。程序不会自动下载或安装更新。有更新时，Web 侧栏连接状态下方会显示 GitHub Release 链接；没有更新则不显示。

在桌面设置的「Android 扫码连接」中选择手机可达的 IP，Android 的「添加服务器 → 扫码添加」即可填入地址和 token，再确认保存。

在设置窗口里改监听地址或 token，只会重建 HTTP 层：**运行中的会话不受影响**。但已经打开的页面不会跟着迁移——它的 WebSocket 根本没断，会继续在旧地址上工作，直到你自己关掉那个页面；只有新连接才会走新地址。退出程序会终止所有会话，所以有会话在跑时会先弹确认框。

**聊天视图与它的暴露面**：托管会话里跑着 Claude Code 等 Agent 时，可以用聊天视图打开它 —— 结构化的消息流，而不是一块 VT 画面。它读的是 agent 自己写的对话记录，**只读、不缓存、不落盘**，并且由 `agents.transcript_enabled` 单独门控（默认开，可在 `[agents]` 里设为 `false` 关掉）。

之所以和装 hook 分成两个开关：装 hook 让 gateterm 看到「它在干什么」，这个开关让它看到「它说了什么」—— 后者是代码、路径、命令、模型的思考过程，暴露面高一个数量级，不该靠前者顺带同意。**尤其要和下面这条一起看**：desktop 默认监听全网卡，而这两个开关默认都开着，等于局域网里拿到 token 的人能读到你的整段对话 —— 在不信任的网络上请关掉它，或把监听改回 127.0.0.1。

**hook 自检与清理**：每次启动（desktop 每次改端口重启服务时也一样）都会检查已装的 hook 是否和当前服务一致（端口、事件、程序路径），对不上就在控制台 / 日志里打警告 —— 只报不改，修复仍在「设置 → AI 集成」里点「重新安装」。异常残留可以在同一页点「清空」，或运行 `gateterm-server purge-hooks`：删掉 agent 配置里**所有** gateterm 写过的 hook（不论端口、不论事件），你自己的 hook 不受影响，改前备份。

**默认监听 `0.0.0.0:7683`（全网卡），与 `gateterm-server` 不同。** desktop 的存在理由就是「从手机/平板连回自己这台机器」，只回环等于开箱即不可用。敢这么定的前提是两条硬边界都在：空 token 进不到监听层（release 构建也拦），`/hook/*` 无条件只认回环。剩下的残余风险是实打实的 —— 同一局域网里拿到 token 的人就能用你的终端，所以在不可信网络里请到设置窗口改回 `127.0.0.1`，或自己套一层网络层安全（Tailscale / 反代 + TLS）。

`gateterm-desktop-web` 托盘宿主不支持 Linux（Linux 桌面端请使用 `clients/gateterm-desktop-web-native`）。

## Android 客户端 (`clients/android`)

服务器连接由应用统一管理：从终端返回会话列表、再返回服务器列表，都保留服务器的 WebSocket。离开终端、切换到聊天或进入后台时会 detach 终端视口，让其他客户端恢复自己的尺寸；重新打开终端时自动 attach。服务器列表显示各连接的状态，并提供重连和手动断开入口。默认开启后台连接，通过前台服务及系统常驻通知维持；通知中可全部断开。设置中可改为仅前台连接。Cloud 推送独立保留，用于服务器不可达或 Android 终止应用进程时的补充。

`clients/android` 是 Kotlin + Jetpack Compose 原生客户端，支持连接管理、服务端启动项、多个会话、回滚与文字选择。界面和终端默认随系统切换 `2026 Light` / `2026 Dark`；软键盘弹出时缩小终端视口，快捷键栏默认双行、可左右滑动。首页右上角的设置统一管理语言、外观和快捷键行数，可切换单行 / 双行。按钮为 35dp 方形。服务端识别出当前终端中的 Agent 后，快捷键自动切换为 Agent 布局，将 `P↑`、`P↓` 和多行输入键放在最前面。通过「设置 → Agent 快捷键布局 → 配置」可按 Agent 分别选择 `⌃J`、`⇧↵` 或两者，调整其他优先键的顺序，也可为某个 Agent 使用普通终端布局。

连接 Windows 服务端时，键盘适配层保留 Shift+Enter、Ctrl+J、Alt+方向键等组合键的身份与修饰状态。快捷栏包含 Esc、Ctrl+C、方向键、Home、End、PgUp/PgDn 等。

Android 终端字号使用与 Web 一致的逻辑像素，不跟随系统字体大小；新配置默认字号为 14。设置中可调整行高和字距，导入、导出 Web 外观 JSON 时会保留这两项。字符格尺寸按 Web 默认的 xterm WebGL 渲染器对齐。

使用 JDK 17 和 Android SDK 36 构建，或用 Android Studio 打开 `clients/android`：

```sh
cd clients/android
./gradlew :app:assembleDebug :app:testDebugUnitTest
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

Windows 使用 `gradlew.bat`。安装后添加服务端地址和 token，也可以从剪贴板导入服务端访问 URL。

## Agent 会话历史

桌面工作台（`clients/gateterm-desktop-web-native`）、Web 侧栏的历史按钮、Android 服务端页面的「Agent 会话历史」可浏览宿主机已有的 Claude Code、Codex、pi、Grok 和 Antigravity CLI 记录，包括在 Gateterm 之外开始的对话。支持按 Agent、标题、原工作目录和会话 ID 搜索，并预览最近的对话。

已关联到运行中 Gateterm 终端的记录可直接进入终端；其他记录在原工作目录中启动一个新的 Gateterm 终端并恢复。需要服务端 PATH 中有对应 CLI、原目录仍存在且记录包含恢复所需信息。pi 使用原会话文件恢复。恢复请求在服务端合并，多端同时点击也不会重复启动。

历史读取沿用 `[agents].transcript_enabled` 和服务端 bearer token。服务端启动时异步构建最多 1000 个去重会话的内存索引，客户端默认每页加载 50 条元数据，列表末尾提示上限。对话正文只在预览时读取。刷新在后台重建索引，不阻塞分页请求；达到扫描上限或遇到坏记录时界面会提示。当前读取各 Agent 的默认用户目录，不扫描 WSL、其他用户目录或自定义存储位置。

## Gateterm Cloud（可选）

登录 Cloud 之后，agent 进入「等你」状态时会通过 Cloud 推送到手机（Android 构建需要配置推送）。不登录不影响任何功能。

- Cloud 不转发终端数据，拿不到本机的访问 token；推送只包含会话编号、agent 名称与工具名。
- 登录态保存在配置目录的 `cloud.json`，在 Unix 上权限仅属主可读写（0600）。
- 同一台机器上的 desktop 应用和独立运行的 gateterm-server 共用这份 cloud.json，其中一边登录或退出，另一边也会同步生效。

```bash
gateterm-server cloud register        # 注册并登录(邮件验证码)
gateterm-server cloud login
gateterm-server cloud status
gateterm-server cloud logout
gateterm-server cloud reset-password
gateterm-server cloud delete-account  # 永久删除账号及全部数据
```

Cloud 地址可写进 `config.toml`：

```toml
[cloud]
url = "https://cloud.example.com"
```

Desktop 用户在「设置…」窗口底部的「Cloud 账号」区选择中继地区，点击「通过浏览器登录」。注册、登录和找回密码在浏览器中完成，成功后自动回调本机并保存登录状态，无需填写 Cloud 地址。

| 中继地区 | 地址 |
|---|---|
| 中国大陆地区（默认） | `https://gateterm.signal.top/` |
| 北美地区（暂未部署） | `https://gateterm.eclosion.ai/` |
| 其它全球（暂未部署） | `https://global.gateterm.eclosion.ai/` |

网页登录需要 Cloud 部署包含 `/auth/desktop` 与 `/v1/auth/desktop/*` 的版本。Android 同样使用三个中继地区预设，并在浏览器中完成登录、注册和找回密码。完成后自动返回 App，保存设备凭据并登记推送；需要 Cloud 部署 `/auth/mobile` 和 `/v1/auth/mobile/*` 接口。登录后账号图标着色并在右下角显示小勾，未登录时显示小叉。账号页显示推送登记结果并提供重试；Cloud 通过部署参数选择阿里云或 FCM，Android 从 `/v1/push-config` 自动读取路线并登记对应设备。阿里云客户端配置放在 `clients/android/app/aliyun-emas-services.json`（不入库），其中的 App 配置与仅供服务端使用的阿里云 AccessKey 分开。阿里云路线不依赖 Google Play 服务，FCM 需要可用的 Google Play 服务和能访问 Google 的网络；两者都需要通知权限。阿里云自有通道要求应用进程可运行，进程被系统杀掉后及时展示还需要额外接入并配置厂商推送通道。更新后的 Desktop/server 还会在 Stop hook 完成本轮任务时发送完成提醒。登录等待最多 10 分钟，可在客户端取消；本机回调只监听 `127.0.0.1` 的临时端口。

## 架构

通信协议规范：[`docs/protocol.md`](docs/protocol.md) —— 线上格式的规范性定义，以及任何协议改动都要过的评审清单。设计背景与取舍：`docs/superpowers/specs/2026-08-16-gateterm-design.md`。

- `crates/protocol` — 帧与消息定义（协议唯一事实来源）
- `crates/server` — daemon：pty、服务端 grid、WebSocket
- `crates/client-core` — Rust 客户端复用逻辑（`ClientTerm`、`ResumeTracker`、`Backoff`）
- `clients/gateterm-desktop-web-web` — 内嵌 server 的托盘常驻宿主（Windows / macOS）
- `clients/gateterm-desktop-web-native` — 原生 GPU 加速桌面终端工作台（Rust + `gpui` + `gpui-component` + `alacritty_terminal`）
- `clients/web` — 浏览器客户端（TS + `xterm.js`）
- `clients/android` — Android 原生终端客户端（Kotlin + Jetpack Compose）

## 路线图

1. **更多终端能力、更多客户端**：持续完善 GPU 桌面端（`clients/gateterm-desktop-web-native`）、Web 与 Android 客户端，并把同一套协议带到 iOS 等平台；
2. **Agent 集成**：让被托管的会话可以在任何设备上接管 AI 的提示、回答它的选择、查看它的状态。**Claude Code 已可用**（装 hook、会话状态、远程授权、聊天视图）；**Codex** 也已支持（状态、授权、聊天视图；hook 要在 Codex 里用 `/hooks` 审核一次）；**pi** 通过生成的扩展 `~/.pi/agent/extensions/gateterm.ts` 支持（状态与聊天视图；pi 本身没有授权审批，所以没有远程授权可做）；**Grok**（xAI 的 `grok`）通过 `~/.grok/hooks/gateterm.json` 支持（状态与聊天视图；它同样没有授权审批事件）；**Antigravity CLI**（Google 的 `agy`）通过共用的 `~/.gemini/config/hooks.json` 里的 `gateterm` 条目支持（状态与聊天视图；不接管它的授权确认）。该适配针对独立的 Antigravity CLI，与 Gemini CLI 和 Antigravity IDE 分开处理。从旧 Windows 版本升级后，需要在设置中重新安装 Antigravity 集成，再重启正在运行的 `agy`，替换旧 hook 命令。聊天视图读取 Antigravity 的步骤日志，包含新版 CLI 的思考内容。
3. **不做文件管理**：文件管理基本不可能加——那是会话里 shell 自己的事。

## 已知限制

resync 恢复内容、光标与常用模式（应用光标键/括号粘贴/鼠标上报），不恢复 alt-screen 与滚动区域（DECSTBM）；弱网重连后全屏应用建议 Ctrl-L 或触发重绘。

<details>
<summary>应用包名与发布配置说明（维护者参考）</summary>

当前 Android 应用标识为 `ai.eclosion.gateterm.android`，桌面端和服务端配置位于各平台的 `gateterm` 配置目录。这些标识对应独立的应用数据。切换前请导出外观设置，并复制需要保留的服务端配置、Cloud 登录凭据和 hook token，随后重新安装 agent hook。

默认 Cloud 地址为 `https://gateterm.signal.top/`。该域名需要有效 HTTPS 证书，Cloud 服务的手机登录回调白名单需要支持 `ai.eclosion.gateterm.android://cloud/auth/callback`。Firebase/EMAS 配置必须登记上述新包名；包名不匹配的可选配置会在构建时提示并跳过。Android 签名配置使用仓库同级的 `GatetermSecrets/gateterm-release.signing.properties`。发布及自动更新链接指向 `CyberTianzun/Gateterm`，发布前需要同步 GitHub 仓库名称。

</details>

---

<div align="center">
<sub>

基于 Rust、[`gpui`](https://github.com/zed-industries/zed) 与 [`alacritty_terminal`](https://github.com/alacritty/alacritty) 构建 · [MIT 许可证](LICENSE.md)

</sub>
</div>
