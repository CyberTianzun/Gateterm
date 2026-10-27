package ai.eclosion.gateterm.android.agent

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Test

class AgentHistoryTest {
    @Test
    fun parsesAvailabilityNullsAndDuplicateIdentities() {
        val row = """{"agent_id":"codex","agent_name":"Codex","agent_session_id":"known","title":"Title","cwd":null,"updated_at":42,"terminal_session":null,"can_resume":false,"unavailable_reason":"missing-directory"}"""
        val page = AgentHistory.parsePage("""{"sessions":[$row,$row],"agents":[{"id":"codex","name":"Codex"}],"total":1,"next_offset":null,"truncated":true,"skipped":2}""")
        assertEquals(1, page.sessions.size)
        assertNull(page.sessions.single().cwd)
        assertNull(page.sessions.single().terminalSession)
        assertFalse(page.sessions.single().canResume)
        assertNull(page.nextOffset)
        assertEquals(2, page.skipped)
        assertEquals("Codex", page.agents.single().name)
    }

    @Test
    fun openPayloadContainsOnlyIdentity() {
        val entry = HistoryEntry("pi", "pi", "known", "Title", "/original", 42, 7, true, null)
        val payload = AgentHistory.identity(entry)
        assertEquals(setOf("agent_id", "agent_session_id"), payload.keys().asSequence().toSet())
        assertEquals("known", payload.getString("agent_session_id"))
    }

    @Test
    fun missingCapabilityDoesNotEnableResume() {
        val page = AgentHistory.parsePage("""{"sessions":[{"agent_id":"codex","agent_name":"Codex","agent_session_id":"x","title":"T","terminal_session":5}],"total":1,"next_offset":50}""")
        assertFalse(page.sessions.single().canResume)
        assertEquals(5L, page.sessions.single().terminalSession)
        assertEquals(50, page.nextOffset)
    }

    @Test
    fun backgroundIndexProgressIsDistinctFromAnEmptyHistory() {
        val page = AgentHistory.parsePage("""{"sessions":[],"total":0,"next_offset":null,"indexing":true,"max_sessions":1000}""")
        assertEquals(true, page.indexing)
        assertEquals(1000, page.maxSessions)
        assertEquals(0, page.total)
    }

    @Test
    fun parsesSessionDescriptionsModelsAndWorkspaceFilters() {
        val page = AgentHistory.parsePage("""{"sessions":[{"agent_id":"codex","agent_name":"Codex","agent_session_id":"known","title":"Fix history","description":"The history dialog is fixed","model":"gpt-test","cwd":"/work/project"}],"workspaces":["/work/project"],"total":1}""")
        assertEquals("The history dialog is fixed", page.sessions.single().description)
        assertEquals("gpt-test", page.sessions.single().model)
        assertEquals(listOf("/work/project"), page.workspaces)
    }
}
