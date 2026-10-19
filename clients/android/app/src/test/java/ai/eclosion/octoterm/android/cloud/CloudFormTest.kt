package ai.eclosion.octoterm.android.cloud

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class CloudFormTest {
    private val form = CloudForm(url = " https://octoterm.signal.top ", email = " A@Example.com ", password = "pw-pw-pw", code = " 012345 ")

    @Test
    fun registerJobIsNormalized() {
        assertEquals(
            CloudJob.Register("https://octoterm.signal.top", "a@example.com", "012345", "pw-pw-pw"),
            prepare(form, loggedIn = false, CloudAction.Register).getOrThrow(),
        )
    }

    @Test
    fun validationErrors() {
        assertTrue(prepare(form.copy(url = "octoterm.signal.top"), false, CloudAction.Login).isFailure)
        assertTrue(prepare(form.copy(code = "12a456"), false, CloudAction.Register).isFailure)
        assertTrue(prepare(form.copy(password = "short"), false, CloudAction.ResetPassword).isFailure)
        assertTrue(prepare(form.copy(email = "nope"), false, CloudAction.SendRegisterCode).isFailure)
    }

    @Test
    fun actionsRequireMatchingLoginState() {
        assertTrue(prepare(form, loggedIn = true, CloudAction.Login).isFailure)
        assertTrue(prepare(form, loggedIn = false, CloudAction.Logout).isFailure)
        assertEquals(CloudJob.Logout, prepare(form, loggedIn = true, CloudAction.Logout).getOrThrow())
    }

    @Test
    fun deleteRequiresConfirmation() {
        assertTrue(prepare(form, true, CloudAction.DeleteAccount).isFailure)
        assertEquals(
            CloudJob.DeleteAccount("pw-pw-pw", "012345"),
            prepare(form.copy(confirmDelete = true), true, CloudAction.DeleteAccount).getOrThrow(),
        )
    }

    @Test
    fun sendCodeKeepsSecrets() {
        assertTrue(CloudAction.SendRegisterCode.keepsSecrets)
        assertTrue(!CloudAction.Register.keepsSecrets)
    }
}
