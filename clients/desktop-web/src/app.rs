//! winit 的 ApplicationHandler:所有状态都在主线程上,tokio 在后台线程。

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use gateterm_server::config::Config;
use gateterm_server::session::manager::SessionManager;
use tokio::runtime::Runtime;
use tokio::sync::broadcast::error::RecvError;
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::window::WindowId;

use crate::supervisor::Supervisor;
use crate::tray::Tray;
use crate::window::EguiWindow;
use crate::i18n::{LanguagePref, Locale};
use crate::tr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    OpenWeb,
    CopyUrl,
    Settings,
    CheckForUpdates,
    ViewLogs,
    Quit,
}

#[derive(Debug, Clone)]
pub struct StagedApplyBundle {
    pub staged: gateterm_updater::StagedUpdate,
    pub canonical_target: PathBuf,
    pub cache_root: PathBuf,
}

#[derive(Debug, Clone)]
pub enum UserEvent {
    MenuClicked(MenuAction),
    SessionsChanged,
    /// 设置窗口发起的 Cloud 操作完成了(在 tokio 线程上跑,结果经 proxy 送回主线程)。
    CloudDone(u64, crate::settings::cloud::CloudAction, Result<String, String>),
    AgentsScanned(u64, Option<u16>, Vec<gateterm_server::agent::AgentStatus>),
    AgentPreviewReady(
        u64,
        String,
        Result<gateterm_server::agent::ops::AgentPlanSummary, String>,
    ),
    AgentActionDone(
        u64,
        String,
        crate::settings::agents::AgentActionKind,
        Result<String, String>,
    ),
    UpdateChecked {
        generation: u64,
        manual: bool,
        is_startup: bool,
        result: Result<gateterm_updater::CheckOutcome, String>,
    },
    UpdateDownloadProgress {
        generation: u64,
        progress: gateterm_updater::DownloadProgress,
    },
    UpdateStaged {
        generation: u64,
        result: Result<StagedApplyBundle, String>,
    },
    RollbackNotice(String),
}

/// 托盘第 `failures` 次创建失败之后,隔多久再试一次;`None` 表示不再试了。
///
/// 会重试是因为这个失败有明确的成因和明确的时间窗口:Windows 上 `Shell_NotifyIcon`
/// 在 explorer.exe 尚未就绪时会失败,而开机自启 —— 这个 app 的主要启动方式 ——
/// 正好落在那个窗口里。偏偏 `resumed` 在 Windows 上基本只会调一次,不重试就等于
/// 永远没有托盘。
///
/// 指数退避,累计 7.5 秒(0.5 + 1 + 2 + 4)后放弃:够覆盖登录时 explorer 起来的
/// 那一段,再久就不该让用户对着空菜单栏干等,该弹框说清楚然后退出了。
pub fn tray_retry_delay(failures: u32) -> Option<Duration> {
    (1..=4).contains(&failures).then(|| Duration::from_millis(500 << (failures - 1)))
}

/// 启动阶段的产物:配置本身,以及两条「起来了但没完全起来」的坏消息。
///
/// 这两个错误必须一路带到 GUI 里 —— 配置坏了 / 端口占用时 Web UI 是打不开的,
/// 设置窗口是用户唯一能看到原因、并且动手修的地方。
#[derive(Debug, Clone)]
pub struct Startup {
    pub language: LanguagePref,
    pub web_host: Option<IpAddr>,
    pub config: Config,
    /// `Config::load` 失败的原因;此时 `config` 是默认值。
    pub config_error: Option<String>,
    /// 启动时 `Supervisor::restart` 失败的原因。保存成功重新监听后会被清掉。
    pub listen_error: Option<String>,
}

pub struct App {
    locale: Locale,
    resolved_web_host: Option<IpAddr>,
    cloud_task: Option<tokio::task::JoinHandle<()>>,
    cloud_generation: u64,
    agents_generation: u64,
    rt: Runtime,
    sup: Supervisor,
    lock_guard: Option<crate::single_instance::Guard>,
    tray: Option<Tray>,
    /// 托盘至今失败了几次。兼作「已经试过了」的标记:`resumed` 只负责发起第一次
    /// 尝试,后续重试由 `new_events` 接管,这个计数一旦非零就再也不归零。
    tray_failures: u32,
    /// 下一次重试托盘的时刻。由 `about_to_wait` 排进 `ControlFlow::WaitUntil`,
    /// 到点由 `new_events` 接回来 —— 不用 sleep,事件循环不能为了重试卡住。
    tray_retry_at: Option<Instant>,
    proxy: EventLoopProxy<UserEvent>,
    log_path: PathBuf,
    /// 启动阶段的产物:配置副本 + 启动失败原因。配置只做只读展示(window_size
    /// 与 [[launcher]] 设置界面不提供修改,改了也不会写回这个副本)。
    startup: Startup,
    /// 平时是 None——托盘常驻应用不该在后台挂着一个隐藏窗口和一整套 GPU 上下文。
    /// 点「设置…」时创建,关窗口时置回 None,`EguiWindow` 的 Drop 负责把 surface
    /// 和 device 还给系统。
    settings_window: Option<EguiWindow>,
    /// 设置窗口的界面状态,和 `settings_window` 同生共死(开窗时建、关窗时清)。
    view: Option<crate::settings::ui::View>,
    update_window: Option<EguiWindow>,
    update_view: Option<crate::update_ui::UpdateView>,
    update_tracker: UpdateTracker,
    download_cancel: Option<tokio::sync::watch::Sender<bool>>,
    update_daily_task: Option<tokio::task::JoinHandle<()>>,
}

impl App {
    pub fn new(
        rt: Runtime,
        sup: Supervisor,
        proxy: EventLoopProxy<UserEvent>,
        log_path: PathBuf,
        startup: Startup,
        lock_guard: crate::single_instance::Guard,
    ) -> Self {
        let resolved_web_host = sup
            .listen()
            .map(|listen| crate::settings::web_addr::resolve_live_web_host(listen, startup.web_host));
        Self {
            locale: startup.language.resolve(Locale::system()),
            resolved_web_host,
            cloud_task: None,
            cloud_generation: 0,
            agents_generation: 0,
            rt,
            sup,
            lock_guard: Some(lock_guard),
            tray: None,
            tray_failures: 0,
            tray_retry_at: None,
            proxy,
            log_path,
            startup,
            settings_window: None,
            view: None,
            update_window: None,
            update_view: None,
            update_tracker: UpdateTracker::default(),
            download_cancel: None,
            update_daily_task: None,
        }
    }

    /// 启动时的致命状况合成的一条常驻横幅。它和 [`crate::settings::ui::View::message`]
    /// 是两回事:横幅说的是「程序现在处于什么坏状态」,`message` 说的是「你刚点的
    /// 这次保存结果如何」——后者随编辑清除,前者一直挂到状况被修好为止。
    fn banner(&self) -> Option<String> {
        match (&self.startup.config_error, &self.startup.listen_error) {
            (Some(c), Some(l)) => Some(tr!(self.locale, "配置文件有错:{c}\n监听失败:{l}", "Invalid configuration: {c}\nCould not start listener: {l}")),
            (Some(c), None) => Some(tr!(self.locale, "配置文件有错(当前使用默认值):{c}", "Invalid configuration (using defaults): {c}")),
            (None, Some(l)) => Some(tr!(self.locale, "当前未监听:{l}", "Not listening: {l}")),
            (None, None) => None,
        }
    }

    /// 打开设置窗口时,从当前生效值构造视图。
    fn build_view(&self) -> crate::settings::ui::View {
        use crate::settings::{state::Form, ui::View};
        let listen = self.sup.listen().unwrap_or_else(|| {
            self.startup.config.listen.unwrap_or_else(gateterm_server::config::desktop_default_listen)
        });
        // 读不出来就当没开:这一项读失败不该阻止用户打开设置窗口
        let autostart = crate::autostart::is_enabled().unwrap_or(false);
        let mut form = Form::from_current(listen, autostart);
        form.language = self.startup.language;
        form.web_host = self.startup.web_host;
        form.web_title = self.sup.web_title().unwrap_or_default();
        // token 回填当前生效值:用户多半是来看它、复制它的,展示为空反而费解。
        // 没在监听时压根没有「当前生效的 token」,框里就是空的(= 不固定)——
        // 保存时 save.rs 会现场生成一个,不会拿空串去跑。
        form.token = match self.sup.token() {
            Some(t) => t.to_string(),
            None => String::new(),
        };
        let banner = self.banner();
        View {
            tab: crate::settings::ui::SettingsTab::General,
            form,
            web_addr: crate::settings::web_addr::WebAddrPicker::new(self.sup.listen()),
            // WindowSize 只有 Smallest / Largest / Latest 三个单词,和 config.toml
            // 里写的小写形式一一对应。
            window_size: format!("{:?}", self.startup.config.window_size).to_lowercase(),
            launchers: self
                .startup
                .config
                .launchers
                .iter()
                .map(|l| (l.name.clone(), l.command.join(" ")))
                .collect(),
            banner,
            message: None,
            agents: crate::settings::agents::AgentsView {
                listen_port: self.sup.listen().map(|a| a.port()),
                install_enabled: self.startup.config.agents.install_enabled,
                ..Default::default()
            },
            cloud: crate::settings::cloud::CloudForm {
                region: crate::settings::cloud::RelayRegion::from_url(
                    &Self::cloud_creds()
                        .map(|c| c.url)
                        .or_else(|| self.startup.config.cloud.url.clone())
                        .unwrap_or_default(),
                ),
                ..Default::default()
            },
            cloud_status: crate::settings::cloud::status_from(Self::cloud_creds()),
            cloud_busy: self.cloud_task.is_some(),
            cloud_message: None,
            update_status_line: self.update_tracker.summary.status_line(self.locale),
            update_checking: self.update_tracker.checking,
        }
    }

    fn agents_scan_port(&self) -> u16 {
        self.sup
            .listen()
            .map(|a| a.port())
            .or_else(|| self.startup.config.listen.map(|a| a.port()).filter(|&p| p != 0))
            .unwrap_or_else(|| gateterm_server::config::desktop_default_listen().port())
    }

