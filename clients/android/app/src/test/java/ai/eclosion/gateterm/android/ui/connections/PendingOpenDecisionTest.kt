package ai.eclosion.gateterm.android.ui.connections

import ai.eclosion.gateterm.android.wire.SessionInfo
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class PendingOpenDecisionTest {
    private fun session(id: Long) = SessionInfo(id = id, name = "s$id", cols = 80, rows = 24, createdAt = 0)

    @Test
    fun noPendingOpenMeansNothingToDo() {
        assertNull(decidePendingOpen(null, AppScreen.Server("a"), listOf(session(1))))
    }

    @Test
    fun waitsUntilOnTheTargetConnectionsServerScreen() {
        val pending = PendingOpen("a", 1L, null)
        assertNull(decidePendingOpen(pending, AppScreen.List, listOf(session(1))))
        assertNull(decidePendingOpen(pending, AppScreen.Server("other"), listOf(session(1))))
    }

    @Test
    fun opensTheSessionWhenItIsInTheList() {
        val pending = PendingOpen("a", 1L, null)
        val decision = decidePendingOpen(pending, AppScreen.Server("a"), listOf(session(1), session(2)))
        assertEquals(PendingOpenDecision.Open(1L), decision)
    }

    @Test
    fun reportsSessionGoneWhenItIsMissingFromTheList() {
        val pending = PendingOpen("a", 1L, null)
        val decision = decidePendingOpen(pending, AppScreen.Server("a"), listOf(session(2)))
        assertEquals(PendingOpenDecision.SessionGone, decision)
    }

    @Test
    fun reportsSessionGoneWhenTheListIsEmpty() {
        val pending = PendingOpen("a", 1L, null)
        val decision = decidePendingOpen(pending, AppScreen.Server("a"), emptyList())
        assertEquals(PendingOpenDecision.SessionGone, decision)
    }
}
