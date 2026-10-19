package ai.eclosion.octoterm.android.push

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.cancel
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class PushTimeoutTest {
    @Test
    fun successReturnsNull() = runBlocking {
        assertNull(registerWithTimeout(1_000) { })
    }

    @Test
    fun ordinaryFailureIsReturnedNotThrown() = runBlocking {
        // 协程调试模式下异常可能被复制(栈恢复),所以比类型与消息而不是同一实例
        val e = registerWithTimeout(1_000) { throw IllegalStateException("boom") }
        assertTrue(e is IllegalStateException)
        assertEquals("boom", e?.message)
    }

    @Test
    fun timeoutIsReturnedNotThrown() = runBlocking {
        assertTrue(registerWithTimeout(10) { awaitCancellation() } is TimeoutCancellationException)
    }

    @Test
    fun outerCancellationIsRethrown() = runBlocking {
        var rethrown = false
        coroutineScope {
            val job = launch {
                try {
                    registerWithTimeout(10_000) { awaitCancellation() }
                } catch (e: CancellationException) {
                    rethrown = true
                    throw e
                }
            }
            launch { job.cancel() }
        }
        assertTrue(rethrown)
    }
}