    fn start_agents_scan(&mut self, refresh_shell_path: bool) {
        let listen_port = self.sup.listen().map(|a| a.port());
        let scan_port = self.agents_scan_port();
        let install_enabled = self.startup.config.agents.install_enabled;
        let generation = if let Some(view) = self.view.as_mut() {
            prepare_agents_scan(
                &mut view.agents,
                &mut self.agents_generation,
                listen_port,
                install_enabled,
            )
        } else {
            self.agents_generation = self.agents_generation.wrapping_add(1);
            self.agents_generation
        };
        let proxy = self.proxy.clone();
        self.rt.spawn(async move {
            let items = tokio::task::spawn_blocking(move || {
                let env = if refresh_shell_path {
                    gateterm_server::agent::detect::DetectEnv::current_refreshed()
                } else {
                    gateterm_server::agent::detect::DetectEnv::current()
                };
                gateterm_server::agent::scan(&env, scan_port)
            })
            .await
            .unwrap_or_default();
            let _ = proxy.send_event(UserEvent::AgentsScanned(generation, listen_port, items));
        });
    }

    fn start_agents_preview(&mut self, id: String) {
        let Some(view) = self.view.as_mut() else { return };
        if view.agents.busy_id.is_some() {
            return;
        }
        view.agents.expanded_preview = Some(id.clone());
        view.agents.previews.remove(&id);

        let generation = self.agents_generation;
        let scan_port = self.agents_scan_port();
        let locale = self.locale;
        let proxy = self.proxy.clone();
        let id_for_task = id.clone();
        self.rt.spawn(async move {
            let res = tokio::task::spawn_blocking(move || {
                let Some(adapter) = gateterm_server::agent::find(&id_for_task) else {
                    return Err(tr!(locale, "未知的 Agent：{id_for_task}", "Unknown agent: {id_for_task}"));
                };
                let home = gateterm_server::agent::detect::DetectEnv::current_home();
                gateterm_server::agent::ops::preview_plan(adapter, &home, scan_port)
                    .map_err(|e| e.to_string())
            })
            .await;
            let summary = match res {
                Ok(inner) => inner,
                Err(e) => Err(tr!(locale, "后台任务失败：{e}", "Background task failed: {e}")),
            };
            let _ = proxy.send_event(UserEvent::AgentPreviewReady(generation, id, summary));
        });
    }

    fn start_agents_action(
        &mut self,
        id: String,
        kind: crate::settings::agents::AgentActionKind,
    ) {
        use crate::settings::{agents::AgentActionKind, state::Message};
        let Some(view) = self.view.as_mut() else { return };
        if view.agents.busy_id.is_some() {
            return;
        }
        let listen_port = self.sup.listen().map(|a| a.port());
        if kind == AgentActionKind::Install && listen_port.is_none() {
            view.agents.messages.insert(
                id,
                Message::Err(
                    self.locale
                        .text(
                            "当前服务未监听，无法安装 hook。",
                            "The server is not listening; cannot install hooks.",
                        )
                        .into(),
                ),
            );
            return;
        }

        self.agents_generation = self.agents_generation.wrapping_add(1);
        let generation = self.agents_generation;
        view.agents.busy_id = Some(id.clone());
        view.agents.confirm_purge_id = None;

        let scan_port = self.agents_scan_port();
        let install_enabled = self.startup.config.agents.install_enabled;
        let locale = self.locale;
        let proxy = self.proxy.clone();
        let id_for_task = id.clone();
        self.rt.spawn(async move {
            let res = tokio::task::spawn_blocking(move || {
                let Some(adapter) = gateterm_server::agent::find(&id_for_task) else {
                    return Err(tr!(locale, "未知的 Agent：{id_for_task}", "Unknown agent: {id_for_task}"));
                };
                let home = gateterm_server::agent::detect::DetectEnv::current_home();
                run_agent_mutation(
                    adapter,
                    &home,
                    scan_port,
                    install_enabled,
                    kind,
                    locale,
                )
            })
            .await;
            let result = match res {
                Ok(inner) => inner,
                Err(e) => Err(tr!(locale, "后台任务失败：{e}", "Background task failed: {e}")),
            };
            let _ = proxy.send_event(UserEvent::AgentActionDone(generation, id, kind, result));
        });
    }

    fn handle_agent_action(&mut self, action: crate::settings::agents::AgentUiAction) {
        use crate::settings::agents::{AgentActionKind, AgentUiAction};
        match action {
            AgentUiAction::Refresh => self.start_agents_scan(true),
            AgentUiAction::Preview(id) => self.start_agents_preview(id),
            AgentUiAction::Install(id) => self.start_agents_action(id, AgentActionKind::Install),
            AgentUiAction::Uninstall(id) => {
                self.start_agents_action(id, AgentActionKind::Uninstall)
            }
            AgentUiAction::Purge(id) => self.start_agents_action(id, AgentActionKind::Purge),
        }
    }

    fn cloud_creds() -> Option<gateterm_cloud_client::Credentials> {
        let path = gateterm_cloud_client::credentials::default_path()?;
        gateterm_cloud_client::credentials::load(&path).ok().flatten()
    }

    fn start_cloud(&mut self, action: crate::settings::cloud::CloudAction) {
        use crate::settings::ui::Message;
        let Some(view) = self.view.as_mut() else { return };
        if action == crate::settings::cloud::CloudAction::CancelLogin {
            if let Some(task) = self.cloud_task.take() {
                task.abort();
                self.cloud_generation += 1;
            }
            view.cloud_busy = false;
            view.cloud_status = crate::settings::cloud::status_from(Self::cloud_creds());
            view.cloud_message = Some(Message::Ok(self.locale.text("已取消登录", "Sign-in canceled").into()));
            return;
        }
        if self.cloud_task.is_some() {
            return;
        }
        let job = match crate::settings::cloud::prepare(&view.cloud, &view.cloud_status, action, self.locale) {
            Ok(job) => job,
            Err(m) => {
                view.cloud_message = Some(Message::Err(m));
                return;
            }
        };
        let Some(path) = gateterm_cloud_client::credentials::default_path() else {
            view.cloud_message = Some(Message::Err(self.locale.text("无法确定配置目录", "Cannot locate the configuration directory").into()));
            return;
        };
        view.cloud_busy = true;
        view.cloud_message = None;
        let proxy = self.proxy.clone();
        self.cloud_generation += 1;
        let generation = self.cloud_generation;
        let locale = self.locale;
        self.cloud_task = Some(self.rt.spawn(async move {
            let result = crate::cloud_ops::run(job, path, gateterm_cloud_client::default_label(), locale).await;
            let _ = proxy.send_event(UserEvent::CloudDone(generation, action, result));
        }));
    }

    /// 执行「保存并应用」。真正的分支逻辑在 [`crate::settings::save::apply`],
    /// 这里只负责把当前状态喂进去、把结果贴回界面。
    fn apply_settings(&mut self) {
        use crate::settings::save::{apply, Current};

        let Some(view) = self.view.as_ref() else { return };
        // 先把表单拷出来:下面要 &mut 借 self.sup,不能同时挂着 self.view 的借用。
        let form = view.form.clone();
        let current = Current {
            // 没在监听时是 None 而不是空串:空串会一路被当成一个真 token 用,
            // 而 server 侧 `token == state.token` 的比较对空串一律放行。
            listen: self.sup.listen(),
            token: self.sup.token().map(str::to_string),
            sessions: self.sup.manager().list().len(),
        };

        let mut fx = AppEffects { rt: &self.rt, sup: &mut self.sup };
        let locale = form.language.resolve(Locale::system());
        let applied = apply(&form, &current, &mut fx, locale);
        // Language changes only become active after the preference was saved.
        // Updating labels in place preserves the tray, window and PTY sessions.
        let saved_ok = matches!(&applied.message, crate::settings::ui::Message::Ok(_));
        let locale_changed = saved_ok && locale != self.locale;
        if saved_ok {
            self.startup.language = form.language;
            self.startup.web_host = form.web_host;
            self.locale = locale;
            self.sup.set_web_title(Some(form.web_title.clone()));
            if let Some(tray) = self.tray.as_mut() { tray.set_language(locale); }
            if let Some(window) = &self.settings_window {
                window.set_title(locale.text("gateterm 设置", "gateterm Settings"));
            }
            if let Some(window) = &self.update_window {
                window.set_title(locale.text("gateterm 更新", "gateterm Update"));
                window.request_redraw();
            }
            if let Some(view) = self.view.as_mut() {
                view.cloud_message = None;
                view.form.web_title = self.sup.web_title().unwrap_or_default();
                view.update_checking = self.update_tracker.checking;
                view.update_status_line = self.update_tracker.summary.status_line(self.locale);
            }
        }

        // 重新监听成功了,启动时那条「未监听」就此作废。不清掉的话托盘状态行和
        // 横幅会一直挂着一条已经不成立的错误(用户明明已经把端口改好了)。
        // config_error 不能跟着清:配置文件仍然是坏的 —— `configfile::save` 靠
        // toml_edit 解析原文,文件坏着的时候这次保存根本落不了盘。
        if applied.restarted {
            self.startup.listen_error = None;
        }

        // 把**实际生效**的 token 写回表单。「表单留空 + 服务没在监听」时 save.rs 会
        // 当场生成一个新 token,不写回的话用户在这个窗口里根本看不到它是什么 ——
        // 退路只有关窗去托盘点「复制访问链接」,而界面上没有任何提示告诉他这一点。
        //
        // 只在 restarted 时写:校验失败、自启失败这些路径上服务没动过,写回去只会
        // 把用户正在输入的内容冲掉。写进去之后再点一次保存会把它固化进 config.toml,
        // 这和开窗时 `build_view` 回填当前 token 的效果一致,不是新增的行为。
        //
        // 位置:`RedrawRequested` 里 `redraw_ui` 闭包的**外面**(闭包同一帧可能被
        // 跑两趟),`apply_settings` 整个都在闭包外,这里同样是一次性副作用。
        if applied.restarted
            && let Some(token) = self.sup.token().map(str::to_string)
            && let Some(view) = self.view.as_mut()
        {
            view.form.token = token;
        }

        let banner = self.banner();
        let listen_port = self.sup.listen().map(|a| a.port());
        let rescan_agents = if let Some(view) = self.view.as_mut() {
            view.banner = banner;
            view.message = Some(applied.message);
            view.web_addr.refresh(self.sup.listen());
            let rescan = apply_agents_on_save(
                &mut view.agents,
                saved_ok,
                locale_changed,
                applied.restarted,
                listen_port,
            );
            self.resolved_web_host = self
                .sup
                .listen()
                .map(|listen| view.web_addr.resolved(listen, self.startup.web_host));
            rescan
        } else {
            self.refresh_resolved_web_host();
            applied.restarted || locale_changed
        };
        if rescan_agents {
            self.start_agents_scan(false);
        }

        // 无条件刷新,不看 `applied.restarted`:restart 失败时它是 false,而
        // 「同地址只换 token」那条失败路径恰恰会让服务停在未监听状态(supervisor
        // 已经先 stop() 了)——只在成功时刷新的话,状态行会一直挂着已经失效的
        // 旧地址和会话数。刷新本身只是重读一次当前状态,多调用没有代价。
        self.refresh_status();
    }

