use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

/// `gateterm-server` 的默认监听:**只回环**。
///
/// 无头部署往往跑在别人的机器上、被别的东西转发,默认对外开是不礼貌也不安全的。
/// 要对外用 `--host` 显式说出来。
pub fn default_listen() -> SocketAddr {
    "127.0.0.1:7683".parse().unwrap()
}

/// `gateterm-desktop` 的默认监听:**全网卡**。
///
/// 和 server 不同是有意的。desktop 的存在理由就是「从手机/平板连回自己这台机器」——
/// 默认只回环等于这个产品开箱即不可用,而绝大多数人不会去翻文档改配置。
///
/// 敢这么定的前提是两条硬边界都已经在了:
///
/// 1. **空 token 进不去**。desktop 里所有 token 都经过 `Supervisor::restart` 开头那个
///    `ensure!`(release 构建也拦),所以「全网卡 + 无鉴权」在这条路径上结构性不可能;
/// 2. **hook 面无条件只认回环**。`/hook/*` 看的是对端地址,主监听是不是 0.0.0.0 都一样
///    —— 那条路上跑的是 `tool_input`(命令原文、文件路径),不对外开。
///
/// 剩下的残余风险是实打实的:同一局域网里的人只要拿到 token 就能用你的终端。
/// 这和 README 里说的「越过 localhost 就自己带网络层安全」是同一件事,只是现在
/// 默认值站在了「能用」这一边。用户可以在设置窗口里改回 127.0.0.1。
pub fn desktop_default_listen() -> SocketAddr {
    "0.0.0.0:7683".parse().unwrap()
}

/// 多个连接同时 attach 同一个会话时,pty 用谁的尺寸(tmux `window-size` 的语义)。
///
/// 一个会话只有一个 pty、一份 grid,所有 attach 收到的是同一份字节流,服务端
/// 无法逐客户端裁剪画面,所以必须从各端的尺寸诉求里归并出一个权威值。这是服务端
/// 策略,不在协议里(G3):客户端只上报自己想要多大,再按 `resized` 渲染。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum WindowSize {
    /// 取所有 attach 的最小值:谁都不会看到被截断的画面,大屏那端留白。
    #[default]
    Smallest,
    /// 取最大值:小屏那端只能看到画面的一部分。
    Largest,
    /// 跟随最近一次 attach/resize 的那一端。
    Latest,
}

/// 用户在 config.toml 里写的一条自定义启动项(`[[launcher]]`)。
///
/// `command` 直接就是 argv,不是命令行字符串 —— 省掉了"该按哪套规则切分"这个
/// 问题(见 launcher/cmdline.rs 里两套互不兼容的规则)。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LauncherSpec {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

/// `[agents]` —— agent 集成。
///
/// 两个开关**默认开**。它们只是「允许」:`install_enabled` 不会自动装任何东西,
/// 真正改 `~/.claude/settings.json` 的仍然只有用户在设置页按下的「安装」(见
/// `crate::agent::apply` —— 不做「每次启动重写」);`transcript_enabled` 只在有人
/// 打开聊天视图时才读对话记录。headless / 共享部署可以在 `[agents]` 里显式关掉。
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct AgentsConfig {
    #[serde(default = "default_true")]
    pub install_enabled: bool,
    /// 多久没有事件就清掉一个 agent 会话。**只对没有关联终端的会话生效** —— 终端还在时
    /// 不按时间清理,见 `crate::agent::store::decide`。
    #[serde(default = "default_session_stale")]
    pub session_stale_secs: u64,
    #[serde(default = "default_working_stale")]
    pub working_stale_secs: u64,
    /// 把**整段对话**送上网。
    ///
    /// 和 `install_enabled` 是**分开**的一个开关:装 hook 让我们看到「它在干什么」,
    /// 这个开关让我们看到「它说了什么」—— 后者是代码、路径、命令、模型的思考过程,
    /// 暴露面高一个数量级,所以要能单独关掉。
    #[serde(default = "default_true")]
    pub transcript_enabled: bool,
    /// 一个挂起请求最多等多久。
    ///
    /// 必须**小于**写进 hook 里的 `timeout`(600 秒),这样超时是我们主动写一个
    /// 「无决定」的响应,而不是让 Claude 那头自己超时 —— 两者行为一样,但前者
    /// 我们知道发生了什么,能记日志、能把状态改回去。
    #[serde(default = "default_pending_timeout")]
    pub pending_timeout_secs: u64,
}

