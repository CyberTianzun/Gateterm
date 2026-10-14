package ai.eclosion.octoterm.android.agent

import okhttp3.OkHttpClient
import okhttp3.Request
import org.json.JSONObject

/**
 * 托管会话里跑着的 coding agent。
 *
 * 状态的**增量**来自 `agent-event` 控制消息广播,**全量**来自 `GET /api/agents/sessions`
 * —— 连上时和每次重连后都要拉一次全量(协议 A5:不做增量对账,断线期间漏掉的事件靠
 * 这次拉取补齐)。逻辑和 web 端 `agents.ts` 一一对应,两边要一起改。
 */
enum class AgentState(val wire: String, val priority: Int) {
    // `waiting` 永远排第一 —— 它是唯一需要用户**动手**的状态
    Waiting("waiting", 5),
    Error("error", 4),
    Working("working", 3),
    Thinking("thinking", 2),
    Idle("idle", 1),
    Done("done", 0),
    ;

    companion object {
        /** 认不出的状态当 idle:新版服务端加了状态不该让旧客户端把会话丢掉 */
        fun parse(s: String): AgentState = entries.firstOrNull { it.wire == s } ?: Idle
    }
}

data class AgentSession(
    val agentId: String,
    val agentSessionId: String,
    /** 关联到的 octoterm 托管会话 id */
    val session: Long?,
    val state: AgentState,
    val detail: String?,
    /** 有值 = 正在等人回答 */
    val pending: String?,
    /** 这家 agent 的对话记录能不能读。`agent-event` 不带它,只有 HTTP 快照给 —— 所以可为 null */
    val supportsTranscript: Boolean?,
) {
    val key: String get() = "$agentId $agentSessionId"
}

data class AgentSnapshot(
    val sessions: List<AgentSession>,
    val transcriptEnabled: Boolean,
)

object AgentSessions {
    /**
     * 把一条 `agent-event` 并进表里。`done` 的直接移除 —— 它已经结束了。
     * 事件不携带 `supports_transcript`,保留快照已经给出的值。
     */
    fun applyEvent(map: Map<String, AgentSession>, ev: AgentSession): Map<String, AgentSession> {
        val next = LinkedHashMap(map)
        if (ev.state == AgentState.Done) {
            next.remove(ev.key)
        } else {
            val prev = map[ev.key]
            next[ev.key] = ev.copy(supportsTranscript = ev.supportsTranscript ?: prev?.supportsTranscript)
        }
        return next
    }

    fun replaceAll(list: List<AgentSession>): Map<String, AgentSession> {
        val next = LinkedHashMap<String, AgentSession>()
        for (s in list) if (s.state != AgentState.Done) next[s.key] = s
        return next
    }

    /** 这个托管会话该显示哪个 agent 的状态:状态优先级最高的那个。 */
    fun forSession(map: Map<String, AgentSession>, sessionId: Long): AgentSession? =
        map.values.filter { it.session == sessionId }.maxByOrNull { it.state.priority }

    /**
     * 聊天视图该看哪个 agent。一个终端里先后跑过两家 agent 是常事,按状态挑出来的未必能读
     * 记录 —— 读得了的优先;都读不了才回退到状态最高的那个,好让回落文案仍能说出是哪一家。
     */
    fun chatAgentFor(map: Map<String, AgentSession>, sessionId: Long): AgentSession? =
        map.values
            .filter { it.session == sessionId && it.supportsTranscript == true }
            .maxByOrNull { it.state.priority }
            ?: forSession(map, sessionId)

    /** 聊天入口既要本会话里有读得了记录的 agent,也要服务端明确允许读。 */
    fun chatAvailable(map: Map<String, AgentSession>, sessionId: Long?, transcriptEnabled: Boolean): Boolean =
        transcriptEnabled && sessionId != null && chatAgentFor(map, sessionId)?.supportsTranscript == true

    /** 协议里的一条 agent 会话(快照条目与 `agent-event` 同形)。缺关键字段就丢掉。 */
    fun parseSession(obj: JSONObject): AgentSession? {
        val agentId = obj.optString("agent_id")
        val agentSessionId = obj.optString("agent_session_id")
        if (agentId.isEmpty() || agentSessionId.isEmpty()) return null
        return AgentSession(
            agentId = agentId,
            agentSessionId = agentSessionId,
            session = if (obj.has("session") && !obj.isNull("session")) obj.optLong("session") else null,
            state = AgentState.parse(obj.optString("state")),
            detail = obj.optNullableString("detail"),
            pending = obj.optNullableString("pending"),
            supportsTranscript = if (obj.has("supports_transcript") && !obj.isNull("supports_transcript")) {
                obj.optBoolean("supports_transcript")
            } else {
                null
            },
        )
    }

    /** 坏响应、旧服务端一律当「没有会话、不许读」—— 聊天入口隐藏,而不是卡住。 */
    fun parseSnapshot(body: String): AgentSnapshot {
        val root = try {
            JSONObject(body)
        } catch (_: Exception) {
            return UNAVAILABLE
        }
        val array = root.optJSONArray("sessions")
        val sessions = buildList {
            if (array != null) {
                for (i in 0 until array.length()) {
                    array.optJSONObject(i)?.let(::parseSession)?.let(::add)
                }
            }
        }
        // 只认明确的 true:旧服务端没有这个字段
        return AgentSnapshot(sessions, root.opt("transcript_enabled") == true)
    }

    /** 协议 T12:路由不存在或失败都降级,不报错。 */
    fun fetch(http: OkHttpClient, origin: String, token: String): AgentSnapshot {
        return try {
            val request = Request.Builder()
                .url(origin.trimEnd('/') + "/api/agents/sessions")
                .header("Authorization", "Bearer $token")
                .build()
            http.newCall(request).execute().use { response ->
                if (!response.isSuccessful) return UNAVAILABLE
                parseSnapshot(response.body?.string().orEmpty())
            }
        } catch (_: Exception) {
            UNAVAILABLE
        }
    }

    private val UNAVAILABLE = AgentSnapshot(emptyList(), false)
}

internal fun JSONObject.optNullableString(name: String): String? =
    if (has(name) && !isNull(name)) optString(name) else null
