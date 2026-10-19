package ai.eclosion.octoterm.android.connection

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class CloudLinkTest {
    private fun conn(id: String, cloud: String?, lastUsed: Long?) =
        ServerConnection(id = id, name = id, url = "http://$id", token = "t", createdAt = 0, lastUsedAt = lastUsed, cloudServerId = cloud)

    @Test
    fun findPicksMostRecentlyUsedMatch() {
        val items = listOf(conn("a", "s1", 10), conn("b", "s1", 20), conn("c", "s2", 30))
        assertEquals("b", CloudLink.find(items, "s1")?.id)
        assertNull(CloudLink.find(items, "nope"))
    }

    @Test
    fun recordUpdatesOnlyWhenChanged() {
        val items = listOf(conn("a", null, null), conn("b", "s1", null))
        val updated = CloudLink.record(items, "a", "s9")!!
        assertEquals("s9", updated.first { it.id == "a" }.cloudServerId)
        assertNull(CloudLink.record(updated, "a", "s9"))
        // server 退出 Cloud 后不再回带:清空
        assertNull(CloudLink.record(items, "b", "s1"))
        assertEquals(null, CloudLink.record(items, "b", null)!!.first { it.id == "b" }.cloudServerId)
    }
}
