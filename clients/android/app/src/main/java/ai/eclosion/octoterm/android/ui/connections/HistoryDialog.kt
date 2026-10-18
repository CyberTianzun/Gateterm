package ai.eclosion.octoterm.android.ui.connections

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import ai.eclosion.octoterm.android.R
import ai.eclosion.octoterm.android.agent.ChatWindow
import ai.eclosion.octoterm.android.agent.HistoryEntry
import ai.eclosion.octoterm.android.agent.HistoryFailure
import ai.eclosion.octoterm.android.agent.HistoryPage
import ai.eclosion.octoterm.android.ui.term.ChatView
import java.text.DateFormat
import java.util.Date
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

@Composable
fun HistoryDialog(
    reconnecting: Boolean,
    onDismiss: () -> Unit,
    onLoad: suspend (String, String, Int, Boolean, String) -> HistoryPage,
    onPreview: suspend (HistoryEntry) -> ChatWindow,
    onResume: suspend (HistoryEntry) -> Unit,
) {
    var query by remember { mutableStateOf("") }
    var agent by remember { mutableStateOf("") }
    var filterOpen by remember { mutableStateOf(false) }
    var workspace by remember { mutableStateOf("") }
    var workspaceOpen by remember { mutableStateOf(false) }
    var refresh by remember { mutableIntStateOf(0) }
    var appliedRefresh by remember { mutableIntStateOf(0) }
    var revision by remember { mutableIntStateOf(0) }
    var page by remember { mutableStateOf<HistoryPage?>(null) }
    var loading by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var preview by remember { mutableStateOf<HistoryEntry?>(null) }
    var transcript by remember { mutableStateOf<ChatWindow?>(null) }
    var working by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val listState = rememberLazyListState()
    LaunchedEffect(query, agent, refresh, workspace) {
        val request = ++revision
        loading = true
        error = null
        page = null
        listState.scrollToItem(0)
        delay(250)
        try {
            var force = refresh != appliedRefresh
            appliedRefresh = refresh
            do {
                val next = onLoad(query, agent, 0, force, workspace)
                force = false
                if (request == revision) page = next
                if (!next.indexing) break
                delay(500)
            } while (request == revision)
        }
        catch (cancel: CancellationException) { throw cancel }
        catch (failure: Exception) { if (request == revision) error = (failure as? HistoryFailure)?.reason ?: "unreadable" }
        finally { if (request == revision) loading = false }
    }
    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        BackHandler(enabled = preview != null && !working) { preview = null; transcript = null; error = null }
        Surface(modifier = Modifier.fillMaxSize().padding(12.dp), shape = MaterialTheme.shapes.large) {
            Column(Modifier.fillMaxSize().padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Row(Modifier.fillMaxWidth()) {
                    Text(stringResource(R.string.history_title), modifier = Modifier.weight(1f), style = MaterialTheme.typography.titleMedium)
                    TextButton(onClick = onDismiss) { Text(stringResource(R.string.history_close)) }
                }
                if (preview == null) {
                    OutlinedTextField(
                        value = query, onValueChange = { query = it }, singleLine = true,
                        label = { Text(stringResource(R.string.history_search)) }, modifier = Modifier.fillMaxWidth(),
                    )
                    Row {
                        Column {
                            TextButton(onClick = { filterOpen = true }) {
                                Text(page?.agents?.find { it.id == agent }?.name ?: stringResource(R.string.history_all_agents))
                            }
                            DropdownMenu(expanded = filterOpen, onDismissRequest = { filterOpen = false }) {
                                DropdownMenuItem(text = { Text(stringResource(R.string.history_all_agents)) }, onClick = { agent = ""; filterOpen = false })
                                page?.agents?.forEach { item ->
                                    DropdownMenuItem(text = { Text(item.name) }, onClick = { agent = item.id; filterOpen = false })
                                }
                            }
                        }
                        TextButton(onClick = { refresh++ }, enabled = !loading) { Text(stringResource(R.string.history_refresh)) }
                    }
                    Column {
                        TextButton(onClick = { workspaceOpen = true }) {
                            Text(workspace.ifBlank { stringResource(R.string.history_all_workspaces) }, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                        DropdownMenu(expanded = workspaceOpen, onDismissRequest = { workspaceOpen = false }) {
                            DropdownMenuItem(text = { Text(stringResource(R.string.history_all_workspaces)) }, onClick = { workspace = ""; workspaceOpen = false })
                            page?.workspaces?.forEach { path ->
                                DropdownMenuItem(text = { Text(path) }, onClick = { workspace = path; workspaceOpen = false })
                            }
                        }
                    }
                } else {
                    TextButton(onClick = { preview = null; transcript = null; error = null }, enabled = !working) {
                        Text(stringResource(R.string.history_back))
                    }
                    Text(historyTitle(preview!!), style = MaterialTheme.typography.titleSmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    HistoryMetadata(preview!!)
                    Text(stringResource(R.string.history_recent_messages), style = MaterialTheme.typography.labelMedium)
                }
                if (loading || working) LinearProgressIndicator(Modifier.fillMaxWidth())
                error?.let { Text(stringResource(historyErrorResource(it)), color = MaterialTheme.colorScheme.error) }
                if (preview != null) {
                    val messages = (transcript as? ChatWindow.Transcript)?.messages.orEmpty()
                    if (error == null || transcript != null) {
                        ChatView(
                            chat = ChatUiState(messages = messages, loading = working),
                            onOpenTerminal = { preview = null }, modifier = Modifier.weight(1f).fillMaxWidth(),
                        )
                    } else Spacer(Modifier.weight(1f))
                } else {
                    page?.let { current ->
                        Text(stringResource(if (current.indexing) R.string.history_indexing else if (current.total == 0) R.string.history_empty else R.string.history_count, current.total), style = MaterialTheme.typography.bodySmall)
                        if (current.truncated) Text(stringResource(R.string.history_truncated), style = MaterialTheme.typography.bodySmall)
                        if (current.skipped > 0) Text(stringResource(R.string.history_skipped, current.skipped), style = MaterialTheme.typography.bodySmall)
                    }
                    LazyColumn(Modifier.weight(1f).fillMaxWidth(), state = listState) {
                        items(page?.sessions.orEmpty(), key = { it.key }) { entry ->
                            Column(Modifier.padding(vertical = 10.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                                Text(historyTitle(entry), style = MaterialTheme.typography.titleSmall)
                                Text(entry.description ?: stringResource(R.string.history_no_preview), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis)
                                HistoryMetadata(entry)
                                Row {
                                    TextButton(enabled = !working, onClick = {
                                        if (!working) scope.launch {
                                            working = true; error = null; preview = entry; transcript = null
                                            try { transcript = onPreview(entry) }
                                            catch (cancel: CancellationException) { throw cancel }
                                            catch (failure: Exception) { error = (failure as? HistoryFailure)?.reason ?: "unreadable" }
                                            finally { working = false }
                                        }
                                    }) { Text(stringResource(R.string.history_preview)) }
                                    Button(enabled = !working && !reconnecting && (entry.terminalSession != null || entry.canResume), onClick = {
                                        if (!working) scope.launch {
                                            working = true; error = null
                                            try { onResume(entry); onDismiss() }
                                            catch (cancel: CancellationException) { throw cancel }
                                            catch (failure: Exception) { error = (failure as? HistoryFailure)?.reason ?: "unreadable" }
                                            finally { working = false }
                                        }
                                    }) { Text(stringResource(if (entry.terminalSession != null) R.string.history_open_terminal else R.string.history_resume)) }
                                }
                                if (entry.terminalSession == null) entry.unavailableReason?.let {
                                    Text(stringResource(historyErrorResource(it)), style = MaterialTheme.typography.bodySmall)
                                }
                            }
                            HorizontalDivider()
                        }
                        page?.nextOffset?.let { offset ->
                            item {
                                TextButton(enabled = !loading && !working, onClick = {
                                    val request = ++revision
                                    scope.launch {
                                        loading = true; error = null
                                        try {
                                            val next = onLoad(query, agent, offset, false, workspace)
                                            if (request == revision) page = next.copy(sessions = (page?.sessions.orEmpty() + next.sessions).distinctBy { it.key })
                                        } catch (cancel: CancellationException) { throw cancel }
                                        catch (failure: Exception) { if (request == revision) error = (failure as? HistoryFailure)?.reason ?: "unreadable" }
                                        finally { if (request == revision) loading = false }
                                    }
                                }) { Text(stringResource(R.string.history_more)) }
                            }
                        }
                        page?.takeIf { !it.indexing && it.nextOffset == null }?.maxSessions?.let { maximum ->
                            item {
                                Text(stringResource(R.string.history_limit, maximum), style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(vertical = 16.dp))
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun historyTitle(entry: HistoryEntry): String = entry.title.takeIf { it.isNotBlank() && it != entry.sessionId }
    ?: stringResource(R.string.history_untitled, entry.agentName)

@Composable
private fun HistoryMetadata(entry: HistoryEntry) {
    val muted = MaterialTheme.colorScheme.onSurfaceVariant
    Text("${entry.agentName} · ${DateFormat.getDateTimeInstance(DateFormat.SHORT, DateFormat.SHORT).format(Date(entry.updatedAt * 1000))} · ${entry.model ?: stringResource(R.string.history_unknown_model)}", style = MaterialTheme.typography.bodySmall, color = muted)
    Text(entry.cwd ?: stringResource(R.string.history_unknown_directory), style = MaterialTheme.typography.bodySmall, color = muted)
    Text(entry.sessionId, style = MaterialTheme.typography.labelSmall, color = muted, fontFamily = FontFamily.Monospace)
}

private fun historyErrorResource(reason: String): Int = when (reason) {
    "disabled" -> R.string.history_disabled
    "missing-directory" -> R.string.history_missing_directory
    "missing-cli" -> R.string.history_missing_cli
    "not-found" -> R.string.history_not_found
    "unsupported-agent" -> R.string.history_unsupported
    "spawn-failed" -> R.string.history_spawn_failed
    else -> R.string.history_unreadable
}
