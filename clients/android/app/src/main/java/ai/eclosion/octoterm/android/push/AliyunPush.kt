package ai.eclosion.octoterm.android.push

import ai.eclosion.octoterm.android.BuildConfig
import android.app.Application
import com.alibaba.sdk.android.push.CloudPushService
import com.alibaba.sdk.android.push.CommonCallback
import com.alibaba.sdk.android.push.noonesdk.PushInitConfig
import com.alibaba.sdk.android.push.noonesdk.PushServiceFactory
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

object AliyunPush {
    val configured: Boolean get() = BuildConfig.ALIYUN_APP_KEY.isNotBlank() && BuildConfig.ALIYUN_APP_SECRET.isNotBlank()
    private var initialized = false
    private var initializationFailure = "NOT_INITIALIZED"

    /** Parameter initialization only; called on the Application main thread in every SDK process. */
    fun initialize(app: Application) {
        if (!configured) return
        try {
            PushServiceFactory.init(PushInitConfig.Builder().application(app)
                .appKey(BuildConfig.ALIYUN_APP_KEY).appSecret(BuildConfig.ALIYUN_APP_SECRET).build())
            PushServiceFactory.getCloudPushService().setLogLevel(CloudPushService.LOG_OFF)
            initialized = true
        } catch (error: Exception) {
            initializationFailure = error.javaClass.simpleName
            initialized = false
        }
    }

    /** Establish the channel only after Cloud has selected Aliyun for the signed-in device. */
    suspend fun deviceId(app: Application): String = withContext(Dispatchers.Main.immediate) {
        if (!initialized) initialize(app)
        if (!initialized) throw AliyunPushException("initialize", initializationFailure)
        suspendCancellableCoroutine { continuation ->
            val finished = AtomicBoolean(false)
            val service = PushServiceFactory.getCloudPushService()
            service.register(app, object : CommonCallback {
                override fun onSuccess(result: String?) {
                    if (!finished.compareAndSet(false, true)) return
                    val id = service.deviceId
                    if (id.isNullOrBlank()) continuation.resumeWithException(AliyunPushException("device-id", "EMPTY_DEVICE_ID"))
                    else continuation.resume(id)
                }
                override fun onFailed(code: String?, message: String?) {
                    if (finished.compareAndSet(false, true)) continuation.resumeWithException(AliyunPushException("register", code))
                }
            })
        }
    }
}