    /// 关掉设置窗口:GPU 资源随 `EguiWindow` 的 Drop 当场还回去,界面状态一并
    /// 丢掉——下次开窗要重新读当前生效值,而不是接着上次没保存的输入往下改。
    fn close_settings(&mut self) {
        self.agents_generation = self.agents_generation.wrapping_add(1);
        self.settings_window = None;
        self.view = None;
    }

    fn refresh_resolved_web_host(&mut self) {
        self.resolved_web_host = self
            .sup
            .listen()
            .map(|listen| crate::settings::web_addr::resolve_live_web_host(listen, self.startup.web_host));
    }

    /// 带 token 的访问 URL,动态解析当前可达 host 并保持状态行同步。
    fn url(&mut self) -> Option<String> {
        let listen = self.sup.listen()?;
        let token = self.sup.token()?;
        let host = crate::settings::web_addr::resolve_live_web_host(listen, self.startup.web_host);
        let url = crate::settings::web_addr::format_access_url(
            host,
            listen.port(),
            token,
            self.sup.web_title().as_deref(),
        );
        if self.resolved_web_host != Some(host) {
            self.resolved_web_host = Some(host);
            self.refresh_status();
        }
        Some(url)
    }

    /// 状态行里能实际打开的 host。使用缓存的 `resolved_web_host` 避免每次
    /// `SessionsChanged` 事件都扫描本机网卡。
    fn status_text(&self) -> String {
        match self.sup.listen() {
            Some(addr) => {
                let n = self.sup.manager().list().len();
                let host = if addr.ip().is_unspecified() {
                    self.resolved_web_host
                        .unwrap_or_else(|| crate::settings::web_addr::loopback_for(addr.ip()))
                } else {
                    addr.ip()
                };
                let endpoint = SocketAddr::new(host, addr.port());
                tr!(self.locale, "gateterm · {endpoint} · {n} 个会话", "gateterm · {endpoint} · {n} sessions")
            }
            // 带上原因:托盘状态行是用户第一眼看到的东西,只说「未监听」等于
            // 让他去翻日志。
            None => match &self.startup.listen_error {
                Some(e) => tr!(self.locale, "gateterm · 未监听({e})", "gateterm · Not listening ({e})"),
                None => self.locale.text("gateterm · 未监听", "gateterm · Not listening").to_string(),
            },
        }
    }

    fn refresh_status(&mut self) {
        let text = self.status_text();
        if let Some(tray) = self.tray.as_mut() {
            tray.set_status(&text);
        }
    }

    /// 订阅会话事件,变化时叫主线程刷新状态行。
    fn watch_sessions(&self, manager: Arc<SessionManager>) {
        let proxy = self.proxy.clone();
        self.rt.spawn(async move {
            let mut rx = manager.events();
            // Lagged 只说明事件来得太快、漏了几条,而状态行本来就只是重新读一次
            // 当前列表 —— 漏掉的那几条不影响结果,继续听就是;只有 Closed 才收工。
            while let Ok(_) | Err(RecvError::Lagged(_)) = rx.recv().await {
                if proxy.send_event(UserEvent::SessionsChanged).is_err() {
                    break; // 事件循环没了,收工
                }
            }
        });
    }

