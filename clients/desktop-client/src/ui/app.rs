//! Main `GatetermWorkbench` GPUI view: manages embedded local server,
//! multi-server WebSocket/HTTP connections, integrated GPUI virtual `TitleBar`,
//! sidebar (servers, sessions, agent state badges), N-ary equal split-pane terminal
//! workbench, structured chat view, in-terminal scrollback search (`Ctrl+F`),
//! `clients/web`-style New Session popup menu, history browser, and settings modal.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use gateterm_protocol::{
    AgentState, AttachMode, ClientMsg, ServerMsg, SessionEventKind, SessionInfo,
};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, actions, div, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, TitleBar,
    button::{Button, ButtonVariant, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    resizable::{h_resizable, resizable_panel},
    scroll::ScrollableElement as _,
    v_flex,
};
use tokio::sync::mpsc;

use super::chat_view::render_chat_view;
use super::history_modal::{HistoryModalState, render_history_modal};
use super::new_session_modal::{NewSessionModalState, filtered_launchers, render_new_session_modal};
use super::pane::{SplitAxis, TabViewMode, WorkspaceTab};
use super::pending_banner::render_pending_banners;
use super::server_dialog::{ServerDialogState, render_server_dialog};
use super::settings_modal::{SettingsModalState, render_settings_modal};
use crate::client::api::{
    AgentSessionInfo, ChatWindow, GateHttpClient, LauncherItem, PendingRequestInfo,
};
use crate::client::ws::{WsCommand, WsConnectionState, WsEvent, spawn_ws_worker};
use crate::client::{
    ChatTranscriptState, apply_agent_event, apply_chat_window, best_agent_for_session,
    chat_agent_for_session, replace_all_agents,
};
use crate::config::{DesktopClientConfig, LOCAL_SERVER_ID, ServerProfile, parse_access_url};
use crate::gpui_tokio::Tokio;
use crate::i18n::{LanguagePref, Locale};
use crate::local_server::LocalServerSupervisor;
use crate::terminal::view::TerminalView;
use crate::theme::{TerminalPalette, TerminalPaletteId, apply_palette};

actions!(
    gateterm_desktop,
    [
        NewShellSession,
        OpenNewSessionMenu,
        ToggleSidebar,
        ToggleTerminalSearch,
        SplitHorizontal,
        SplitVertical,
        ToggleZoomPane,
        FocusNextPane,
        ToggleChatView,
        OpenHistoryModal,
        OpenSettingsModal,
        CloseActivePane,
    ]
);

pub struct ServerConnection {
    pub profile: ServerProfile,
    pub state: WsConnectionState,
    pub server_os: String,
    pub win32_input_mode: bool,
    pub cloud_server_id: Option<String>,
    pub cmd_tx: Option<mpsc::UnboundedSender<WsCommand>>,
    pub http: Option<GateHttpClient>,
    pub sessions: Vec<SessionInfo>,
    pub launchers: Vec<LauncherItem>,
    pub agent_sessions: HashMap<String, AgentSessionInfo>,
    pub transcript_enabled: bool,
    pub pending_requests: Vec<PendingRequestInfo>,
    pub chat_states: HashMap<u64, ChatTranscriptState>,
    pub auto_created_initial: bool,
    pub _worker_task: Option<Task<()>>,
}

impl ServerConnection {
    pub fn new(profile: ServerProfile) -> Self {
        Self {
            profile,
            state: WsConnectionState::Disconnected,
            server_os: String::new(),
            win32_input_mode: false,
            cloud_server_id: None,
            cmd_tx: None,
            http: None,
            sessions: Vec::new(),
            launchers: Vec::new(),
            agent_sessions: HashMap::new(),
            transcript_enabled: true,
            pending_requests: Vec::new(),
            chat_states: HashMap::new(),
            auto_created_initial: false,
            _worker_task: None,
        }
    }

    pub fn send_client_msg(&self, msg: ClientMsg) {
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.send(WsCommand::Control(msg));
        }
    }
}

pub struct GatetermWorkbench {
    pub focus_handle: FocusHandle,
    pub config: DesktopClientConfig,
    pub locale: Locale,
    pub servers: BTreeMap<String, ServerConnection>,
    pub active_server_id: String,
    pub local_supervisor: Option<LocalServerSupervisor>,

    pub tabs: Vec<WorkspaceTab>,
    pub active_tab_idx: usize,
    pub next_tab_id: u64,
    pub next_channel_id: u32,
    /// Map `(server_id, channel) -> Entity<TerminalView>`
    pub channel_views: HashMap<(String, u32), Entity<TerminalView>>,
    /// Pending split axis when a `NewSession` request is in-flight for `server_id`
    pub pending_create_split: HashMap<String, Option<SplitAxis>>,

    pub dismissed_pending: HashSet<String>,
    pub sidebar_collapsed: bool,
    pub terminal_search_open: bool,
    pub renaming_session: Option<(String, u64)>,

    pub new_session_modal: NewSessionModalState,
    pub history_modal: HistoryModalState,
    pub server_dialog: ServerDialogState,
    pub settings_modal: SettingsModalState,

    pub new_session_filter_input: Entity<InputState>,
    pub terminal_search_input: Entity<InputState>,
    pub rename_input: Entity<InputState>,
    pub chat_prompt_input: Entity<InputState>,
    pub history_search_input: Entity<InputState>,
    pub server_name_input: Entity<InputState>,
    pub server_url_input: Entity<InputState>,
    pub server_token_input: Entity<InputState>,
    pub font_family_input: Entity<InputState>,
    pub embedded_listen_input: Entity<InputState>,

    _subscriptions: Vec<Subscription>,
}

impl Focusable for GatetermWorkbench {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl GatetermWorkbench {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let config = DesktopClientConfig::load();
        let locale = config.language.resolve();
        apply_palette(config.palette, cx);

