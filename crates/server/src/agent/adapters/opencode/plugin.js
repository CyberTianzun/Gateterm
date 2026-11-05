// gateterm-managed — 由 gateterm 生成,卸载 / 清空会删掉这个文件。不要手改:重新安装会覆盖。
//
// 把 OpenCode 的生命周期事件、工具权限审批与会话记录同步给本机的 gateterm。
// 只在 gateterm 托管的终端会话里生效:那里的环境里有 GATETERM_HOOK_TOKEN / GATETERM_SESSION_ID;
// 在别处启动的 opencode 拿不到这两个变量,这个插件什么都不做。

import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const BASE = "__GATETERM_HOOK_BASE__";
const BLOCKING = __GATETERM_BLOCKING__;

function safeSessionId(id) {
  return typeof id === "string" && /^[A-Za-z0-9][A-Za-z0-9_-]{0,159}$/.test(id);
}

export const GatetermOpenCodePlugin = async (ctx) => {
  const token = process.env.GATETERM_HOOK_TOKEN;
  const termSession = process.env.GATETERM_SESSION_ID;
  if (!token || !termSession) return {};

  const home = process.env.HOME || process.env.USERPROFILE || os.homedir() || "";
  const sessionsDir = home
    ? path.join(home, ".local", "share", "opencode", "gateterm-sessions")
    : "";
  const defaultCwd = ctx?.directory || ctx?.worktree || process.cwd();

  let currentSessionId = null;
  const sessionMeta = new Map();
  const writtenHeaders = new Set();
  const seenMessages = new Set();
  const seenParts = new Set();
  const pendingControllers = new Map();

  const transcriptPathFor = (sessionId) => {
    if (!sessionsDir || !safeSessionId(sessionId)) return undefined;
    return path.join(sessionsDir, `${sessionId}.jsonl`);
  };

  const appendRecord = (sessionId, record) => {
    const filePath = transcriptPathFor(sessionId);
    if (!filePath) return;
    try {
      fs.mkdirSync(sessionsDir, { recursive: true });
      if (!writtenHeaders.has(sessionId)) {
        writtenHeaders.add(sessionId);
        const meta = sessionMeta.get(sessionId) || {};
        const header = {
          type: "session",
          id: sessionId,
          cwd: meta.cwd || defaultCwd,
          title: meta.title,
          model: meta.model,
          timestamp: Date.now(),
        };
        fs.appendFileSync(filePath, JSON.stringify(header) + "\n", "utf8");
      }
      fs.appendFileSync(filePath, JSON.stringify(record) + "\n", "utf8");
    } catch {
      // 镜像对话记录失败绝不能影响 OpenCode 主流程
    }
  };

  const ensureMeta = (sessionId, patch = {}) => {
    if (!safeSessionId(sessionId)) return {};
    currentSessionId = sessionId;
    const prev = sessionMeta.get(sessionId) || { cwd: defaultCwd };
    const next = {
      cwd: patch.cwd || prev.cwd || defaultCwd,
      title: patch.title || prev.title,
      model: patch.model || prev.model,
    };
    sessionMeta.set(sessionId, next);
    return next;
  };

  const headers = {
    "Content-Type": "application/json",
    Authorization: `Bearer ${token}`,
    "X-Gateterm-Session": termSession,
  };

  const post = (eventSlug, sessionId, extra = {}) => {
    const sid = safeSessionId(sessionId) ? sessionId : currentSessionId;
    if (!safeSessionId(sid)) return Promise.resolve();
    const meta = ensureMeta(sid, {
      cwd: extra.cwd,
      title: extra.session_title,
    });
    const body = {
      session_id: sid,
      cwd: meta.cwd,
      transcript_path: transcriptPathFor(sid),
      session_title: meta.title,
      ...extra,
    };
    return fetch(BASE + eventSlug, {
      method: "POST",
      headers,
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(3000),
    }).then(
      () => {},
      () => {},
    );
  };

  const postBlocking = async (sessionId, extra = {}, cancelSignal) => {
    const sid = safeSessionId(sessionId) ? sessionId : currentSessionId;
    if (!safeSessionId(sid)) return null;
    const meta = ensureMeta(sid, { cwd: extra.cwd });
    const body = {
      session_id: sid,
      cwd: meta.cwd,
      transcript_path: transcriptPathFor(sid),
      session_title: meta.title,
      ...extra,
    };
    const timeoutSignal = AbortSignal.timeout(595000);
    const signal = cancelSignal
      ? AbortSignal.any([timeoutSignal, cancelSignal])
      : timeoutSignal;
    try {
      const res = await fetch(BASE + "permission-request", {
        method: "POST",
        headers,
        body: JSON.stringify(body),
        signal,
      });
      if (!res.ok) return null;
      return await res.json();
    } catch {
      return null;
    }
  };

  const cancelPendingFor = (key) => {
    const ctrl = pendingControllers.get(key);
    if (ctrl) {
      pendingControllers.delete(key);
      try {
        ctrl.abort();
      } catch {}
    }
  };

  return {
    event: async ({ event }) => {
      if (!event || typeof event.type !== "string") return;
      const props = event.properties || {};
      const sid =
        props.sessionID ||
        props.info?.sessionID ||
        props.part?.sessionID ||
        (event.type.startsWith("session.") ? props.info?.id : undefined) ||
        currentSessionId;

      switch (event.type) {
        case "session.created": {
          const info = props.info || {};
          ensureMeta(sid, { cwd: info.directory, title: info.title });
          appendRecord(sid, {
            type: "session_info",
            id: sid,
            cwd: info.directory || defaultCwd,
            name: info.title,
            timestamp: Date.now(),
          });
          void post("session-start", sid, {
            cwd: info.directory,
            session_title: info.title,
          });
          break;
        }
        case "session.updated": {
          const info = props.info || {};
          ensureMeta(sid, { cwd: info.directory, title: info.title });
          if (info.title) {
            appendRecord(sid, {
              type: "session_info",
              id: sid,
              cwd: info.directory || defaultCwd,
              name: info.title,
              timestamp: Date.now(),
            });
          }
          break;
        }
        case "session.status": {
          const statusType = props.status?.type;
          if (statusType === "busy") {
            void post("user-prompt-submit", sid);
          } else if (statusType === "idle") {
            cancelPendingFor(sid);
            void post("stop", sid);
          }
          break;
        }
        case "session.idle": {
          cancelPendingFor(sid);
          void post("stop", sid);
          break;
        }
        case "session.error": {
          cancelPendingFor(sid);
          void post("error-occurred", sid, {
            error: props.error?.name || "error",
          });
          break;
        }
        case "session.deleted": {
          cancelPendingFor(sid);
          void post("session-end", sid);
          break;
        }
        case "message.updated": {
          const info = props.info || {};
          const model = info.modelID || info.model?.modelID;
          if (model) {
            ensureMeta(sid, { model });
          }
          break;
        }
        case "message.part.updated": {
          const part = props.part;
          if (!part || !part.id || !sid) break;
          if (part.type === "text" && part.time?.end && !seenParts.has(part.id)) {
            seenParts.add(part.id);
            const role = part.role || (part.synthetic ? "system" : "assistant");
            appendRecord(sid, {
              id: part.id,
              type: "message",
              message: {
                role,
                timestamp: part.time.end || Date.now(),
                content: [{ type: "text", text: part.text || "" }],
              },
            });
          } else if (
            part.type === "reasoning" &&
            part.time?.end &&
            !seenParts.has(part.id)
          ) {
            seenParts.add(part.id);
            appendRecord(sid, {
              id: part.id,
              type: "message",
              message: {
                role: "assistant",
                timestamp: part.time.end || Date.now(),
                content: [{ type: "thinking", thinking: part.text || "" }],
              },
            });
          }
          break;
        }
        case "permission.asked":
        case "permission.v2.asked":
        case "permission.updated": {
          const reqId = props.id || props.requestID || props.permissionID || sid;
          const toolName =
            props.permission || props.type || props.tool || "permission";
          const toolInput = {
            title: props.title,
            patterns: props.patterns || props.pattern,
            metadata: props.metadata,
          };
          if (!BLOCKING) {
            void post("notification", sid, {
              notification_type: "permission_prompt",
              tool_name: toolName,
            });
            break;
          }
          cancelPendingFor(reqId);
          const ctrl = new AbortController();
          pendingControllers.set(reqId, ctrl);
          if (sid && reqId !== sid) {
            pendingControllers.set(sid, ctrl);
          }
          void (async () => {
            const reply = await postBlocking(
              sid,
              { tool_name: toolName, tool_input: toolInput },
              ctrl.signal,
            );
            pendingControllers.delete(reqId);
            const behavior = reply?.hookSpecificOutput?.decision?.behavior;
            if (behavior !== "allow" && behavior !== "deny") return;
            const decision = behavior === "allow" ? "once" : "reject";
            try {
              if (typeof ctx?.client?.permission?.reply === "function") {
                await ctx.client.permission.reply({
                  requestID: reqId,
                  reply: decision,
                });
              } else if (
                typeof ctx?.client?.postSessionIdPermissionsPermissionId ===
                "function"
              ) {
                await ctx.client.postSessionIdPermissionsPermissionId({
                  path: { id: sid, permissionID: reqId },
                  body: { response: decision },
                });
              }
            } catch {}
          })();
          break;
        }
        case "permission.replied":
        case "permission.v2.replied": {
          const reqId = props.requestID || props.permissionID || sid;
          cancelPendingFor(reqId);
          if (sid) cancelPendingFor(sid);
          void post("pre-tool-use", sid);
          break;
        }
        case "question.asked": {
          void post("notification", sid, {
            notification_type: "elicitation_dialog",
          });
          break;
        }
      }
    },

    "chat.message": async (input, output) => {
      const sid = input?.sessionID || output?.message?.sessionID || currentSessionId;
      const model = input?.model?.modelID || output?.message?.modelID;
      ensureMeta(sid, { model });
      const msgId = output?.message?.id || input?.messageID;
      if (sid && msgId && !seenMessages.has(msgId)) {
        seenMessages.add(msgId);
        const texts = (output?.parts || [])
          .filter((p) => p?.type === "text" && typeof p.text === "string")
          .map((p) => p.text)
          .join("\n");
        if (texts.trim()) {
          appendRecord(sid, {
            id: msgId,
            type: "message",
            message: {
              role: "user",
              model,
              timestamp: Date.now(),
              content: [{ type: "text", text: texts }],
            },
          });
        }
      }
      void post("user-prompt-submit", sid);
    },

    "tool.execute.before": async (input, output) => {
      const sid = input?.sessionID || currentSessionId;
      const toolName = input?.tool || "tool";
      if (sid) {
        appendRecord(sid, {
          id: input?.callID ? `${input.callID}-call` : undefined,
          type: "message",
          message: {
            role: "assistant",
            timestamp: Date.now(),
            content: [
              {
                type: "toolCall",
                name: toolName,
                arguments: output?.args ?? {},
              },
            ],
          },
        });
      }
      void post("pre-tool-use", sid, { tool_name: toolName });
    },

    "tool.execute.after": async (input, output) => {
      const sid = input?.sessionID || currentSessionId;
      const toolName = input?.tool || "tool";
      if (sid) {
        appendRecord(sid, {
          id: input?.callID ? `${input.callID}-result` : undefined,
          type: "message",
          message: {
            role: "toolResult",
            isError: false,
            timestamp: Date.now(),
            content: typeof output?.output === "string" ? output.output : "",
          },
        });
      }
      void post("post-tool-use", sid, { tool_name: toolName });
    },

    "permission.ask": async (input, output) => {
      const sid = input?.sessionID || currentSessionId;
      const toolName = input?.type || input?.tool || "permission";
      if (!BLOCKING) {
        void post("notification", sid, {
          notification_type: "permission_prompt",
          tool_name: toolName,
        });
        return;
      }
      const reply = await postBlocking(sid, {
        tool_name: toolName,
        tool_input: {
          title: input?.title,
          pattern: input?.pattern,
          metadata: input?.metadata,
        },
      });
      const behavior = reply?.hookSpecificOutput?.decision?.behavior;
      if (behavior === "allow" && output) {
        output.status = "allow";
      } else if (behavior === "deny" && output) {
        output.status = "deny";
      }
    },
  };
};

export default GatetermOpenCodePlugin;
