package ai.eclosion.octoterm.android.push

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.withTimeout

/** 登记推送 token 的超时:没有 Play 服务时 FCM 取 token 可能一直不回。 */
const val PUSH_REGISTER_TIMEOUT_MS = 15_000L

/**
 * 带超时执行一次推送登记,失败只返回异常、不抛出 —— 登记失败不该影响登录或启动,下次再补登。
 * 超时(TimeoutCancellationException)属于预期内的"放弃等待",照常返回;
 * 外部真正的取消必须重新抛出,否则协程会假装正常完成,继续访问可能已关闭的 account。
 */
suspend fun registerWithTimeout(timeoutMs: Long = PUSH_REGISTER_TIMEOUT_MS, block: suspend () -> Unit): Throwable? =
    try {
        withTimeout(timeoutMs) { block() }
        null
    } catch (e: TimeoutCancellationException) {
        e
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        e
    }
