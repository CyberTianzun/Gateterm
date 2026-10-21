package ai.eclosion.octoterm.android.push

/** Retain SDK error identifiers, never SDK messages which can contain app secrets or device IDs. */
fun safeAliyunErrorCode(code: String?): String = code?.takeIf {
    it.matches(Regex("(?:PUSH|EAGOO|ACCS|ELS)_[A-Za-z0-9_.-]{1,48}|-?[0-9]{1,6}"))
} ?: "UNKNOWN"

class AliyunPushException(val stage: String, rawCode: String?) : Exception("Aliyun push registration failed") {
    val code: String = if (stage == "initialize") {
        rawCode?.takeIf { it.matches(Regex("[A-Za-z]{1,48}(?:Exception|Error)")) } ?: "INIT_FAILED"
    } else if (stage == "device-id") "EMPTY_DEVICE_ID" else safeAliyunErrorCode(rawCode)
}
