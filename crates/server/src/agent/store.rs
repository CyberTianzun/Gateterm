//! agent 会话表:内存态,不落盘。
//!
//! server 重启后,下一个 hook 事件就会把会话重建出来;丢掉的只是历史,而历史不是
//! 这个功能的目标。
//!
//! **任何事件都能惰性创建会话**,不假设 `SessionStart` 一定先到 —— Task 3 的端到端
//! 里就观察到过 `-p` 模式下第一个到达的是 `UserPromptSubmit`。

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use gateterm_protocol::{AgentState, ServerMsg};
use serde::Serialize;

/// hook 密钥。
///
/// 写进 `settings.json` 的是字面量 `$GATETERM_HOOK_TOKEN`,插值发生在 hook 触发的那一刻,
/// 取自 agent 进程的环境,而那份环境是 gateterm 在 spawn 这个会话时给的。
///
/// 顺带一个恰好正确的副作用:**环境变量就是能力本身**。在 gateterm 之外启动的
/// agent 拿不到这个变量,hook 照样触发,但没有 `Authorization` 头 —— 安静收下、什么都不做。
/// 「只管托管会话」这条边界由机制保证,不需要额外的判别逻辑。
///
/// **为什么要落盘**(见 [`init_hook_token`]):本来「会话与 server 同生共死,不存在老会话
/// 拿着旧密钥」,但 Codex 打破了这个前提 —— 它的 TUI 把会话交给一个常驻的共享守护进程
/// (`codex app-server --managed-daemon`),hook 由守护进程执行,用的是**第一次拉起它的
/// 那个终端**的环境。server 一重启就换密钥的话,守护进程手里的永远是旧的,所有事件被
/// 静默丢弃,只能杀掉守护进程重来。
///
/// 优先级:同名环境变量(运维 / 端到端测试固定它) > 入口初始化时读到或生成的文件 >
/// 进程内随机值。没人调 [`init_hook_token`] 时(库的使用方、测试)就是最后一种,不碰磁盘。
pub fn hook_token() -> &'static str {
    HOOK_TOKEN.get_or_init(|| env_hook_token().unwrap_or_else(random_hook_token))
}

static HOOK_TOKEN: OnceLock<String> = OnceLock::new();

/// 入口(`gateterm-server` / desktop)启动时调一次,让密钥跨重启保持不变。
///
/// 必须在第一次 spawn 会话之前调;之后再调不改变已经定下的值。读写失败就退回进程内
/// 随机值并打一条警告 —— 密钥只关乎 hook,不能因为它让整个服务起不来。
pub fn init_hook_token(path: Option<&std::path::Path>) -> &'static str {
    HOOK_TOKEN.get_or_init(|| {
        if let Some(t) = env_hook_token() {
            return t;
        }
        match path.map(load_or_create_hook_token) {
            Some(Ok(t)) => t,
            Some(Err(e)) => {
                tracing::warn!(error = %e, "hook 密钥读写失败,本次用临时值(重启后 Codex 守护进程里的会失效)");
                random_hook_token()
            }
            None => random_hook_token(),
        }
    })
}

/// 和备份放在一起:gateterm 自己的配置目录。
pub fn default_hook_token_path() -> Option<std::path::PathBuf> {
    directories::ProjectDirs::from("", "", "gateterm").map(|d| d.config_dir().join("hook-token"))
}

