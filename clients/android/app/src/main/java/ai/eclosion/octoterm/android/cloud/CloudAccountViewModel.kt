package ai.eclosion.octoterm.android.cloud

import ai.eclosion.octoterm.android.R
import ai.eclosion.octoterm.android.push.AgentNotifications
import ai.eclosion.octoterm.android.push.PushRegistrar
import ai.eclosion.octoterm.android.push.registerWithTimeout
import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.ViewModelProvider.AndroidViewModelFactory.Companion.APPLICATION_KEY
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.octoterm_cloud_ffi.CloudFfiException
import uniffi.octoterm_cloud_ffi.CloudStatus
import uniffi.octoterm_cloud_ffi.Purpose

const val DEFAULT_CLOUD_URL = "https://octoterm.signal.top"

/** 一条给用户看的结果:成功文案或失败文案(资源 id + 可选参数)。 */
data class CloudMessage(val ok: Boolean, val res: Int, val arg: String? = null)

data class CloudUiState(
    val status: CloudStatus? = null,
    val form: CloudForm = CloudForm(url = DEFAULT_CLOUD_URL),
    val busy: Boolean = false,
    val message: CloudMessage? = null,
    /** 登录成功后置 true,界面据此申请通知权限,随后清零。 */
    val askNotificationPermission: Boolean = false,
)

class CloudAccountViewModel(private val app: Application) : AndroidViewModel(app) {
    private val account = PushRegistrar.account(app)
    private val _state = MutableStateFlow(CloudUiState(status = account.status()))
    val state: StateFlow<CloudUiState> = _state.asStateFlow()

    fun updateForm(transform: (CloudForm) -> CloudForm) = _state.update { it.copy(form = transform(it.form), message = null) }

    fun consumePermissionRequest() = _state.update { it.copy(askNotificationPermission = false) }

    /** 进入/回到 Cloud 页时重读登录状态(token 可能已在后台失效或被别处登出)。操作进行中不打断。 */
    fun refreshStatus() {
        if (_state.value.busy) return
        _state.update { it.copy(status = account.status()) }
    }

    fun run(action: CloudAction) {
        val st = _state.value
        if (st.busy) return
        val job = prepare(st.form, st.status != null, action).getOrElse { e ->
            val res = (e as? CloudFormError)?.msg ?: R.string.cloud_err_generic
            _state.update { it.copy(message = CloudMessage(false, res)) }
            return
        }
        _state.update { it.copy(busy = true, message = null) }
        viewModelScope.launch {
            val result = runCatching { withContext(Dispatchers.IO) { execute(job) } }
            // ViewModel 若在等待期间被清理(onCleared 已调用 account.close()),这里会收到
            // CancellationException;必须原样重新抛出以终止协程,否则会继续往下执行并在
            // account 已销毁后调用 account.status(),触发 UniFFI 的 "object has already
            // been destroyed" 崩溃。
            result.exceptionOrNull()?.let { if (it is CancellationException) throw it }
            val message = result.fold(
                onSuccess = { it },
                onFailure = { e -> CloudMessage(false, (e as? CloudFfiException)?.toFailure()?.messageRes() ?: R.string.cloud_err_generic) },
            )
            val loggedInNow = result.isSuccess && (job is CloudJob.Login || job is CloudJob.Register)
            // 渠道按需创建:登录成功后才需要,通知设置里也才有它可调。
            if (loggedInNow) AgentNotifications.ensureChannel(app)
            _state.update {
                it.copy(
                    busy = false,
                    status = account.status(),
                    message = message,
                    form = if (result.isSuccess && !action.keepsSecrets) it.form.copy(password = "", code = "", confirmDelete = false) else it.form,
                    askNotificationPermission = loggedInNow,
                )
            }
        }
    }

    private suspend fun execute(job: CloudJob): CloudMessage = when (job) {
        is CloudJob.RequestCode -> {
            account.requestCode(job.url, job.email, if (job.reset) Purpose.RESET else Purpose.REGISTER)
            if (job.reset) CloudMessage(true, R.string.cloud_ok_reset_code_sent) else CloudMessage(true, R.string.cloud_ok_code_sent, job.email)
        }
        is CloudJob.Register -> {
            account.register(job.url, job.email, job.code, job.password)
            registerPush()
            CloudMessage(true, R.string.cloud_ok_registered)
        }
        is CloudJob.Login -> {
            account.login(job.url, job.email, job.password)
            registerPush()
            CloudMessage(true, R.string.cloud_ok_logged_in)
        }
        is CloudJob.ResetPassword -> {
            account.resetPassword(job.url, job.email, job.code, job.password)
            CloudMessage(true, R.string.cloud_ok_reset)
        }
        CloudJob.Logout -> if (account.logout()) CloudMessage(true, R.string.cloud_ok_logged_out) else CloudMessage(true, R.string.cloud_ok_logged_out_local)
        CloudJob.RequestDeleteCode -> CloudMessage(true, R.string.cloud_ok_delete_code_sent, account.requestDeleteCode())
        is CloudJob.DeleteAccount -> {
            account.deleteAccount(job.password, job.code)
            CloudMessage(true, R.string.cloud_ok_deleted)
        }
    }

    /** 登录已成功,推送登记失败不影响结果;加超时,免得没有 Play 服务时一直转圈。 */
    private suspend fun registerPush() {
        registerWithTimeout { PushRegistrar.registerCurrentToken(app, account) }?.let(PushRegistrar::logFailure)
    }

    override fun onCleared() {
        account.close()
    }

    companion object {
        val Factory: ViewModelProvider.Factory = viewModelFactory {
            initializer { CloudAccountViewModel(checkNotNull(this[APPLICATION_KEY])) }
        }
    }
}
