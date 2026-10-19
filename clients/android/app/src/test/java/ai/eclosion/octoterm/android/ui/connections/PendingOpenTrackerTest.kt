package ai.eclosion.octoterm.android.ui.connections

import ai.eclosion.octoterm.android.wire.SessionInfo
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class PendingOpenTrackerTest {
    private fun session(id: Long) = SessionInfo(id = id, name = "s$id", cols = 80, rows = 24, createdAt = 0)
    private val target = PendingOpen(connectionId = "a", session = 1L, notificationTag = "srv:1:codex")

    private fun tapped() = PendingOpenTracker().apply {
        onTap(target)
        onConnectStart("a")
    }

    @Test
    fun connectFailureClearsIt() {
        val t = tapped()
        t.onConnectFailed()
        assertNull(t.current)
        assertNull(t.onSessions(AppScreen.Server("a"), listOf(session(1))))
    }

    @Test
    fun disconnectClearsIt() {
        val t = tapped()
        t.onDisconnect()
        assertNull(t.current)
        assertNull(t.onSessions(AppScreen.Server("a"), listOf(session(1))))
    }

    @Test
    fun connectingToAnotherServerDropsIt() {
        val t = tapped()
        t.onConnectStart("b")
        assertNull(t.current)
    }

    @Test
    fun connectingToTheTargetKeepsIt() {
        val t = PendingOpenTracker()
        t.onTap(target)
        t.onConnectStart("a")
        assertEquals(target, t.current)
    }

    @Test
    fun sessionsForTheTargetAreConsumedOnce() {
        val t = tapped()
        assertEquals(
            PendingOpenDecision.Open(1L, "srv:1:codex"),
            t.onSessions(AppScreen.Server("a"), listOf(session(1))),
        )
        assertNull(t.current)
        assertNull(t.onSessions(AppScreen.Server("a"), listOf(session(1))))
    }

    @Test
    fun sessionsGoneIsAlsoConsumedOnce() {
        val t = tapped()
        assertEquals(PendingOpenDecision.SessionGone, t.onSessions(AppScreen.Server("a"), listOf(session(2))))
        assertNull(t.onSessions(AppScreen.Server("a"), listOf(session(2))))
    }

    @Test
    fun sessionsForAnotherConnectionAreIgnored() {
        val t = tapped()
        assertNull(t.onSessions(AppScreen.Server("b"), listOf(session(1))))
        assertNull(t.onSessions(AppScreen.List, listOf(session(1))))
        assertEquals(target, t.current)
    }

    @Test
    fun latestTapWins() {
        val t = tapped()
        val other = PendingOpen("b", 9L, null)
        t.onTap(other)
        assertEquals(other, t.current)
    }
}
