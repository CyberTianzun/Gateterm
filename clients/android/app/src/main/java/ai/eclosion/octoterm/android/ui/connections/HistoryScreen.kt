package ai.eclosion.octoterm.android.ui.connections

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconToggleButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
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
import ai.eclosion.octoterm.android.R
import ai.eclosion.octoterm.android.ui.components.AppBarTitle
import ai.eclosion.octoterm.android.agent.ChatWindow
import ai.eclosion.octoterm.android.agent.HistoryAgent
import ai.eclosion.octoterm.android.agent.HistoryEntry
import ai.eclosion.octoterm.android.agent.HistoryFailure
import ai.eclosion.octoterm.android.agent.HistoryPage
import ai.eclosion.octoterm.android.ui.term.ChatView
import java.text.DateFormat
import java.util.Date
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HistoryScreen(
    reconnecting: Boolean,
    onBack: () -> Unit,
    onLoad: suspend (String, String, Int, Boolean, String) -> HistoryPage,
    onPreview: suspend (HistoryEntry) -> ChatWindow,
    onResume: suspend (HistoryEntry) -> Unit,
) {
    var filters by remember { mutableStateOf(HistoryFilters()) }
    var filtersOpen by remember { mutableStateOf(false) }
    val query = filters.query
    val agent = filters.agent
    val workspace = filters.workspace
    var refresh by remember { mutableIntStateOf(0) }
    var appliedRefresh by remember { mutableIntStateOf(0) }
    var revision by remember { mutableIntStateOf(0) }
    var page by remember { mutableStateOf<HistoryPage?>(null) }
    var loading by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var preview by remember { mutableStateOf<HistoryEntry?>(null) }
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
        } catch (cancel: CancellationException) {
            throw cancel
        } catch (failure: Exception) {
            if (request == revision) error = (failure as? HistoryFailure)?.reason ?: "unreadable"
        } finally {
            if (request == revision) loading = false
        }
    }
    BackHandler(onBack = onBack)
    if (filtersOpen) {
        HistoryFilterScreen(
            filters = filters,
            agents = page?.agents.orEmpty(),
            workspaces = page?.workspaces.orEmpty(),
            onBack = { filtersOpen = false },
            onApply = { filters = it; filtersOpen = false },
        )
        return
    }
    preview?.let { entry ->
        HistoryConversationScreen(entry = entry, onBack = { preview = null }, onPreview = onPreview)
        return
    }
    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    AppBarTitle(stringResource(R.string.history_title), maxLines = 1, overflow = TextOverflow.Ellipsis)
                },
                navigationIcon = { HistoryBackButton(onBack) },
                actions = {
                    IconButton(onClick = { refresh++ }, enabled = !loading) {
                        Icon(Icons.Filled.Refresh, contentDescription = stringResource(R.string.history_refresh))
                    }
                    TextButton(onClick = { filtersOpen = true }) {
                        val count = listOf(query, agent, workspace).count { it.isNotBlank() }
                        Text(stringResource(if (count == 0) R.string.history_filter else R.string.history_filter_count, count))
                    }
                },
            )
        },
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).consumeWindowInsets(padding),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            if (loading || working) LinearProgressIndicator(Modifier.fillMaxWidth())
            error?.let {
                Text(stringResource(historyErrorResource(it)), color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(horizontal = 16.dp))
            }
            page?.let { current ->
                Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(stringResource(if (current.indexing) R.string.history_indexing else if (current.total == 0) R.string.history_empty else R.string.history_count, current.total), style = MaterialTheme.typography.bodySmall)
                    if (current.truncated) Text(stringResource(R.string.history_truncated), style = MaterialTheme.typography.bodySmall)
                    if (current.skipped > 0) Text(stringResource(R.string.history_skipped, current.skipped), style = MaterialTheme.typography.bodySmall)
                }
            }
            LazyColumn(
                Modifier.weight(1f).fillMaxWidth(), state = listState,
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
            ) {
                items(page?.sessions.orEmpty(), key = { it.key }) { entry ->
                    Column(Modifier.padding(vertical = 10.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text(historyTitle(entry), style = MaterialTheme.typography.titleSmall)
                        Text(entry.description ?: stringResource(R.string.history_no_preview), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis)
                        HistoryMetadata(entry)
                        Row {
                            TextButton(enabled = !working, onClick = { preview = entry }) { Text(stringResource(R.string.history_preview)) }
                            Button(enabled = !working && !reconnecting && (entry.terminalSession != null || entry.canResume), onClick = {
                                if (!working) scope.launch {
                                    working = true; error = null
                                    try {
                                        onResume(entry)
                                        onBack()
                                    } catch (cancel: CancellationException) {
                                        throw cancel
                                    } catch (failure: Exception) {
                                        error = (failure as? HistoryFailure)?.reason ?: "unreadable"
                                    } finally {
                                        working = false
                                    }
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
                                } catch (cancel: CancellationException) {
                                    throw cancel
                                } catch (failure: Exception) {
                                    if (request == revision) error = (failure as? HistoryFailure)?.reason ?: "unreadable"
                                } finally {
                                    if (request == revision) loading = false
                                }
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

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun HistoryConversationScreen(
    entry: HistoryEntry,
    onBack: () -> Unit,
    onPreview: suspend (HistoryEntry) -> ChatWindow,
) {
    var transcript by remember(entry.key) { mutableStateOf<ChatWindow?>(null) }
    var loading by remember(entry.key) { mutableStateOf(true) }
    var error by remember(entry.key) { mutableStateOf<String?>(null) }
    var detailsOpen by remember(entry.key) { mutableStateOf(false) }
    LaunchedEffect(entry.key) {
        try {
            transcript = onPreview(entry)
        } catch (cancel: CancellationException) {
            throw cancel
        } catch (failure: Exception) {
            error = (failure as? HistoryFailure)?.reason ?: "unreadable"
        } finally {
            loading = false
        }
    }
    BackHandler(onBack = onBack)
    Scaffold(
        topBar = {
            TopAppBar(
                title = { AppBarTitle(historyTitle(entry), maxLines = 1, overflow = TextOverflow.Ellipsis) },
                navigationIcon = { HistoryBackButton(onBack) },
                actions = {
                    IconToggleButton(checked = detailsOpen, onCheckedChange = { detailsOpen = it }) {
                        Icon(Icons.Filled.Info, contentDescription = stringResource(R.string.history_details))
                    }
                },
            )
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding).consumeWindowInsets(padding)) {
            if (detailsOpen) {
                Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(historyTitle(entry), style = MaterialTheme.typography.titleSmall)
                    HistoryMetadata(entry)
                }
                HorizontalDivider()
            }
            if (error != null) {
                Text(stringResource(historyErrorResource(error!!)), color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(16.dp))
            } else {
                ChatView(
                    chat = ChatUiState(messages = (transcript as? ChatWindow.Transcript)?.messages.orEmpty(), loading = loading),
                    onOpenTerminal = onBack,
                    modifier = Modifier.weight(1f).fillMaxWidth(),
                )
            }
        }
    }
}

@Composable
private fun HistoryBackButton(onBack: () -> Unit) {
    IconButton(onClick = onBack) {
        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.action_back))
    }
}

private data class HistoryFilters(val query: String = "", val agent: String = "", val workspace: String = "")

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun HistoryFilterScreen(
    filters: HistoryFilters,
    agents: List<HistoryAgent>,
    workspaces: List<String>,
    onBack: () -> Unit,
    onApply: (HistoryFilters) -> Unit,
) {
    var draft by remember(filters) { mutableStateOf(filters) }
    var agentOpen by remember { mutableStateOf(false) }
    var workspaceOpen by remember { mutableStateOf(false) }
    BackHandler(onBack = onBack)
    Scaffold(
        topBar = {
            TopAppBar(
                title = { AppBarTitle(stringResource(R.string.history_filter)) },
                navigationIcon = { HistoryBackButton(onBack) },
                actions = {
                    TextButton(onClick = { onApply(draft) }) { Text(stringResource(R.string.history_filter_apply)) }
                },
            )
        },
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).consumeWindowInsets(padding)
                .verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            OutlinedTextField(
                value = draft.query, onValueChange = { draft = draft.copy(query = it) }, singleLine = true,
                label = { Text(stringResource(R.string.history_search)) }, modifier = Modifier.fillMaxWidth(),
            )
            Column {
                Text(stringResource(R.string.history_filter_agent), style = MaterialTheme.typography.labelLarge)
                Box(Modifier.fillMaxWidth()) {
                    OutlinedButton(onClick = { agentOpen = true }, modifier = Modifier.fillMaxWidth()) {
                        Text(agents.find { it.id == draft.agent }?.name ?: draft.agent.ifBlank { stringResource(R.string.history_all_agents) })
                    }
                    DropdownMenu(expanded = agentOpen, onDismissRequest = { agentOpen = false }) {
                        DropdownMenuItem(text = { Text(stringResource(R.string.history_all_agents)) }, onClick = { draft = draft.copy(agent = ""); agentOpen = false })
                        agents.forEach { item ->
                            DropdownMenuItem(text = { Text(item.name) }, onClick = { draft = draft.copy(agent = item.id); agentOpen = false })
                        }
                    }
                }
            }
            Column {
                Text(stringResource(R.string.history_filter_workspace), style = MaterialTheme.typography.labelLarge)
                Box(Modifier.fillMaxWidth()) {
                    OutlinedButton(onClick = { workspaceOpen = true }, modifier = Modifier.fillMaxWidth()) {
                        Text(draft.workspace.ifBlank { stringResource(R.string.history_all_workspaces) }, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    }
                    DropdownMenu(expanded = workspaceOpen, onDismissRequest = { workspaceOpen = false }) {
                        DropdownMenuItem(text = { Text(stringResource(R.string.history_all_workspaces)) }, onClick = { draft = draft.copy(workspace = ""); workspaceOpen = false })
                        workspaces.forEach { path ->
                            DropdownMenuItem(text = { Text(path) }, onClick = { draft = draft.copy(workspace = path); workspaceOpen = false })
                        }
                    }
                }
            }
            TextButton(onClick = { draft = HistoryFilters() }) { Text(stringResource(R.string.history_filter_reset)) }
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
