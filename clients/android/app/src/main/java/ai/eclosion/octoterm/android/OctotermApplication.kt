package ai.eclosion.octoterm.android

import android.app.Application
import ai.eclosion.octoterm.android.i18n.LocaleStore
import ai.eclosion.octoterm.android.push.AgentNotifications
import ai.eclosion.octoterm.android.push.PushRegistrar

class OctotermApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        LocaleStore.apply(LocaleStore.load(this))
        AgentNotifications.ensureChannel(this)
        PushRegistrar.refreshOnStart(this)
    }
}
