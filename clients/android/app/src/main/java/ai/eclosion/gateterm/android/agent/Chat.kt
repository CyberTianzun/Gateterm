package ai.eclosion.gateterm.android.agent

import kotlinx.coroutines.CancellationException
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.OkHttpClient
import okhttp3.Request
import org.json.JSONObject

/**
 * 聊天视图的数据层。
 *
 * 消息来自 agent 自己写的对话记录(服务端读、归一化),走 HTTP 拉取 —— 一段对话可以是
 * 几 MB,协议 R4 不许在控制通道走大块数据。**增量由 `agent-event` 触发**:有事件才可能
 * 有新消息,所以不做轮询。和 web 端 `chat.ts` 一一对应。
 */
enum class ChatRole { User, Assistant, System }

sealed class ChatBlock {
    data class Text(val text: String) : ChatBlock()
    data class Thinking(val text: String) : ChatBlock()
    /** `input` 是服务端给的「给人看的一行」,不是原始 JSON */
    data class ToolUse(val name: String, val input: String) : ChatBlock()
    data class ToolResult(val ok: Boolean, val text: String) : ChatBlock()
}

data class ChatMessage(
    val id: String,
    val role: ChatRole,
    val ts: Long?,
    val blocks: List<ChatBlock>,
)

const val MAX_AUTO_OLDER_PAGES = 16

private val HARNESS_USER_PREFIXES = listOf(
    "# AGENTS.md instructions",
    "<environment_context>",
    "<permissions instructions>",
    "<developer_instructions>",
    "<system-reminder>",
    "<local-command-caveat>",
    "<command-name>",
    "This session is being continued from a previous conversation",
)

private const val CODEX_REQUEST_MARKER = "## My request for Codex:"

/** 判断 role="user" 的文本块是否为空白或框架注入上下文（对齐服务端 history::prompt_body）。 */
fun isHarnessUserText(text: String): Boolean {
    val trimmed = text.trim()
    if (trimmed.isEmpty()) return true
    if (HARNESS_USER_PREFIXES.any { trimmed.startsWith(it) }) return true
    val markerIdx = trimmed.indexOf(CODEX_REQUEST_MARKER)
    if (markerIdx != -1 && trimmed.substring(markerIdx + CODEX_REQUEST_MARKER.length).trim().isEmpty()) {
        return true
    }
    return false
}

/** 仅提取用户主动发出的非空正文块，剔除同条里的 tool-result、空白或框架注入上下文。 */
fun ChatMessage.userPromptTextBlocks(): List<ChatBlock.Text> {
    if (role != ChatRole.User) return emptyList()
    return blocks.filterIsInstance<ChatBlock.Text>().filter { it.text.isNotBlank() && !isHarnessUserText(it.text) }
}

/**
 * 判断一条消息是否为用户真正输入的一轮提示（而非工具回传/空白占位的 user turn）。
 * 不少 agent 的记录会把 tool-result 甚至 Pi 的 [tool-use, tool-result] 记成 role="user"。
 */
fun ChatMessage.isUserPrompt(): Boolean = userPromptTextBlocks().isNotEmpty()

/** 读不到对话记录时的原因。服务端给的是机器可读的键,文案在 UI 层按语言渲染。 */
enum class ChatFallback(val wire: String) {
    Disabled("disabled"),
    NoTranscriptPath("no-transcript-path"),
    UnsupportedAgent("unsupported-agent"),
    Unreadable("unreadable"),
    ParseFailed("parse-failed"),
    ;

    companion object {
        /** 认不出的原因也要说人话 —— 归到「读不到」,而不是把机器键怼给用户 */
        fun parse(s: String): ChatFallback = entries.firstOrNull { it.wire == s } ?: Unreadable
    }
}

sealed class ChatWindow {
    /** `cursor` 是不透明的:原样带回,**不解析**(协议 A9) */
    data class Transcript(
        val messages: List<ChatMessage>,
        val cursor: String,
        val reset: Boolean,
        val more: Boolean,
        val olderCursor: String? = null,
        val historySupported: Boolean = false,
    ) : ChatWindow()

