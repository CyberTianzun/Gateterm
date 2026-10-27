package ai.eclosion.gateterm.android.agent

import ai.eclosion.gateterm.android.wire.ControlJson
import ai.eclosion.gateterm.android.wire.ServerMsg
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.charset.StandardCharsets

class AgentSessionsTest {
    private fun s(
        agent: String,
        sid: String,
        session: Long?,
        state: AgentState = AgentState.Idle,
        supports: Boolean? = null,
    ) = AgentSession(agent, sid, session, state, null, null, supports)

    @Test
    fun snapshotParsesSessionsAndOnlyTrustsExplicitTranscriptFlag() {
        val snap = AgentSessions.parseSnapshot(
            """{"sessions":[
                 {"agent_id":"codex","agent_session_id":"a","session":2,"state":"working","detail":"Bash",
                  "pending":null,"supports_transcript":true},
                 {"agent_id":"","agent_session_id":"x","state":"idle"}
               ],"transcript_enabled":true}""",
        )
        assertTrue(snap.transcriptEnabled)
        assertEquals(1, snap.sessions.size)
        val a = snap.sessions[0]
        assertEquals(2L, a.session)
        assertEquals(AgentState.Working, a.state)
        assertEquals("Bash", a.detail)
        assertNull(a.pending)
        assertEquals(true, a.supportsTranscript)

        assertFalse(AgentSessions.parseSnapshot("""{"sessions":[]}""").transcriptEnabled)
        assertFalse(AgentSessions.parseSnapshot("""{"sessions":[],"transcript_enabled":"true"}""").transcriptEnabled)
        assertFalse(AgentSessions.parseSnapshot("not json").transcriptEnabled)
    }

    @Test
    fun agentEventKeepsTranscriptSupportFromSnapshot() {
        var map = AgentSessions.replaceAll(listOf(s("codex", "a", 2, supports = true)))
        val payload = """{"type":"agent-event","agent_id":"codex","agent_session_id":"a","session":2,
            "state":"thinking","pending":null,"detail":null}"""
        val msg = ControlJson.parse(payload.toByteArray(StandardCharsets.UTF_8))
        assertTrue(msg is ServerMsg.AgentEvent)
        map = AgentSessions.applyEvent(map, (msg as ServerMsg.AgentEvent).session)
        assertEquals(AgentState.Thinking, map["codex a"]?.state)
        assertEquals(true, map["codex a"]?.supportsTranscript)
    }

    @Test
    fun doneRemovesTheSession() {
        var map = AgentSessions.replaceAll(listOf(s("codex", "a", 2), s("claude-code", "b", 2, AgentState.Done)))
        assertEquals(setOf("codex a"), map.keys)
        map = AgentSessions.applyEvent(map, s("codex", "a", 2, AgentState.Done))
        assertTrue(map.isEmpty())
    }

    @Test
    fun sessionListStatusTracksEventsAndFallsBackAfterAgentExit() {
        var map = AgentSessions.replaceAll(listOf(
            s("codex", "active", 7, AgentState.Thinking),
            s("claude-code", "idle", 7),
            s("pi", "elsewhere", 8, AgentState.Waiting),
            s("grok", "unmanaged", null, AgentState.Waiting),
        ))
        assertEquals("codex", AgentSessions.forSession(map, 7)?.agentId)
        val working = s("codex", "active", 7, AgentState.Working).copy(detail = "Bash")
        map = AgentSessions.applyEvent(map, working)
        assertEquals(working, AgentSessions.forSession(map, 7))
        val waiting = working.copy(state = AgentState.Waiting, pending = "approval-1")
        map = AgentSessions.applyEvent(map, waiting)
        assertEquals(waiting, AgentSessions.forSession(map, 7))
        map = AgentSessions.applyEvent(map, waiting.copy(state = AgentState.Done))
        assertEquals("claude-code", AgentSessions.forSession(map, 7)?.agentId)
        assertEquals(AgentState.Idle, AgentSessions.forSession(map, 7)?.state)
        map = AgentSessions.replaceAll(listOf(s("codex", "after-reconnect", 7)))
        assertEquals("after-reconnect", AgentSessions.forSession(map, 7)?.agentSessionId)
        assertNull(AgentSessions.forSession(map, 7)?.pending)
        assertNull(AgentSessions.forSession(map, 99))
    }

    @Test
    fun sessionListEmphasizesTheAgentNeedingAttentionRegardlessOfTranscriptSupport() {
        val states = listOf(AgentState.Idle, AgentState.Thinking, AgentState.Working, AgentState.Error, AgentState.Waiting)
        var map = AgentSessions.replaceAll(states.map { state ->
            s("agent-${state.wire}", state.wire, 7, state, supports = state == AgentState.Idle)
        })
        for (state in states.reversed()) {
            val selected = AgentSessions.forSession(map, 7)!!
            assertEquals(state, selected.state)
            map = AgentSessions.applyEvent(map, selected.copy(state = AgentState.Done))
        }
        assertNull(AgentSessions.forSession(map, 7))
    }

    @Test
    fun malformedAgentEventIsIgnoredNotFatal() {
        val msg = ControlJson.parse("""{"type":"agent-event","state":"idle"}""".toByteArray(StandardCharsets.UTF_8))
        assertTrue(msg is ServerMsg.Unknown)
    }

    /** 先 codex 后 claude:按状态挑出来的那个读不了记录时,聊天要看读得了的那个 */
    @Test
    fun chatPrefersAReadableAgentOverAHigherPriorityOne() {
        val map = AgentSessions.replaceAll(
            listOf(
                s("pi", "p", 3, AgentState.Waiting, supports = false),
                s("codex", "c", 3, AgentState.Idle, supports = true),
            ),
        )
        assertEquals("pi", AgentSessions.forSession(map, 3)?.agentId)
        assertEquals("codex", AgentSessions.chatAgentFor(map, 3)?.agentId)
        assertTrue(AgentSessions.chatAvailable(map, 3, transcriptEnabled = true))
        assertFalse(AgentSessions.chatAvailable(map, 3, transcriptEnabled = false))
        assertFalse(AgentSessions.chatAvailable(map, 4, transcriptEnabled = true))
        assertFalse(AgentSessions.chatAvailable(map, null, transcriptEnabled = true))
    }

    /** 事件先于快照到达时还不知道能不能读 —— 不开入口,等快照补齐 */
    @Test
    fun unknownTranscriptSupportDoesNotOpenChat() {
        val map = AgentSessions.applyEvent(emptyMap(), s("codex", "a", 2))
        assertFalse(AgentSessions.chatAvailable(map, 2, transcriptEnabled = true))
    }
}