fn env_hook_token() -> Option<String> {
    std::env::var("GATETERM_HOOK_TOKEN")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn random_hook_token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// 读已有的密钥;没有(或内容不像密钥)就生成一个写进去,权限 0600。
///
/// 用 `create_new` 写:desktop 和独立 server 同时首次启动时,后到的那个读先到的写好的值,
/// 两边最终一致。内容坏了(空、带空白、太短)视为没有,覆盖重写。
pub fn load_or_create_hook_token(path: &std::path::Path) -> std::io::Result<String> {
    if let Some(t) = read_hook_token(path)? {
        return Ok(t);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let token = random_hook_token();
    let _ = std::fs::remove_file(path); // 坏内容;不存在时这里本来就会失败
    match write_new_private(path, &token) {
        Ok(()) => Ok(token),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            read_hook_token(path)?.ok_or_else(|| std::io::Error::other("hook 密钥文件内容无效"))
        }
        Err(e) => Err(e),
    }
}

fn read_hook_token(path: &std::path::Path) -> std::io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(t) => {
            let t = t.trim();
            let valid = t.len() >= 16 && t.chars().all(|c| c.is_ascii_alphanumeric());
            Ok(valid.then(|| t.to_string()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn write_new_private(path: &std::path::Path, token: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    f.write_all(token.as_bytes())?;
    f.sync_all()
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// adapter 把 agent 方言的 hook payload 归一化成这个。
#[derive(Debug, Clone, Default)]
pub struct Update {
    /// `None` 表示这个事件不改变状态(例如纯信息类事件)
    pub state: Option<AgentState>,
    pub detail: Option<String>,
    pub cwd: Option<String>,
    pub title: Option<String>,
    /// agent 自己写的对话记录在哪。**每个 hook 事件都带着它**,存下来是聊天视图
    /// 的地基(见 `transcript` 模块)。
    ///
    /// 原样发给客户端:它由 `cwd` + 会话 id 推导而来,而这两样客户端本来就有,
    /// 所以没有泄漏新东西;客户端只把它当「有没有」的信号用。
    pub transcript: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentSession {
    pub agent_id: String,
    pub agent_session_id: String,
    pub session: Option<u64>,
    pub state: AgentState,
    pub detail: Option<String>,
    pub cwd: Option<String>,
    pub title: Option<String>,
    /// agent 自己写的对话记录在哪。**每个 hook 事件都带着它**,存下来是聊天视图
    /// 的地基(见 `transcript` 模块)。
    ///
    /// 原样发给客户端:它由 `cwd` + 会话 id 推导而来,而这两样客户端本来就有,
    /// 所以没有泄漏新东西;客户端只把它当「有没有」的信号用。
    pub transcript: Option<String>,
    /// 这家 agent 的对话记录能不能读。客户端据此在同一个托管会话里**优先挑读得了的
    /// 那个** —— 一个会话里先后跑过两家 agent 是常事,按状态优先级挑出来的那个未必
    /// 是能看聊天的那个。
    pub supports_transcript: bool,
    /// 有值 = 正在等人回答。Task 6 接上阻塞式决策后由它填。
    pub pending: Option<String>,
    pub updated_at: u64,
    /// 用户「知道了」的时刻。清理的空闲基准取 `max(updated_at, acked_at)` ——
    /// 否则用户刚看过一眼的会话会因为「很久没有新事件」被扫掉。
    pub acked_at: u64,
    #[serde(skip)]
    pub(crate) process: Option<super::process::Identity>,
}

impl AgentSession {
    pub fn to_msg(&self) -> ServerMsg {
        ServerMsg::AgentEvent {
            agent_id: self.agent_id.clone(),
            agent_session_id: self.agent_session_id.clone(),
            session: self.session,
            state: self.state,
            pending: self.pending.clone(),
            detail: self.detail.clone(),
        }
    }
}

/// 用户对一个挂起请求的答复。
///
/// `NoDecision` 不是「拒绝」:它表示我们不替用户做决定,让 agent 回落到它自己终端里
/// 的审批流程。超时、没人连着、客户端断开,都走这条 —— **宁可不作决定,也不代替
/// 用户 allow 或 deny**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow {
        message: Option<String>,
        /// 改写后的工具入参,原样回传给 agent。
        ///
        /// 这是**回答选择题**的机制:`AskUserQuestion` 这类工具走的也是
        /// `PermissionRequest` 通道,答案不是 allow/deny,而是「放行,并且把用户选的
        /// 答案填进入参里」。参见 `claude_code::render`。
        updated_input: Option<serde_json::Value>,
    },
    Deny {
        message: Option<String>,
    },
    NoDecision,
}

/// 一个正在等人回答的请求。
#[derive(Debug, Clone, Serialize)]
pub struct PendingRequest {
    pub id: String,
    pub agent_id: String,
    pub agent_session_id: String,
    pub session: Option<u64>,
    pub tool_name: Option<String>,
    /// 工具入参。可能很长(命令原文),客户端自己决定怎么截断展示。
    pub tool_input: serde_json::Value,
    pub created_at: u64,
    /// 什么时候作废(unix 秒)。**必须由服务端算好给出去** —— 超时长度是服务端配置,
    /// 客户端不知道也不该猜;而「我还有多久」正是决定要不要现在处理的关键信息。
    pub expires_at: u64,
}

struct PendingEntry {
    meta: PendingRequest,
    tx: Option<tokio::sync::oneshot::Sender<Decision>>,
    /// 已经有人答过了。留着这个标记(而不是直接删)是为了让第二次回答拿到 409
    /// 而不是 404 —— 「重复提交」和「这个请求根本不存在」对客户端是两件事。
    answered: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AnswerResult {
    Ok,
    NotFound,
    AlreadyAnswered,
}

/// 记住最近答过的 id 有多少个。
///
/// 为什么需要它:用户答完之后,挂在 socket 上的那个 handler 立刻醒来、写响应、
/// Drop guard 摘掉条目 —— 这中间只有微秒级的窗口。没有这份记录的话,客户端因为
/// 网络抖动重试一次,拿到的会是 404「这个请求根本不存在」,而事实是「你已经答过了」。
/// 这两件事对客户端不是一回事,不能混成同一个码。
const ANSWERED_MEMORY: usize = 64;
const RETIRED_PROCESS_MEMORY: usize = 64;

#[derive(Default)]
pub struct AgentSessionStore {
    pub(crate) history: Mutex<super::history::Catalog>,
    pub(crate) history_open: tokio::sync::Mutex<()>,
    sessions: Mutex<HashMap<(String, String), AgentSession>>,
    /// Codex 的共享后台进程可能在 CLI 退出后补发工具事件。保留少量结束记录,
    /// 直到新的 SessionStart/UserPromptSubmit,避免旧事件把已关闭的聊天入口重新打开。
    retired_processes: Mutex<VecDeque<AgentSession>>,
    pending: Mutex<HashMap<String, PendingEntry>>,
    answered: Mutex<VecDeque<String>>,
    sweeper_started: AtomicBool,
}

/// 对一个 agent 会话的清理判决。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sweep {
    /// 从表里删掉
    Drop,
    /// 状态显示成 idle。用于「卡在 working 上很久了」—— 它多半已经不在跑了,
    /// 但还不到该删的程度。
    MarkIdle,
}

/// 清理判决。**纯函数**:`now` 与「托管会话是否还活着」都由调用方给,
/// 这里不读时钟、不查进程,于是可以逐条断言而不引入时间相关的 flaky。
///
/// 规则的顺序是有讲究的:
///
/// 1. **托管会话没了 ⇒ 立即删**。pty 子进程退出意味着里面的 agent 必死,这是比任何
///    超时都硬的证据,所以排在一切时间判断之前。
/// 2. **托管会话还在 ⇒ 不按时间删**。agent 停在那里等你输入时本来就一个事件都不发,
///    「很久没动静」不等于「退出了」—— 按时间删的结果是聊天视图在用户眼前凭空消失
///    (实际发生过:Codex 闲置 10 分钟后被扫掉)。真正退出的由两条更硬的证据收走:
///    终端关了(第 1 条),或同一终端里同一家 agent 开了新会话(见 `apply_superseding`)。
///    `session_stale_secs` 因此只管**没有关联终端**的会话。
/// 3. **正在等人的一律不扫**。人可能在睡觉。挂起请求自己有超时(`pending_timeout_secs`),
///    到点了会把 `pending` 摘掉,那时这条会话才重新进入清理的视野。
/// 4. 空闲基准取 `max(updated_at, acked_at)` —— 用户刚看过一眼的会话不该因为
///    「很久没有新事件」被扫掉。
///
/// 注意 `MarkIdle` **不刷新时间戳**。参考实现在这里踩过坑:刷新之后「超时转 idle」
/// 会不断把删除的时刻往后推,结果那条会话永远删不掉。这里让两个时钟都从同一个
/// 基准走,`MarkIdle` 只改显示,不续命。
pub fn decide(
    now: u64,
    s: &AgentSession,
    host_alive: bool,
    session_stale_secs: u64,
    working_stale_secs: u64,
) -> Option<Sweep> {
    if !host_alive {
        return Some(Sweep::Drop);
    }
    if s.pending.is_some() {
        return None;
    }
    // agent 自己报了结束(SessionEnd)。客户端收到事件时已经移除了,表里留着只会越积越多 ——
    // 终端还在时不按时间删,这类会话就只剩这一条出路
    if s.state == AgentState::Done {
        return Some(Sweep::Drop);
    }
    let age = now.saturating_sub(s.updated_at.max(s.acked_at));
    if s.session.is_none() && age > session_stale_secs {
        return Some(Sweep::Drop);
    }
    if age > working_stale_secs
        && matches!(s.state, AgentState::Working | AgentState::Thinking)
    {
        return Some(Sweep::MarkIdle);
    }
    None
}

impl AgentSessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 应用一次更新,返回更新后的快照。会话不存在就地创建。不顶替任何会话。
    pub fn apply(
        &self,
        agent_id: &str,
        agent_session_id: &str,
        session: Option<u64>,
        up: Update,
    ) -> AgentSession {
        self.apply_inner(agent_id, agent_session_id, session, up, false).0
    }

    /// 同 [`Self::apply`],另外返回被顶替掉的旧会话(状态已标成 `Done`,调用方负责广播)。
    ///
    /// **顶替**:一个终端的前台同一时刻只跑得了一个同家 agent。同一终端里出现了同一家
    /// agent 的**新**会话(用户退出 codex 又开了一个,或 Claude 里 `/clear`),旧的那个
    /// 必然已经结束 —— 这是按时间清理之外收走已退出会话的主要途径。不同家的 agent
    /// 互不顶替:先 codex 后 claude 时两边的记录都还有用。
    ///
    /// **只该用在「用户直接驱动」的事件上**(会话开始、用户提交提示词),见
    /// `routes::supersedes`。子代理会用自己的会话 id 发工具事件(Grok 文档写明了,
    /// 其他家没排除),拿任意事件来顶替,子代理一动就会把主会话挤掉。
    pub fn apply_superseding(
        &self,
        agent_id: &str,
        agent_session_id: &str,
        session: Option<u64>,
        up: Update,
    ) -> (AgentSession, Vec<AgentSession>) {
        self.apply_inner(agent_id, agent_session_id, session, up, true)
    }

    fn apply_inner(
        &self,
        agent_id: &str,
        agent_session_id: &str,
        session: Option<u64>,
        up: Update,
        supersede: bool,
    ) -> (AgentSession, Vec<AgentSession>) {
        let supports = crate::agent::find(agent_id).is_some_and(|a| a.supports_transcript());
        let key = (agent_id.to_string(), agent_session_id.to_string());
        let mut guard = self.sessions.lock().unwrap();
        {
            let mut retired = self.retired_processes.lock().unwrap();
            let matches = |s: &AgentSession| s.agent_id == agent_id
                && s.agent_session_id == agent_session_id && s.session == session;
            if supersede {
                retired.retain(|s| !matches(s));
            } else if let Some(gone) = retired.iter().find(|s| matches(s)) {
                return (gone.clone(), Vec::new());
            }
        }
        let mut superseded = Vec::new();
        if supersede
            && let Some(host) = session
            && !guard.contains_key(&key)
        {
            guard.retain(|_, s| {
                let replaced = s.agent_id == agent_id && s.session == Some(host) && s.pending.is_none();
                if replaced {
                    let mut gone = s.clone();
                    gone.state = AgentState::Done;
                    superseded.push(gone);
                }
                !replaced
            });
        }
        let entry = guard.entry(key).or_insert_with(|| AgentSession {
            agent_id: agent_id.to_string(),
            agent_session_id: agent_session_id.to_string(),
            session,
            state: AgentState::Idle,
            detail: None,
            cwd: None,
            title: None,
            transcript: None,
            supports_transcript: false,
            pending: None,
            updated_at: now(),
            acked_at: 0,
            process: None,
        });
        if let Some(s) = up.state {
            entry.state = s;
        }
        // 只覆盖有值的字段:一个只带 tool_name 的事件不该把 cwd 抹掉
        if up.detail.is_some() {
            entry.detail = up.detail;
        }
        if up.cwd.is_some() {
            entry.cwd = up.cwd;
        }
        if up.title.is_some() {
            entry.title = up.title;
        }
        if up.transcript.is_some() {
            entry.transcript = up.transcript;
        }
        entry.supports_transcript = supports;
        if session.is_some() {
            entry.session = session;
        }
        entry.updated_at = now();
        (entry.clone(), superseded)
    }

    /// 记下推导出来的记录路径,免得每次拉消息都重新去文件系统里找一遍。
    pub fn set_transcript(&self, agent_id: &str, agent_session_id: &str, path: String) {
        if let Some(s) = self
            .sessions
            .lock()
            .unwrap()
            .get_mut(&(agent_id.to_string(), agent_session_id.to_string()))
        {
            s.transcript = Some(path);
        }
    }

    pub fn snapshot(&self, agent_id: &str, agent_session_id: &str) -> Option<AgentSession> {
        self.sessions
            .lock()
            .unwrap()
            .get(&(agent_id.to_string(), agent_session_id.to_string()))
            .cloned()
    }

    pub(crate) fn bind_process(&self, s: &AgentSession, process: super::process::Identity) -> bool {
        let mut guard = self.sessions.lock().unwrap();
        let Some(current) = guard.get_mut(&(s.agent_id.clone(), s.agent_session_id.clone())) else {
            return false;
        };
        if current.session != s.session || current.state == AgentState::Done
            || current.process == Some(process)
        {
            return false;
        }
        current.process = Some(process);
        true
    }

    /// 只移除仍绑定这个进程的会话,旧进程的监视器不能删掉后来 resume 的新实例。
    pub(crate) fn finish_process(
        &self,
        s: &AgentSession,
        process: super::process::Identity,
    ) -> Option<AgentSession> {
        let mut guard = self.sessions.lock().unwrap();
        let key = (s.agent_id.clone(), s.agent_session_id.clone());
        let current = guard.get(&key)?;
        if current.session != s.session || current.process != Some(process) {
            return None;
        }
        let mut gone = guard.remove(&key)?;
        let pending = gone.pending.take();
        gone.state = AgentState::Done;
        {
            let mut retired = self.retired_processes.lock().unwrap();
            retired.push_back(gone.clone());
            while retired.len() > RETIRED_PROCESS_MEMORY {
                retired.pop_front();
            }
        }
        drop(guard);
        if let Some(id) = pending {
            self.answer(&id, Decision::NoDecision);
        }
        Some(gone)
    }

    pub fn list(&self) -> Vec<AgentSession> {
        let mut v: Vec<_> = self.sessions.lock().unwrap().values().cloned().collect();
        v.sort_by(|a, b| {
            a.agent_id.cmp(&b.agent_id).then(a.agent_session_id.cmp(&b.agent_session_id))
        });
        v
    }

    /// 登记一个挂起请求,返回等待答复的接收端。
    ///
    /// 同时把对应会话置为 `Waiting` 并挂上 `pending` —— 客户端的「有事找你」就是
    /// 靠这两个字段。
    pub fn insert_pending(
        &self,
        meta: PendingRequest,
    ) -> tokio::sync::oneshot::Receiver<Decision> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let key = (meta.agent_id.clone(), meta.agent_session_id.clone());
        if let Some(s) = self.sessions.lock().unwrap().get_mut(&key) {
            s.state = AgentState::Waiting;
            s.pending = Some(meta.id.clone());
            s.updated_at = now();
        }
        self.pending
            .lock()
            .unwrap()
            .insert(meta.id.clone(), PendingEntry { meta, tx: Some(tx), answered: false });
        rx
    }

    /// 摘掉一个挂起请求。**由 handler 的 Drop guard 调用** —— agent 侧断开连接时
    /// axum 会把 handler future 丢掉,只有 Drop 能保证条目不会一直挂到超时。
    pub fn remove_pending(&self, id: &str) -> Option<PendingRequest> {
        let entry = self.pending.lock().unwrap().remove(id)?;
        let key = (entry.meta.agent_id.clone(), entry.meta.agent_session_id.clone());
        if let Some(s) = self.sessions.lock().unwrap().get_mut(&key)
            && s.pending.as_deref() == Some(id)
        {
            s.pending = None;
            // 不再等人了,但也说不上在干什么 —— 交给下一个 hook 事件去纠正
            s.state = AgentState::Working;
            s.updated_at = now();
        }
        Some(entry.meta)
    }

    pub fn answer(&self, id: &str, decision: Decision) -> AnswerResult {
        let mut guard = self.pending.lock().unwrap();
        let Some(entry) = guard.get_mut(id) else {
            // 条目不在了:可能是刚答完被摘掉,也可能是压根没有过
            return if self.answered.lock().unwrap().iter().any(|x| x == id) {
                AnswerResult::AlreadyAnswered
            } else {
                AnswerResult::NotFound
            };
        };
        if entry.answered {
            return AnswerResult::AlreadyAnswered;
        }
        entry.answered = true;
        let tx = entry.tx.take();
        drop(guard);
        self.remember_answered(id);
        match tx {
            // 接收端已经没了(agent 断开),答复无处可去 —— 但仍算「答过了」
            Some(tx) => {
                let _ = tx.send(decision);
                AnswerResult::Ok
            }
            None => AnswerResult::AlreadyAnswered,
        }
    }

    fn remember_answered(&self, id: &str) {
        let mut a = self.answered.lock().unwrap();
        a.push_back(id.to_string());
        while a.len() > ANSWERED_MEMORY {
            a.pop_front();
        }
    }

    pub fn list_pending(&self) -> Vec<PendingRequest> {
        let mut v: Vec<_> =
            self.pending.lock().unwrap().values().map(|e| e.meta.clone()).collect();
        v.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        v
    }

    /// 跑一轮清理,返回需要广播出去的变化。
    ///
    /// `host_alive` 由调用方提供(问 `SessionManager` 那个托管会话还在不在)——
    /// 保持这个模块不依赖 session 层。
    pub fn sweep(
        &self,
        now: u64,
        host_alive: &dyn Fn(u64) -> bool,
        session_stale_secs: u64,
        working_stale_secs: u64,
    ) -> Vec<AgentSession> {
        let mut changed = Vec::new();
        let mut guard = self.sessions.lock().unwrap();
        guard.retain(|_, s| {
            let alive = s.session.map(host_alive).unwrap_or(true);
            match decide(now, s, alive, session_stale_secs, working_stale_secs) {
                Some(Sweep::Drop) => {
                    let mut gone = s.clone();
                    gone.state = AgentState::Done;
                    gone.pending = None;
                    changed.push(gone);
                    false
                }
                Some(Sweep::MarkIdle) => {
                    s.state = AgentState::Idle;
                    // 刻意不动 updated_at:见 `decide` 的文档
                    changed.push(s.clone());
                    true
                }
                None => true,
            }
        });
        changed
    }

    /// 起一个后台清理任务。**只会真正启动一次** —— desktop 重建 HTTP 层时
    /// `serve()` 会再调一次,不能因此多出一个扫描器。
    pub fn start_sweeper(
        self: &Arc<Self>,
        manager: Arc<crate::session::manager::SessionManager>,
        session_stale_secs: u64,
        working_stale_secs: u64,
    ) {
        if self.sweeper_started.swap(true, Ordering::SeqCst) {
            return;
        }
        let store = Arc::downgrade(self);
        tokio::spawn(async move {
            // interval 的第一跳是**立即**触发的,而这个任务的首次轮询会被推迟到
            // 运行时下一次调度 —— 于是「立即」有可能落在启动之后的任意一点,把刚
            // 建好的会话当场扫掉。用 interval_at 把第一跳推到一个周期之后,启动
            // 瞬间不做任何判断。
            let period = std::time::Duration::from_secs(10);
            let mut tick =
                tokio::time::interval_at(tokio::time::Instant::now() + period, period);
            loop {
                tick.tick().await;
                // store 没人要了就退出,不留悬空任务
                let Some(store) = store.upgrade() else { break };
                let alive = |id: u64| manager.get(id).is_some();
                for s in store.sweep(now(), &alive, session_stale_secs, working_stale_secs) {
                    manager.publish(s.to_msg());
                }
            }
        });
    }

    /// 托管会话没了 ⇒ 里面的 agent 必死。这是比任何超时都硬的证据。
    pub fn drop_by_session(&self, session: u64) -> Vec<AgentSession> {
        let mut guard = self.sessions.lock().unwrap();
        let dead: Vec<_> = guard
            .values()
            .filter(|s| s.session == Some(session))
            .map(|s| {
                let mut s = s.clone();
                s.state = AgentState::Done;
                s.pending = None;
                s
            })
            .collect();
        guard.retain(|_, s| s.session != Some(session));
        dead
    }
}


#[cfg(test)]
mod tests {
    use super::{load_or_create_hook_token, read_hook_token};

    #[test]
    fn hook_token_survives_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("hook-token");
        let first = load_or_create_hook_token(&path).unwrap();
        assert_eq!(load_or_create_hook_token(&path).unwrap(), first);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn broken_hook_token_file_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hook-token");
        for bad in ["", "   \n", "short", "has space in it 1234567890"] {
            std::fs::write(&path, bad).unwrap();
            let t = load_or_create_hook_token(&path).unwrap();
            assert!(t.len() >= 16, "{bad:?}");
            assert_eq!(read_hook_token(&path).unwrap().as_deref(), Some(t.as_str()));
        }
    }

    use super::*;

    fn session(state: AgentState, updated_at: u64) -> AgentSession {
        AgentSession {
            agent_id: "claude-code".into(),
            agent_session_id: "s1".into(),
            session: Some(1),
            state,
            detail: None,
            cwd: None,
            title: None,
            transcript: None,
            supports_transcript: false,
            pending: None,
            updated_at,
            acked_at: 0,
            process: None,
        }
    }

    /// 托管会话没了是比任何超时都硬的证据,必须排在时间判断之前 —— 哪怕它刚刚
    /// 才有过活动。
    #[test]
    fn dead_host_beats_every_timeout() {
        let s = session(AgentState::Working, 1000);
        assert_eq!(decide(1000, &s, false, 600, 300), Some(Sweep::Drop));
    }

    #[test]
    fn fresh_session_is_left_alone() {
        let s = session(AgentState::Working, 1000);
        assert_eq!(decide(1010, &s, true, 600, 300), None);
    }

    /// 人可能在睡觉。挂起请求自己有超时,轮不到清理来替它做决定。
    #[test]
    fn waiting_for_a_human_is_never_swept() {
        let mut s = session(AgentState::Waiting, 0);
        s.pending = Some("p1".into());
        assert_eq!(decide(99_999, &s, true, 600, 300), None);
    }

    #[test]
    fn stuck_working_becomes_idle() {
        let s = session(AgentState::Working, 0);
        assert_eq!(decide(301, &s, true, 600, 300), Some(Sweep::MarkIdle));
    }

    /// idle 的会话不该被「卡住」规则碰,它只等 session_stale。
    #[test]
    fn idle_session_is_not_marked_again() {
        let s = session(AgentState::Idle, 0);
        assert_eq!(decide(301, &s, true, 600, 300), None);
    }

    /// agent 报了结束的,下一轮清理就删 —— 终端还在时不按时间删,不然它们会一直留着。
    #[test]
    fn done_session_is_dropped_even_while_host_lives() {
        let s = session(AgentState::Done, 0);
        assert_eq!(decide(1, &s, true, 600, 300), Some(Sweep::Drop));
    }

    /// 没有关联终端的会话才按时间清理。
    #[test]
    fn very_old_unhosted_session_is_dropped() {
        let mut s = session(AgentState::Idle, 0);
        s.session = None;
        assert_eq!(decide(601, &s, true, 600, 300), Some(Sweep::Drop));
    }

    /// 终端还在时,闲置多久都不删 —— agent 等你输入时本来就不发事件(Codex 闲置
    /// 10 分钟后聊天视图凭空消失,就是按时间删出来的)。
    #[test]
    fn hosted_idle_session_survives_any_age() {
        let s = session(AgentState::Idle, 0);
        assert_eq!(decide(1_000_000, &s, true, 600, 300), None);
    }

    /// 用户点过「知道了」之后倒计时应当从那一刻重新开始。
    #[test]
    fn ack_extends_the_clock() {
        let mut s = session(AgentState::Idle, 0);
        s.session = None;
        s.acked_at = 600;
        assert_eq!(decide(900, &s, true, 600, 300), None);
        assert_eq!(decide(1201, &s, true, 600, 300), Some(Sweep::Drop));
    }

    /// `MarkIdle` 不刷新时间戳 —— 否则「超时转 idle」会把删除时刻不断往后推,
    /// 那条会话永远删不掉(参考实现踩过的坑)。
    #[test]
    fn mark_idle_does_not_extend_life() {
        let store = AgentSessionStore::new();
        store.apply(
            "claude-code",
            "s1",
            None,
            Update { state: Some(AgentState::Working), ..Default::default() },
        );
        let before = store.list()[0].updated_at;
        let changed = store.sweep(before + 301, &|_| true, 600, 300);
        assert_eq!(changed.len(), 1);
        assert_eq!(store.list()[0].state, AgentState::Idle);
        assert_eq!(store.list()[0].updated_at, before, "MarkIdle 不该续命");
        // 再走到 session_stale,必须能被删掉
        let changed = store.sweep(before + 601, &|_| true, 600, 300);
        assert_eq!(changed.len(), 1);
        assert!(store.list().is_empty(), "转成 idle 之后仍然必须能被删掉");
    }

    #[test]
    fn sweep_drops_sessions_whose_host_is_gone() {
        let store = AgentSessionStore::new();
        store.apply("claude-code", "s1", Some(7), Update::default());
        let changed = store.sweep(now(), &|_| false, 600, 300);
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].state, AgentState::Done);
        assert!(store.list().is_empty());
    }

    /// 同一终端里同一家 agent 开了新会话 ⇒ 旧的必然结束了,标成 Done 交给调用方广播。
    #[test]
    fn new_session_of_same_agent_in_same_terminal_supersedes_old() {
        let store = AgentSessionStore::new();
        store.apply("codex", "old", Some(2), Update::default());
        store.apply("codex", "elsewhere", Some(3), Update::default());
        let (_, gone) = store.apply_superseding("codex", "new", Some(2), Update::default());
        assert_eq!(gone.len(), 1);
        assert_eq!((gone[0].agent_session_id.as_str(), gone[0].state), ("old", AgentState::Done));
        let mut left: Vec<_> = store.list().into_iter().map(|s| s.agent_session_id).collect();
        left.sort();
        assert_eq!(left, ["elsewhere", "new"]);
        // 已有会话的后续事件不再顶替任何人
        let (_, gone) = store.apply_superseding("codex", "new", Some(2), Update::default());
        assert!(gone.is_empty());
    }

    /// 普通事件(子代理的工具调用就是这种)不顶替:否则子代理一动,主会话就被挤掉。
    #[test]
    fn plain_apply_never_supersedes() {
        let store = AgentSessionStore::new();
        store.apply("grok", "main", Some(2), Update::default());
        store.apply("grok", "subagent", Some(2), Update::default());
        assert_eq!(store.list().len(), 2);
    }

    /// 不同家的 agent 互不顶替:先 codex 后 claude 时两边的记录都还有用。
    /// 等人回答的也不顶替 —— 那个请求还挂着,得等它自己收场。
    #[test]
    fn other_agents_and_pending_requests_are_not_superseded() {
        let store = AgentSessionStore::new();
        store.apply("codex", "c", Some(2), Update::default());
        store.apply("claude-code", "w", Some(2), Update::default());
        let _rx = store.insert_pending(PendingRequest {
            id: "p1".into(),
            agent_id: "claude-code".into(),
            agent_session_id: "w".into(),
            session: Some(2),
            tool_name: None,
            tool_input: serde_json::Value::Null,
            created_at: 0,
            expires_at: 0,
        });
        let (_, gone) = store.apply_superseding("claude-code", "x", Some(2), Update::default());
        assert!(gone.is_empty());
        assert_eq!(store.list().len(), 3);
    }
}