fn default_true() -> bool {
    true
}

fn default_pending_timeout() -> u64 {
    590
}

fn default_session_stale() -> u64 {
    600
}

fn default_working_stale() -> u64 {
    300
}

impl Default for AgentsConfig {
    fn default() -> Self {
        Self {
            install_enabled: true,
            session_stale_secs: default_session_stale(),
            working_stale_secs: default_working_stale(),
            pending_timeout_secs: default_pending_timeout(),
            transcript_enabled: true,
        }
    }
}

/// `[cloud]` —— 可选的 Gateterm Cloud(账号 + 手机推送中继)。
/// `url` 只是登录时的默认地址;真正的登录态在 `cloud.json`(见 `gateterm_cloud_client::credentials`)。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CloudConfig {
    #[serde(default)]
    pub url: Option<String>,
}

/// `[clipboard]` —— 会话内程序的「复制」同步到远端客户端。
///
/// OSC 52 那条路始终开着(它只把会话自己发出的序列定向转给操作的那一端)。这里的
/// 开关管的是另一条:监听服务端机器的**系统剪贴板**,把会话里的进程写进去的内容
/// 转给远端(目前只有 Windows,见 `crate::clipboard`)。默认开;不希望服务端剪贴板
/// 的任何内容离开这台机器时可以关掉。
fn default_osc52_env() -> bool {
    !cfg!(windows)
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct ClipboardConfig {
    #[serde(default = "default_true")]
    pub host_watch: bool,
    /// 启动 PTY 时是否额外注入 `SSH_CONNECTION`,以引导 Codex / Grok 等 CLI 走 OSC 52。
    ///
    /// Windows 上默认关闭:
    /// 1. Claude Code 在 Windows 检测到 `SSH_CONNECTION` 时会触发 `win_ssh_auto_off`,
    ///    强制禁用全屏 TUI 并提示 `Still the classic renderer · fullscreen is off over SSH to Windows`;
    /// 2. Windows 已有 `host_watch` 监听系统剪贴板兜底 Codex / Grok / agy,而 Claude / Pi / OpenCode
    ///    本身无需 `SSH_CONNECTION` 即可走 OSC 52 / 5522。
    /// 非 Windows(macOS / Linux)目前没有 `host_watch`,默认开启。需要时可在 `config.toml` 显式覆盖。
    #[serde(default = "default_osc52_env")]
    pub osc52_env: bool,
}

impl Default for ClipboardConfig {
    fn default() -> Self {
        Self {
            host_watch: true,
            osc52_env: default_osc52_env(),
        }
    }
}

/// `[mcp]` —— 外部 agent MCP server 与 HTTP API 开关与节点身份元数据。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct McpConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            name: None,
            description: None,
        }
    }
}