    /// Start daily checks after the tray is ready; only a non-skipped startup result opens the update window.
    fn check_for_updates(&mut self) {
        let cfg = match gateterm_updater::CheckConfig::for_app(
            gateterm_updater::AppKind::DesktopWeb,
            env!("CARGO_PKG_VERSION"),
        ) {
            Ok(cfg) => cfg,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "无法初始化更新检查配置");
                return;
            }
        };
        let proxy = self.proxy.clone();
        let _rt = self.rt.enter();
        self.update_daily_task = Some(gateterm_updater::start_daily_checks(cfg, move |is_startup, result| {
            if let Err(e) = &result {
                tracing::debug!(error = %format!("{e:#}"), is_startup, "后台检查更新失败");
            }
            let _ = proxy.send_event(UserEvent::UpdateChecked {
                generation: 0,
                manual: false,
                is_startup,
                result: result.map_err(|e| format!("{e:#}")),
            });
        }));
    }

    fn sync_settings_update_status(&mut self) {
        if let Some(view) = self.view.as_mut() {
            view.update_checking = self.update_tracker.checking;
            view.update_status_line = self.update_tracker.summary.status_line(self.locale);
        }
        if let Some(w) = &self.settings_window {
            w.request_redraw();
        }
    }

    fn open_or_focus_update_window(&mut self, event_loop: &ActiveEventLoop) {
        if self.update_window.is_none() {
            match EguiWindow::open(
                event_loop,
                self.locale.text("gateterm 更新", "gateterm Update"),
                (520, 480),
            ) {
                Ok(w) => {
                    self.update_window = Some(w);
                }
                Err(e) => {
                    tracing::error!(error = %format!("{e:#}"), "无法打开更新窗口");
                    self.cancel_active_download();
                    cancel_update_state(&mut self.update_tracker, &mut self.update_view);
                    self.sync_settings_update_status();
                    return;
                }
            }
        }
        if let Some(w) = &self.update_window {
            w.focus();
            w.request_redraw();
        }
    }

    fn cancel_active_download(&mut self) {
        if let Some(tx) = self.download_cancel.take() {
            let _ = tx.send(true);
        }
    }

    fn cancel_and_close_update(&mut self) {
        self.cancel_active_download();
        cancel_update_state(&mut self.update_tracker, &mut self.update_view);
        self.update_window = None;
        self.sync_settings_update_status();
    }

    fn start_manual_update_check(&mut self, event_loop: &ActiveEventLoop) {
        if self.download_cancel.is_some()
            || self.update_view.as_ref().is_some_and(|v| {
                matches!(
                    v.phase,
                    crate::update_ui::UpdatePhase::Downloading(_)
                        | crate::update_ui::UpdatePhase::Preparing
                )
            })
        {
            self.open_or_focus_update_window(event_loop);
            return;
        }

        let active_sessions = self.sup.manager().list().len();
        let Some(generation) = prepare_manual_update_check(
            &mut self.update_tracker,
            &mut self.update_view,
            env!("CARGO_PKG_VERSION"),
            active_sessions,
        ) else {
            return;
        };
        self.sync_settings_update_status();
        self.open_or_focus_update_window(event_loop);

        let cfg = match gateterm_updater::CheckConfig::for_app(
            gateterm_updater::AppKind::DesktopWeb,
            env!("CARGO_PKG_VERSION"),
        ) {
            Ok(cfg) => cfg,
            Err(e) => {
                let _ = self.proxy.send_event(UserEvent::UpdateChecked {
                    generation,
                    manual: true,
                    is_startup: false,
                    result: Err(format!("{e:#}")),
                });
                return;
            }
        };
        let proxy = self.proxy.clone();
        self.rt.spawn(async move {
            let result = gateterm_updater::check_now(&cfg, true)
                .await
                .map_err(|e| format!("{e:#}"));
            let _ = proxy.send_event(UserEvent::UpdateChecked {
                generation,
                manual: true,
                is_startup: false,
                result,
            });
        });
    }

    fn start_update_download(&mut self) {
        if self.download_cancel.is_some() {
            return;
        }
        let Some(update) = self.update_view.as_ref().and_then(|v| v.update.clone()) else {
            return;
        };
        if !update.can_self_apply {
            return;
        }
        let Some(artifact) = update.artifact.clone() else {
            return;
        };

        let generation =
            prepare_update_download(&mut self.update_tracker, &mut self.update_view, artifact.size);
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        self.download_cancel = Some(cancel_tx);
        if let Some(w) = &self.update_window {
            w.request_redraw();
        }

        let locale = self.locale;
        let proxy = self.proxy.clone();
        let version = update.version.clone();
        self.rt.spawn(async move {
            let preflight = tokio::task::spawn_blocking(move || -> Result<(PathBuf, PathBuf), String> {
                let current_exe = std::env::current_exe()
                    .map_err(|e| format_current_exe_error(locale, &e))?;
                let canonical_target = gateterm_updater::preflight_writable(&current_exe)
                    .map_err(|e| format_download_error(locale, &e))?;
                let cache_root = gateterm_updater::updates_cache_dir()
                    .map_err(|e| format_cache_dir_error(locale, &e))?;
                Ok((canonical_target, cache_root))
            })
            .await;

            let (canonical_target, cache_root) = match preflight {
                Ok(Ok(paths)) => paths,
                Ok(Err(msg)) => {
                    let _ = proxy.send_event(UserEvent::UpdateStaged {
                        generation,
                        result: Err(msg),
                    });
                    return;
                }
                Err(e) => {
                    let _ = proxy.send_event(UserEvent::UpdateStaged {
                        generation,
                        result: Err(tr!(locale, "后台任务失败：{e}", "Background task failed: {e}")),
                    });
                    return;
                }
            };

            let client =
                match gateterm_updater::build_download_http_client(env!("CARGO_PKG_VERSION"), false)
                {
                Ok(c) => c,
                Err(e) => {
                    let _ = proxy.send_event(UserEvent::UpdateStaged {
                        generation,
                        result: Err(tr!(
                            locale,
                            "无法初始化下载客户端：{e:#}",
                            "Could not initialize HTTP client: {e:#}"
                        )),
                    });
                    return;
                }
            };

            let last_sent = std::sync::atomic::AtomicU64::new(0);
            let progress_proxy = proxy.clone();
            let staged_res = gateterm_updater::download_and_stage(
                &client,
                gateterm_updater::AppKind::DesktopWeb,
                &version,
                &artifact,
                &cache_root,
                cancel_rx,
                move |progress| {
                    let prev = last_sent.load(std::sync::atomic::Ordering::Relaxed);
                    if progress.downloaded == 0
                        || progress.downloaded >= progress.total
                        || progress.downloaded.saturating_sub(prev) >= 64 * 1024
                    {
                        last_sent.store(progress.downloaded, std::sync::atomic::Ordering::Relaxed);
                        let _ = progress_proxy.send_event(UserEvent::UpdateDownloadProgress {
                            generation,
                            progress,
                        });
                    }
                },
            )
            .await;

            let result = match staged_res {
                Ok(staged) => Ok(StagedApplyBundle {
                    staged,
                    canonical_target,
                    cache_root,
                }),
                Err(err) => Err(format_download_error(locale, &err)),
            };
            let _ = proxy.send_event(UserEvent::UpdateStaged { generation, result });
        });
    }

    fn handle_settings_window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        event: WindowEvent,
    ) {
        let Some(w) = self.settings_window.as_mut() else {
            return;
        };
        // 先喂给 egui:它要靠这些事件维护自己的输入状态(按键、光标、缩放因子),
        // 返回值说明它有没有把这个事件吃掉。这里我们只关心窗口生命周期,不用管。
        let _consumed = w.on_window_event(&event);
        match event {
            WindowEvent::CloseRequested => {
                // 关窗口只是关窗口,程序继续常驻。置 None 触发 Drop,GPU 资源当场
                // 还回去——不是 set_visible(false) 那种「藏起来留着」。
                self.close_settings();
            }
            WindowEvent::RedrawRequested => {
                use crate::settings::ui::{draw, Outcome};
                // 这个闭包同一帧里可能被调用多次(Grid 之类的容器会让 egui 重跑一
                // 趟),所以只能描述界面、只能往 outcome 上赋值,一次性的副作用
                // 全部挪到闭包外做一次。
                let mut outcome = Outcome::None;
                if let Some(view) = self.view.as_mut() {
                    w.redraw_ui(|ui| outcome = draw(ui, view, self.locale));
                }
                let closed = w.close_requested();
                let acted = outcome != Outcome::None;
                match outcome {
                    Outcome::None => {}
                    Outcome::Save => self.apply_settings(),
                    Outcome::Cancel => self.close_settings(),
                    Outcome::OpenConfigFile => match crate::configfile::default_path() {
                        Ok(p) => {
                            if let Err(e) = open::that_detached(&p) {
                                tracing::error!(error = %format!("{e:#}"), "打开配置文件失败");
                            }
                        }
                        Err(e) => tracing::error!(error = %format!("{e:#}"), "无法确定配置文件位置"),
                    },
                    Outcome::RegenerateToken => {
                        if let Some(view) = self.view.as_mut() {
                            // 和 server 的 resolve_token 用同一种格式(32 位十六进制)
                            view.form.token = uuid::Uuid::new_v4().simple().to_string();
                        }
                    }
                    Outcome::RefreshWebAddrs => {
                        if let Some(view) = self.view.as_mut() {
                            view.web_addr.refresh(self.sup.listen());
                            self.resolved_web_host = self
                                .sup
                                .listen()
                                .map(|listen| view.web_addr.resolved(listen, self.startup.web_host));
                        } else {
                            self.refresh_resolved_web_host();
                        }
                        self.refresh_status();
                    }
                    Outcome::CheckForUpdates => self.start_manual_update_check(event_loop),
                    Outcome::Agents(action) => self.handle_agent_action(action),
                    Outcome::Cloud(action) => self.start_cloud(action),
                }
                // 界面里 `send_viewport_cmd(ViewportCommand::Close)` 的结果从这里
                // 出来:窗口自己关不掉自己,得由持有它的这一层置 None。设置界面
                // 目前不发这条命令(「取消」走的是 `Outcome::Cancel`),但
                // `EguiWindow` 的约定要求每帧看一眼,漏了以后加就会静默失效。
                if closed {
                    self.close_settings();
                }
                // 保存结果、重新生成的 token 这些都是刚刚才写进 view 的,而这一帧
                // 已经画完了 —— 不主动再要一帧,用户点完按钮会看不到任何变化。
                if acted
                    && let Some(w) = &self.settings_window
                {
                    w.request_redraw();
                }
            }
            _ => w.request_redraw(),
        }
    }

    fn handle_update_window_event(&mut self, event_loop: &ActiveEventLoop, event: WindowEvent) {
        let Some(w) = self.update_window.as_mut() else {
            return;
        };
        let _consumed = w.on_window_event(&event);
        match event {
            WindowEvent::CloseRequested => {
                self.cancel_and_close_update();
            }
            WindowEvent::RedrawRequested => {
                use crate::update_ui::{draw, UpdateOutcome};
                let mut outcome = UpdateOutcome::None;
                if let Some(view) = self.update_view.as_mut() {
                    w.redraw_ui(|ui| outcome = draw(ui, view, self.locale));
                }
                let closed = w.close_requested();
                let acted = outcome != UpdateOutcome::None;
                match outcome {
                    UpdateOutcome::None => {}
                    UpdateOutcome::UpdateAndRestart => self.start_update_download(),
                    UpdateOutcome::OpenDownloadPage(url) => {
                        if let Err(e) = open::that_detached(&url) {
                            tracing::error!(error = %e, "打开 Release 页面失败");
                            crate::dialog::notice(
                                self.locale.text("无法打开 Release 页面", "Could not open the release page"),
                                &tr!(
                                    self.locale,
                                    "请在浏览器中打开以下链接:\n\n{url}\n\n{e}",
                                    "Open this link in your browser:\n\n{url}\n\n{e}"
                                ),
                            );
                        } else {
                            self.cancel_and_close_update();
                        }
                    }
                    UpdateOutcome::SkipVersion(version) => {
                        if let Ok(path) = gateterm_updater::UpdateState::default_path(
                            gateterm_updater::AppKind::DesktopWeb,
                        ) {
                            let mut state = gateterm_updater::UpdateState::load(&path);
                            state.skipped_version = Some(version);
                            if let Err(e) = state.save(&path) {
                                tracing::warn!(error = %format!("{e:#}"), "保存跳过版本状态失败");
                            }
                        }
                        self.cancel_and_close_update();
                    }
                    UpdateOutcome::Cancel => self.cancel_and_close_update(),
                    UpdateOutcome::RetryCheck => self.start_manual_update_check(event_loop),
                }
                if closed {
                    self.cancel_and_close_update();
                }
                if acted
                    && let Some(w) = &self.update_window
                {
                    w.request_redraw();
                }
            }
            _ => w.request_redraw(),
        }
    }

    /// 建托盘。成功了顺手把随之而来的一次性初始化做掉,失败了排重试、退避用完就
    /// 弹框退出。
    ///
    /// **绝不能只记一行日志就 return**:托盘是这个进程唯一的界面,它没建起来的时候
    /// 程序既没有窗口也没有退出入口,事件循环却还在 `Wait` 上永久阻塞 —— HTTP 层
    /// 占着端口、单实例锁挡着用户重开,只能去任务管理器杀进程。所以这里的出口只有
    /// 两个:托盘起来了,或者进程退出了。
    fn init_tray(&mut self, event_loop: &ActiveEventLoop) {
        match Tray::new(self.proxy.clone(), self.locale) {
            Ok(tray) => {
                tracing::info!(attempts = self.tray_failures + 1, "托盘已就绪");
                self.tray = Some(tray);
                self.tray_retry_at = None;
                self.watch_sessions(self.sup.manager().clone());
                self.refresh_status();
                self.check_for_updates();

                // 起不来的时候必须主动把窗口推到用户面前:这时候 Web UI 是打不开的,
                // 设置窗口是唯一的出路。
                //
                // 位置在这个分支**里面**:托盘只会成功建起来一次,所以这一下也只会
                // 发生一次,不会变成每次回到前台都弹一次设置窗口。
                if self.startup.config_error.is_some()
                    || self.startup.listen_error.is_some()
                    || std::env::var_os("GATETERM_OPEN_SETTINGS").is_some()
                {
                    self.proxy.send_event(UserEvent::MenuClicked(MenuAction::Settings)).ok();
                }
            }
            Err(e) => {
                let reason = format!("{e:#}");
                self.tray_failures += 1;
                match tray_retry_delay(self.tray_failures) {
                    Some(delay) => {
                        tracing::warn!(
                            error = %reason,
                            failures = self.tray_failures,
                            ?delay,
                            "托盘创建失败,稍后重试"
                        );
                        self.tray_retry_at = Some(Instant::now() + delay);
                    }
                    None => {
                        tracing::error!(
                            error = %reason,
                            failures = self.tray_failures,
                            "托盘创建失败,放弃"
                        );
                        // 收尾必须排在弹框之前:`fatal` 是阻塞模态,没人点就不返回,
                        // 而它挂着的这段时间里 HTTP 层还在跑、会话一个没杀 —— 那正是
                        // 这条分支要收口的状态。`exit()` 只是置标志位,回调返回后才
                        // 生效,提前调语义不变。
                        //
                        // 和「退出」菜单走同一条收尾:先关 HTTP 层再杀会话。这里不问
                        // 用户 —— 托盘都没起来,谈不上有会话在跑,也没地方让他确认。
                        shutdown(&mut self.sup);
                        self.lock_guard.take();
                        event_loop.exit();
                        crate::dialog::fatal(
                            self.locale.text("gateterm 无法启动", "gateterm could not start"),
                            &tr!(self.locale,
                                "无法创建托盘图标,重试 {} 次仍未成功。\n\n{reason}\n\n\
                                 gateterm 只有托盘这一个界面,没有它就没法操作,现在退出。\
                                 稍后可以再启动一次。",
                                "Could not create the tray icon after {} attempts.\n\n{reason}\n\ngateterm needs the tray menu to operate and will now exit. Please try starting it again later.",
                                self.tray_failures
                            ),
                        );
                    }
                }
            }
        }
    }
}

