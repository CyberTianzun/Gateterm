package ai.eclosion.octoterm.android.cloud

import ai.eclosion.octoterm.android.R
import org.junit.Assert.assertEquals
import org.junit.Test

class CloudFailureTest {
    @Test
    fun apiCodesMapToSpecificMessages() {
        assertEquals(R.string.cloud_err_invalid_credentials, CloudFailure.Api("invalid_credentials").messageRes())
        assertEquals(R.string.cloud_err_invalid_code, CloudFailure.Api("invalid_code").messageRes())
        assertEquals(R.string.cloud_err_code_exhausted, CloudFailure.Api("code_exhausted").messageRes())
        assertEquals(R.string.cloud_err_email_taken, CloudFailure.Api("email_taken").messageRes())
        assertEquals(R.string.cloud_err_weak_password, CloudFailure.Api("weak_password").messageRes())
        assertEquals(R.string.cloud_err_wrong_password, CloudFailure.Api("wrong_password").messageRes())
        assertEquals(R.string.cloud_err_generic, CloudFailure.Api("something_new").messageRes())
    }

    @Test
    fun otherFailures() {
        assertEquals(R.string.cloud_err_unauthorized, CloudFailure.Unauthorized.messageRes())
        assertEquals(R.string.cloud_err_unreachable, CloudFailure.Unreachable.messageRes())
        assertEquals(R.string.cloud_err_rate_limited, CloudFailure.RateLimited(30).messageRes())
        assertEquals(R.string.cloud_err_not_logged_in, CloudFailure.NotLoggedIn.messageRes())
    }
}
