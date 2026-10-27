package ai.eclosion.gateterm.android.cloud

import ai.eclosion.gateterm.android.R
import androidx.annotation.StringRes
import uniffi.gateterm_cloud_ffi.CloudFfiException

/** 与 FFI 解耦的错误分类:单测不必加载 native 库。 */
sealed interface CloudFailure {
    data object NotLoggedIn : CloudFailure
    data object Unauthorized : CloudFailure
    data class RateLimited(val retryAfter: Long?) : CloudFailure
    data class Api(val code: String) : CloudFailure
    data object Unreachable : CloudFailure
    data object Local : CloudFailure
}

@StringRes
fun CloudFailure.messageRes(): Int = when (this) {
    CloudFailure.NotLoggedIn -> R.string.cloud_err_not_logged_in
    CloudFailure.Unauthorized -> R.string.cloud_err_unauthorized
    is CloudFailure.RateLimited -> R.string.cloud_err_rate_limited
    CloudFailure.Unreachable -> R.string.cloud_err_unreachable
    CloudFailure.Local -> R.string.cloud_err_local
    is CloudFailure.Api -> when (code) {
        "invalid_credentials" -> R.string.cloud_err_invalid_credentials
        "invalid_authorization_code" -> R.string.cloud_err_authorization_code
        "invalid_code" -> R.string.cloud_err_invalid_code
        "code_exhausted" -> R.string.cloud_err_code_exhausted
        "email_taken" -> R.string.cloud_err_email_taken
        "weak_password" -> R.string.cloud_err_weak_password
        "invalid_email" -> R.string.cloud_err_invalid_email
        "wrong_password" -> R.string.cloud_err_wrong_password
        else -> R.string.cloud_err_generic
    }
}

fun CloudFfiException.toFailure(): CloudFailure = when (this) {
    is CloudFfiException.NotLoggedIn -> CloudFailure.NotLoggedIn
    is CloudFfiException.Unauthorized -> CloudFailure.Unauthorized
    is CloudFfiException.RateLimited -> CloudFailure.RateLimited(retryAfter?.toLong())
    is CloudFfiException.Api -> CloudFailure.Api(code)
    is CloudFfiException.Unreachable -> CloudFailure.Unreachable
    is CloudFfiException.Local -> CloudFailure.Local
}
