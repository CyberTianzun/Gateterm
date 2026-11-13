package ai.eclosion.gateterm.android.push

import org.junit.Assert.*
import org.junit.Test

class FcmInitializationTest {
    @Test
    fun configuredFcmDoesNotInitializeBeforeConsent() {
        var initialized = false
        assertFalse(initializeFcmWithConsent(false, "fcm") { initialized = true; true })
        assertFalse(initialized)
    }

    @Test
    fun consentDoesNotInitializeFcmForAnotherOrUnknownProvider() {
        var initializations = 0
        for (provider in listOf("aliyun", "none", null, "unknown")) {
            assertFalse(initializeFcmWithConsent(true, provider) { initializations++; true })
        }
        assertEquals(0, initializations)
    }

    @Test
    fun acceptedFcmSelectionInitializesAndReportsFailureForRetry() {
        var initializations = 0
        assertFalse(initializeFcmWithConsent(true, "fcm") { initializations++; false })
        assertTrue(initializeFcmWithConsent(true, "fcm") { initializations++; true })
        assertEquals(2, initializations)
    }
}