/// 退出前的清理:先停掉 HTTP 层(挡住新连接建新会话),再**显式**杀掉每一个
/// 已有的会话。
///
/// 不依赖 winit、不弹对话框,好让「退出会杀掉全部会话」这条能被测试直接验证。
///
/// ⚠️ 这个循环看着多余,其实一步都不能省 —— 靠 drop 是收不掉会话的:
/// `Session` 没有 `Drop` 实现,而 pty 的读线程和 wait 线程各自握着一份
/// `Arc<Session>`(见 `crates/server/src/session/pty.rs`),`Arc` 计数永远归不了
/// 零,`Session::kill()` / `killer.kill()` 因此从来不会被调用。现在进程退出后
/// 还能收拾干净,纯粹是靠 OS 关掉 pty master fd、内核给 slave 的前台进程组发
/// SIGHUP —— 被 `nohup` / `disown` 的孙进程、或者干脆忽略 SIGHUP 的程序会原地
/// 留成孤儿。只有走 `manager.kill(id)`(→ `Session::kill` → `killer.kill()` +
/// `force_close_pty()`)才是真的杀。**别把这个循环删了。**
pub fn shutdown(sup: &mut Supervisor) {
    // 先停 HTTP 层再杀会话,而不是反过来:kill 这几行期间如果 HTTP 层还活着,
    // 理论上会有一个 WebSocket 客户端在拿到 `manager.list()` 这份快照**之后**、
    // 循环把它 kill 掉**之前**发一条 `NewSession`——那个新会话就漏在快照外面,
    // 逃过这次 kill,变成孤儿。先 `stop()` 关掉 accept 循环能挡住*新连接*建
    // 新会话。
    //
    // 但这堵不死:`Supervisor::stop` 自己的文档写清楚了,已经升级完成的旧
    // WebSocket 不受 abort 影响,会带着旧 AppState 继续跑到客户端自己断开 ——
    // 这样的客户端理论上仍能在 `stop()` 之后、kill 循环跑到它之前发
    // `NewSession`。所以这个改动只是把竞争窗口从「HTTP 层整个存活期间」收窄到
    // 「一条已升级的旧 WebSocket 连接期间」,不是把这条竞争消除了。
    sup.stop();

    // 先把 id 收集出来再杀:`kill` 会改 manager 内部的 map,不能边遍历边改。
    let ids: Vec<u64> = sup.manager().list().iter().map(|s| s.id).collect();
    if !ids.is_empty() {
        tracing::info!(count = ids.len(), "退出:正在终止全部会话");
    }
    for id in ids {
        // 返回值(是否真的杀到了)故意丢弃:false 只代表会话在这期间自己退出了
        // (比如 shell 自然退出),是良性竞争。`SessionManager::kill` 在找不到
        // 会话时会自己打一条 `warn!("kill: no such session")`——这不是故障,
        // 退出日志里出现这条不代表哪里坏了,不必因此去改 server 侧的日志级别
        // (server 代码不允许改动)。
        sup.manager().kill(id);
    }
}

/// 「退出」按钮的文案。按钮上显示的文案与下面判断返回值时比较的字符串必须是
/// **同一份**:两处各写一遍字面量的话,以后改按钮文案很容易只改一处、忘了改
/// 比较那一处 —— macOS 上没有 `Ok` 兜底(见下方判断逻辑),那样会静默变成
/// 「有会话时永远退不出去」,而且不会有任何编译错误或测试失败提醒你。
///
/// 有活跃会话时确认;没有会话就别打扰用户。返回 true 表示「确认退出」。
///
/// 用系统原生对话框而不是再开一个 egui 窗口:退出确认必须能在设置窗口没开的
/// 情况下弹出来,而且它是模态的 —— 借一个 winit 窗口反而要处理一堆生命周期。
///
/// 平台差异(rfd 0.15.4 实测,与 brief 的写法有出入):
/// * macOS 无父窗口时走 `CFUserNotificationDisplayAlert`,自定义按钮文案生效,
///   点第一个按钮返回 `Custom(quit_label)`;
/// * Windows 启用 `common-controls-v6`,并由 build.rs 嵌入激活清单,支持自定义
///   按钮文案。保留 `Ok` 判断以兼容未使用该特性的 rfd 后端。
fn confirm_quit(sessions: usize, locale: Locale) -> bool {
    let quit_label = locale.text("退出", "Quit");
    let result = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title(locale.text("退出 gateterm", "Quit gateterm"))
        .set_description(tr!(locale,
            "还有 {sessions} 个会话正在运行。退出会终止它们,里面跑的程序都会被杀掉。",
            "There are {sessions} active sessions. Quitting will terminate them and all programs running inside them."
        ))
        .set_buttons(rfd::MessageButtons::OkCancelCustom(quit_label.into(), locale.text("取消", "Cancel").into()))
        .show();
    matches!(result, rfd::MessageDialogResult::Custom(ref s) if s == quit_label)
        || result == rfd::MessageDialogResult::Ok
}

/// [`crate::settings::save::Effects`] 的真实实现:保存流程要做的 IO 都在这儿。
///
/// 单独一个结构体而不是给 `App` 实现:`apply` 要 `&mut` 借 supervisor,而同一时刻
/// 界面状态(`App::view`)还得留着写回提示文案,借整个 `App` 就冲突了。
struct AppEffects<'a> {
    rt: &'a Runtime,
    sup: &'a mut Supervisor,
}

