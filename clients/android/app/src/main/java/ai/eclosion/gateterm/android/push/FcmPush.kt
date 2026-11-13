package ai.eclosion.gateterm.android.push

import ai.eclosion.gateterm.android.cloud.CloudConsentStore
import android.content.Context
import com.google.firebase.FirebaseApp
import com.google.firebase.FirebaseOptions
import com.google.firebase.messaging.FirebaseMessaging
import java.io.File

object FcmPush {
    /** Inspect packaged configuration without constructing Firebase or its eager components. */
    fun configured(context: Context): Boolean = FirebaseOptions.fromResource(context) != null

    fun initialize(context: Context, selectedProvider: String?): Boolean = initializeFcmWithConsent(
        CloudConsentStore.load(context).accepted,
        selectedProvider,
    ) {
        val options = FirebaseOptions.fromResource(context) ?: return@initializeFcmWithConsent false
        if (FirebaseApp.getApps(context).none { it.name == FirebaseApp.DEFAULT_APP_NAME }) {
            FirebaseApp.initializeApp(context, options)
        }
        FirebaseMessaging.getInstance().isAutoInitEnabled = false
        true
    }

    /** Restore an opted-in FCM route before an incoming service runs after a cold start. */
    fun initializeSelectedOnStart(context: Context) {
        if (!File(context.filesDir, "cloud.json").isFile) return
        val provider = context.getSharedPreferences("push", Context.MODE_PRIVATE).getString("provider", null)
        initialize(context, provider)
    }
}
