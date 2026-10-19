package ai.eclosion.octoterm.android.push

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class AgentPushTest {
    private val base = mapOf("server_id" to "srv", "session" to "7", "agent_id" to "claude-code", "kind" to "waiting")

    @Test
    fun parsesWaiting() {
        val p = AgentPush.parse(base)!!
        assertEquals(AgentPush("srv", 7, "claude-code", AgentPush.Kind.Waiting, null, null), p)
        assertEquals("srv:7:claude-code", p.tag)
        assertEquals("srv:7:claude-code".hashCode(), p.notificationId)
    }

    @Test
    fun parsesPermissionWithToolAndExpiry() {
        val p = AgentPush.parse(base + mapOf("kind" to "permission", "tool_name" to "Bash", "expires_at" to "1000"))!!
        assertEquals(AgentPush.Kind.Permission, p.kind)
        assertEquals("Bash", p.toolName)
        assertEquals(400_000L, p.timeoutMillis(nowSeconds = 600))
        assertEquals(0L, p.timeoutMillis(nowSeconds = 2000))
        assertNull(AgentPush.parse(base)!!.timeoutMillis(600))
    }

    @Test
    fun ignoresDoneUnknownAndBroken() {
        assertNull(AgentPush.parse(base + ("kind" to "done")))
        assertNull(AgentPush.parse(base + ("kind" to "boom")))
        assertNull(AgentPush.parse(base - "server_id"))
        assertNull(AgentPush.parse(base + ("session" to "x")))
        assertEquals(AgentPush.Kind.Resolved, AgentPush.parse(base + ("kind" to "resolved"))!!.kind)
    }

    @Test
    fun displayNames() {
        assertEquals("Claude Code", agentDisplayName("claude-code"))
        assertEquals("Codex", agentDisplayName("codex"))
        assertEquals("some-new-agent", agentDisplayName("some-new-agent"))
    }
}