    data class Terminal(val reason: ChatFallback) : ChatWindow()
}

enum class OlderLoadStop {
    FoundUserPrompt,
    Beginning,
    PageLimit,
    Reset,
    UnsupportedOldServer,
    Failed,
    Cancelled,
}

data class OlderLoadResult(val stop: OlderLoadStop, val pagesFetched: Int)

object Chat {
    fun parseWindow(body: String): ChatWindow {
        val root = try {
            JSONObject(body)
        } catch (_: Exception) {
            return ChatWindow.Terminal(ChatFallback.Unreadable)
        }
        if (root.optString("source") != "transcript") {
            return ChatWindow.Terminal(ChatFallback.parse(root.optString("reason")))
        }
        val array = root.optJSONArray("messages")
        val messages = buildList {
            if (array != null) {
                for (i in 0 until array.length()) {
                    array.optJSONObject(i)?.let(::parseMessage)?.let(::add)
                }
            }
        }
        return ChatWindow.Transcript(
            messages = messages,
            cursor = root.optString("cursor"),
            reset = root.optBoolean("reset"),
            more = root.optBoolean("more"),
            olderCursor = root.optString("older_cursor").takeUnless { it.isEmpty() || it == "null" },
            historySupported = root.has("older_cursor"),
        )
    }

    private fun parseMessage(obj: JSONObject): ChatMessage? {
        val id = obj.optString("id").ifEmpty { return null }
        val role = when (obj.optString("role")) {
            "user" -> ChatRole.User
            "assistant" -> ChatRole.Assistant
            else -> ChatRole.System
        }
        val array = obj.optJSONArray("blocks")
        val blocks = buildList {
            if (array != null) {
                for (i in 0 until array.length()) {
                    val b = array.optJSONObject(i) ?: continue
                    // 认不出的块跳过:新版服务端加了块类型,旧客户端照样能看其余部分
                    when (b.optString("kind")) {
                        "text" -> add(ChatBlock.Text(b.optString("text")))
                        "thinking" -> add(ChatBlock.Thinking(b.optString("text")))
                        "tool-use" -> add(ChatBlock.ToolUse(b.optString("name"), b.optString("input")))
                        "tool-result" -> add(ChatBlock.ToolResult(b.optBoolean("ok", true), b.optString("text")))
                    }
                }
            }
        }
        val ts = if (obj.has("ts") && !obj.isNull("ts")) obj.optLong("ts") else null
        return ChatMessage(id, role, ts, blocks)
    }

    private fun uniqueIncoming(base: List<ChatMessage>, messages: List<ChatMessage>): List<ChatMessage> {
        val seen = base.mapTo(HashSet()) { it.id }
        return messages.filter { seen.add(it.id) }
    }

    /**
     * 把一次拉取合并进已有的消息。
     *
     * `reset` 是**整段替换**:服务端认定旧游标失效(记录被 compact 了、或换了会话),旧内容和
     * 新内容不是同一条时间线。追加时按 id 去重:窗口边界上的那条可能被重复送来。
     *
     * 同一批里也去重:拿不到 agent 自己的 id 时服务端按内容合成,两条一模一样的消息会撞 id,
     * 而列表按 id 作 key,重复的 key 会让界面直接崩掉。
     */
    fun merge(prev: List<ChatMessage>, win: ChatWindow, prepend: Boolean = false): List<ChatMessage> {
        if (win !is ChatWindow.Transcript) return prev
        val base = if (win.reset) emptyList() else prev
        val incoming = uniqueIncoming(base, win.messages)
        return if (prepend) incoming + base else base + incoming
    }

    /** 计算本次向后翻页真正新增（未与已有消息重复）的前置消息集合。 */
    fun newlyPrepended(prev: List<ChatMessage>, win: ChatWindow): List<ChatMessage> {
        if (win !is ChatWindow.Transcript) return emptyList()
        val base = if (win.reset) emptyList() else prev
        return uniqueIncoming(base, win.messages)
    }

