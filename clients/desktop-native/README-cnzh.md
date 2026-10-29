<div align="center">

### Gateterm Desktop (`clients/desktop-native`)

**原生 GPU 加速终端工作台：虚拟标题栏、N 叉等分分屏、多节点远程连接、原生支持 AI Coding Agent。**

<sub>纯 Rust · GPU 渲染基于 Zed 的 `gpui` + `gpui-component` · VT 内核来自 `alacritty_terminal` · 复用 `gateterm-protocol`</sub>

<br />

[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-orange)](#)
[![Renderer](https://img.shields.io/badge/GPU-Zed%20gpui%20%2B%20Alacritty-3FDD8C)](#)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%C2%B7%20macOS%20%C2%B7%20Linux-blue)](#)
[![License](https://img.shields.io/badge/license-MIT-blue)](../../LICENSE.md)

<sub>[English](README.md) · 简体中文</sub>

</div>

---

## 为什么构建原生桌面端

Gateterm 的核心设计是 **「后台常驻的 `gateterm-server` 持有你的 PTY 与屏幕状态，而不是窗口本身」**。在此基础上，`clients/desktop-native` 参考 [`l0ng-ai/tty7`](https://github.com/l0ng-ai/tty7) 与 `clients/web` 的交互细节，将原本运行在浏览器中的体验升级为**零浏览器开销的原生 GPU 桌面工作台**：

- **沉浸式 GPUI 虚拟标题栏 (`TitleBar`)** — 采用 `gpui-component` 的 `TitleBar::title_bar_options()` 自绘无边框/融合标题栏，去除多余的顶部水平标签栏；左侧边栏头部保留 `gateterm`、历史会话与 `+` 新建会话，标题栏保留侧边栏折叠、当前会话面包屑（独立拖拽区）以及右上角纯图标按钮（终端内搜索、左右/上下均分分屏、多窗格缩放、结构化对话切换、设置），职责清晰不冗余。
- **极致渲染性能** — 基于 Zed 的 `gpui` 与 `alacritty_terminal` 状态机自定义 `TerminalElement`，在 GPU 场景中批量绘制背景块、禁连字等宽字形（自动识别 CJK 宽字符）、选区、搜索匹配高亮与光标。
- **N 叉均分分屏 (`PaneTree`)** — 同方向连续分屏（如连续点击两次上下分屏或左右分屏）自动归并为同级兄弟节点并按 `1/N` 等分空间（2 分屏各 1/2，3 分屏各 1/3，4 分屏各 1/4），单窗格零多余顶栏。
- **与 `clients/web` 一致的智能新建会话弹窗 (`New Session Modal`)** — 点击 `+` 弹出带实时筛选框的分组菜单，按 `provider`（内置、自定义、iTerm2、Windows Terminal）分组展示 `name` 与 `detail · cwd`，点击即启动。
- **开箱即用 + 多服务端连接** — 启动时默认在进程内自动拉起内置 `gateterm-server`（端口冲突自动回退），同时支持一键添加多台远端 `gateterm-server` 节点（支持直接粘贴 `http://host:port/#token=...` 链接）。
- **原生感知 AI Coding Agent** — 实时感知 Claude Code、Codex、Gemini CLI、OpenCode、pi、Grok 与 Antigravity CLI 的运行状态，顶部弹出远程工具审批横幅，并支持一键切换为「结构化对话视图」。

## 有什么 (`What's inside`)

| 模块 | 能力一览 |
| :--- | :--- |
| **GPUI 虚拟标题栏 (`TitleBar`)** | 融合系统窗口控制按钮 · 独立窗口拖拽区（不遮挡按钮点击） · 纯图标右上角工具栏（切换侧边栏、终端内搜索、左右/上下均分分屏、多窗格最大化、切换对话视图、设置） |
| **N 叉等分分屏 (`PaneTree`)** | <kbd>Ctrl+Shift+D</kbd> 左右等分分屏 · <kbd>Ctrl+Shift+E</kbd> 上下等分分屏 · 同轴多次分屏自动 `1/N` 均分 · <kbd>Ctrl+Shift+Z</kbd> 单窗格最大化缩放 · <kbd>Ctrl+Tab</kbd> 窗格焦点循环 |
| **终端内回滚搜索 (`Ctrl+F`)** | <kbd>Ctrl+F</kbd> 呼出右上角紧凑浮动搜索条 · 实时大小写不敏感匹配全部回滚历史与当前视口 · 终端画布内高亮全部匹配与当前焦点匹配 · <kbd>Enter</kbd> / <kbd>Shift+Enter</kbd> 上下跳转并自动居中滚动 |
| **分组新建会话弹窗 (`+`)** | 对齐 `clients/web` 的弹出式选择器 · 顶部关键词过滤 · 按 `内置` / `自定义` / `iTerm2` / `Windows Terminal` 分组展示启动项详情与工作目录 · 点击即创建会话 |
| **AI Agent 实时协同** | 会话侧边栏实时状态徽章 (`Waiting` / `Working` / `Thinking` / `Error` / `Idle`) · 顶部工具权限审批横幅（一键 **允许 Allow** / **拒绝 Deny** / 点击徽章跳转会话） |
| **结构化对话视图 (`Chat View`)** | <kbd>Ctrl+Shift+M</kbd> 随时在「VT 终端画面」与「结构化消息流」间切换 · 独立渲染 `Thinking` 思考过程、`ToolUse` 工具调用与 `ToolResult` 输出 · 底部输入框直接向终端发送指令 |
| **Agent 历史会话与恢复** | <kbd>Ctrl+Shift+H</kbd> 检索宿主机已有的 Claude Code / Codex / Gemini / pi / Grok 历史会话 · 按 Agent 过滤与关键词搜索 · 右侧预览完整对话 · 一键在原目录恢复（Resume） |
| **GPU 终端与跨平台输入** | 完整支持 IME 中文输入法预编辑浮层 · Windows ConPTY `win32-input-mode` 精确按键序列（保留 `Shift+Enter`、`Ctrl+J`、`Alt+方向键` 修饰状态） · SGR 鼠标追踪穿透与拖拽选区复制 |
| **主题、字体与钩子管理** | <kbd>Ctrl+,</kbd> 内置 5 套主题配色（`2026 Dark` / `2026 Light` / `Tokyo Night` / `Catppuccin Mocha` / `Solarized Dark`，同步驱动 UI 与终端 16 色） · 实时调节字体与字号 · 一键安装/卸载 Agent Hooks |

## 快速开始

```powershell
cd clients/desktop-native

# 若前端 clients/web/dist 已存在或无需重新构建 web 静态资源，可设置跳过 npm 构建加速编译：
$env:GATETERM_SKIP_WEB_BUILD="1"

# 启动原生桌面客户端
cargo run --release
```

## 快捷键参考

| 快捷键 | 功能 |
| :--- | :--- |
| <kbd>Ctrl+Shift+N</kbd> / <kbd>Cmd+N</kbd> | 打开 **新建会话 (New Session)** 分组启动弹窗 |
| <kbd>Ctrl+Shift+T</kbd> / <kbd>Cmd+T</kbd> | 在当前活动服务器直接新建默认终端会话 |
| <kbd>Ctrl+B</kbd> / <kbd>Cmd+B</kbd> | 折叠 / 展开左侧边栏 |
| <kbd>Ctrl+F</kbd> / <kbd>Cmd+F</kbd> | 打开 / 关闭 **终端内回滚搜索** 浮动栏 |
| <kbd>Ctrl+Shift+D</kbd> / <kbd>Cmd+D</kbd> | 左右水平等分分屏（新建终端） |
| <kbd>Ctrl+Shift+E</kbd> / <kbd>Cmd+Shift+D</kbd> | 上下垂直等分分屏（新建终端） |
| <kbd>Ctrl+Shift+Z</kbd> / <kbd>Cmd+Shift+Enter</kbd> | 最大化 / 还原当前活动窗格 |
| <kbd>Ctrl+Tab</kbd> | 在当前工作区的多个分屏窗格间切换焦点 |
| <kbd>Ctrl+Shift+M</kbd> | 切换「终端视图」与「Agent 结构化对话视图」 |
| <kbd>Ctrl+Shift+H</kbd> | 打开 Agent 历史会话浏览与恢复面板 |
| <kbd>Ctrl+,</kbd> / <kbd>Cmd+,</kbd> | 打开桌面端设置面板 |
| <kbd>Ctrl+Shift+W</kbd> | 关闭当前活动分屏窗格或会话视图 |
| <kbd>Ctrl+C</kbd>（有选区时） / <kbd>Ctrl+Shift+C</kbd> | 复制终端选区文本 |
| <kbd>Ctrl+V</kbd> / <kbd>Ctrl+Shift+V</kbd> / 右键 | 粘贴剪贴板文本（自动适配 Bracketed Paste） |

## 目录结构

```text
clients/desktop-native/
├── Cargo.toml                   # 独立 Cargo Workspace，通过 path 依赖复用 crates/{protocol,client-core,server,cloud-client}
├── src/
│   ├── main.rs                  # GPUI 应用入口、虚拟标题栏配置、全局快捷键绑定与窗口初始化
│   ├── gpui_tokio.rs            # GPUI 与 Tokio 异步运行时桥接层
│   ├── config.rs                # 桌面端配置持久化 (desktop-client.toml) 与 #token=... 链接解析
│   ├── local_server.rs          # 内置 gateterm-server 生命周期监管 (LocalServerSupervisor)
│   ├── theme.rs                 # 5 套终端 16 色调色板与 gpui-component 主题联动
│   ├── i18n.rs                  # 中英文双语国际化支持
│   ├── client/
│   │   ├── ws.rs                # 单连接多通道二进制 WebSocket 客户端 + Backoff 重连 + ResumeTracker
│   │   ├── api.rs               # REST API 客户端 (Launchers / Agent Sessions / Pending / Chat / History / Hooks)
│   │   └── mod.rs               # Agent 状态优先级归并与结构化对话增量合并
│   ├── terminal/
│   │   ├── element.rs           # 自定义 GPU 终端渲染元素 (TerminalElement，含搜索高亮与选区渲染)
│   │   ├── input.rs             # xterm / modifyOtherKeys / ConPTY win32-input-mode 键盘编码器
│   │   └── view.rs              # TerminalView 实体（选区、回滚搜索、IME 输入法、鼠标协议与剪贴板）
│   └── ui/
│       ├── app.rs               # GatetermWorkbench 主工作台视图（集成虚拟 TitleBar、可折叠侧边栏与浮动终端搜索）
│       ├── pane.rs              # WorkspaceTab 与 N 叉等分分屏树 (PaneTree / PaneNode)
│       ├── new_session_modal.rs # 对齐 clients/web 的分组过滤「新建会话」弹出菜单
│       ├── pending_banner.rs    # AI Agent 工具调用权限审批横幅
│       ├── chat_view.rs         # AI Agent 结构化对话与 Thinking/Tool 渲染视图
│       ├── history_modal.rs     # Agent 历史会话检索、预览与一键恢复弹窗
│       ├── server_dialog.rs     # 远程服务器节点添加/编辑弹窗
│       └── settings_modal.rs    # 外观、字体、内置服务、Agent 钩子与 Cloud 状态设置面板
```
