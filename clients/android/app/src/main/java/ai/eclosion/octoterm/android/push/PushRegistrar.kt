package ai.eclosion.octoterm.android.push

import android.content.Context
import android.util.Log
import com.google.firebase.FirebaseApp
import com.google.firebase.messaging.FirebaseMessaging
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import uniffi.octoterm_cloud_ffi.CloudAccount
import java.io.File
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/** 取 FCM token 并登记到 Cloud。没有 google-services.json 的构建里整体降级为空操作。 */
object PushRegistrar {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    fun pushAvailable(context: Context): Boolean = FirebaseApp.getApps(context).isNotEmpty()

    private fun accountFile(context: Context) = File(context.filesDir, "cloud.json")

    fun account(context: Context): CloudAccount =
        CloudAccount(accountFile(context).absolutePath, android.os.Build.MODEL ?: "Android")

    suspend fun registerCurrentToken(context: Context, account: CloudAccount) {
        if (!pushAvailable(context)) return
        val token = suspendCancellableCoroutine { cont ->
            FirebaseMessaging.getInstance().token
                .addOnSuccessListener { cont.resume(it) }
                .addOnFailureListener { cont.resumeWithException(it) }
        }
        account.registerPushTarget("fcm", token)
    }

    /** FCM 换了 token:已登录就重新登记;失败只记日志,下次启动/登录会再登记。 */
    fun onNewToken(context: Context, token: String) {
        val app = context.applicationContext
        if (!accountFile(app).exists()) return
        scope.launch {
            account(app).use { account ->
                if (account.status() == null) return@launch
                registerWithTimeout { account.registerPushTarget("fcm", token) }?.let(::logFailure)
            }
        }
    }

    /** App 启动时调用:已登录则补登一次(token 可能在 App 未运行时变过)。 */
    fun refreshOnStart(context: Context) {
        val app = context.applicationContext
        // 从没登录过就没有账号文件:直接返回,不为不用 Cloud 的用户加载原生库。
        if (!accountFile(app).exists()) return
        scope.launch {
            account(app).use { account ->
                if (account.status() == null) return@launch
                registerWithTimeout { registerCurrentToken(app, account) }?.let(::logFailure)
            }
        }
    }

    /** 只记异常类型:FFI 的 reason 文本可能带服务器返回的内容,不进 logcat。 */
    fun logFailure(e: Throwable) {
        Log.w("Octoterm", "登记推送 token 失败: ${e::class.simpleName}")
    }
}
