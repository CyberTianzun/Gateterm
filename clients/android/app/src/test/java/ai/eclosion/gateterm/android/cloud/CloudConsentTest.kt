package ai.eclosion.gateterm.android.cloud

import org.junit.Assert.*
import org.junit.Test

class CloudConsentTest {
    private var revision: String? = null
    private fun gate() = CloudConsent({ revision }, { revision = it; true })

    @Test
    fun signInWaitsForExplicitAcceptanceAndPersistsBeforeContinuing() {
        val consent = gate()
        var launches = 0
        assertFalse(consent.request { assertTrue(gate().accepted); launches++ })
        assertEquals(0, launches)
        assertTrue(consent.accept())
        assertEquals(1, launches)
        assertTrue(gate().request { launches++ })
        assertEquals(2, launches)
    }

    @Test
    fun dismissingDoesNotGrantConsentOrRunThePendingAction() {
        val consent = gate()
        var launches = 0
        consent.request { launches++ }
        consent.dismiss()
        assertFalse(consent.accept())
        assertFalse(gate().accepted)
        assertEquals(0, launches)
    }

    @Test
    fun signedInUpgradeStillNeedsCurrentConsentBeforePushRetry() {
        revision = "older-policy"
        val consent = gate()
        var registrations = 0
        assertFalse(consent.accepted)
        assertFalse(consent.request { registrations++ })
        assertEquals(0, registrations)
        assertTrue(consent.accept())
        assertEquals(1, registrations)
        assertTrue(gate().accepted)
    }

    @Test
    fun failedPersistenceCannotStartCloudActivity() {
        val consent = CloudConsent({ null }, { false })
        var launches = 0
        consent.request { launches++ }
        assertFalse(consent.accept())
        assertFalse(consent.accepted)
        assertEquals(0, launches)
    }

    @Test
    fun failedCommitUpdatingMemoryDoesNotAuthorizeLaterRequests() {
        var canPersist = false
        val consent = CloudConsent({ revision }, { revision = it; canPersist })
        var launches = 0
        consent.request { launches++ }
        assertFalse(consent.accept())
        assertFalse(consent.accepted)
        assertFalse(consent.request { launches++ })
        assertEquals(0, launches)
        canPersist = true
        assertTrue(consent.accept())
        assertTrue(consent.accepted)
        assertEquals(1, launches)
    }

    @Test
    fun acceptingTwiceDoesNotRepeatThePendingAction() {
        val consent = gate()
        var launches = 0
        consent.request { launches++ }
        assertTrue(consent.accept())
        assertFalse(consent.accept())
        assertEquals(1, launches)
    }
}