/// 规范化 `[mcp]` 文本字段:将 `\r`/`\n`/`\t` 替换为空格、首尾 trim、截断至 `max_chars` 字符并再次 trim 尾部,空串视为 `None`。
pub fn normalize_mcp_text(raw: Option<&str>, max_chars: usize) -> Option<String> {
    let raw = raw?;
    let cleaned = raw.replace(['\r', '\n', '\t'], " ");
    let trimmed = cleaned.trim();
    if trimmed.is_empty() || max_chars == 0 {
        return None;
    }
    let truncated: String = trimmed.chars().take(max_chars).collect();
    let final_str = truncated.trim_end();
    (!final_str.is_empty()).then(|| final_str.to_string())
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Config {
    /// 配置文件里没写就是 `None` —— **「没指定」是个真实存在的状态**,不能在这一层
    /// 塞一个默认值把它抹平:server 和 desktop 的默认监听本来就不一样,谁用谁定。
    #[serde(default)]
    pub listen: Option<SocketAddr>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub window_size: WindowSize,
    /// TOML 里是 `[[launcher]]`(单数),读出来是一组。
    #[serde(default, rename = "launcher")]
    pub launchers: Vec<LauncherSpec>,
    #[serde(default)]
    pub agents: AgentsConfig,
    #[serde(default)]
    pub cloud: CloudConfig,
    #[serde(default)]
    pub clipboard: ClipboardConfig,
    #[serde(default)]
    pub mcp: McpConfig,
    /// 自定义 Web 页面标题前缀。非空时页面 `<title>` 渲染为 `{web_title} - gateterm`;
    /// 未设置或纯空白时保持默认 `gateterm`。
    #[serde(default)]
    pub web_title: Option<String>,
}


fn default_path() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "gateterm")
        .context("cannot determine config directory")?;
    Ok(dirs.config_dir().join("config.toml"))
}

impl Config {
    /// 只读加载:**自己的** config.toml 由 gateterm-desktop 写,server 只读。
    /// (server 唯一会写的是 agent 集成装 hook 时改的**别人的**配置文件,受
    /// `agents.install_enabled` 门控,见 `crate::agent::apply`。)
    /// 显式路径必须存在;缺省路径存在则读,不存在用默认值。
    pub fn load(path: Option<PathBuf>) -> Result<Config> {
        let path = match path {
            Some(p) => {
                if !p.exists() {
                    bail!("config file not found: {}", p.display());
                }
                p
            }
            None => {
                let p = default_path()?;
                if !p.exists() {
                    return Ok(Config::default());
                }
                p
            }
        };
        Ok(toml::from_str(&std::fs::read_to_string(&path)?)?)
    }
}

/// token 优先级:--token > 配置文件 > 每次启动新生成(Jupyter 式)。
/// 返回 (token, 是否本次生成)。
///
/// **空白值等于没配**:`""` 和 `"   "` 一律当作 `None` 处理,顺着优先级往下走
/// (`--token ""` 因此也不会盖掉配置文件里的好 token),最终落到随机生成那条路。
/// 这是鉴权底线 —— 鉴权是 `token == state.token` 的字面比较(见 app.rs 的
/// WebSocket 握手),空 token 生效就等于「空对空一律放行」,配上 `--host 0.0.0.0`
/// 就是一个全网卡、无鉴权的终端服务。而空值恰恰是最容易写出来的:设置界面写着
/// 「留空表示不固定」,配置文件里 `token = ""` 看着也像「不设 token」。
///
/// **前后空白会被去掉**:`token = "  abc  "` 的生效值是 `"abc"`。原样保留的话,
/// 客户端几乎不可能把两端的空格一字不差地发上来,结果是个谁也连不上的 server。
pub fn resolve_token(cli: Option<String>, config: Option<String>) -> (String, bool) {
    let pick = |v: Option<String>| v.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    match pick(cli).or_else(|| pick(config)) {
        Some(t) => (t, false),
        None => (uuid::Uuid::new_v4().simple().to_string(), true),
    }
}

/// 规范化自定义 Web 标题前缀:将 `\r`/`\n` 替换为空格并 trim,纯空白视为未设置(`None`)。
pub fn normalize_web_title(raw: Option<String>) -> Option<String> {
    raw.map(|s| s.replace(['\r', '\n'], " ").trim().to_string())
        .filter(|s| !s.is_empty())
}

/// web_title 优先级:--web-title > 配置文件 > None(默认只显示 `gateterm`)。
pub fn resolve_web_title(cli: Option<String>, config: Option<String>) -> Option<String> {
    normalize_web_title(cli).or_else(|| normalize_web_title(config))
}

