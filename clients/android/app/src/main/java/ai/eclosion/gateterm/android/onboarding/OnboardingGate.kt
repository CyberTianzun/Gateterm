package ai.eclosion.gateterm.android.onboarding

/**
 * Startup decision for whether to display or migrate onboarding.
 */
sealed class OnboardingDecision {
    data object ShowFirstLaunch : OnboardingDecision()
    data object MigrateExistingUser : OnboardingDecision()
    data object Skip : OnboardingDecision()
}

/**
 * Pure gating and navigation rules for the onboarding tutorial.
 */
object OnboardingGate {
    fun evaluate(
        completedVersion: Int,
        hasSavedConnections: Boolean,
        currentVersion: Int = OnboardingConfig.CURRENT_VERSION,
    ): OnboardingDecision = when {
        completedVersion >= currentVersion -> OnboardingDecision.Skip
        hasSavedConnections -> OnboardingDecision.MigrateExistingUser
        else -> OnboardingDecision.ShowFirstLaunch
    }

    fun previousPageOrExit(currentPage: Int): Int? =
        if (currentPage > 0) currentPage - 1 else null

    fun <T> dismissTarget(returnTo: T, openScanner: Boolean, listScreen: T): T =
        if (openScanner) listScreen else returnTo
}
