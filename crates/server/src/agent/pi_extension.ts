// octoterm-managed — 由 octoterm 生成,卸载 / 清空会删掉这个文件。不要手改:重新安装会覆盖。
//
// 把 pi 的生命周期事件报给本机的 octoterm(会话状态、聊天视图)。只在 octoterm 托管的
// 会话里生效:那里的环境里有 OCTOTERM_HOOK_TOKEN / OCTOTERM_SESSION_ID;在别处启动的
// pi 拿不到这两个变量,这个扩展什么都不做。
//
// 只上报、不拦截:所有请求都是发出去就不管,失败静默 —— octoterm 没在跑时 pi 照常用。

const BASE = "__OCTOTERM_HOOK_BASE__";

export default function (pi: any) {
  const token = process.env.OCTOTERM_HOOK_TOKEN;
  const session = process.env.OCTOTERM_SESSION_ID;
  if (!token || !session) return;

  const post = (event: string, ctx: any, extra: Record<string, unknown> = {}): Promise<void> => {
    const sm = ctx?.sessionManager;
    const body = {
      session_id: sm?.getSessionId?.(),
      cwd: ctx?.cwd,
      transcript_path: sm?.getSessionFile?.(),
      session_title: sm?.getSessionName?.(),
      ...extra,
    };
    return fetch(BASE + event, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${token}`,
        "X-Octoterm-Session": session,
      },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(3000),
    }).then(
      () => {},
      () => {},
    );
  };

  // 不 await:上报绝不能拖住 pi 的主循环
  pi.on("session_start", (_e: any, ctx: any) => void post("session-start", ctx));
  pi.on("agent_start", (_e: any, ctx: any) => void post("user-prompt-submit", ctx));
  pi.on("tool_execution_start", (e: any, ctx: any) => void post("pre-tool-use", ctx, { tool_name: e?.toolName }));
  pi.on("tool_execution_end", (e: any, ctx: any) => void post("post-tool-use", ctx, { tool_name: e?.toolName }));
  pi.on("agent_end", (_e: any, ctx: any) => void post("stop", ctx));
  // 唯一 await 的:退出时进程马上就没了,不等一下这条发不出去(最多等 3 秒)
  pi.on("session_shutdown", (_e: any, ctx: any) => post("session-end", ctx));
}
