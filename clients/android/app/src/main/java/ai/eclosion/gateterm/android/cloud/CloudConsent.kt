package ai.eclosion.gateterm.android.cloud

/** Cloud data opt-in is independent of account state and Android notification permission. */
class CloudConsent(
    readRevision: () -> String?,
    private val persistRevision: (String) -> Boolean,
) {
    @Volatile private var confirmedRevision = readRevision()
    val accepted: Boolean get() = confirmedRevision == POLICY_REVISION
    private var pending: (() -> Unit)? = null

    fun request(action: () -> Unit): Boolean {
        if (!accepted) {
            pending = action
            return false
        }
        action()
        return true
    }

    fun accept(): Boolean {
        val action = pending ?: return false
        if (!persistRevision(POLICY_REVISION)) return false
        confirmedRevision = POLICY_REVISION
        pending = null
        action()
        return true
    }

    fun dismiss() { pending = null }

    companion object {
        const val POLICY_REVISION = "2026-11-13"
    }
}
