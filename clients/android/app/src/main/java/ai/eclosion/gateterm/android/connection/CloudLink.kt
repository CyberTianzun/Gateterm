package ai.eclosion.gateterm.android.connection

/** 推送里的 server_id ↔ 本地连接。纯函数,便于单测。 */
object CloudLink {
    /** 同一台 server 可能存了多条连接(不同地址):取最近用过的那条。 */
    fun find(items: List<ServerConnection>, cloudServerId: String): ServerConnection? =
        items.filter { it.cloudServerId == cloudServerId }.maxByOrNull { it.lastUsedAt ?: 0L }

    /** 把握手拿到的 id 记到连接上;没有变化返回 null,调用方据此跳过持久化。 */
    fun record(items: List<ServerConnection>, connectionId: String, cloudServerId: String?): List<ServerConnection>? {
        val current = items.firstOrNull { it.id == connectionId } ?: return null
        if (current.cloudServerId == cloudServerId) return null
        return items.map { if (it.id == connectionId) it.copy(cloudServerId = cloudServerId) else it }
    }
}
