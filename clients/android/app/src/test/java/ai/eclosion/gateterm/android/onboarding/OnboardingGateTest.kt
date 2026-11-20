package ai.eclosion.gateterm.android.onboarding

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class OnboardingGateTest {
    @Test
    fun freshInstallWithNoServersShowsFirstLaunch() {
        assertEquals(
            OnboardingDecision.ShowFirstLaunch,
            OnboardingGate.evaluate(completedVersion = 0, hasSavedConnections = false),
        )
    }

    @Test
    fun upgradedUserWithSavedServersSilentlyMigrates() {
        assertEquals(
            OnboardingDecision.MigrateExistingUser,
            OnboardingGate.evaluate(completedVersion = 0, hasSavedConnections = true),
        )
    }

    @Test
    fun completedUserDeletingLastServerStillSkips() {
        assertEquals(
            OnboardingDecision.Skip,
            OnboardingGate.evaluate(
                completedVersion = OnboardingConfig.CURRENT_VERSION,
                hasSavedConnections = false,
            ),
        )
    }

    @Test
    fun futureCompletedVersionSkips() {
        assertEquals(
            OnboardingDecision.Skip,
            OnboardingGate.evaluate(
                completedVersion = OnboardingConfig.CURRENT_VERSION + 1,
                hasSavedConnections = false,
            ),
        )
    }

    @Test
    fun backNavigationStepsBackwardUntilFirstPageThenExits() {
        assertEquals(2, OnboardingGate.previousPageOrExit(3))
        assertEquals(1, OnboardingGate.previousPageOrExit(2))
        assertEquals(0, OnboardingGate.previousPageOrExit(1))
        assertNull(OnboardingGate.previousPageOrExit(0))
    }

    @Test
    fun dismissTargetReturnsToOriginExceptWhenLaunchingScanner() {
        assertEquals("Appearance", OnboardingGate.dismissTarget(returnTo = "Appearance", openScanner = false, listScreen = "List"))
        assertEquals("List", OnboardingGate.dismissTarget(returnTo = "Appearance", openScanner = true, listScreen = "List"))
        assertEquals("List", OnboardingGate.dismissTarget(returnTo = "List", openScanner = false, listScreen = "List"))
    }
}