impl crate::settings::save::Effects for AppEffects<'_> {
    fn set_autostart(&mut self, enabled: bool) -> Result<()> {
        crate::autostart::set(enabled)
    }

    fn config_path(&mut self) -> Result<PathBuf> {
        crate::configfile::default_path()
    }

    fn save_config(&mut self, path: &Path, edit: &crate::configfile::Editable) -> Result<()> {
        crate::configfile::save(path, edit)
    }

    fn restart(&mut self, listen: SocketAddr, token: String) -> Result<SocketAddr> {
        // 有意用 block_on 把 UI 线程卡住:换来的是保存流程完全串行、界面上不存在
        // 「正在保存」这种中间态。一次点击卡这一下(bind + spawn,毫秒级)可以接受。
        self.rt.block_on(self.sup.restart(listen, token))
    }

    fn new_token(&mut self) -> String {
        // 和 server 的 resolve_token 用同一种格式(32 位无连字符十六进制)
        uuid::Uuid::new_v4().simple().to_string()
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // 托盘常驻应用启动时不建窗口:设置窗口在用户点「设置…」时才创建。
        //
        // 这里只发起**第一次**尝试。`resumed` 会被调用多次(macOS 上每次 activate
        // 都可能来一发),而 `tray_failures` 一旦非零就再也不归零 —— 正在重试的、
        // 以及退避用完已经放弃的,都不会被这里重新拉起一轮。
        if self.tray.is_none() && self.tray_failures == 0 {
            self.init_tray(event_loop);
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::SessionsChanged => {
                self.refresh_status();
                if let Some(view) = self.update_view.as_mut() {
                    view.active_sessions = self.sup.manager().list().len();
                    if let Some(w) = &self.update_window {
                        w.request_redraw();
                    }
                }
            }
            UserEvent::CloudDone(generation, action, result) => {
                if generation != self.cloud_generation {
                    return;
                }
                self.cloud_task = None;
                use crate::settings::ui::Message;
                if let Some(view) = self.view.as_mut() {
                    view.cloud_busy = false;
                    view.cloud_status = crate::settings::cloud::status_from(Self::cloud_creds());
                    if result.is_ok() && !action.keeps_secrets() {
                        view.cloud.password.clear();
                        view.cloud.code.clear();
                        view.cloud.confirm_delete = false;
                    }
                    view.cloud_message = Some(match result {
                        Ok(m) => Message::Ok(m),
                        Err(m) => Message::Err(m),
                    });
                }
                if let Some(w) = &self.settings_window {
                    w.request_redraw();
                }
            }
            UserEvent::AgentsScanned(generation, listen_port, items) => {
                let current_generation = self.agents_generation;
                if let Some(view) = self.view.as_mut() {
                    if !apply_agents_scanned(
                        &mut view.agents,
                        current_generation,
                        generation,
                        listen_port,
                        items,
                    ) {
                        return;
                    }
                } else if generation != current_generation {
                    return;
                }
                if let Some(w) = &self.settings_window {
                    w.request_redraw();
                }
            }
            UserEvent::AgentPreviewReady(generation, id, result) => {
                let current_generation = self.agents_generation;
                if let Some(view) = self.view.as_mut() {
                    if !apply_agent_preview_ready(
                        &mut view.agents,
                        current_generation,
                        generation,
                        id,
                        result,
                    ) {
                        return;
                    }
                } else if generation != current_generation {
                    return;
                }
                if let Some(w) = &self.settings_window {
                    w.request_redraw();
                }
            }
            UserEvent::AgentActionDone(generation, id, _kind, result) => {
                let current_generation = self.agents_generation;
                if let Some(view) = self.view.as_mut() {
                    if !apply_agent_action_done(
                        &mut view.agents,
                        current_generation,
                        generation,
                        id,
                        result,
                    ) {
                        return;
                    }
                } else if generation != current_generation {
                    return;
                }
                self.start_agents_scan(false);
                if let Some(w) = &self.settings_window {
                    w.request_redraw();
                }
            }
            UserEvent::UpdateChecked {
                generation,
                manual,
                is_startup,
                result,
            } => {
                if let Ok(outcome) = &result {
                    let release_update = match outcome {
                        gateterm_updater::CheckOutcome::UpdateAvailable { update, .. } => {
                            Some(gateterm_server::update::ReleaseUpdate {
                                version: update.version.clone(),
                                url: update.release_url.clone(),
                            })
                        }
                        gateterm_updater::CheckOutcome::UpToDate { .. } => None,
                    };
                    gateterm_server::update::record_external_status(
                        env!("CARGO_PKG_VERSION"),
                        release_update,
                    );
                }

                let active_sessions = self.sup.manager().list().len();
                let transition = apply_update_checked(
                    &mut self.update_tracker,
                    &mut self.update_view,
                    generation,
                    manual,
                    is_startup,
                    env!("CARGO_PKG_VERSION"),
                    active_sessions,
                    self.locale,
                    &result,
                );
                self.sync_settings_update_status();
                match transition {
                    CheckTransition::Ignored | CheckTransition::NoWindow => {}
                    CheckTransition::OpenWindow => self.open_or_focus_update_window(event_loop),
                    CheckTransition::RedrawWindow => {
                        if let Some(w) = &self.update_window {
                            w.request_redraw();
                        }
                    }
                }
            }
            UserEvent::UpdateDownloadProgress {
                generation,
                progress,
            } => {
                if apply_download_progress(
                    &self.update_tracker,
                    &mut self.update_view,
                    generation,
                    progress,
                ) && let Some(w) = &self.update_window
                {
                    w.request_redraw();
                }
            }
            UserEvent::UpdateStaged { generation, result } => {
                if generation != self.update_tracker.download_generation {
                    if let Ok(bundle) = result {
                        std::thread::spawn(move || {
                            let _ = std::fs::remove_dir_all(&bundle.staged.staging_dir);
                        });
                    }
                    return;
                }
                self.download_cancel = None;
                match apply_update_staged(
                    &self.update_tracker,
                    &mut self.update_view,
                    generation,
                    result,
                ) {
                    StagedTransition::Ignored => {}
                    StagedTransition::Failed => {
                        if let Some(w) = &self.update_window {
                            w.request_redraw();
                        }
                    }
                    StagedTransition::ReadyToApply(bundle) => {
                        if let Err(err) =
                            gateterm_updater::preflight_writable(&bundle.canonical_target)
                        {
                            let staging_dir = bundle.staged.staging_dir;
                            std::thread::spawn(move || {
                                let _ = std::fs::remove_dir_all(&staging_dir);
                            });
                            let msg = format_download_error(self.locale, &err);
                            if let Some(view) = self.update_view.as_mut() {
                                let fallback_url =
                                    view.update.as_ref().map(|u| u.release_url.clone());
                                view.phase = crate::update_ui::UpdatePhase::Error {
                                    message: msg,
                                    fallback_url,
                                };
                            }
                            if let Some(w) = &self.update_window {
                                w.request_redraw();
                            }
                            return;
                        }

                        self.cancel_and_close_update();
                        self.settings_window = None;
                        self.view = None;
                        if let Some(task) = self.update_daily_task.take() {
                            task.abort();
                        }
                        shutdown(&mut self.sup);
                        drop(self.lock_guard.take());
                        if let Err(err) = gateterm_updater::spawn_apply_helper(
                            &bundle.staged,
                            &bundle.canonical_target,
                            &bundle.cache_root,
                        ) {
                            let reason = format!("{err:#}");
                            tracing::error!(error = %reason, "无法启动更新替换进程");
                            event_loop.exit();
                            crate::dialog::fatal(
                                self.locale.text("gateterm 更新失败", "gateterm update failed"),
                                &tr!(
                                    self.locale,
                                    "后台服务已停止，但无法启动更新程序。\n\n{reason}\n\n请重新启动 gateterm，或前往 GitHub Release 页面手动下载安装。",
                                    "Background services were stopped, but the updater helper could not be started.\n\n{reason}\n\nPlease restart gateterm or download the latest release from GitHub."
                                ),
                            );
                            return;
                        }
                        event_loop.exit();
                    }
                }
            }
            UserEvent::RollbackNotice(err_msg) => {
                crate::dialog::notice(
                    self.locale
                        .text("gateterm 更新未完成", "gateterm update did not complete"),
                    &tr!(
                        self.locale,
                        "上次应用更新未能完成，已自动恢复为当前版本。\n\n{err_msg}",
                        "The previous update could not be applied and was rolled back to this version.\n\n{err_msg}"
                    ),
                );
            }
            UserEvent::MenuClicked(MenuAction::OpenWeb) => match self.url() {
                Some(url) => {
                    if let Err(e) = open::that_detached(url) {
                        tracing::error!(error = %format!("{e:#}"), "打开浏览器失败");
                    }
                }
                // http 层没监听成功(比如端口被占用),点了菜单却什么都不会发生——
                // 不留日志的话用户根本无从判断是没反应还是自己没点中。
                None => tracing::warn!("尚未监听,无法打开 Web 客户端"),
            },
            UserEvent::MenuClicked(MenuAction::CopyUrl) => match self.url() {
                Some(url) => match arboard::Clipboard::new().and_then(|mut c| c.set_text(url)) {
                    Ok(()) => {}
                    Err(e) => tracing::error!(error = %format!("{e:#}"), "复制到剪贴板失败"),
                },
                None => tracing::warn!("尚未监听,无法复制访问链接"),
            },
            UserEvent::MenuClicked(MenuAction::ViewLogs) => {
                if let Err(e) = open::that_detached(&self.log_path) {
                    tracing::error!(error = %format!("{e:#}"), "打开日志失败");
                }
            }
            UserEvent::MenuClicked(MenuAction::Settings) => {
                if self.settings_window.is_none() {
                    // 每次开窗都重新读一遍当前生效值,而不是复用上次关窗时的表单:
                    // 期间监听地址可能已经被别处改过了。
                    self.view = Some(self.build_view());
                    match EguiWindow::open(event_loop, self.locale.text("gateterm 设置", "gateterm Settings"), (600, 680)) {
                        Ok(w) => {
                            self.settings_window = Some(w);
                            self.start_agents_scan(false);
                        }
                        Err(e) => {
                            tracing::error!(error = %format!("{e:#}"), "无法打开设置窗口");
                            self.agents_generation = self.agents_generation.wrapping_add(1);
                            self.view = None;
                        }
                    }
                }
                // 窗口已经开着的话,这一下就当「把它叫到前面来」。
                if let Some(w) = &self.settings_window {
                    w.focus();
                    w.request_redraw();
                }
            }
            UserEvent::MenuClicked(MenuAction::CheckForUpdates) => {
                self.start_manual_update_check(event_loop);
            }
            UserEvent::MenuClicked(MenuAction::Quit) => {
                // 「会话在断连后存活」正是这个项目的卖点,而内嵌模型下退出会把它们
                // 全杀掉 —— 这一下必须显眼。没有会话时不打扰用户。
                //
                // 对话框是同步模态的,弹着的时候事件循环整个卡住:这是原生模态的
                // 正常行为,退出确认本来也不该让用户还能去点别的菜单。
                let n = self.sup.manager().list().len();
                if n > 0 && !confirm_quit(n, self.locale) {
                    return;
                }
                self.cancel_and_close_update();
                self.settings_window = None;
                self.view = None;
                if let Some(task) = self.update_daily_task.take() {
                    task.abort();
                }
                shutdown(&mut self.sup);
                self.lock_guard.take();
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.settings_window.as_ref().is_some_and(|w| w.id() == id) {
            self.handle_settings_window_event(event_loop, event);
        } else if self.update_window.as_ref().is_some_and(|w| w.id() == id) {
            self.handle_update_window_event(event_loop, event);
        }
    }

    /// `WaitUntil` 到点只是把事件循环叫醒,它自己不产生重绘请求——光标闪烁、
    /// tooltip 延迟显示这类靠非零 `repaint_delay` 排期的东西,得在这里补一脚。
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if !matches!(cause, StartCause::ResumeTimeReached { .. }) {
            return;
        }
        // 托盘重试排在同一套 `WaitUntil` 上,到点了在这儿接回来。
        if self.tray_retry_at.is_some_and(|at| Instant::now() >= at) {
            self.tray_retry_at = None;
            self.init_tray(event_loop);
        }
        if let Some(w) = &self.settings_window {
            w.request_redraw();
        }
        if let Some(w) = &self.update_window {
            w.request_redraw();
        }
    }

    /// 每轮事件处理完、真正去睡之前决定「睡多久」。
    ///
    /// 没有窗口时是 `Wait`(无限期阻塞,直到有事件)——托盘常驻应用空闲时必须真的
    /// 空闲,不能靠 `Poll` 忙转。有窗口且 egui 排了定时重绘、或者托盘还欠一次重试
    /// 时才 `WaitUntil`,到点由上面的 `new_events` 处理。
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // 排期取最早的那个,谁都不能被对方拖晚。
        let settings_repaint = self.settings_window.as_ref().and_then(|w| w.repaint_at());
        let update_repaint = self.update_window.as_ref().and_then(|w| w.repaint_at());
        let deadline = [settings_repaint, update_repaint, self.tray_retry_at]
            .into_iter()
            .flatten()
            .min();
        let flow = match deadline {
            Some(at) => ControlFlow::WaitUntil(at),
            None => ControlFlow::Wait,
        };
        event_loop.set_control_flow(flow);
    }
}

fn run_agent_mutation(
    adapter: &'static dyn gateterm_server::agent::AgentAdapter,
    home: &Path,
    port: u16,
    install_enabled: bool,
    kind: crate::settings::agents::AgentActionKind,
    locale: Locale,
) -> Result<String, String> {
    use crate::settings::agents::AgentActionKind;
    use gateterm_server::agent::ops;

    match kind {
        AgentActionKind::Install => {
            let res = ops::install(adapter, home, port, install_enabled)
                .map_err(|e| locale.apply_error(&e))?;
            let changed = res.outcomes.iter().any(|o| o.changed);
            let backup = res.outcomes.iter().find_map(|o| o.backup.as_ref());
            if !res.include_blocking {
                Ok(locale
                    .text(
                        "已安装状态上报 hook（检测到冲突，未安装授权拦截）",
                        "Installed status reporting hooks (skipped approval interception due to conflict)",
                    )
                    .into())
            } else if changed {
                Ok(match backup {
                    Some(b) => tr!(
                        locale,
                        "已安装（原配置已备份至 {}）",
                        "Installed (backup saved to {})",
                        b.display()
                    ),
                    None => locale.text("已安装", "Installed").into(),
                })
            } else {
                Ok(locale
                    .text("已是最新，无需变更", "Already up to date; no changes needed")
                    .into())
            }
        }
        AgentActionKind::Uninstall => {
            let res = ops::uninstall(adapter, home, port, install_enabled)
                .map_err(|e| locale.apply_error(&e))?;
            let changed = res.outcomes.iter().any(|o| o.changed);
            let backup = res.outcomes.iter().find_map(|o| o.backup.as_ref());
            if changed {
                Ok(match backup {
                    Some(b) => tr!(
                        locale,
                        "已卸载（已备份至 {}）",
                        "Uninstalled (backup saved to {})",
                        b.display()
                    ),
                    None => locale.text("已卸载", "Uninstalled").into(),
                })
            } else {
                Ok(locale
                    .text(
                        "未发现需要卸载的 Gateterm hook",
                        "No Gateterm hooks needed removal",
                    )
                    .into())
            }
        }
        AgentActionKind::Purge => {
            let outcomes = ops::purge(adapter, home).map_err(|e| locale.apply_error(&e))?;
            let changed = outcomes.iter().any(|o| o.changed);
            let backup = outcomes.iter().find_map(|o| o.backup.as_ref());
            if changed {
                Ok(match backup {
                    Some(b) => tr!(
                        locale,
                        "已清理 Gateterm hooks（已备份至 {}）。其他程序的 hook 会保留。",
                        "Gateterm hooks removed (backup saved to {}). Other programs' hooks are preserved.",
                        b.display()
                    ),
                    None => locale
                        .text(
                            "已清理 Gateterm hooks。其他程序的 hook 会保留。",
                            "Gateterm hooks removed. Other programs' hooks are preserved.",
                        )
                        .into(),
                })
            } else {
                Ok(locale
                    .text(
                        "没有需要清理的 Gateterm hook。其他程序的 hook 会保留。",
                        "No Gateterm hooks needed removal. Other programs' hooks are preserved.",
                    )
                    .into())
            }
        }
    }
}