    /** 筛出所有用户输入的消息并保持原始时间顺序。 */
    fun userPrompts(messages: List<ChatMessage>): List<ChatMessage> =
        messages.filter { it.isUserPrompt() }

    /** 判断消息列表中是否已包含至少一条用户主动提问。 */
    fun hasUserPrompt(messages: List<ChatMessage>): Boolean =
        messages.any { it.isUserPrompt() }

    /**
     * 自动连续向后拉取更早消息，直到新前置的消息中包含用户主动提问、到达开头、
     * 触及单次上限（16 页）、遇到 reset、失败或被取消为止。
     */
    suspend fun loadOlderUntilUserPrompt(
        initialCursor: String?,
        maxPages: Int = MAX_AUTO_OLDER_PAGES,
        isHistoryPreview: Boolean = false,
        isCancelled: () -> Boolean,
        fetchPage: suspend (String) -> ChatWindow?,
        getMessages: () -> List<ChatMessage>,
        onPage: (ChatWindow.Transcript, List<ChatMessage>) -> Unit,
        onReset: (ChatWindow.Transcript) -> Unit,
    ): OlderLoadResult {
        var cursor = initialCursor
        var pagesFetched = 0
        while (true) {
            if (isCancelled()) return OlderLoadResult(OlderLoadStop.Cancelled, pagesFetched)
            if (cursor.isNullOrEmpty()) return OlderLoadResult(OlderLoadStop.Beginning, pagesFetched)
            if (pagesFetched >= maxPages) return OlderLoadResult(OlderLoadStop.PageLimit, pagesFetched)

            val win = try {
                fetchPage(cursor)
            } catch (cancel: CancellationException) {
                throw cancel
            } catch (_: Exception) {
                if (isCancelled()) return OlderLoadResult(OlderLoadStop.Cancelled, pagesFetched)
                return OlderLoadResult(OlderLoadStop.Failed, pagesFetched)
            }
            if (isCancelled()) return OlderLoadResult(OlderLoadStop.Cancelled, pagesFetched)
            if (win !is ChatWindow.Transcript) return OlderLoadResult(OlderLoadStop.Failed, pagesFetched)

            val nextCursor = win.olderCursor
            if (win.reset) {
                if (nextCursor == cursor) {
                    val stop = if (isHistoryPreview) OlderLoadStop.UnsupportedOldServer else OlderLoadStop.Failed
                    return OlderLoadResult(stop, pagesFetched)
                }
                onReset(win)
                return OlderLoadResult(OlderLoadStop.Reset, pagesFetched)
            }
            if (nextCursor == cursor) return OlderLoadResult(OlderLoadStop.Failed, pagesFetched)

            val incoming = newlyPrepended(getMessages(), win)
            pagesFetched += 1
            onPage(win, incoming)
            if (hasUserPrompt(incoming)) return OlderLoadResult(OlderLoadStop.FoundUserPrompt, pagesFetched)
            cursor = nextCursor
        }
    }

    /** 老版本服务端没有这个路由:降级成「读不到」,不是报错(协议 T12)。 */
    fun fetch(
        http: OkHttpClient,
        origin: String,
        token: String,
        agentId: String,
        agentSessionId: String,
        after: String?,
        before: String? = null,
    ): ChatWindow {
        val unreadable = ChatWindow.Terminal(ChatFallback.Unreadable)
        return try {
            val url = (origin.trimEnd('/') + "/api/agents/messages").toHttpUrlOrNull()
                ?.newBuilder()
                ?.addQueryParameter("agent_id", agentId)
                ?.addQueryParameter("agent_session_id", agentSessionId)
                ?.apply { if (after != null) addQueryParameter("after", after) }
                ?.apply { if (!before.isNullOrEmpty()) addQueryParameter("before", before) }
                ?.build()
                ?: return unreadable
            val request = Request.Builder().url(url).header("Authorization", "Bearer $token").build()
            http.newCall(request).execute().use { response ->
                if (!response.isSuccessful) return unreadable
                parseWindow(response.body?.string().orEmpty())
            }
        } catch (_: Exception) {
            unreadable
        }
    }
}