/// 渲染给网页 `<title>` 的最终标题文本。
pub fn format_web_title(custom: Option<&str>) -> String {
    match normalize_web_title(custom.map(str::to_string)) {
        Some(title) => format!("{title} - gateterm"),
        None => "gateterm".to_string(),
    }
}

/// 线程安全的共享 Web 标题状态,供 `AppState` 与 `Supervisor` 热更新使用。
#[derive(Debug, Clone, Default)]
pub struct SharedWebTitle(std::sync::Arc<std::sync::RwLock<Option<String>>>);

impl SharedWebTitle {
    pub fn new(title: Option<String>) -> Self {
        Self(std::sync::Arc::new(std::sync::RwLock::new(
            normalize_web_title(title),
        )))
    }

    pub fn get(&self) -> Option<String> {
        self.0
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set(&self, title: Option<String>) {
        *self.0.write().unwrap_or_else(|e| e.into_inner()) = normalize_web_title(title);
    }
}

/// 命令行 --host/--port 覆盖配置文件的 listen;None 表示沿用配置值
pub fn effective_listen(
    base: SocketAddr,
    host: Option<std::net::IpAddr>,
    port: Option<u16>,
) -> SocketAddr {
    let mut listen = base;
    if let Some(host) = host {
        listen.set_ip(host);
    }
    if let Some(port) = port {
        listen.set_port(port);
    }
    listen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> SocketAddr {
        "127.0.0.1:7683".parse().unwrap()
    }

    #[test]
    fn no_overrides_keeps_config() {
        assert_eq!(effective_listen(base(), None, None), base());
    }

    /// 「配置文件里没写 listen」必须能和「写了 127.0.0.1」区分开 —— 两个二进制的
    /// 默认值不一样,在 Config 这一层塞默认值就把这个区别抹平了。
    #[test]
    fn absent_listen_stays_absent() {
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg.listen, None);
        let cfg: Config = toml::from_str(r#"listen = "127.0.0.1:1234""#).unwrap();
        assert_eq!(cfg.listen, Some("127.0.0.1:1234".parse().unwrap()));
    }

    /// server 只回环、desktop 全网卡,这是刻意的差异,不是笔误。
    #[test]
    fn the_two_binaries_default_differently() {
        assert!(default_listen().ip().is_loopback());
        assert!(desktop_default_listen().ip().is_unspecified());
        assert_eq!(default_listen().port(), desktop_default_listen().port());
    }

    #[test]
    fn host_and_port_override_independently() {
        assert_eq!(
            effective_listen(base(), Some("0.0.0.0".parse().unwrap()), None).to_string(),
            "0.0.0.0:7683"
        );
        assert_eq!(
            effective_listen(base(), None, Some(9000)).to_string(),
            "127.0.0.1:9000"
        );
    }

    #[test]
    fn both_overrides_apply() {
        assert_eq!(
            effective_listen(base(), Some("::1".parse().unwrap()), Some(9000)).to_string(),
            "[::1]:9000"
        );
    }

    #[test]
    fn window_size_defaults_to_smallest_and_parses_kebab_case() {
        assert_eq!(Config::default().window_size, WindowSize::Smallest);
        let cfg: Config = toml::from_str("window_size = \"latest\"").unwrap();
        assert_eq!(cfg.window_size, WindowSize::Latest);
    }

    /// agent 集成默认开;`[agents]` 里显式写 false 仍然能关掉。
    #[test]
    fn agent_switches_default_on_and_can_be_turned_off() {
        let cfg: Config = toml::from_str("").unwrap();
        assert!(cfg.agents.install_enabled && cfg.agents.transcript_enabled);
        let cfg: Config = toml::from_str("[agents]\nsession_stale_secs = 60").unwrap();
        assert!(cfg.agents.install_enabled && cfg.agents.transcript_enabled);
        let cfg: Config =
            toml::from_str("[agents]\ninstall_enabled = false\ntranscript_enabled = false").unwrap();
        assert!(!cfg.agents.install_enabled && !cfg.agents.transcript_enabled);
    }

    #[test]
    fn launchers_parse_from_double_bracket_tables() {
        let cfg: Config = toml::from_str(
            r#"
            [[launcher]]
            name = "prod ssh"
            command = ["ssh", "prod01"]
            cwd = "~/work"

            [[launcher]]
            name = "python"
            command = ["python3", "-i"]
            "#,
        )
        .unwrap();
        assert_eq!(cfg.launchers.len(), 2);
        assert_eq!(cfg.launchers[0].command, ["ssh", "prod01"]);
        assert_eq!(cfg.launchers[0].cwd.as_deref(), Some("~/work"));
        assert_eq!(cfg.launchers[1].cwd, None);
    }

    #[test]
    fn missing_launcher_section_yields_an_empty_list() {
        let cfg: Config = toml::from_str("").unwrap();
        assert!(cfg.launchers.is_empty());
    }

    #[test]
    fn token_priority_cli_then_config_then_generated() {
        assert_eq!(
            resolve_token(Some("cli".into()), Some("cfg".into())),
            ("cli".into(), false)
        );
        assert_eq!(resolve_token(None, Some("cfg".into())), ("cfg".into(), false));
        let (t1, generated) = resolve_token(None, None);
        assert!(generated && !t1.is_empty());
        let (t2, _) = resolve_token(None, None);
        assert_ne!(t1, t2, "每次生成的 token 必须不同");
    }

    #[test]
    fn blank_cli_token_does_not_shadow_config() {
        // `--token ""` 不是「把 token 设成空」,而是「没给」—— 否则它会盖掉配置
        // 文件里那个好 token,把 server 变成空对空放行。
        assert_eq!(
            resolve_token(Some(String::new()), Some("cfg".into())),
            ("cfg".into(), false)
        );
        assert_eq!(
            resolve_token(Some("   ".into()), Some("cfg".into())),
            ("cfg".into(), false)
        );
    }

    #[test]
    fn blank_token_falls_back_to_generated() {
        // 配置文件里 `token = ""` / `token = "   "` 等同于没配,走随机生成。
        for blank in ["", "   ", "\t\n"] {
            let (t, generated) = resolve_token(None, Some(blank.into()));
            assert!(generated, "空白 token 必须回退到随机生成:{blank:?}");
            assert!(!t.trim().is_empty());
        }
        let (t, generated) = resolve_token(Some("  ".into()), None);
        assert!(generated && !t.trim().is_empty());
    }

    #[test]
    fn cloud_section_is_optional() {
        let c: Config = toml::from_str("").unwrap();
        assert_eq!(c.cloud.url, None);
        let c: Config = toml::from_str("[cloud]\nurl = \"https://cloud.example.com\"\n").unwrap();
        assert_eq!(c.cloud.url.as_deref(), Some("https://cloud.example.com"));
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        // 生效值必须是 trim 过的:鉴权是字面比较,带空格的 token 客户端发不对。
        assert_eq!(
            resolve_token(None, Some("  abc  ".into())),
            ("abc".into(), false)
        );
        assert_eq!(
            resolve_token(Some("\tcli\n".into()), Some("cfg".into())),
            ("cli".into(), false)
        );
    }

    #[test]
    fn web_title_is_optional_in_toml() {
        let absent: Config = toml::from_str("").unwrap();
        assert_eq!(absent.web_title, None);
        let present: Config = toml::from_str(r#"web_title = "prod-01""#).unwrap();
        assert_eq!(present.web_title.as_deref(), Some("prod-01"));
    }

    #[test]
    fn resolve_web_title_prioritizes_cli_and_normalizes_whitespace() {
        assert_eq!(
            resolve_web_title(Some("cli".into()), Some("cfg".into())).as_deref(),
            Some("cli")
        );
        assert_eq!(
            resolve_web_title(Some("   ".into()), Some("  cfg\r\nbox  ".into())).as_deref(),
            Some("cfg  box")
        );
        assert_eq!(resolve_web_title(Some("\r\n ".into()), Some("  ".into())), None);
        assert_eq!(resolve_web_title(None, None), None);
    }

    #[test]
    fn format_web_title_uses_custom_prefix_or_default() {
        assert_eq!(format_web_title(None), "gateterm");
        assert_eq!(format_web_title(Some("   ")), "gateterm");
        assert_eq!(format_web_title(Some("prod-01")), "prod-01 - gateterm");
        assert_eq!(format_web_title(Some("  prod\n01  ")), "prod 01 - gateterm");
    }

    #[test]
    fn shared_web_title_normalizes_and_updates_across_clones() {
        let shared = SharedWebTitle::new(Some("  node-a\n ".into()));
        let cloned = shared.clone();
        assert_eq!(cloned.get().as_deref(), Some("node-a"));

        shared.set(Some("  node-b\r\n2  ".into()));
        assert_eq!(cloned.get().as_deref(), Some("node-b  2"));

        shared.set(Some("   ".into()));
        assert_eq!(cloned.get(), None);
    }

    #[test]
    fn mcp_config_defaults_to_enabled_and_parses_false() {
        let default_cfg: Config = toml::from_str("").unwrap();
        assert!(default_cfg.mcp.enabled);
        assert_eq!(default_cfg.mcp.name, None);
        assert_eq!(default_cfg.mcp.description, None);

        let disabled_cfg: Config = toml::from_str("[mcp]\nenabled = false\n").unwrap();
        assert!(!disabled_cfg.mcp.enabled);
        assert_eq!(disabled_cfg.mcp.name, None);
        assert_eq!(disabled_cfg.mcp.description, None);
    }

    #[test]
    fn mcp_config_parses_name_and_description_and_normalizes_text() {
        let cfg: Config = toml::from_str(
            "[mcp]\nenabled = true\nname = \"  mac-build\\n \"\ndescription = \"\\tARM64\\r\\nbuilder  \"\n",
        )
        .unwrap();
        assert_eq!(
            normalize_mcp_text(cfg.mcp.name.as_deref(), 64).as_deref(),
            Some("mac-build")
        );
        assert_eq!(
            normalize_mcp_text(cfg.mcp.description.as_deref(), 256).as_deref(),
            Some("ARM64  builder")
        );
        assert_eq!(normalize_mcp_text(Some("   \r\n\t "), 64), None);
        let long = "a".repeat(80);
        assert_eq!(normalize_mcp_text(Some(&long), 64).unwrap().chars().count(), 64);
        let cjk = "中".repeat(70);
        assert_eq!(normalize_mcp_text(Some(&cjk), 64).unwrap().chars().count(), 64);
    }

    #[test]
    fn clipboard_config_defaults_and_overrides() {
        let default_cfg: Config = toml::from_str("").unwrap();
        assert!(default_cfg.clipboard.host_watch);
        assert_eq!(default_cfg.clipboard.osc52_env, !cfg!(windows));

        let partial: Config = toml::from_str("[clipboard]\nhost_watch = false\n").unwrap();
        assert!(!partial.clipboard.host_watch);
        assert_eq!(partial.clipboard.osc52_env, !cfg!(windows));

        let explicit_off: Config =
            toml::from_str("[clipboard]\nhost_watch = false\nosc52_env = false\n").unwrap();
        assert!(!explicit_off.clipboard.host_watch);
        assert!(!explicit_off.clipboard.osc52_env);

        let explicit_on: Config =
            toml::from_str("[clipboard]\nhost_watch = true\nosc52_env = true\n").unwrap();
        assert!(explicit_on.clipboard.host_watch);
        assert!(explicit_on.clipboard.osc52_env);
    }
}
