package ai.eclosion.gateterm.android.ui.connections

import ai.eclosion.gateterm.android.onboarding.OnboardingDecision
import org.junit.Assert.assertEquals
import org.junit.Test

class ConnectionOnboardingTest {
    @Test
    fun initialScreenShowsOnboardingOnlyForFirstLaunch() {
        assertEquals(
            AppScreen.Onboarding(returnTo = AppScreen.List, isFirstLaunch = true),
            ConnectionController.initialScreenFor(OnboardingDecision.ShowFirstLaunch),
        )
        assertEquals(
            AppScreen.List,
            ConnectionController.initialScreenFor(OnboardingDecision.MigrateExistingUser),
        )
        assertEquals(
            AppScreen.List,
            ConnectionController.initialScreenFor(OnboardingDecision.Skip),
        )
    }

    @Test
    fun dismissOnboardingReturnsToOriginUnlessOpeningScanner() {
        assertEquals(
            AppScreen.Appearance,
            ConnectionController.resolveOnboardingDismissScreen(
                current = AppScreen.Onboarding(returnTo = AppScreen.Appearance, isFirstLaunch = false),
                openScanner = false,
            ),
        )
        assertEquals(
            AppScreen.List,
            ConnectionController.resolveOnboardingDismissScreen(
                current = AppScreen.Onboarding(returnTo = AppScreen.Appearance, isFirstLaunch = false),
                openScanner = true,
            ),
        )
        assertEquals(
            AppScreen.List,
            ConnectionController.resolveOnboardingDismissScreen(
                current = AppScreen.Onboarding(returnTo = AppScreen.List, isFirstLaunch = true),
                openScanner = false,
            ),
        )
        assertEquals(
            AppScreen.List,
            ConnectionController.resolveOnboardingDismissScreen(
                current = AppScreen.List,
                openScanner = false,
            ),
        )
    }

    @Test
    fun missingServerNotificationEscapesOnboardingWithoutDisruptingOtherScreens() {
        assertEquals(
            AppScreen.List,
            ConnectionController.resolveNotificationMissingServerScreen(
                AppScreen.Onboarding(returnTo = AppScreen.List, isFirstLaunch = true),
            ),
        )
        assertEquals(
            AppScreen.List,
            ConnectionController.resolveNotificationMissingServerScreen(
                AppScreen.Onboarding(returnTo = AppScreen.Appearance, isFirstLaunch = false),
            ),
        )
        assertEquals(
            AppScreen.Appearance,
            ConnectionController.resolveNotificationMissingServerScreen(AppScreen.Appearance),
        )
        assertEquals(
            AppScreen.Server("srv-1"),
            ConnectionController.resolveNotificationMissingServerScreen(AppScreen.Server("srv-1")),
        )
    }
}