fn prepare_agents_scan(
    agents: &mut crate::settings::agents::AgentsView,
    agents_generation: &mut u64,
    listen_port: Option<u16>,
    install_enabled: bool,
) -> u64 {
    *agents_generation = agents_generation.wrapping_add(1);
    agents.loading = true;
    agents.listen_port = listen_port;
    agents.install_enabled = install_enabled;
    agents.previews.clear();
    agents.expanded_preview = None;
    *agents_generation
}

fn apply_agents_on_save(
    agents: &mut crate::settings::agents::AgentsView,
    saved_ok: bool,
    locale_changed: bool,
    restarted: bool,
    listen_port: Option<u16>,
) -> bool {
    agents.listen_port = listen_port;
    if saved_ok {
        agents.clear_localized_feedback();
    }
    restarted || locale_changed
}

fn apply_agents_scanned(
    agents: &mut crate::settings::agents::AgentsView,
    current_generation: u64,
    event_generation: u64,
    listen_port: Option<u16>,
    items: Vec<gateterm_server::agent::AgentStatus>,
) -> bool {
    if event_generation != current_generation {
        return false;
    }
    agents.loading = false;
    agents.busy_id = None;
    agents.listen_port = listen_port;
    agents.items = items;
    true
}

fn apply_agent_preview_ready(
    agents: &mut crate::settings::agents::AgentsView,
    current_generation: u64,
    event_generation: u64,
    id: String,
    result: Result<gateterm_server::agent::ops::AgentPlanSummary, String>,
) -> bool {
    if event_generation != current_generation {
        return false;
    }
    agents.previews.insert(id, result);
    true
}

