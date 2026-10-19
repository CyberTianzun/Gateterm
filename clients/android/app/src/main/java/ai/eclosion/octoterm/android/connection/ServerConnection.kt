package ai.eclosion.octoterm.android.connection

data class ServerConnection(
    val id: String,
    val name: String,
    val url: String,
    val token: String,
    val createdAt: Long,
    val lastUsedAt: Long? = null,
    /** server 登录 Octoterm Cloud 后在握手里回带的 id;推送靠它找回这条连接 */
    val cloudServerId: String? = null,
) {
    fun title(): String {
        val trimmed = name.trim()
        return trimmed.ifEmpty { ConnectionUrl.displayHost(url) }
    }

    fun subtitle(): String = ConnectionUrl.displayHost(url)
}
