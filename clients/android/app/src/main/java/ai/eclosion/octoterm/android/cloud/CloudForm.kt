package ai.eclosion.octoterm.android.cloud

import ai.eclosion.octoterm.android.R
import androidx.annotation.StringRes

enum class CloudAction {
    SendRegisterCode, Register, Login, SendResetCode, ResetPassword, Logout, SendDeleteCode, DeleteAccount;

    /** 发码只是中间一步,用户接下来还要用刚填的密码 —— 这时不清空。 */
    val keepsSecrets: Boolean get() = this == SendRegisterCode || this == SendResetCode || this == SendDeleteCode
}

data class CloudForm(
    val url: String = "",
    val email: String = "",
    val password: String = "",
    val code: String = "",
    val confirmDelete: Boolean = false,
)

sealed interface CloudJob {
    data class RequestCode(val url: String, val email: String, val reset: Boolean) : CloudJob
    data class Register(val url: String, val email: String, val code: String, val password: String) : CloudJob
    data class Login(val url: String, val email: String, val password: String) : CloudJob
    data class ResetPassword(val url: String, val email: String, val code: String, val password: String) : CloudJob
    data object Logout : CloudJob
    data object RequestDeleteCode : CloudJob
    data class DeleteAccount(val password: String, val code: String) : CloudJob
}

class CloudFormError(@StringRes val msg: Int) : Exception()

private fun fail(@StringRes msg: Int): Nothing = throw CloudFormError(msg)

private fun CloudForm.urlOrFail(): String =
    url.trim().takeIf { it.startsWith("http://") || it.startsWith("https://") } ?: fail(R.string.cloud_form_bad_url)

private fun CloudForm.emailOrFail(): String = email.trim().lowercase().takeIf { '@' in it } ?: fail(R.string.cloud_form_bad_email)

private fun CloudForm.newPasswordOrFail(): String = password.takeIf { it.length in 8..128 } ?: fail(R.string.cloud_form_bad_password)

private fun CloudForm.codeOrFail(): String =
    code.trim().takeIf { it.length == 6 && it.all(Char::isDigit) } ?: fail(R.string.cloud_form_bad_code)

/** 与 desktop `settings::cloud::prepare` 同一套规则。 */
fun prepare(form: CloudForm, loggedIn: Boolean, action: CloudAction): Result<CloudJob> = runCatching {
    val needsLogin = action == CloudAction.Logout || action == CloudAction.SendDeleteCode || action == CloudAction.DeleteAccount
    if (needsLogin != loggedIn) fail(if (loggedIn) R.string.cloud_form_already_logged_in else R.string.cloud_form_not_logged_in)
    when (action) {
        CloudAction.SendRegisterCode -> CloudJob.RequestCode(form.urlOrFail(), form.emailOrFail(), reset = false)
        CloudAction.SendResetCode -> CloudJob.RequestCode(form.urlOrFail(), form.emailOrFail(), reset = true)
        CloudAction.Register -> CloudJob.Register(form.urlOrFail(), form.emailOrFail(), form.codeOrFail(), form.newPasswordOrFail())
        CloudAction.Login -> {
            if (form.password.isEmpty()) fail(R.string.cloud_form_need_password)
            CloudJob.Login(form.urlOrFail(), form.emailOrFail(), form.password)
        }
        CloudAction.ResetPassword -> CloudJob.ResetPassword(form.urlOrFail(), form.emailOrFail(), form.codeOrFail(), form.newPasswordOrFail())
        CloudAction.Logout -> CloudJob.Logout
        CloudAction.SendDeleteCode -> CloudJob.RequestDeleteCode
        CloudAction.DeleteAccount -> {
            if (!form.confirmDelete) fail(R.string.cloud_form_confirm_delete)
            if (form.password.isEmpty()) fail(R.string.cloud_form_need_password)
            CloudJob.DeleteAccount(form.password, form.codeOrFail())
        }
    }
}