fn apply_agent_action_done(
    agents: &mut crate::settings::agents::AgentsView,
    current_generation: u64,
    event_generation: u64,
    id: String,
    result: Result<String, String>,
) -> bool {
    if event_generation != current_generation {
        return false;
    }
    agents.busy_id = None;
    agents.messages.insert(
        id,
        match result {
            Ok(m) => crate::settings::state::Message::Ok(m),
            Err(m) => crate::settings::state::Message::Err(m),
        },
    );
    true
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum UpdateCheckSummary {
    #[default]
    NotChecked,
    Checking,
    UpToDate,
    Available(String),
    Failed(String),
}

impl UpdateCheckSummary {
    fn status_line(&self, locale: Locale) -> String {
        match self {
            Self::NotChecked => locale.text("尚未检查更新", "Not checked yet").to_owned(),
            Self::Checking => locale
                .text("正在检查更新…", "Checking for updates…")
                .to_owned(),
            Self::UpToDate => tr!(
                locale,
                "已是最新版本（v{}）",
                "Up to date (v{})",
                env!("CARGO_PKG_VERSION")
            ),
            Self::Available(v) => tr!(locale, "发现新版本 v{v}", "Update available: v{v}"),
            Self::Failed(e) => tr!(locale, "检查失败：{e}", "Update check failed: {e}"),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct UpdateTracker {
    check_generation: u64,
    download_generation: u64,
    checking: bool,
    summary: UpdateCheckSummary,
    latest_update: Option<gateterm_updater::AvailableUpdate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckTransition {
    Ignored,
    OpenWindow,
    RedrawWindow,
    NoWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StagedTransition<T> {
    Ignored,
    ReadyToApply(T),
    Failed,
}

fn prepare_manual_update_check(
    tracker: &mut UpdateTracker,
    update_view: &mut Option<crate::update_ui::UpdateView>,
    current_version: &str,
    active_sessions: usize,
) -> Option<u64> {
    tracker.check_generation = tracker.check_generation.wrapping_add(1);
    tracker.checking = true;
    tracker.summary = UpdateCheckSummary::Checking;
    *update_view = Some(crate::update_ui::UpdateView {
        current_version: current_version.to_owned(),
        update: None,
        phase: crate::update_ui::UpdatePhase::Checking,
        active_sessions,
    });
    Some(tracker.check_generation)
}

#[allow(clippy::too_many_arguments)]
fn apply_update_checked(
    tracker: &mut UpdateTracker,
    update_view: &mut Option<crate::update_ui::UpdateView>,
    event_generation: u64,
    manual: bool,
    is_startup: bool,
    current_version: &str,
    active_sessions: usize,
    locale: Locale,
    result: &Result<gateterm_updater::CheckOutcome, String>,
) -> CheckTransition {
    if manual {
        if event_generation != tracker.check_generation || update_view.is_none() {
            return CheckTransition::Ignored;
        }
        tracker.checking = false;
        match result {
            Ok(gateterm_updater::CheckOutcome::UpdateAvailable { update, .. }) => {
                tracker.latest_update = Some(update.clone());
                tracker.summary = UpdateCheckSummary::Available(update.version.clone());
                *update_view = Some(crate::update_ui::UpdateView {
                    current_version: current_version.to_owned(),
                    update: Some(update.clone()),
                    phase: crate::update_ui::UpdatePhase::Available,
                    active_sessions,
                });
            }
            Ok(gateterm_updater::CheckOutcome::UpToDate { .. }) => {
                tracker.latest_update = None;
                tracker.summary = UpdateCheckSummary::UpToDate;
                *update_view = Some(crate::update_ui::UpdateView {
                    current_version: current_version.to_owned(),
                    update: None,
                    phase: crate::update_ui::UpdatePhase::UpToDate,
                    active_sessions,
                });
            }
            Err(err_msg) => {
                tracker.summary = UpdateCheckSummary::Failed(err_msg.clone());
                let fallback_url = tracker
                    .latest_update
                    .as_ref()
                    .map(|u| u.release_url.clone())
                    .or_else(|| {
                        Some("https://github.com/CyberTianzun/Gateterm/releases/latest".to_owned())
                    });
                *update_view = Some(crate::update_ui::UpdateView {
                    current_version: current_version.to_owned(),
                    update: None,
                    phase: crate::update_ui::UpdatePhase::Error {
                        message: tr!(
                            locale,
                            "检查更新失败：{err_msg}",
                            "Update check failed: {err_msg}"
                        ),
                        fallback_url,
                    },
                    active_sessions,
                });
            }
        }
        return CheckTransition::RedrawWindow;
    }

    match result {
        Ok(gateterm_updater::CheckOutcome::UpdateAvailable { update, skipped }) => {
            tracker.latest_update = Some(update.clone());
            if !tracker.checking {
                tracker.summary = UpdateCheckSummary::Available(update.version.clone());
            }
            if is_startup && !skipped && tracker.check_generation == 0 && update_view.is_none() {
                *update_view = Some(crate::update_ui::UpdateView {
                    current_version: current_version.to_owned(),
                    update: Some(update.clone()),
                    phase: crate::update_ui::UpdatePhase::Available,
                    active_sessions,
                });
                return CheckTransition::OpenWindow;
            }
        }
        Ok(gateterm_updater::CheckOutcome::UpToDate { .. }) => {
            tracker.latest_update = None;
            if !tracker.checking {
                tracker.summary = UpdateCheckSummary::UpToDate;
            }
        }
        Err(_) => {}
    }
    CheckTransition::NoWindow
}

fn prepare_update_download(
    tracker: &mut UpdateTracker,
    update_view: &mut Option<crate::update_ui::UpdateView>,
    expected_total: u64,
) -> u64 {
    tracker.download_generation = tracker.download_generation.wrapping_add(1);
    if let Some(view) = update_view.as_mut() {
        view.phase = crate::update_ui::UpdatePhase::Downloading(
            gateterm_updater::DownloadProgress {
                downloaded: 0,
                total: expected_total,
            },
        );
    }
    tracker.download_generation
}

fn apply_download_progress(
    tracker: &UpdateTracker,
    update_view: &mut Option<crate::update_ui::UpdateView>,
    event_generation: u64,
    progress: gateterm_updater::DownloadProgress,
) -> bool {
    if event_generation != tracker.download_generation {
        return false;
    }
    let Some(view) = update_view.as_mut() else {
        return false;
    };
    if !matches!(view.phase, crate::update_ui::UpdatePhase::Downloading(_)) {
        return false;
    }
    view.phase = crate::update_ui::UpdatePhase::Downloading(progress);
    true
}

fn apply_update_staged<T>(
    tracker: &UpdateTracker,
    update_view: &mut Option<crate::update_ui::UpdateView>,
    event_generation: u64,
    result: Result<T, String>,
) -> StagedTransition<T> {
    if event_generation != tracker.download_generation {
        return StagedTransition::Ignored;
    }
    let Some(view) = update_view.as_mut() else {
        return StagedTransition::Ignored;
    };
    match result {
        Ok(staged) => {
            view.phase = crate::update_ui::UpdatePhase::Preparing;
            StagedTransition::ReadyToApply(staged)
        }
        Err(message) => {
            let fallback_url = view.update.as_ref().map(|u| u.release_url.clone());
            view.phase = crate::update_ui::UpdatePhase::Error {
                message,
                fallback_url,
            };
            StagedTransition::Failed
        }
    }
}

fn cancel_update_state(
    tracker: &mut UpdateTracker,
    update_view: &mut Option<crate::update_ui::UpdateView>,
) {
    tracker.check_generation = tracker.check_generation.wrapping_add(1);
    tracker.download_generation = tracker.download_generation.wrapping_add(1);
    tracker.checking = false;
    if tracker.summary == UpdateCheckSummary::Checking {
        tracker.summary = match &tracker.latest_update {
            Some(u) => UpdateCheckSummary::Available(u.version.clone()),
            None => UpdateCheckSummary::NotChecked,
        };
    }
    *update_view = None;
}

fn format_current_exe_error(locale: Locale, err: &std::io::Error) -> String {
    tr!(
        locale,
        "无法定位当前可执行文件路径：{err}",
        "Could not locate current executable path: {err}"
    )
}

fn format_cache_dir_error(locale: Locale, err: &anyhow::Error) -> String {
    tr!(
        locale,
        "无法定位更新缓存目录：{err:#}",
        "Could not locate update cache directory: {err:#}"
    )
}

fn format_download_error(locale: Locale, err: &gateterm_updater::DownloadError) -> String {
    use gateterm_updater::DownloadError;
    match err {
        DownloadError::NotWritable { path, source } => tr!(
            locale,
            "当前安装目录不可写（{}）：{source}\n\n请点击下方按钮前往 GitHub 手动下载安装。",
            "The installation directory is not writable ({}): {source}\n\nPlease download the update manually from GitHub.",
            path.display()
        ),
        DownloadError::Cancelled => locale.text("已取消下载。", "Download cancelled.").to_owned(),
        DownloadError::SizeMismatch { expected, actual } => tr!(
            locale,
            "安装包大小校验失败（预期 {expected} 字节，实际 {actual} 字节）。",
            "Downloaded archive size mismatch (expected {expected} bytes, got {actual} bytes)."
        ),
        DownloadError::HashMismatch { .. } => locale
            .text(
                "安装包 SHA-256 校验失败，文件可能已损坏或被篡改。",
                "Downloaded archive SHA-256 verification failed. The file may be corrupted.",
            )
            .to_owned(),
        DownloadError::UnsafeArchivePath(entry) => tr!(
            locale,
            "更新包包含不安全路径：{entry}",
            "Update archive contains an unsafe entry path: {entry}"
        ),
        DownloadError::BinaryNotFound(binary) => tr!(
            locale,
            "更新包中未找到可执行文件：{binary}",
            "Expected executable not found in update archive: {binary}"
        ),
        DownloadError::Other(inner) => tr!(
            locale,
            "下载或解压更新失败：{inner:#}",
            "Failed to download or extract update: {inner:#}"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{agents::AgentsView, state::Message};

    #[test]
    fn locale_change_clears_stale_agent_feedback_and_discards_old_generation_events() {
        let mut generation = 1u64;
        let mut agents = AgentsView {
            listen_port: Some(7683),
            install_enabled: true,
            busy_id: Some("codex".into()),
            expanded_preview: Some("pi".into()),
            ..Default::default()
        };
        agents
            .messages
            .insert("codex".into(), Message::Err("已安装，但有中文提示".into()));
        agents
            .previews
            .insert("pi".into(), Err("未知的 Agent：pi".into()));

        let rescan = apply_agents_on_save(&mut agents, true, true, false, Some(7683));
        assert!(rescan, "locale change must schedule a fresh scan");
        assert!(agents.messages.is_empty());
        assert!(agents.previews.is_empty());
        assert_eq!(agents.expanded_preview, None);

        let next_gen = prepare_agents_scan(&mut agents, &mut generation, Some(7683), true);
        assert_eq!(next_gen, 2);
        assert!(agents.loading);

        // Stale old-locale events from generation 1 must be ignored after the switch.
        assert!(!apply_agent_action_done(
            &mut agents,
            generation,
            1,
            "codex".into(),
            Err("旧语言错误".into()),
        ));
        assert!(!apply_agent_preview_ready(
            &mut agents,
            generation,
            1,
            "pi".into(),
            Err("旧语言预览失败".into()),
        ));
        assert!(agents.messages.is_empty());
        assert!(agents.previews.is_empty());

        // Matching generation 2 scan finishes cleanly and clears loading/busy_id.
        assert!(apply_agents_scanned(
            &mut agents,
            generation,
            next_gen,
            Some(7683),
            Vec::new(),
        ));
        assert!(!agents.loading);
        assert_eq!(agents.busy_id, None);
    }

    #[test]
    fn failed_save_without_restart_preserves_agent_state_and_generation() {
        let generation = 5u64;
        let mut agents = AgentsView {
            listen_port: Some(7683),
            install_enabled: true,
            expanded_preview: Some("claude-code".into()),
            ..Default::default()
        };
        agents
            .messages
            .insert("claude-code".into(), Message::Ok("Saved".into()));

        let rescan = apply_agents_on_save(&mut agents, false, false, false, Some(7683));
        assert!(!rescan);
        assert_eq!(generation, 5);
        assert_eq!(agents.expanded_preview.as_deref(), Some("claude-code"));
        assert!(agents.messages.contains_key("claude-code"));
    }

    #[test]
    fn same_locale_ok_save_clears_feedback_and_only_restarted_requests_rescan() {
        let mut agents = AgentsView {
            listen_port: Some(7683),
            install_enabled: true,
            expanded_preview: Some("claude-code".into()),
            ..Default::default()
        };
        agents
            .messages
            .insert("claude-code".into(), Message::Ok("Installed".into()));
        agents
            .previews
            .insert("claude-code".into(), Err("old preview error".into()));

        assert!(!apply_agents_on_save(
            &mut agents,
            true,
            false,
            false,
            Some(7683)
        ));
        assert!(agents.messages.is_empty());
        assert!(agents.previews.is_empty());
        assert_eq!(agents.expanded_preview, None);

        assert!(apply_agents_on_save(
            &mut agents,
            true,
            false,
            true,
            Some(9000)
        ));
        assert_eq!(agents.listen_port, Some(9000));
    }

    #[test]
    fn stale_check_download_and_staged_events_after_cancel_are_ignored() {
        let update = gateterm_updater::AvailableUpdate {
            version: "1.0.4".into(),
            tag: "release-v1.0.4".into(),
            pub_date: "2026-11-27T08:00:00Z".into(),
            release_url: "https://github.com/CyberTianzun/Gateterm/releases/tag/release-v1.0.4"
                .into(),
            notes: "Bug fixes".into(),
            artifact: Some(gateterm_updater::TargetArtifact {
                url: "https://github.com/CyberTianzun/Gateterm/releases/download/release-v1.0.4/app.zip"
                    .into(),
                sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
                size: 1024,
                format: gateterm_updater::ArchiveFormat::Zip,
                binary_path: "gateterm-desktop-web.exe".into(),
            }),
            can_self_apply: true,
        };

        let mut tracker = UpdateTracker::default();
        let mut update_view: Option<crate::update_ui::UpdateView> = None;

        // 1. Background non-startup and skipped checks must not open the window.
        assert_eq!(
            apply_update_checked(
                &mut tracker,
                &mut update_view,
                0,
                false,
                false,
                "1.0.3",
                1,
                Locale::En,
                &Ok(gateterm_updater::CheckOutcome::UpdateAvailable {
                    update: update.clone(),
                    skipped: false,
                }),
            ),
            CheckTransition::NoWindow
        );
        assert!(update_view.is_none());

        assert_eq!(
            apply_update_checked(
                &mut tracker,
                &mut update_view,
                0,
                false,
                true,
                "1.0.3",
                1,
                Locale::En,
                &Ok(gateterm_updater::CheckOutcome::UpdateAvailable {
                    update: update.clone(),
                    skipped: true,
                }),
            ),
            CheckTransition::NoWindow
        );
        assert!(update_view.is_none());

        // Startup non-skipped check opens the update window when no check has run yet.
        let mut startup_tracker = UpdateTracker::default();
        let mut startup_view = None;
        assert_eq!(
            apply_update_checked(
                &mut startup_tracker,
                &mut startup_view,
                0,
                false,
                true,
                "1.0.3",
                1,
                Locale::En,
                &Ok(gateterm_updater::CheckOutcome::UpdateAvailable {
                    update: update.clone(),
                    skipped: false,
                }),
            ),
            CheckTransition::OpenWindow
        );
        assert!(startup_view.is_some());

        // 2. Manual check starts with check_generation = 1; Cancel bumps it and closes view.
        let check_gen =
            prepare_manual_update_check(&mut tracker, &mut update_view, "1.0.3", 2).unwrap();
        assert_eq!(check_gen, 1);
        assert!(update_view.is_some());

        cancel_update_state(&mut tracker, &mut update_view);
        assert!(update_view.is_none());

        // Late check event from check_gen=1 after Cancel is ignored and does not re-open window.
        assert_eq!(
            apply_update_checked(
                &mut tracker,
                &mut update_view,
                check_gen,
                true,
                false,
                "1.0.3",
                2,
                Locale::En,
                &Ok(gateterm_updater::CheckOutcome::UpdateAvailable {
                    update: update.clone(),
                    skipped: false,
                }),
            ),
            CheckTransition::Ignored
        );
        assert!(update_view.is_none());

        // 3. Start fresh manual check -> Available -> Download -> Cancel -> stale progress & staged ignored.
        let check_gen2 =
            prepare_manual_update_check(&mut tracker, &mut update_view, "1.0.3", 2).unwrap();
        assert_eq!(
            apply_update_checked(
                &mut tracker,
                &mut update_view,
                check_gen2,
                true,
                false,
                "1.0.3",
                2,
                Locale::En,
                &Ok(gateterm_updater::CheckOutcome::UpdateAvailable {
                    update: update.clone(),
                    skipped: false,
                }),
            ),
            CheckTransition::RedrawWindow
        );

        let dl_gen = prepare_update_download(&mut tracker, &mut update_view, 1024);
        assert!(dl_gen > 0);

        cancel_update_state(&mut tracker, &mut update_view);
        assert!(update_view.is_none());

        assert!(!apply_download_progress(
            &tracker,
            &mut update_view,
            dl_gen,
            gateterm_updater::DownloadProgress {
                downloaded: 512,
                total: 1024,
            },
        ));
        assert!(matches!(
            apply_update_staged(&tracker, &mut update_view, dl_gen, Ok("staged-handle")),
            StagedTransition::Ignored
        ));
        assert!(update_view.is_none());
    }
}
