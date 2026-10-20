package ai.eclosion.octoterm.android.cloud

import ai.eclosion.octoterm.android.R
import androidx.annotation.StringRes

enum class CloudAction {
    Login, ReopenBrowser, CancelLogin, Logout, SendDeleteCode, DeleteAccount;
    val keepsSecrets: Boolean get() = this == SendDeleteCode
}

data class CloudForm(
    val region: RelayRegion = RelayRegion.China,
    val password: String = "",
    val code: String = "",
    val confirmDelete: Boolean = false,
)

sealed interface CloudJob {
    data class BrowserLogin(val region: RelayRegion) : CloudJob
    data object Logout : CloudJob
    data object RequestDeleteCode : CloudJob
    data class DeleteAccount(val password: String, val code: String) : CloudJob
}

class CloudFormError(@StringRes val msg: Int) : Exception()

private fun fail(@StringRes msg: Int): Nothing = throw CloudFormError(msg)

private fun CloudForm.codeOrFail(): String =
    code.trim().takeIf { it.length == 6 && it.all { digit -> digit in '0'..'9' } } ?: fail(R.string.cloud_form_bad_code)

/** 与 desktop `settings::cloud::prepare` 同一套规则。 */
fun prepare(form: CloudForm, loggedIn: Boolean, action: CloudAction): Result<CloudJob> = runCatching {
    val needsLogin = action == CloudAction.Logout || action == CloudAction.SendDeleteCode || action == CloudAction.DeleteAccount
    if (needsLogin != loggedIn) fail(if (loggedIn) R.string.cloud_form_already_logged_in else R.string.cloud_form_not_logged_in)
    when (action) {
        CloudAction.Login -> CloudJob.BrowserLogin(form.region)
        CloudAction.ReopenBrowser, CloudAction.CancelLogin -> fail(R.string.cloud_form_not_logged_in)
        CloudAction.Logout -> CloudJob.Logout
        CloudAction.SendDeleteCode -> CloudJob.RequestDeleteCode
        CloudAction.DeleteAccount -> {
            if (!form.confirmDelete) fail(R.string.cloud_form_confirm_delete)
            if (form.password.isEmpty()) fail(R.string.cloud_form_need_password)
            CloudJob.DeleteAccount(form.password, form.codeOrFail())
        }
    }
}
