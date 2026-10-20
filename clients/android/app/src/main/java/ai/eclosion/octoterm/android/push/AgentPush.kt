package ai.eclosion.octoterm.android.push

/** FCM 数据消息 → 结构化推送。只有路由所需的最小信息;文案在本地生成。 */
data class AgentPush(
    val serverId: String,
    val session: Long,
    val agentId: String,
    val kind: Kind,
    val toolName: String?,
    val expiresAt: Long?,
) {
    enum class Kind { Waiting, Permission, Done, Resolved }

    /** 收到推送后该做什么:撤回这条通知,或发出(可带到期自动消失的毫秒数)。 */
    sealed class Display {
        data object Cancel : Display()
        data class Post(val timeoutMillis: Long?) : Display()
    }

    /** 同一会话里同一个 agent 只占一条通知:新的覆盖旧的,resolved 按它撤回。
     *  带上 agent_id:一个终端会话里跑着两个 agent 时,一个的撤回不该误伤另一个。 */
    val tag: String get() = tag(serverId, session, agentId)
    val notificationId: Int get() = notificationId(tag)

    /** 到期自动消失的剩余毫秒;没有过期时间返回 null,已过期返回 0。 */
    fun timeoutMillis(nowSeconds: Long): Long? = expiresAt?.let { ((it - nowSeconds) * 1000).coerceAtLeast(0) }

    /** resolved 撤回;已经过期的(比如推送迟到)也撤回,而不是发一条马上作废的通知。 */
    fun display(nowSeconds: Long): Display {
        if (kind == Kind.Resolved) return Display.Cancel
        val timeout = timeoutMillis(nowSeconds)
        return if (timeout == 0L) Display.Cancel else Display.Post(timeout)
    }

    companion object {
        fun tag(serverId: String, session: Long, agentId: String): String = "$serverId:$session:$agentId"
        fun notificationId(tag: String): Int = tag.hashCode()

        fun parse(data: Map<String, String>): AgentPush? {
            val serverId = data["server_id"]?.takeIf { it.isNotEmpty() } ?: return null
            val session = data["session"]?.toLongOrNull() ?: return null
            val agentId = data["agent_id"]?.takeIf { it.isNotEmpty() } ?: return null
            val kind = when (data["kind"]) {
                "waiting" -> Kind.Waiting
                "permission" -> Kind.Permission
                "done" -> Kind.Done
                "resolved" -> Kind.Resolved
                else -> return null
            }
            return AgentPush(serverId, session, agentId, kind, data["tool_name"], data["expires_at"]?.toLongOrNull())
        }
    }
}

fun agentDisplayName(agentId: String): String = when (agentId) {
    "claude-code" -> "Claude Code"
    "codex" -> "Codex"
    "grok" -> "Grok"
    "pi" -> "Pi"
    "antigravity" -> "Antigravity"
    else -> agentId
}
