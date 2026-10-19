package ai.eclosion.octoterm.android.push

import android.content.Context
import android.util.Log
import com.google.firebase.FirebaseApp
import com.google.firebase.messaging.FirebaseMessaging
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import uniffi.octoterm_cloud_ffi.CloudAccount
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/** 取 FCM token 并登记到 Cloud。没有 google-services.json 的构建里整体降级为空操作。 */
object PushRegistrar {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    fun pushAvailable(context: Context): Boolean = FirebaseApp.getApps(context).isNotEmpty()

    fun account(context: Context): CloudAccount =
        CloudAccount(java.io.File(context.filesDir, "cloud.json").absolutePath, android.os.Build.MODEL ?: "Android")

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
        scope.launch {
            account(app).use { account ->
                if (account.status() == null) return@launch
                // 协程若被取消,必须重新抛出 CancellationException(而不是当成普通失败吞掉),
                // 遵循协作式取消的约定,避免 use{} 以为 block 正常结束。
                runCatching { account.registerPushTarget("fcm", token) }
                    .onFailure { e ->
                        if (e is CancellationException) throw e
                        Log.w("Octoterm", "登记推送 token 失败", e)
                    }
            }
        }
    }

    /** App 启动时调用:已登录则补登一次(token 可能在 App 未运行时变过)。 */
    fun refreshOnStart(context: Context) {
        val app = context.applicationContext
        scope.launch {
            account(app).use { account ->
                if (account.status() == null) return@launch
                // 同上:取消必须重新抛出,不能当成普通失败吞掉。
                runCatching { registerCurrentToken(app, account) }
                    .onFailure { e ->
                        if (e is CancellationException) throw e
                        Log.w("Octoterm", "登记推送 token 失败", e)
                    }
            }
        }
    }
}
