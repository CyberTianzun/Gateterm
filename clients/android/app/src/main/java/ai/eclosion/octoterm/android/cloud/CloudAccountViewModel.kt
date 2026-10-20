package ai.eclosion.octoterm.android.cloud

import ai.eclosion.octoterm.android.R
import ai.eclosion.octoterm.android.i18n.LocaleStore
import ai.eclosion.octoterm.android.i18n.resolveLocale
import ai.eclosion.octoterm.android.push.AgentNotifications
import ai.eclosion.octoterm.android.push.PushRegistrar
import ai.eclosion.octoterm.android.push.registerWithTimeout
import android.app.Application
import android.content.res.Resources
import android.os.Build
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.ViewModelProvider.AndroidViewModelFactory.Companion.APPLICATION_KEY
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import java.io.File
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.octoterm_cloud_ffi.CloudFfiException
import uniffi.octoterm_cloud_ffi.CloudAccount
import uniffi.octoterm_cloud_ffi.CloudStatus

data class CloudMessage(val ok: Boolean, val res: Int, val arg: String? = null)

data class CloudUiState(
    val status: CloudStatus? = null,
    val form: CloudForm = CloudForm(),
    val busy: Boolean = false,
    val waitingForBrowser: Boolean = false,
    val browserLaunch: String? = null,
    val message: CloudMessage? = null,
    val askNotificationPermission: Boolean = false,
)

class CloudAccountViewModel(private val app: Application) : AndroidViewModel(app) {
    private val accountDelegate: Lazy<CloudAccount> = lazy { PushRegistrar.account(app) }
    private val account: CloudAccount get() = accountDelegate.value
    private val pendingStore = BrowserLoginStore(app.noBackupFilesDir)
    private val initialStatus = currentStatus()
    private val restored = if (initialStatus == null) pendingStore.load() else null
    private val _state = MutableStateFlow(CloudUiState(
        status = initialStatus,
        form = CloudForm(region = initialStatus?.let { RelayRegion.fromUrl(it.url) } ?: restored?.region ?: RelayRegion.China),
        waitingForBrowser = restored != null,
    ))
    val state: StateFlow<CloudUiState> = _state.asStateFlow()
    private var expiry: Job? = null

    init { restored?.let(::scheduleExpiry) }

    private fun currentStatus(): CloudStatus? =
        if (accountDelegate.isInitialized() || File(app.filesDir, "cloud.json").isFile) account.status() else null

    fun updateForm(transform: (CloudForm) -> CloudForm) = _state.update {
        if (it.busy || it.waitingForBrowser) it else it.copy(form = transform(it.form), message = null)
    }
    fun consumePermissionRequest() = _state.update { it.copy(askNotificationPermission = false) }
    fun consumeBrowserLaunch() = _state.update { it.copy(browserLaunch = null) }

    fun browserUnavailable() {
        pendingStore.clear()
        expiry?.cancel()
        _state.update { it.copy(waitingForBrowser = false, browserLaunch = null, message = CloudMessage(false, R.string.cloud_browser_unavailable)) }
    }

    fun refreshStatus() {
        if (_state.value.busy) return
        if (_state.value.waitingForBrowser && pendingStore.load() == null) expireLogin()
        val status = currentStatus()
        _state.update { it.copy(status = status,
            form = if (status == null && it.status != null) it.form.copy(password = "", code = "", confirmDelete = false) else it.form) }
    }

