package ai.eclosion.octoterm.android.ui.connections

import ai.eclosion.octoterm.android.wire.SessionInfo

/** 点通知要去的地方:本地连接 id、会话 id,以及打开后要撤回的那条通知的 tag(旧通知可能没有)。 */
internal data class PendingOpen(val connectionId: String, val session: Long, val notificationTag: String?)

/** `consumePendingOpen` 该做什么:打开会话、报告会话已消失,或者(`null`)还没到时候,继续等。纯函数,便于单测。 */
internal sealed class PendingOpenDecision {
    data class Open(val session: Long, val notificationTag: String? = null) : PendingOpenDecision()
    data object SessionGone : PendingOpenDecision()
}

/**
 * 给定点通知要去的目标、当前屏幕与会话列表,决定下一步。
 * 还没连到目标连接(或者根本没有待处理的跳转)时返回 `null` —— 这表示"等下一次 Sessions 快照再看"。
 */
internal fun decidePendingOpen(
    pendingOpen: PendingOpen?,
    screen: AppScreen,
    sessions: List<SessionInfo>,
): PendingOpenDecision? {
    val target = pendingOpen ?: return null
    if (screen !is AppScreen.Server || screen.connectionId != target.connectionId) return null
    return if (sessions.any { it.id == target.session }) {
        PendingOpenDecision.Open(target.session, target.notificationTag)
    } else {
        PendingOpenDecision.SessionGone
    }
}

/**
 * 点通知跳转的生命周期。跳转只属于为它发起的那次连接:连接失败/超时/断开/改连别的服务器都作废,
 * 免得很久以后用户手动连上同一台服务器时突然被拽进某个会话。拿到目标连接的会话列表后只消费一次。
 */
internal class PendingOpenTracker {
    var current: PendingOpen? = null
        private set

    /** 用户最新的一次点击说了算。 */
    fun onTap(target: PendingOpen) {
        current = target
    }

    /** 开始连某个连接:目标不是它,说明用户改主意了。 */
    fun onConnectStart(connectionId: String) {
        if (current?.connectionId != connectionId) current = null
    }

    /** 连接没起来(含握手超时、致命错误、connect 提前返回)。 */
    fun onConnectFailed() {
        current = null
    }

    fun onDisconnect() {
        current = null
    }

    /** 收到会话列表:到了目标连接就给出决定并清掉;否则继续等。 */
    fun onSessions(screen: AppScreen, sessions: List<SessionInfo>): PendingOpenDecision? {
        val decision = decidePendingOpen(current, screen, sessions) ?: return null
        current = null
        return decision
    }
}
