package ai.eclosion.octoterm.android.push

import org.junit.Assert.assertEquals
import org.junit.Test

class AliyunPushFailureTest {
    @Test
    fun retainsProviderCodesWithoutEchoingErrorMessagesOrCredentials() {
        for (code in listOf("PUSH_10103", "PUSH_ACCS_-28", "EAGOO_SDK_accs_disabled", "10107", "-29")) {
            assertEquals(code, safeAliyunErrorCode(code))
        }
        for (value in listOf(null, "invalid key=secret", "https://example.com/?token=secret", "123456789", "a".repeat(30))) {
            assertEquals("UNKNOWN", safeAliyunErrorCode(value))
        }
    }

    @Test
    fun distinguishesInitializationFromRegistrationAndMissingDeviceId() {
        assertEquals("IllegalStateException", AliyunPushException("initialize", "IllegalStateException").code)
        assertEquals("PUSH_10103", AliyunPushException("register", "PUSH_10103").code)
        assertEquals("EMPTY_DEVICE_ID", AliyunPushException("device-id", "ignored").code)
        assertEquals("INIT_FAILED", AliyunPushException("initialize", "appSecret=secret").code)
    }
}
