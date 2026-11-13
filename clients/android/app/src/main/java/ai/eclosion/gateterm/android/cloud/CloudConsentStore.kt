package ai.eclosion.gateterm.android.cloud

import android.content.Context

object CloudConsentStore {
    private var consent: CloudConsent? = null

    @Synchronized
    fun load(context: Context): CloudConsent {
        consent?.let { return it }
        val preferences = context.getSharedPreferences("cloud_privacy", Context.MODE_PRIVATE)
        // Share the confirmed revision for this process. A failed SharedPreferences commit
        // can update its memory cache; rereading that cache must never authorize SDK activity.
        return CloudConsent(
            readRevision = { preferences.getString("accepted_revision", null) },
            // Complete durable storage before any browser or SDK activity begins.
            persistRevision = { preferences.edit().putString("accepted_revision", it).commit() },
        ).also { consent = it }
    }
}
