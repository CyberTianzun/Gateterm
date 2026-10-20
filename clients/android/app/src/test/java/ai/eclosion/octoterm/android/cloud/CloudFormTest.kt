package ai.eclosion.octoterm.android.cloud

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class CloudFormTest {
    @Test
    fun browserLoginNeedsOnlyARegion() {
        assertEquals(RelayRegion.China, CloudForm().region)
        for (region in RelayRegion.entries) {
            assertEquals(CloudJob.BrowserLogin(region), prepare(CloudForm(region = region), false, CloudAction.Login).getOrThrow())
            assertEquals(region, RelayRegion.fromUrl(region.url.trimEnd('/')))
        }
        assertEquals("https://octoterm.signal.top/", RelayRegion.China.url)
        assertEquals("https://octoterm.eclosion.ai/", RelayRegion.NorthAmerica.url)
        assertEquals("https://global.octoterm.eclosion.ai/", RelayRegion.Global.url)
    }

    @Test
    fun actionsRequireMatchingLoginState() {
        assertTrue(prepare(CloudForm(), true, CloudAction.Login).isFailure)
        assertTrue(prepare(CloudForm(), false, CloudAction.Logout).isFailure)
        assertEquals(CloudJob.Logout, prepare(CloudForm(), true, CloudAction.Logout).getOrThrow())
    }

    @Test
    fun deletingStillRequiresConfirmationPasswordAndAsciiEmailCode() {
        val form = CloudForm(password = "password", code = " 012345 ")
        assertTrue(prepare(form, true, CloudAction.DeleteAccount).isFailure)
        assertEquals(CloudJob.DeleteAccount("password", "012345"), prepare(form.copy(confirmDelete = true), true, CloudAction.DeleteAccount).getOrThrow())
        assertTrue(prepare(form.copy(confirmDelete = true, code = "１２３４５６"), true, CloudAction.DeleteAccount).isFailure)
        assertTrue(prepare(form.copy(confirmDelete = true, password = ""), true, CloudAction.DeleteAccount).isFailure)
    }
}
