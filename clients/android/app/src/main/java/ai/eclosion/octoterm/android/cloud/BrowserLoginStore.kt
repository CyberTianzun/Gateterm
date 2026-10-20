package ai.eclosion.octoterm.android.cloud

import android.util.AtomicFile
import org.json.JSONObject
import java.io.File

/** Pending PKCE proof is private and excluded from Android backups. No Cloud access token is stored here. */
class BrowserLoginStore(directory: File) {
    private val file = AtomicFile(File(directory, "cloud-browser-login.json"))

    @Synchronized
    fun load(now: Long = System.currentTimeMillis()): PendingCloudLogin? {
        return try {
            val json = JSONObject(String(file.readFully(), Charsets.UTF_8))
            val pending = PendingCloudLogin(
                RelayRegion.valueOf(json.getString("region")), json.getString("state"),
                json.getString("verifier"), json.getLong("createdAt"), json.getString("language"), json.getString("label"),
            )
            if (pending.expired(now)) { file.delete(); null } else pending
        } catch (_: Exception) { file.delete(); null }
    }

    @Synchronized
    fun save(pending: PendingCloudLogin) {
        val json = JSONObject().put("region", pending.region.name).put("state", pending.state)
            .put("verifier", pending.verifier).put("createdAt", pending.createdAt)
            .put("language", pending.language).put("label", pending.label)
        val stream = file.startWrite()
        try {
            stream.write(json.toString().toByteArray(Charsets.UTF_8))
            file.finishWrite(stream)
        } catch (error: Exception) { file.failWrite(stream); throw error }
    }

    @Synchronized
    fun claim(raw: String): Pair<PendingCloudLogin, String>? {
        val pending = load() ?: return null
        val code = callbackCode(raw, pending, System.currentTimeMillis()) ?: return null
        file.delete()
        return pending to code
    }

    @Synchronized
    fun clear() = file.delete()
}
