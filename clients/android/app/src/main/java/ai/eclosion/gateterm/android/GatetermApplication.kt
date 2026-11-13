package ai.eclosion.gateterm.android

import android.app.Application
import ai.eclosion.gateterm.android.i18n.LocaleStore
import ai.eclosion.gateterm.android.push.PushRegistrar
import ai.eclosion.gateterm.android.push.FcmPush

class GatetermApplication : Application() {
    val connections by lazy { ai.eclosion.gateterm.android.ui.connections.ConnectionController(this) }
    override fun onCreate() {
        super.onCreate()
        FcmPush.initializeSelectedOnStart(this)
        LocaleStore.apply(LocaleStore.load(this))
        PushRegistrar.refreshOnStart(this)
    }
}
