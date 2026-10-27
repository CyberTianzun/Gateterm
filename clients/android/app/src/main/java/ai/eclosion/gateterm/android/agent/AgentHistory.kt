package ai.eclosion.gateterm.android.agent

import ai.eclosion.gateterm.android.wire.SessionInfo
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import org.json.JSONObject

data class HistoryEntry(
    val agentId: String,
    val agentName: String,
    val sessionId: String,
    val title: String,
    val cwd: String?,
    val updatedAt: Long,
    val terminalSession: Long?,
    val canResume: Boolean,
    val unavailableReason: String?,
    val description: String? = null,
    val model: String? = null,
) {
    val key: String get() = "$agentId:$sessionId"
}

data class HistoryAgent(val id: String, val name: String)
data class HistoryPage(
    val sessions: List<HistoryEntry>,
    val agents: List<HistoryAgent>,
    val total: Int,
    val nextOffset: Int?,
    val truncated: Boolean,
    val skipped: Int,
    val indexing: Boolean = false,
    val maxSessions: Int? = null,
    val workspaces: List<String> = emptyList(),
)

class HistoryFailure(val reason: String) : Exception(reason)

object AgentHistory {
    fun parsePage(body: String): HistoryPage {
        val root = JSONObject(body)
        val rows = root.getJSONArray("sessions")
        val sessions = buildList {
            for (i in 0 until rows.length()) {
                val row = rows.getJSONObject(i)
                add(HistoryEntry(
                    row.getString("agent_id"), row.getString("agent_name"), row.getString("agent_session_id"),
                    row.getString("title"), row.optNullableString("cwd"), row.optLong("updated_at"),
                    if (row.isNull("terminal_session")) null else row.getLong("terminal_session"),
                    row.opt("can_resume") == true, row.optNullableString("unavailable_reason"),
                    row.optNullableString("description"), row.optNullableString("model"),
                ))
            }
        }.distinctBy { it.key }
        val agents = root.optJSONArray("agents")
        return HistoryPage(
            sessions,
            buildList {
                if (agents != null) for (i in 0 until agents.length()) {
                    val agent = agents.getJSONObject(i)
                    add(HistoryAgent(agent.getString("id"), agent.getString("name")))
                }
            },
            root.optInt("total"), if (root.isNull("next_offset")) null else root.getInt("next_offset"),
            root.optBoolean("truncated"), root.optInt("skipped"),
            root.optBoolean("indexing"), if (root.isNull("max_sessions")) null else root.getInt("max_sessions"),
            buildList {
                root.optJSONArray("workspaces")?.let { paths ->
                    for (i in 0 until paths.length()) paths.optString(i).takeIf { it.isNotBlank() }?.let { add(it) }
                }
            },
        )
    }

    fun identity(entry: HistoryEntry): JSONObject = JSONObject()
        .put("agent_id", entry.agentId).put("agent_session_id", entry.sessionId)

    private fun request(http: OkHttpClient, token: String, request: Request.Builder): String {
        try {
            http.newCall(request.header("Authorization", "Bearer $token").build()).execute().use { response ->
                val body = response.body?.string().orEmpty()
                if (!response.isSuccessful) {
                    val reason = runCatching { JSONObject(body).optString("error", "unreadable") }.getOrDefault("unreadable")
                    throw HistoryFailure(reason)
                }
                return body
            }
        } catch (error: HistoryFailure) { throw error }
        catch (_: Exception) { throw HistoryFailure("unreadable") }
    }

    fun fetch(http: OkHttpClient, origin: String, token: String, query: String, agent: String, offset: Int, refresh: Boolean, workspace: String = ""): HistoryPage {
        val url = (origin.trimEnd('/') + "/api/agents/history").toHttpUrl().newBuilder()
            .addQueryParameter("q", query).addQueryParameter("agent_id", agent)
            .addQueryParameter("offset", offset.toString()).addQueryParameter("refresh", refresh.toString())
            .addQueryParameter("workspace", workspace).build()
        return try { parsePage(request(http, token, Request.Builder().url(url))) }
        catch (error: HistoryFailure) { throw error }
        catch (_: Exception) { throw HistoryFailure("unreadable") }
    }

    fun messages(http: OkHttpClient, origin: String, token: String, entry: HistoryEntry): ChatWindow {
        val url = (origin.trimEnd('/') + "/api/agents/history/messages").toHttpUrl().newBuilder()
            .addQueryParameter("agent_id", entry.agentId).addQueryParameter("agent_session_id", entry.sessionId).build()
        val window = Chat.parseWindow(request(http, token, Request.Builder().url(url)))
        if (window !is ChatWindow.Transcript) throw HistoryFailure("unreadable")
        return window
    }

    fun open(http: OkHttpClient, origin: String, token: String, entry: HistoryEntry): SessionInfo {
        val body = identity(entry).toString().toRequestBody("application/json".toMediaType())
        val row = try {
            JSONObject(request(http, token, Request.Builder().url(origin.trimEnd('/') + "/api/agents/history/open").post(body)))
                .getJSONObject("session")
        } catch (error: HistoryFailure) { throw error }
        catch (_: Exception) { throw HistoryFailure("unreadable") }
        return SessionInfo(row.getLong("id"), row.optString("name"), row.optInt("cols"), row.optInt("rows"), row.optLong("created_at"))
    }
}