        let new_session_filter_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(locale.t("筛选终端或 Agent 启动项…", "Filter shells & agent launchers..."))
        });
        let terminal_search_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(locale.t("搜索终端输出 (Enter 下一个)…", "Find in terminal (Enter next)..."))
        });
        let rename_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(locale.t("新会话名称", "New session name"))
        });
        let chat_prompt_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(locale.t(
                "向此终端会话发送指令 (按回车发送)…",
                "Send prompt to this terminal session (press Enter)...",
            ))
        });
        let history_search_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(locale.t(
                "按标题、目录或模型搜索历史会话…",
                "Search history by title, directory, or model...",
            ))
        });
        let server_name_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(locale.t("例如：开发机 / GPU 节点", "e.g. Devbox / GPU Node"))
        });
        let server_url_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("http://127.0.0.1:7683/#token=...")
        });
        let server_token_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(
                locale.t(
                    "留空则自动从链接解析",
                    "Leave blank if URL includes #token=...",
                ),
            )
        });
        let font_family_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(config.font_family.clone())
                .placeholder("Cascadia Mono")
        });
        let embedded_listen_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(config.embedded_listen.clone())
                .placeholder("127.0.0.1:17683")
        });

        let mut subscriptions = Vec::new();
        subscriptions.push(cx.subscribe_in(
            &chat_prompt_input,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    if let Some(tab) = this.tabs.get(this.active_tab_idx) {
                        let (srv_id, sid) = tab.active_session_key(cx);
                        this.send_chat_prompt_to_session(&srv_id, sid, window, cx);
                    }
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &rename_input,
            window,
            |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.commit_rename_session(cx);
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &history_search_input,
            window,
            |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.refresh_history_list(false, cx);
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &new_session_filter_input,
            window,
            |this, _, event: &InputEvent, _, cx| {
                match event {
                    InputEvent::Change => {
                        this.new_session_modal.selected_index = 0;
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => {
                        this.launch_selected_from_new_session_modal(cx);
                    }
                    _ => {}
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &terminal_search_input,
            window,
            |this, input, event: &InputEvent, _, cx| {
                match event {
                    InputEvent::Change => {
                        let q = input.read(cx).value().to_string();
                        if let Some(tab) = this.tabs.get(this.active_tab_idx) {
                            let view = tab.active_terminal.clone();
                            view.update(cx, |v, cx| v.update_search_query(&q, cx));
                        }
                        cx.notify();
                    }
                    InputEvent::PressEnter { secondary } => {
                        let forward = !*secondary;
                        if let Some(tab) = this.tabs.get(this.active_tab_idx) {
                            let view = tab.active_terminal.clone();
                            view.update(cx, |v, cx| v.step_search_match(forward, cx));
                        }
                        cx.notify();
                    }
                    _ => {}
                }
            },
        ));

        let mut servers = BTreeMap::new();
        for profile in &config.servers {
            servers.insert(profile.id.clone(), ServerConnection::new(profile.clone()));
        }
        let active_server_id = if servers.contains_key(LOCAL_SERVER_ID) {
            LOCAL_SERVER_ID.to_string()
        } else {
            servers
                .keys()
                .next()
                .cloned()
                .unwrap_or_else(|| LOCAL_SERVER_ID.to_string())
        };

        let mut workbench = Self {
            focus_handle: cx.focus_handle(),
            config,
            locale,
            servers,
            active_server_id,
            local_supervisor: None,
            tabs: Vec::new(),
            active_tab_idx: 0,
            next_tab_id: 1,
            next_channel_id: 1,
            channel_views: HashMap::new(),
            pending_create_split: HashMap::new(),
            dismissed_pending: HashSet::new(),
            sidebar_collapsed: false,
            terminal_search_open: false,
            renaming_session: None,
            new_session_modal: NewSessionModalState::default(),
            history_modal: HistoryModalState::default(),
            server_dialog: ServerDialogState::default(),
            settings_modal: SettingsModalState::default(),
            new_session_filter_input,
            terminal_search_input,
            rename_input,
            chat_prompt_input,
            history_search_input,
            server_name_input,
            server_url_input,
            server_token_input,
            font_family_input,
            embedded_listen_input,
            _subscriptions: subscriptions,
        };

        workbench.initialize_connections(cx);
        workbench
    }

    fn initialize_connections(&mut self, cx: &mut Context<Self>) {
        if self.config.start_embedded_server {
            self.start_embedded_server_async(cx);
        }

        let remote_ids: Vec<String> = self
            .servers
            .values()
            .filter(|s| !s.profile.is_embedded && s.profile.auto_connect)
            .map(|s| s.profile.id.clone())
            .collect();

        for id in remote_ids {
            self.connect_server(&id, cx);
        }
    }

    pub fn start_embedded_server_async(&mut self, cx: &mut Context<Self>) {
        let listen = self.config.embedded_listen.clone();
        let token = self.config.embedded_token.clone();

        if let Some(conn) = self.servers.get_mut(LOCAL_SERVER_ID) {
            conn.state = WsConnectionState::Connecting;
        }
        cx.notify();

        let task = Tokio::spawn_result(cx, async move {
            LocalServerSupervisor::start(&listen, token).await
        });

        cx.spawn(async move |this, cx| {
            match task.await {
                Ok(supervisor) => {
                    let base_url = supervisor.base_url();
                    let token = supervisor.token().to_string();
                    let _ = this.update(cx, |this, cx| {
                        this.local_supervisor = Some(supervisor);
                        if let Some(conn) = this.servers.get_mut(LOCAL_SERVER_ID) {
                            conn.profile.base_url = base_url;
                            conn.profile.token = token;
                        } else {
                            let mut profile = ServerProfile::embedded_default();
                            profile.base_url = base_url;
                            profile.token = token;
                            this.servers
                                .insert(LOCAL_SERVER_ID.to_string(), ServerConnection::new(profile));
                        }
                        this.connect_server(LOCAL_SERVER_ID, cx);
                    });
                }
                Err(err) => {
                    tracing::error!("Failed to start embedded gateterm-server: {err:#}");
                    let _ = this.update(cx, |this, cx| {
                        if let Some(conn) = this.servers.get_mut(LOCAL_SERVER_ID) {
                            conn.state =
                                WsConnectionState::Error(format!("Embedded server error: {err}"));
                        }
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    pub fn connect_server(&mut self, server_id: &str, cx: &mut Context<Self>) {
        let Some(conn) = self.servers.get_mut(server_id) else {
            return;
        };
        if let Some(old_tx) = conn.cmd_tx.take() {
            let _ = old_tx.send(WsCommand::Disconnect);
        }

        let profile = conn.profile.clone();
        conn.http = Some(GateHttpClient::new(
            profile.http_base_url(),
            profile.token.clone(),
        ));

        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();

        let handle = Tokio::handle(cx);
        let _guard = handle.enter();
        let _join = spawn_ws_worker(profile.ws_url(), profile.token.clone(), cmd_rx, ev_tx);

        conn.cmd_tx = Some(cmd_tx);
        conn.state = WsConnectionState::Connecting;

        let srv_id = profile.id.clone();
        let worker_task = cx.spawn(async move |this, cx| {
            while let Some(evt) = ev_rx.recv().await {
                let id_clone = srv_id.clone();
                let Ok(()) = this.update(cx, |this, cx| {
                    this.handle_ws_event(id_clone, evt, cx);
                }) else {
                    break;
                };
            }
        });
        conn._worker_task = Some(worker_task);
        cx.notify();
    }

    pub fn disconnect_server(&mut self, server_id: &str, cx: &mut Context<Self>) {
        if let Some(conn) = self.servers.get_mut(server_id) {
            if let Some(tx) = conn.cmd_tx.take() {
                let _ = tx.send(WsCommand::Disconnect);
            }
            conn._worker_task = None;
            conn.state = WsConnectionState::Disconnected;
            cx.notify();
        }
    }

    fn handle_ws_event(&mut self, server_id: String, event: WsEvent, cx: &mut Context<Self>) {
        match event {
            WsEvent::Connecting => {
                if let Some(conn) = self.servers.get_mut(&server_id) {
                    conn.state = WsConnectionState::Connecting;
                }
                cx.notify();
            }
            WsEvent::Reconnecting => {
                if let Some(conn) = self.servers.get_mut(&server_id) {
                    conn.state = WsConnectionState::Reconnecting;
                }
                cx.notify();
            }
            WsEvent::Disconnected => {
                if let Some(conn) = self.servers.get_mut(&server_id) {
                    conn.state = WsConnectionState::Disconnected;
                }
                cx.notify();
            }
            WsEvent::FatalAuthError(err) => {
                if let Some(conn) = self.servers.get_mut(&server_id) {
                    conn.state = WsConnectionState::Error(err);
                }
                cx.notify();
            }
            WsEvent::Connected {
                os,
                win32_input_mode,
                cloud_server_id,
            } => {
                if let Some(conn) = self.servers.get_mut(&server_id) {
                    conn.state = WsConnectionState::Connected;
                    conn.server_os = os.clone();
                    conn.win32_input_mode = win32_input_mode;
                    conn.cloud_server_id = cloud_server_id;
                }
                for ((srv, _), view) in &self.channel_views {
                    if srv == &server_id {
                        let os_clone = os.clone();
                        view.update(cx, |v, _| {
                            v.server_os = os_clone;
                            v.win32_input_mode = win32_input_mode;
                        });
                    }
                }
                self.refresh_server_aux_data(&server_id, cx);
                cx.notify();
            }
            WsEvent::ChannelData { channel, payload } => {
                if let Some(view) = self.channel_views.get(&(server_id, channel)).cloned() {
                    view.update(cx, |v, cx| {
                        v.write_output(&payload, cx);
                    });
                }
            }
            WsEvent::Control(msg) => {
                self.handle_server_control_msg(server_id, msg, cx);
            }
        }
    }

    fn handle_server_control_msg(
        &mut self,
        server_id: String,
        msg: ServerMsg,
        cx: &mut Context<Self>,
    ) {
        match msg {
            ServerMsg::Sessions { sessions } => {
                let mut should_create_initial = false;
                let mut first_session_to_open: Option<u64> = None;

                if let Some(conn) = self.servers.get_mut(&server_id) {
                    conn.sessions = sessions;
                    if server_id == self.active_server_id && self.tabs.is_empty() {
                        if let Some(first) = conn.sessions.first() {
                            first_session_to_open = Some(first.id);
                        } else if !conn.auto_created_initial {
                            conn.auto_created_initial = true;
                            should_create_initial = true;
                        }
                    }
                }

                if let Some(sid) = first_session_to_open {
                    self.open_session_in_new_tab(&server_id, sid, cx);
                } else if should_create_initial {
                    self.create_new_session(&server_id, None, None, None, None, cx);
                }
                cx.notify();
            }
            ServerMsg::SessionEvent { event, session } => {
                let mut newly_created_id = None;
                if let Some(conn) = self.servers.get_mut(&server_id) {
                    match event {
                        SessionEventKind::Created => {
                            if !conn.sessions.iter().any(|s| s.id == session.id) {
                                conn.sessions.push(session.clone());
                            }
                            newly_created_id = Some(session.id);
                        }
                        SessionEventKind::Renamed => {
                            if let Some(existing) =
                                conn.sessions.iter_mut().find(|s| s.id == session.id)
                            {
                                existing.name = session.name.clone();
                            }
                            for ((srv, _), view) in &self.channel_views {
                                if srv == &server_id && view.read(cx).session_id == session.id {
                                    let new_name = session.name.clone();
                                    view.update(cx, |v, cx| {
                                        v.session_name = new_name;
                                        cx.notify();
                                    });
                                }
                            }
                        }
                        SessionEventKind::Closed => {
                            conn.sessions.retain(|s| s.id != session.id);
                        }
                    }
                }

                if let Some(sid) = newly_created_id {
                    let split = self.pending_create_split.remove(&server_id).flatten();
                    if let Some(axis) = split {
                        self.split_active_tab_with_session(&server_id, sid, axis, cx);
                    } else if server_id == self.active_server_id {
                        let already_attached = self
                            .channel_views
                            .iter()
                            .any(|((srv, _), v)| srv == &server_id && v.read(cx).session_id == sid);
                        if !already_attached {
                            self.open_session_in_new_tab(&server_id, sid, cx);
                        }
                    }
                }
                cx.notify();
            }
            ServerMsg::Attached { channel, mode, .. } => {
                if mode == AttachMode::Resync {
                    if let Some(view) = self.channel_views.get(&(server_id, channel)).cloned() {
                        view.update(cx, |v, cx| {
                            v.reset_terminal(cx);
                        });
                    }
                }
            }
            ServerMsg::Resized {
                channel,
                cols,
                rows,
            } => {
                if let Some(view) = self.channel_views.get(&(server_id, channel)).cloned() {
                    view.update(cx, |v, cx| {
                        v.apply_authoritative_resize(cols, rows, cx);
                    });
                }
            }
            ServerMsg::ResyncBegin { channel } => {
                if let Some(view) = self.channel_views.get(&(server_id, channel)).cloned() {
                    view.update(cx, |v, cx| {
                        v.reset_terminal(cx);
                    });
                }
            }
            ServerMsg::ResyncEnd { .. } | ServerMsg::PreviewData { .. } => {}
            ServerMsg::SessionExited { channel, .. } => {
                if let Some(view) = self.channel_views.get(&(server_id, channel)).cloned() {
                    view.update(cx, |v, cx| {
                        v.mark_exited(cx);
                    });
                }
                cx.notify();
            }
            ServerMsg::AgentEvent {
                agent_id,
                agent_session_id,
                session,
                state,
                pending,
                detail,
            } => {
                let mut need_pending_refresh = pending.is_some();
                if let Some(conn) = self.servers.get_mut(&server_id) {
                    apply_agent_event(
                        &mut conn.agent_sessions,
                        AgentSessionInfo {
                            agent_id,
                            agent_session_id,
                            session,
                            state,
                            detail,
                            pending,
                            supports_transcript: true,
                        },
                    );
                    if state != AgentState::Waiting {
                        need_pending_refresh = true;
                    }
                }
                if need_pending_refresh {
                    self.refresh_pending_requests(&server_id, cx);
                }
                if let Some(sid) = session {
                    if let Some(tab) = self.tabs.get(self.active_tab_idx) {
                        if tab.view_mode == TabViewMode::Chat
                            && tab.active_session_key(cx) == (server_id.clone(), sid)
                        {
                            self.fetch_chat_messages(server_id, sid, false, cx);
                        }
                    }
                }
                cx.notify();
            }
            ServerMsg::Error { message, .. } => {
                tracing::warn!("Server error from {server_id}: {message}");
            }
            ServerMsg::HelloOk { .. } => {}
        }
    }

    pub fn refresh_server_aux_data(&mut self, server_id: &str, cx: &mut Context<Self>) {
        let Some(http) = self.servers.get(server_id).and_then(|s| s.http.clone()) else {
            return;
        };
        let srv_id = server_id.to_string();
        let task = Tokio::spawn_result(cx, async move {
            let launchers = http.fetch_launchers().await.unwrap_or_default();
            let agents = http.fetch_agent_sessions().await.unwrap_or_default();
            let pending = http.fetch_pending().await.unwrap_or_default();
            Ok((launchers, agents, pending))
        });

        cx.spawn(async move |this, cx| {
            if let Ok((launchers, agents_resp, pending)) = task.await {
                let _ = this.update(cx, |this, cx| {
                    if let Some(conn) = this.servers.get_mut(&srv_id) {
                        conn.launchers = launchers;
                        conn.transcript_enabled = agents_resp.transcript_enabled;
                        replace_all_agents(&mut conn.agent_sessions, agents_resp.sessions);
                        conn.pending_requests = pending;
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn refresh_pending_requests(&mut self, server_id: &str, cx: &mut Context<Self>) {
        let Some(http) = self.servers.get(server_id).and_then(|s| s.http.clone()) else {
            return;
        };
        let srv_id = server_id.to_string();
        let task = Tokio::spawn_result(cx, async move { http.fetch_pending().await });
        cx.spawn(async move |this, cx| {
            if let Ok(pending) = task.await {
                let _ = this.update(cx, |this, cx| {
                    if let Some(conn) = this.servers.get_mut(&srv_id) {
                        conn.pending_requests = pending;
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn create_new_session(
        &mut self,
        server_id: &str,
        name: Option<String>,
        command: Option<Vec<String>>,
        cwd: Option<String>,
        split: Option<SplitAxis>,
        cx: &mut Context<Self>,
    ) {
        self.pending_create_split
            .insert(server_id.to_string(), split);
        if let Some(conn) = self.servers.get(server_id) {
            conn.send_client_msg(ClientMsg::NewSession { name, command, cwd });
        }
        self.new_session_modal.open = false;
        cx.notify();
    }

    pub fn open_new_session_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_session_modal.open = true;
        self.new_session_modal.selected_index = 0;
        self.new_session_filter_input.update(cx, |s, cx| {
            s.set_value("", window, cx);
        });
        let fh = self.new_session_filter_input.read(cx).focus_handle(cx);
        window.focus(&fh, cx);
        cx.notify();
    }

    pub fn launch_selected_from_new_session_modal(&mut self, cx: &mut Context<Self>) {
        if !self.new_session_modal.open {
            return;
        }
        let srv_id = self.active_server_id.clone();
        let query = self.new_session_filter_input.read(cx).value().to_string();
        let items = self
            .servers
            .get(&srv_id)
            .map(|c| filtered_launchers(&c.launchers, &query))
            .unwrap_or_default();
        self.new_session_modal.open = false;
        if let Some(item) = items
            .get(self.new_session_modal.selected_index)
            .or_else(|| items.first())
        {
            let name = Some(item.name.clone());
            let cmd = (!item.command.is_empty()).then(|| item.command.clone());
            let cwd = item.cwd.clone();
            self.create_new_session(&srv_id, name, cmd, cwd, None, cx);
        } else {
            self.create_new_session(&srv_id, None, None, None, None, cx);
        }
    }

    pub fn toggle_terminal_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.terminal_search_open = !self.terminal_search_open;
        if self.terminal_search_open {
            let fh = self.terminal_search_input.read(cx).focus_handle(cx);
            window.focus(&fh, cx);
            let q = self.terminal_search_input.read(cx).value().to_string();
            if let Some(tab) = self.tabs.get(self.active_tab_idx) {
                let view = tab.active_terminal.clone();
                view.update(cx, |v, cx| v.update_search_query(&q, cx));
            }
        } else {
            if let Some(tab) = self.tabs.get(self.active_tab_idx) {
                let view = tab.active_terminal.clone();
                view.update(cx, |v, cx| v.clear_search(cx));
                tab.focus_active(window, cx);
            }
        }
        cx.notify();
    }

    fn spawn_terminal_view_for_session(
        &mut self,
        server_id: &str,
        session_id: u64,
        cx: &mut Context<Self>,
    ) -> Option<Entity<TerminalView>> {
        let conn = self.servers.get(server_id)?;
        let cmd_tx = conn.cmd_tx.clone()?;
        let session_name = conn
            .sessions
            .iter()
            .find(|s| s.id == session_id)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| format!("session-{session_id}"));
        let server_os = conn.server_os.clone();
        let win32_input_mode = conn.win32_input_mode;

        let channel = self.next_channel_id;
        self.next_channel_id = self.next_channel_id.wrapping_add(1).max(1);

        let tx_in = cmd_tx.clone();
        let on_input: crate::terminal::view::InputCallback = Rc::new(move |bytes, _cx| {
            let _ = tx_in.send(WsCommand::Input {
                channel,
                bytes: bytes.to_vec(),
            });
        });

        let palette_id = self.config.palette;
        let tx_resize = cmd_tx.clone();
        let on_resize: crate::terminal::view::ResizeCallback = Rc::new(move |cols, rows, _cx| {
            let colors = Some(TerminalPalette::get(palette_id).to_term_colors());
            let _ = tx_resize.send(WsCommand::Resize {
                channel,
                cols,
                rows,
                colors,
            });
        });

        let font_family = self.config.font_family.clone();
        let font_size = self.config.font_size;
        let line_height = self.config.line_height;
        let max_scrollback = self.config.max_scrollback;
        let srv_id = server_id.to_string();

        let view = cx.new(|cx| {
            TerminalView::new(
                srv_id,
                session_id,
                session_name,
                channel,
                server_os,
                win32_input_mode,
                font_family,
                font_size,
                line_height,
                max_scrollback,
                cx,
            )
            .with_callbacks(on_input, on_resize)
        });

        self.channel_views
            .insert((server_id.to_string(), channel), view.clone());

        let colors = Some(TerminalPalette::get(self.config.palette).to_term_colors());
        let _ = cmd_tx.send(WsCommand::Attach {
            session_id,
            channel,
            cols: 100,
            rows: 30,
            colors,
        });

        Some(view)
    }

    pub fn open_session_in_new_tab(
        &mut self,
        server_id: &str,
        session_id: u64,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.spawn_terminal_view_for_session(server_id, session_id, cx) else {
            return;
        };
        let tab_id = self.next_tab_id;
        self.next_tab_id += 1;
        self.tabs.push(WorkspaceTab::new(tab_id, view));
        self.active_tab_idx = self.tabs.len() - 1;
        cx.notify();
    }

    pub fn split_active_tab_with_session(
        &mut self,
        server_id: &str,
        session_id: u64,
        axis: SplitAxis,
        cx: &mut Context<Self>,
    ) {
        if self.tabs.is_empty() {
            self.open_session_in_new_tab(server_id, session_id, cx);
            return;
        }
        let Some(new_view) = self.spawn_terminal_view_for_session(server_id, session_id, cx) else {
            return;
        };
        if let Some(tab) = self.tabs.get_mut(self.active_tab_idx) {
            let target = tab.active_terminal.clone();
            if tab.root.split_target(&target, new_view.clone(), axis) {
                tab.active_terminal = new_view;
                tab.zoomed_terminal = None;
                tab.view_mode = TabViewMode::Terminal;
            }
        }
        cx.notify();
    }

    pub fn attach_or_focus_session(
        &mut self,
        server_id: &str,
        session_id: u64,
        split: Option<SplitAxis>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(axis) = split {
            self.split_active_tab_with_session(server_id, session_id, axis, cx);
            if let Some(tab) = self.tabs.get(self.active_tab_idx) {
                tab.focus_active(window, cx);
            }
            return;
        }

        for (idx, tab) in self.tabs.iter_mut().enumerate() {
            if let Some(found) = tab.root.terminals().into_iter().find(|v| {
                let term = v.read(cx);
                term.server_id == server_id && term.session_id == session_id
            }) {
                self.active_tab_idx = idx;
                tab.active_terminal = found;
                tab.focus_active(window, cx);
                cx.notify();
                return;
            }
        }

        self.open_session_in_new_tab(server_id, session_id, cx);
        if let Some(tab) = self.tabs.get(self.active_tab_idx) {
            tab.focus_active(window, cx);
        }
    }

    pub fn close_pane_or_tab(
        &mut self,
        tab_idx: usize,
        target_opt: Option<Entity<TerminalView>>,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.get_mut(tab_idx) else {
            return;
        };
        let target = target_opt.unwrap_or_else(|| tab.active_terminal.clone());

        let detach_view = |view: &Entity<TerminalView>,
                           servers: &BTreeMap<String, ServerConnection>,
                           channel_views: &mut HashMap<(String, u32), Entity<TerminalView>>,
                           cx: &App| {
            let term = view.read(cx);
            let srv = term.server_id.clone();
            let ch = term.channel;
            channel_views.remove(&(srv.clone(), ch));
            if let Some(conn) = servers.get(&srv) {
                if let Some(tx) = &conn.cmd_tx {
                    let _ = tx.send(WsCommand::Detach { channel: ch });
                }
            }
        };

        match tab.root.remove_terminal(&target) {
            Ok(true) => {
                detach_view(&target, &self.servers, &mut self.channel_views, cx);
                tab.active_terminal = tab.root.first_terminal();
                tab.zoomed_terminal = None;
            }
            Err(()) => {
                let removed_tab = self.tabs.remove(tab_idx);
                for v in removed_tab.root.terminals() {
                    detach_view(&v, &self.servers, &mut self.channel_views, cx);
                }
                if self.active_tab_idx >= self.tabs.len() && !self.tabs.is_empty() {
                    self.active_tab_idx = self.tabs.len() - 1;
                }
            }
            Ok(false) => {}
        }
        cx.notify();
    }

    pub fn kill_session(&mut self, server_id: &str, session_id: u64, cx: &mut Context<Self>) {
        if let Some(conn) = self.servers.get(server_id) {
            conn.send_client_msg(ClientMsg::KillSession { id: session_id });
        }
        let matching_views: Vec<Entity<TerminalView>> = self
            .channel_views
            .values()
            .filter(|v| {
                let t = v.read(cx);
                t.server_id == server_id && t.session_id == session_id
            })
            .cloned()
            .collect();
        for view in matching_views {
            if let Some(idx) = self
                .tabs
                .iter()
                .position(|t| t.root.contains_terminal(&view))
            {
                self.close_pane_or_tab(idx, Some(view), cx);
            }
        }
        cx.notify();
    }

    pub fn start_rename_session(
        &mut self,
        server_id: String,
        session_id: u64,
        current_name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.renaming_session = Some((server_id, session_id));
        self.rename_input.update(cx, |state, cx| {
            state.set_value(current_name, window, cx);
        });
        cx.notify();
    }

    pub fn commit_rename_session(&mut self, cx: &mut Context<Self>) {
        let Some((server_id, session_id)) = self.renaming_session.take() else {
            return;
        };
        let new_name = self.rename_input.read(cx).value().trim().to_string();
        if !new_name.is_empty() {
            if let Some(conn) = self.servers.get(&server_id) {
                conn.send_client_msg(ClientMsg::RenameSession {
                    id: session_id,
                    name: new_name,
                });
            }
        }
        cx.notify();
    }

    pub fn answer_pending_request(
        &mut self,
        server_id: String,
        req_id: String,
        decision: &'static str,
        cx: &mut Context<Self>,
    ) {
        let Some(http) = self.servers.get(&server_id).and_then(|s| s.http.clone()) else {
            return;
        };
        let req_id_remove = req_id.clone();
        let srv_id = server_id.clone();
        let task = Tokio::spawn_result(cx, async move {
            http.answer_pending(&req_id, decision, None, None).await
        });

        cx.spawn(async move |this, cx| {
            let _ = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(conn) = this.servers.get_mut(&srv_id) {
                    conn.pending_requests.retain(|r| r.id != req_id_remove);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn set_active_tab_view_mode(
        &mut self,
        mode: TabViewMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut fetch_target = None;
        if let Some(tab) = self.tabs.get_mut(self.active_tab_idx) {
            tab.sync_active_from_focus(window, cx);
            tab.view_mode = mode;
            if mode == TabViewMode::Chat {
                fetch_target = Some(tab.active_session_key(cx));
            } else {
                tab.focus_active(window, cx);
            }
        }
        if let Some((srv_id, sid)) = fetch_target {
            self.fetch_chat_messages(srv_id, sid, false, cx);
        }
        cx.notify();
    }

    pub fn fetch_chat_messages(
        &mut self,
        server_id: String,
        session_id: u64,
        older: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(conn) = self.servers.get_mut(&server_id) else {
            return;
        };
        let Some(http) = conn.http.clone() else {
            return;
        };
        let Some(agent) = chat_agent_for_session(&conn.agent_sessions, session_id) else {
            let st = conn.chat_states.entry(session_id).or_default();
            st.fallback_reason = Some("no-transcript-path".to_string());
            cx.notify();
            return;
        };

        let before = if older {
            conn.chat_states
                .get(&session_id)
                .and_then(|st| st.older_cursor.clone())
        } else {
            None
        };
        let after = if !older {
            conn.chat_states
                .get(&session_id)
                .map(|st| st.cursor.clone())
                .filter(|c| !c.is_empty())
        } else {
            None
        };

        let agent_id = agent.agent_id.clone();
        let agent_session_id = agent.agent_session_id.clone();

        let task = Tokio::spawn_result(cx, async move {
            Ok(http
                .fetch_messages(
                    &agent_id,
                    &agent_session_id,
                    after.as_deref(),
                    before.as_deref(),
                )
                .await)
        });

        cx.spawn(async move |this, cx| {
            if let Ok(window_data) = task.await {
                let _ = this.update(cx, |this, cx| {
                    if let Some(conn) = this.servers.get_mut(&server_id) {
                        let st = conn.chat_states.entry(session_id).or_default();
                        apply_chat_window(st, window_data, older);
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn send_chat_prompt_to_session(
        &mut self,
        server_id: &str,
        session_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self.chat_prompt_input.read(cx).value().to_string();
        if text.trim().is_empty() {
            return;
        }
        self.chat_prompt_input.update(cx, |state, cx| {
            state.set_value("", window, cx);
        });

        if let Some(view) = self
            .channel_views
            .values()
            .find(|v| {
                let t = v.read(cx);
                t.server_id == server_id && t.session_id == session_id
            })
            .cloned()
        {
            let mut payload = text.into_bytes();
            payload.push(b'\r');
            view.update(cx, |v, cx| {
                v.send_bytes(&payload, cx);
            });
        }
    }

    pub fn open_history_modal(&mut self, cx: &mut Context<Self>) {
        self.history_modal.open = true;
        self.history_modal.server_id = self.active_server_id.clone();
        self.refresh_history_list(false, cx);
        cx.notify();
    }

    pub fn refresh_history_list(&mut self, append: bool, cx: &mut Context<Self>) {
        let srv_id = if self.history_modal.server_id.is_empty() {
            self.active_server_id.clone()
        } else {
            self.history_modal.server_id.clone()
        };
        let Some(http) = self.servers.get(&srv_id).and_then(|s| s.http.clone()) else {
            return;
        };
        let query = self
            .history_search_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        let agent_filter = self.history_modal.agent_filter.clone().unwrap_or_default();
        let offset = if append {
            self.history_modal.next_offset.unwrap_or(0)
        } else {
            0
        };
        self.history_modal.loading = true;
        cx.notify();

        let task = Tokio::spawn_result(cx, async move {
            http.fetch_history(&query, &agent_filter, offset, !append)
                .await
        });

        cx.spawn(async move |this, cx| {
            match task.await {
                Ok(resp) => {
                    let _ = this.update(cx, |this, cx| {
                        this.history_modal.loading = false;
                        this.history_modal.agents = resp.agents;
                        this.history_modal.next_offset = resp.next_offset;
                        if append {
                            this.history_modal.entries.extend(resp.sessions);
                        } else {
                            this.history_modal.entries = resp.sessions;
                        }
                        cx.notify();
                    });
                }
                Err(err) => {
                    let _ = this.update(cx, |this, cx| {
                        this.history_modal.loading = false;
                        this.history_modal.error = Some(err.to_string());
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    pub fn select_history_entry(
        &mut self,
        agent_id: String,
        agent_session_id: String,
        cx: &mut Context<Self>,
    ) {
        self.history_modal.selected_key = Some((agent_id.clone(), agent_session_id.clone()));
        self.history_modal.preview_loading = true;
        self.history_modal.preview_messages.clear();
        cx.notify();

        let srv_id = self.history_modal.server_id.clone();
        let Some(http) = self.servers.get(&srv_id).and_then(|s| s.http.clone()) else {
            return;
        };
        let task = Tokio::spawn_result(cx, async move {
            http.fetch_history_messages(&agent_id, &agent_session_id)
                .await
        });

        cx.spawn(async move |this, cx| {
            if let Ok(ChatWindow::Transcript { messages, .. }) = task.await {
                let _ = this.update(cx, |this, cx| {
                    this.history_modal.preview_loading = false;
                    this.history_modal.preview_messages = messages;
                    cx.notify();
                });
            } else {
                let _ = this.update(cx, |this, cx| {
                    this.history_modal.preview_loading = false;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn open_history_entry(
        &mut self,
        agent_id: String,
        agent_session_id: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let srv_id = self.history_modal.server_id.clone();
        let Some(http) = self.servers.get(&srv_id).and_then(|s| s.http.clone()) else {
            return;
        };
        let task = Tokio::spawn_result(cx, async move {
            http.open_history(&agent_id, &agent_session_id).await
        });

        cx.spawn(async move |this, cx| {
            if let Ok(resp) = task.await {
                let _ = this.update(cx, |this, cx| {
                    this.history_modal.open = false;
                    if let Some(conn) = this.servers.get_mut(&srv_id) {
                        if !conn.sessions.iter().any(|s| s.id == resp.session.id) {
                            conn.sessions.push(resp.session.clone());
                        }
                    }
                    this.open_session_in_new_tab(&srv_id, resp.session.id, cx);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn open_add_server_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.server_dialog.open = true;
        self.server_dialog.editing_id = None;
        self.server_dialog.error = None;
        self.server_name_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.server_url_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.server_token_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        cx.notify();
    }

    pub fn save_server_dialog(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let name = self.server_name_input.read(cx).value().trim().to_string();
        let raw_url = self.server_url_input.read(cx).value().trim().to_string();
        let raw_token = self.server_token_input.read(cx).value().trim().to_string();

        if raw_url.is_empty() {
            self.server_dialog.error = Some(
                self.locale
                    .t("请输入服务器 URL", "Please enter a server URL")
                    .to_string(),
            );
            cx.notify();
            return;
        }

        let (base_url, parsed_token, host_display) =
            parse_access_url(&raw_url).unwrap_or((raw_url.clone(), None, raw_url.clone()));
        let token = if !raw_token.is_empty() {
            raw_token
        } else {
            parsed_token.unwrap_or_default()
        };

        let display_name = if name.is_empty() {
            host_display
        } else {
            name
        };

        let id = self.server_dialog.editing_id.clone().unwrap_or_else(|| {
            format!(
                "srv-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(1)
            )
        });

        let profile = ServerProfile {
            id: id.clone(),
            name: display_name,
            base_url,
            token,
            is_embedded: false,
            auto_connect: true,
        };

        self.config.upsert_server(profile.clone());
        let _ = self.config.save();
        self.servers
            .insert(id.clone(), ServerConnection::new(profile));
        self.active_server_id = id.clone();
        self.server_dialog.open = false;
        self.connect_server(&id, cx);
        cx.notify();
    }

    pub fn remove_server(&mut self, server_id: &str, cx: &mut Context<Self>) {
        if server_id == LOCAL_SERVER_ID {
            return;
        }
        self.disconnect_server(server_id, cx);
        self.servers.remove(server_id);
        self.config.remove_server(server_id);
        let _ = self.config.save();
        if self.active_server_id == server_id {
            self.active_server_id = LOCAL_SERVER_ID.to_string();
        }
        cx.notify();
    }

    pub fn open_settings_modal(&mut self, cx: &mut Context<Self>) {
        self.settings_modal.open = true;
        self.refresh_settings_hooks_and_cloud(cx);
        cx.notify();
    }

    pub fn refresh_settings_hooks_and_cloud(&mut self, cx: &mut Context<Self>) {
        let Some(http) = self
            .servers
            .get(&self.active_server_id)
            .and_then(|s| s.http.clone())
        else {
            return;
        };
        self.settings_modal.hooks_loading = true;
        let task = Tokio::spawn_result(cx, async move {
            let hooks = http.fetch_agents_status().await.unwrap_or_default();
            let cloud = http.fetch_cloud_status().await.ok();
            Ok((hooks, cloud))
        });

        cx.spawn(async move |this, cx| {
            if let Ok((hooks, cloud)) = task.await {
                let _ = this.update(cx, |this, cx| {
                    this.settings_modal.hooks_loading = false;
                    this.settings_modal.agent_hooks = hooks;
                    this.settings_modal.cloud_status = cloud;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn manage_agent_hook(
        &mut self,
        agent_id: String,
        action: &'static str,
        cx: &mut Context<Self>,
    ) {
        let Some(http) = self
            .servers
            .get(&self.active_server_id)
            .and_then(|s| s.http.clone())
        else {
            return;
        };
        let task = Tokio::spawn_result(cx, async move {
            http.mutate_agent_hook(&agent_id, action).await
        });

        cx.spawn(async move |this, cx| {
            match task.await {
                Ok(()) => {
                    let _ = this.update(cx, |this, cx| {
                        this.settings_modal.hooks_message = Some(
                            this.locale
                                .t("钩子操作已生效", "Hook operation succeeded")
                                .to_string(),
                        );
                        this.refresh_settings_hooks_and_cloud(cx);
                        cx.notify();
                    });
                }
                Err(err) => {
                    let _ = this.update(cx, |this, cx| {
                        this.settings_modal.hooks_message = Some(err.to_string());
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    pub fn update_palette(&mut self, palette: TerminalPaletteId, cx: &mut Context<Self>) {
        self.config.palette = palette;
        let _ = self.config.save();
        apply_palette(palette, cx);
        cx.notify();
    }

    pub fn update_language(&mut self, lang: LanguagePref, cx: &mut Context<Self>) {
        self.config.language = lang;
        self.locale = lang.resolve();
        let _ = self.config.save();
        cx.notify();
    }

    pub fn apply_font_family_from_input(&mut self, cx: &mut Context<Self>) {
        let val = self.font_family_input.read(cx).value().trim().to_string();
        if val.is_empty() {
            return;
        }
        self.config.font_family = val;
        let _ = self.config.save();
        self.sync_font_settings_to_views(cx);
    }

    pub fn adjust_font_size(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.config.font_size = (self.config.font_size + delta).clamp(10.0, 32.0);
        let _ = self.config.save();
        self.sync_font_settings_to_views(cx);
    }

    pub fn adjust_line_height(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.config.line_height = (self.config.line_height + delta).clamp(1.0, 2.0);
        let _ = self.config.save();
        self.sync_font_settings_to_views(cx);
    }

    fn sync_font_settings_to_views(&mut self, cx: &mut Context<Self>) {
        let family = self.config.font_family.clone();
        let size = self.config.font_size;
        let lh = self.config.line_height;
        for view in self.channel_views.values() {
            let fam = family.clone();
            view.update(cx, |v, cx| {
                v.set_font(fam, size, lh, cx);
            });
        }
        cx.notify();
    }

    pub fn toggle_embedded_server(&mut self, cx: &mut Context<Self>) {
        self.config.start_embedded_server = !self.config.start_embedded_server;
        let _ = self.config.save();
        if self.config.start_embedded_server {
            self.start_embedded_server_async(cx);
        } else {
            self.disconnect_server(LOCAL_SERVER_ID, cx);
            if let Some(mut sup) = self.local_supervisor.take() {
                sup.stop();
            }
        }
        cx.notify();
    }

    pub fn save_and_restart_embedded_server(&mut self, cx: &mut Context<Self>) {
        let listen = self
            .embedded_listen_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        if !listen.is_empty() {
            self.config.embedded_listen = listen;
            let _ = self.config.save();
        }
        self.disconnect_server(LOCAL_SERVER_ID, cx);
        if let Some(mut sup) = self.local_supervisor.take() {
            sup.stop();
        }
        if self.config.start_embedded_server {
            self.start_embedded_server_async(cx);
        }
    }

    fn render_title_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let locale = self.locale;
        let active_tab = self.tabs.get(self.active_tab_idx);
        let is_chat_mode = active_tab.is_some_and(|t| t.view_mode == TabViewMode::Chat);
        let is_zoomed = active_tab.is_some_and(|t| t.zoomed_terminal.is_some());
        let has_splits = active_tab.is_some_and(|t| t.root.terminals().len() > 1);
        let sidebar_collapsed = self.sidebar_collapsed;
        let search_open = self.terminal_search_open;

        let conn = self.servers.get(&self.active_server_id);
        let dot_color = match conn.map(|c| &c.state) {
            Some(WsConnectionState::Connected) => cx.theme().success,
            Some(WsConnectionState::Connecting | WsConnectionState::Reconnecting) => {
                cx.theme().warning
            }
            Some(WsConnectionState::Error(_)) => cx.theme().danger,
            _ => cx.theme().muted_foreground,
        };
        let srv_name: SharedString = conn
            .map(|c| c.profile.name.clone())
            .unwrap_or_else(|| "Gateterm".to_string())
            .into();

        let active_session_title: SharedString = active_tab
            .map(|t| t.title(cx))
            .unwrap_or_else(|| "Gateterm".to_string())
            .into();

        let srv_split_h = self.active_server_id.clone();
        let srv_split_v = self.active_server_id.clone();

        TitleBar::new()
            .child(
                h_flex()
                    .w_full()
                    .h_full()
                    .items_center()
                    .justify_between()
                    .pr_2()
                    .child(
                        // Left: Sidebar toggle button (outside drag region)
                        div()
                            .id("titlebar-left-actions")
                            .occlude()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .child(
                                Button::new("titlebar-toggle-sidebar")
                                    .xsmall()
                                    .with_variant(if sidebar_collapsed {
                                        ButtonVariant::Secondary
                                    } else {
                                        ButtonVariant::Ghost
                                    })
                                    .icon(IconName::PanelLeft)
                                    .tooltip(locale.t(
                                        "切换侧边栏 (Ctrl+B)",
                                        "Toggle Sidebar (Ctrl+B)",
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.sidebar_collapsed = !this.sidebar_collapsed;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        // Middle: Dedicated window drag area (never overlaps interactive buttons)
                        h_flex()
                            .id("titlebar-drag-area")
                            .flex_1()
                            .h_full()
                            .min_w(px(32.0))
                            .items_center()
                            .gap_1p5()
                            .px_2()
                            .window_control_area(gpui::WindowControlArea::Drag)
                            .child(div().h(px(14.0)).w(px(1.0)).bg(cx.theme().border))
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_1p5()
                                    .px_1p5()
                                    .child(div().size(px(7.0)).rounded(px(3.5)).bg(dot_color))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(srv_name),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("/"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(cx.theme().foreground)
                                            .child(active_session_title),
                                    ),
                            ),
                    )
                    .child(
                        // Right (before OS window controls): Terminal/Pane actions only (no duplication with sidebar)
                        h_flex()
                            .id("titlebar-right-actions")
                            .occlude()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .items_center()
                            .gap_1()
                            .child(
                                Button::new("titlebar-terminal-search")
                                    .xsmall()
                                    .with_variant(if search_open {
                                        ButtonVariant::Primary
                                    } else {
                                        ButtonVariant::Ghost
                                    })
                                    .icon(IconName::Search)
                                    .tooltip(locale.t(
                                        "终端内搜索 (Ctrl+F)",
                                        "Search in Terminal (Ctrl+F)",
                                    ))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_terminal_search(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("titlebar-split-h")
                                    .xsmall()
                                    .ghost()
                                    .icon(IconName::PanelRight)
                                    .tooltip(locale.t(
                                        "左右均分分屏 (Ctrl+Shift+D)",
                                        "Split Right Equally (Ctrl+Shift+D)",
                                    ))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.create_new_session(
                                            &srv_split_h,
                                            None,
                                            None,
                                            None,
                                            Some(SplitAxis::Horizontal),
                                            cx,
                                        );
                                    })),
                            )
                            .child(
                                Button::new("titlebar-split-v")
                                    .xsmall()
                                    .ghost()
                                    .icon(IconName::PanelBottom)
                                    .tooltip(locale.t(
                                        "上下均分分屏 (Ctrl+Shift+E)",
                                        "Split Down Equally (Ctrl+Shift+E)",
                                    ))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.create_new_session(
                                            &srv_split_v,
                                            None,
                                            None,
                                            None,
                                            Some(SplitAxis::Vertical),
                                            cx,
                                        );
                                    })),
                            )
                            .children((has_splits || is_zoomed).then(|| {
                                Button::new("titlebar-zoom-pane")
                                    .xsmall()
                                    .with_variant(if is_zoomed {
                                        ButtonVariant::Primary
                                    } else {
                                        ButtonVariant::Ghost
                                    })
                                    .icon(if is_zoomed {
                                        IconName::Minimize
                                    } else {
                                        IconName::Maximize
                                    })
                                    .tooltip(locale.t(
                                        "最大化/还原当前分屏窗格 (Ctrl+Shift+Z)",
                                        "Zoom / Unzoom Active Split Pane (Ctrl+Shift+Z)",
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(tab) = this.tabs.get_mut(this.active_tab_idx) {
                                            tab.toggle_zoom();
                                            cx.notify();
                                        }
                                    }))
                            }))
                            .child(
                                Button::new("titlebar-toggle-chat")
                                    .xsmall()
                                    .with_variant(if is_chat_mode {
                                        ButtonVariant::Primary
                                    } else {
                                        ButtonVariant::Ghost
                                    })
                                    .icon(if is_chat_mode {
                                        IconName::SquareTerminal
                                    } else {
                                        IconName::Bot
                                    })
                                    .tooltip(if is_chat_mode {
                                        locale.t(
                                            "切回终端视图 (Ctrl+Shift+M)",
                                            "Switch to Terminal View (Ctrl+Shift+M)",
                                        )
                                    } else {
                                        locale.t(
                                            "切换 Agent 对话视图 (Ctrl+Shift+M)",
                                            "Switch to Agent Chat View (Ctrl+Shift+M)",
                                        )
                                    })
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        let next_mode = if is_chat_mode {
                                            TabViewMode::Terminal
                                        } else {
                                            TabViewMode::Chat
                                        };
                                        this.set_active_tab_view_mode(next_mode, window, cx);
                                    })),
                            )
                            .child(
                                Button::new("titlebar-settings")
                                    .xsmall()
                                    .ghost()
                                    .icon(IconName::Settings)
                                    .tooltip(locale.t("设置 (Ctrl+,)", "Settings (Ctrl+,)"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.open_settings_modal(cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_terminal_search_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.terminal_search_open {
            return None;
        }
        let locale = self.locale;
        let match_info: SharedString = self
            .tabs
            .get(self.active_tab_idx)
            .map(|t| {
                let v = t.active_terminal.read(cx);
                let total = v.search_matches.len();
                if total == 0 {
                    "0/0".to_string()
                } else {
                    let cur = v.search_current_idx.map(|i| i + 1).unwrap_or(1);
                    format!("{cur}/{total}")
                }
            })
            .unwrap_or_else(|| "0/0".to_string())
            .into();

        Some(
            h_flex()
                .id("terminal-floating-search-bar")
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                    cx.stop_propagation();
                })
                .absolute()
                .top(px(8.0))
                .right(px(14.0))
                .h(px(34.0))
                .px_2()
                .gap_1p5()
                .items_center()
                .rounded(px(6.0))
                .bg(cx.theme().popover)
                .border_1()
                .border_color(cx.theme().border)
                .shadow_md()
                .child(
                    div().w(px(200.0)).child(
                        Input::new(&self.terminal_search_input)
                            .xsmall()
                            .prefix(
                                Icon::new(IconName::Search)
                                    .xsmall()
                                    .text_color(cx.theme().muted_foreground),
                            )
                            .cleanable(true),
                    ),
                )
                .child(
                    div()
                        .min_w(px(36.0))
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(match_info),
                )
                .child(
                    Button::new("term-search-prev")
                        .xsmall()
                        .ghost()
                        .icon(IconName::ChevronUp)
                        .tooltip(locale.t("上一个匹配 (Shift+Enter)", "Previous Match (Shift+Enter)"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(tab) = this.tabs.get(this.active_tab_idx) {
                                let view = tab.active_terminal.clone();
                                view.update(cx, |v, cx| v.step_search_match(false, cx));
                            }
                        })),
                )
                .child(
                    Button::new("term-search-next")
                        .xsmall()
                        .ghost()
                        .icon(IconName::ChevronDown)
                        .tooltip(locale.t("下一个匹配 (Enter)", "Next Match (Enter)"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(tab) = this.tabs.get(this.active_tab_idx) {
                                let view = tab.active_terminal.clone();
                                view.update(cx, |v, cx| v.step_search_match(true, cx));
                            }
                        })),
                )
                .child(
                    Button::new("term-search-close")
                        .xsmall()
                        .ghost()
                        .icon(IconName::Close)
                        .tooltip(locale.t("关闭搜索", "Close Search"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_terminal_search(window, cx);
                        })),
                )
                .into_any_element(),
        )
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let locale = self.locale;
        let mut srv_section = v_flex().w_full().gap_1();

        for (srv_id, conn) in &self.servers {
            let is_active = srv_id == &self.active_server_id;
            let srv_select = srv_id.clone();
            let srv_reconnect = srv_id.clone();
            let srv_remove = srv_id.clone();
            let is_local = conn.profile.is_embedded;

            let dot_color = match &conn.state {
                WsConnectionState::Connected => cx.theme().success,
                WsConnectionState::Connecting | WsConnectionState::Reconnecting => {
                    cx.theme().warning
                }
                WsConnectionState::Disconnected => cx.theme().muted_foreground,
                WsConnectionState::Error(_) => cx.theme().danger,
            };

            let row = h_flex()
                .id(SharedString::from(format!("srv-row-{srv_id}")))
                .w_full()
                .px_2()
                .py_1p5()
                .rounded(px(6.0))
                .items_center()
                .justify_between()
                .cursor_pointer()
                .bg(if is_active {
                    cx.theme().primary.opacity(0.16)
                } else {
                    cx.theme().transparent
                })
                .hover(|s| s.bg(cx.theme().secondary))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.active_server_id = srv_select.clone();
                    cx.notify();
                }))
                .child(
                    h_flex()
                        .flex_1()
                        .min_w(px(0.0))
                        .items_center()
                        .gap_2()
                        .child(div().size(px(7.0)).rounded(px(3.5)).bg(dot_color))
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .text_xs()
                                .font_weight(if is_active {
                                    gpui::FontWeight::SEMIBOLD
                                } else {
                                    gpui::FontWeight::NORMAL
                                })
                                .text_color(cx.theme().foreground)
                                .child(SharedString::from(conn.profile.name.clone())),
                        ),
                )
                .child({
                    let show_reconnect = !matches!(conn.state, WsConnectionState::Connected);
                    h_flex()
                        .items_center()
                        .gap_0p5()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                            cx.stop_propagation();
                        })
                        .children(show_reconnect.then(|| {
                            Button::new(SharedString::from(format!("srv-rc-{srv_id}")))
                                .xsmall()
                                .ghost()
                                .icon(IconName::Redo)
                                .tooltip(locale.t("重连服务器", "Reconnect Server"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.connect_server(&srv_reconnect, cx);
                                }))
                        }))
                        .children((!is_local).then(|| {
                            Button::new(SharedString::from(format!("srv-del-{srv_id}")))
                                .xsmall()
                                .ghost()
                                .icon(IconName::Delete)
                                .tooltip(locale.t("移除服务器", "Remove Server"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.remove_server(&srv_remove, cx);
                                }))
                        }))
                });

            srv_section = srv_section.child(row);
        }

        let active_conn = self.servers.get(&self.active_server_id);
        let active_srv_id = self.active_server_id.clone();

        // Sessions list for active server (styled like clients/web `.srow` + tty7 sidebar cards)
        let mut session_list = v_flex().w_full().gap_1();
        if let Some(conn) = active_conn {
            let active_session_key = self
                .tabs
                .get(self.active_tab_idx)
                .map(|t| t.active_session_key(cx));
            let active_tab_terms = self
                .tabs
                .get(self.active_tab_idx)
                .map(|t| t.root.terminals())
                .unwrap_or_default();

            for sess in &conn.sessions {
                let is_current =
                    active_session_key.as_ref() == Some(&(active_srv_id.clone(), sess.id));
                let is_in_split = active_tab_terms.iter().any(|v| {
                    let term = v.read(cx);
                    term.server_id == active_srv_id && term.session_id == sess.id
                });

                let srv_click = active_srv_id.clone();
                let srv_kill = active_srv_id.clone();
                let srv_ren = active_srv_id.clone();
                let sid = sess.id;
                let sname = sess.name.clone();

                let agent_badge = best_agent_for_session(&conn.agent_sessions, sid).map(|ag| {
                    let (badge_txt, badge_col) = match ag.state {
                        AgentState::Waiting => ("Waiting", cx.theme().warning),
                        AgentState::Working => ("Working", cx.theme().success),
                        AgentState::Thinking => ("Thinking", cx.theme().primary),
                        AgentState::Error => ("Error", cx.theme().danger),
                        AgentState::Idle | AgentState::Done => ("Idle", cx.theme().muted_foreground),
                    };
                    div()
                        .px_1p5()
                        .py_0p5()
                        .rounded(px(4.0))
                        .bg(badge_col.opacity(0.18))
                        .text_xs()
                        .text_color(badge_col)
                        .child(badge_txt)
                });

                let is_renaming =
                    self.renaming_session.as_ref() == Some(&(active_srv_id.clone(), sid));

                let meta_sub: SharedString = format!("{}×{}", sess.cols, sess.rows).into();

                let item = if is_renaming {
                    h_flex()
                        .w_full()
                        .px_2()
                        .py_1p5()
                        .gap_1()
                        .items_center()
                        .rounded(px(6.0))
                        .bg(cx.theme().secondary)
                        .child(div().flex_1().child(Input::new(&self.rename_input).xsmall()))
                        .child(
                            Button::new(SharedString::from(format!("ren-ok-{sid}")))
                                .xsmall()
                                .primary()
                                .icon(IconName::Check)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.commit_rename_session(cx);
                                })),
                        )
                        .into_any_element()
                } else {
                    h_flex()
                        .id(SharedString::from(format!("sess-row-{sid}")))
                        .w_full()
                        .px_2p5()
                        .py_2()
                        .rounded(px(6.0))
                        .items_center()
                        .justify_between()
                        .cursor_pointer()
                        .border_l_2()
                        .border_color(if is_current {
                            cx.theme().primary
                        } else if is_in_split {
                            cx.theme().primary.opacity(0.4)
                        } else {
                            cx.theme().transparent
                        })
                        .bg(if is_current {
                            cx.theme().secondary
                        } else if is_in_split {
                            cx.theme().secondary.opacity(0.45)
                        } else {
                            cx.theme().transparent
                        })
                        .hover(|s| s.bg(cx.theme().secondary))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.attach_or_focus_session(&srv_click, sid, None, window, cx);
                        }))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w(px(0.0))
                                .gap_0p5()
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap_1p5()
                                        .child(
                                            Icon::new(IconName::SquareTerminal)
                                                .xsmall()
                                                .text_color(if is_current {
                                                    cx.theme().primary
                                                } else {
                                                    cx.theme().muted_foreground
                                                }),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .truncate()
                                                .text_xs()
                                                .font_weight(if is_current {
                                                    gpui::FontWeight::SEMIBOLD
                                                } else {
                                                    gpui::FontWeight::MEDIUM
                                                })
                                                .text_color(cx.theme().foreground)
                                                .child(SharedString::from(sess.name.clone())),
                                        )
                                        .children(agent_badge),
                                )
                                .child(
                                    div()
                                        .pl_4()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(meta_sub),
                                ),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_0p5()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation();
                                })
                                .child(
                                    Button::new(SharedString::from(format!("sess-ren-{sid}")))
                                        .xsmall()
                                        .ghost()
                                        .label("✎")
                                        .tooltip(locale.t("重命名会话", "Rename Session"))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            cx.stop_propagation();
                                            this.start_rename_session(
                                                srv_ren.clone(),
                                                sid,
                                                sname.clone(),
                                                window,
                                                cx,
                                            );
                                        })),
                                )
                                .child(
                                    Button::new(SharedString::from(format!("sess-kill-{sid}")))
                                        .xsmall()
                                        .ghost()
                                        .icon(IconName::Close)
                                        .tooltip(locale.t("关闭并终止会话", "Close & Kill Session"))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.kill_session(&srv_kill, sid, cx);
                                        })),
                                ),
                        )
                        .into_any_element()
                };

                session_list = session_list.child(item);
            }
        }

        v_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .border_r_1()
            .border_color(cx.theme().border)
            .child(
                // Sidebar header: brand + History and `+` (New Session) icon buttons
                h_flex()
                    .w_full()
                    .px_3()
                    .py_2p5()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(cx.theme().foreground)
                            .child("gateterm"),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1()
                            .child(
                                Button::new("sidebar-head-history")
                                    .xsmall()
                                    .ghost()
                                    .icon(IconName::Inbox)
                                    .tooltip(locale.t("历史会话", "Session History"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.open_history_modal(cx);
                                    })),
                            )
                            .child(
                                Button::new("sidebar-head-new-session")
                                    .xsmall()
                                    .secondary()
                                    .icon(IconName::Plus)
                                    .tooltip(locale.t("新建会话", "New Session"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_new_session_modal(window, cx);
                                    })),
                            ),
                    ),
            )
            .child(
                // Servers section
                v_flex()
                    .w_full()
                    .p_2p5()
                    .gap_1p5()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(cx.theme().muted_foreground)
                                    .child(locale.t("服务器", "SERVERS")),
                            )
                            .child(
                                Button::new("add-remote-server")
                                    .xsmall()
                                    .ghost()
                                    .icon(IconName::Plus)
                                    .tooltip(locale.t("添加远程服务器", "Add Remote Server"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_add_server_dialog(window, cx);
                                    })),
                            ),
                    )
                    .child(srv_section),
            )
            .child(
                // Sessions list
                v_flex()
                    .flex_1()
                    .min_h(px(0.0))
                    .w_full()
                    .p_2()
                    .gap_1p5()
                    .child(
                        div()
                            .px_1()
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(cx.theme().muted_foreground)
                            .child(locale.t("会话列表", "SESSIONS")),
                    )
                    .child(
                        div()
                            .id("sidebar-sessions-scroll")
                            .flex_1()
                            .w_full()
                            .overflow_y_scrollbar()
                            .child(session_list),
                    ),
            )
            .into_any_element()
    }
}

impl Render for GatetermWorkbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(tab) = self.tabs.get_mut(self.active_tab_idx) {
            tab.sync_active_from_focus(window, cx);
        }

        let title_bar = self.render_title_bar(cx);
        let sidebar = self.render_sidebar(cx);
        let pending_banner = render_pending_banners(self, cx);
        let terminal_search_bar = self.render_terminal_search_bar(cx);

        let center_body: AnyElement = if let Some(tab) = self.tabs.get(self.active_tab_idx) {
            match tab.view_mode {
                TabViewMode::Chat => {
                    let (srv_id, sid) = tab.active_session_key(cx);
                    render_chat_view(self, &srv_id, sid, cx)
                }
                TabViewMode::Terminal => {
                    let focused_id = Some(tab.active_terminal.entity_id());
                    if let Some(zoomed) = &tab.zoomed_terminal {
                        div().size_full().child(zoomed.clone()).into_any_element()
                    } else {
                        tab.root.render(focused_id, cx)
                    }
                }
            }
        } else {
            let locale = self.locale;
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_3()
                .bg(cx.theme().background)
                .child(
                    Icon::new(IconName::SquareTerminal)
                        .large()
                        .text_color(cx.theme().muted_foreground),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(locale.t(
                            "当前没有打开的终端会话",
                            "No open terminal session",
                        )),
                )
                .child(
                    Button::new("empty-new-session")
                        .primary()
                        .icon(IconName::Plus)
                        .label(locale.t("新建会话", "New Session"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_new_session_modal(window, cx);
                        })),
                )
                .into_any_element()
        };

        let new_session_modal = render_new_session_modal(self, cx);
        let history_modal = render_history_modal(self, cx);
        let server_dialog = render_server_dialog(self, cx);
        let settings_modal = render_settings_modal(self, cx);

        let main_workspace = v_flex()
            .size_full()
            .relative()
            .overflow_hidden()
            .children(pending_banner)
            .child(
                div()
                    .flex_1()
                    .size_full()
                    .relative()
                    .overflow_hidden()
                    .child(center_body)
                    .children(terminal_search_bar),
            );

        let body_area: AnyElement = if self.sidebar_collapsed {
            main_workspace.into_any_element()
        } else {
            h_resizable("gateterm-main-split")
                .child(
                    resizable_panel()
                        .size(px(236.0))
                        .size_range(px(180.0)..px(420.0))
                        .child(sidebar),
                )
                .child(resizable_panel().child(main_workspace))
                .into_any_element()
        };

        let srv_action_new = self.active_server_id.clone();
        let srv_action_sh = self.active_server_id.clone();
        let srv_action_sv = self.active_server_id.clone();

        v_flex()
            .id("gateterm-workbench-root")
            .track_focus(&self.focus_handle)
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(move |this, _: &NewShellSession, _, cx| {
                this.create_new_session(&srv_action_new, None, None, None, None, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenNewSessionMenu, window, cx| {
                this.open_new_session_modal(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.sidebar_collapsed = !this.sidebar_collapsed;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleTerminalSearch, window, cx| {
                this.toggle_terminal_search(window, cx);
            }))
            .on_action(cx.listener(move |this, _: &SplitHorizontal, _, cx| {
                this.create_new_session(
                    &srv_action_sh,
                    None,
                    None,
                    None,
                    Some(SplitAxis::Horizontal),
                    cx,
                );
            }))
            .on_action(cx.listener(move |this, _: &SplitVertical, _, cx| {
                this.create_new_session(
                    &srv_action_sv,
                    None,
                    None,
                    None,
                    Some(SplitAxis::Vertical),
                    cx,
                );
            }))
            .on_action(cx.listener(|this, _: &ToggleZoomPane, _, cx| {
                if let Some(tab) = this.tabs.get_mut(this.active_tab_idx) {
                    tab.toggle_zoom();
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &FocusNextPane, window, cx| {
                if let Some(tab) = this.tabs.get_mut(this.active_tab_idx) {
                    tab.focus_next_pane(window, cx);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleChatView, window, cx| {
                let next = this
                    .tabs
                    .get(this.active_tab_idx)
                    .map(|t| {
                        if t.view_mode == TabViewMode::Chat {
                            TabViewMode::Terminal
                        } else {
                            TabViewMode::Chat
                        }
                    })
                    .unwrap_or(TabViewMode::Terminal);
                this.set_active_tab_view_mode(next, window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenHistoryModal, _, cx| {
                this.open_history_modal(cx);
            }))
            .on_action(cx.listener(|this, _: &OpenSettingsModal, _, cx| {
                this.open_settings_modal(cx);
            }))
            .on_action(cx.listener(|this, _: &CloseActivePane, _, cx| {
                let idx = this.active_tab_idx;
                this.close_pane_or_tab(idx, None, cx);
            }))
            .child(title_bar)
            .child(div().flex_1().size_full().overflow_hidden().child(body_area))
            .children(new_session_modal)
            .children(history_modal)
            .children(server_dialog)
            .children(settings_modal)
    }
}
