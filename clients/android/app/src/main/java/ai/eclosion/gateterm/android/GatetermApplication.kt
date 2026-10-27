package ai.eclosion.gateterm.android

import android.app.Application
import ai.eclosion.gateterm.android.i18n.LocaleStore
import ai.eclosion.gateterm.android.push.PushRegistrar
import ai.eclosion.gateterm.android.push.AliyunPush
import com.google.firebase.FirebaseApp
import com.google.firebase.messaging.FirebaseMessaging

class GatetermApplication : Application() {
    val connections by lazy { ai.eclosion.gateterm.android.ui.connections.ConnectionController(this) }
    override fun onCreate() {
        super.onCreate()
        AliyunPush.initialize(this)
        // FCM is started explicitly only when the deployment selects it.
        if (FirebaseApp.getApps(this).isNotEmpty()) FirebaseMessaging.getInstance().isAutoInitEnabled = false
        LocaleStore.apply(LocaleStore.load(this))
        PushRegistrar.refreshOnStart(this)
    }
}
