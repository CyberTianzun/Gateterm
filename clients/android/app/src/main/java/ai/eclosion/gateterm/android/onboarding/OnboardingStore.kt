package ai.eclosion.gateterm.android.onboarding

import android.content.Context

/**
 * Versioned SharedPreferences store tracking completed onboarding version.
 */
class OnboardingStore(context: Context) {
    private val prefs = context.applicationContext.getSharedPreferences(
        "gateterm.onboarding",
        Context.MODE_PRIVATE,
    )

    fun completedVersion(): Int = prefs.getInt("completed_version", 0)

    fun markCompleted(version: Int = OnboardingConfig.CURRENT_VERSION) {
        prefs.edit().putInt("completed_version", version).apply()
    }
}