    fun retryPushRegistration() {
        if (_state.value.busy || _state.value.status == null) return
        _state.update { it.copy(busy = true, message = null) }
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                registerWithTimeout { PushRegistrar.registerCurrentToken(app, account) }?.let(PushRegistrar::logFailure)
            }
            _state.update { it.copy(busy = false, status = currentStatus(), askNotificationPermission = true) }
        }
    }

    fun run(action: CloudAction) {
        val st = _state.value
        if (st.busy) return
        if (action == CloudAction.CancelLogin) {
            pendingStore.clear()
            expiry?.cancel()
            _state.update { it.copy(waitingForBrowser = false, browserLaunch = null, message = CloudMessage(true, R.string.cloud_login_cancelled)) }
            return
        }
        if (action == CloudAction.ReopenBrowser) {
            val pending = pendingStore.load()
            if (pending == null) expireLogin() else _state.update { it.copy(browserLaunch = pending.authorizationUrl()) }
            return
        }
        if (st.waitingForBrowser) return
        val job = prepare(st.form, st.status != null, action).getOrElse { error ->
            _state.update { it.copy(message = CloudMessage(false, (error as? CloudFormError)?.msg ?: R.string.cloud_err_generic)) }
            return
        }
        if (job is CloudJob.BrowserLogin) {
            val system = Resources.getSystem().configuration.locales
            val language = resolveLocale(LocaleStore.load(app), (0 until system.size()).map { system[it].toLanguageTag() }).tag
            val pending = PendingCloudLogin.create(job.region, language, Build.MODEL ?: "Android")
            try { pendingStore.save(pending) }
            catch (_: Exception) {
                _state.update { it.copy(message = CloudMessage(false, R.string.cloud_err_local)) }
                return
            }
            scheduleExpiry(pending)
            _state.update { it.copy(waitingForBrowser = true, browserLaunch = pending.authorizationUrl(), message = null) }
            return
        }
        _state.update { it.copy(busy = true, message = null) }
        viewModelScope.launch {
            val result = runCatching { withContext(Dispatchers.IO) { execute(job) } }
            result.exceptionOrNull()?.let { if (it is CancellationException) throw it }
            _state.update {
                it.copy(busy = false, status = currentStatus(), message = result.getOrElse(::failure),
                    form = if (result.isSuccess && !action.keepsSecrets) it.form.copy(password = "", code = "", confirmDelete = false) else it.form)
            }
        }
    }

    /** Consuming the persisted attempt before exchange prevents duplicate intents from signing in twice. */
    fun finishBrowserLogin(callback: String): Boolean {
        if (_state.value.busy) return false
        val (pending, code) = pendingStore.claim(callback) ?: return false
        expiry?.cancel()
        _state.update { it.copy(busy = true, waitingForBrowser = false, browserLaunch = null, message = null) }
        viewModelScope.launch {
            val result = runCatching {
                withContext(Dispatchers.IO) {
                    account.finishBrowserLogin(pending.region.url, code, pending.verifier, CLOUD_CALLBACK_URI)
                    registerWithTimeout { PushRegistrar.registerCurrentToken(app, account) }?.let(PushRegistrar::logFailure)
                }
            }
            result.exceptionOrNull()?.let { if (it is CancellationException) throw it }
            val updatedStatus = currentStatus()
            val signedIn = result.isSuccess && updatedStatus != null
            if (signedIn) AgentNotifications.ensureChannel(app)
            _state.update {
                it.copy(busy = false, status = updatedStatus,
                    message = result.fold({ if (signedIn) CloudMessage(true, R.string.cloud_ok_logged_in) else CloudMessage(false, R.string.cloud_err_unauthorized) }, ::failure),
                    form = it.form.copy(password = "", code = "", confirmDelete = false),
                    askNotificationPermission = signedIn)
            }
        }
        return true
    }

    private fun scheduleExpiry(pending: PendingCloudLogin) {
        expiry?.cancel()
        expiry = viewModelScope.launch {
            delay((pending.createdAt + BROWSER_LOGIN_TIMEOUT_MS - System.currentTimeMillis()).coerceAtLeast(0))
            if (_state.value.waitingForBrowser) expireLogin()
        }
    }
    private fun expireLogin() {
        pendingStore.clear()
        _state.update { it.copy(waitingForBrowser = false, browserLaunch = null, message = CloudMessage(false, R.string.cloud_login_timeout)) }
    }
    private fun failure(error: Throwable) = CloudMessage(false,
        (error as? CloudFfiException)?.toFailure()?.messageRes() ?: R.string.cloud_err_generic)

    private suspend fun execute(job: CloudJob): CloudMessage = when (job) {
        is CloudJob.BrowserLogin -> error("Browser login is handled before account operations")
        CloudJob.Logout -> if (account.logout()) CloudMessage(true, R.string.cloud_ok_logged_out) else CloudMessage(true, R.string.cloud_ok_logged_out_local)
        CloudJob.RequestDeleteCode -> CloudMessage(true, R.string.cloud_ok_delete_code_sent, account.requestDeleteCode())
        is CloudJob.DeleteAccount -> {
            account.deleteAccount(job.password, job.code)
            CloudMessage(true, R.string.cloud_ok_deleted)
        }
    }

    override fun onCleared() {
        expiry?.cancel()
        if (accountDelegate.isInitialized()) account.close()
    }
    companion object {
        val Factory: ViewModelProvider.Factory = viewModelFactory {
            initializer { CloudAccountViewModel(checkNotNull(this[APPLICATION_KEY])) }
        }
    }
}
