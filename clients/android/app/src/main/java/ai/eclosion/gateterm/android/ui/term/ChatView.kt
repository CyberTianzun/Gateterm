package ai.eclosion.gateterm.android.ui.term

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import ai.eclosion.gateterm.android.R
import ai.eclosion.gateterm.android.agent.Chat
import ai.eclosion.gateterm.android.agent.ChatBlock
import ai.eclosion.gateterm.android.agent.ChatFallback
import ai.eclosion.gateterm.android.agent.ChatMessage
import ai.eclosion.gateterm.android.agent.ChatRole
import ai.eclosion.gateterm.android.agent.userPromptTextBlocks
import ai.eclosion.gateterm.android.appearance.UiPalette
import ai.eclosion.gateterm.android.appearance.uiPalette
import ai.eclosion.gateterm.android.ui.connections.ChatNotice
import ai.eclosion.gateterm.android.ui.connections.ChatUiState
import ai.eclosion.gateterm.android.ui.theme.LocalTermAppearance
import kotlinx.coroutines.delay

private val UserStripe = Color(0xFF4EA1FF)
private val SystemStripe = Color(0xFF9A8CFF)
private const val JUMP_HIGHLIGHT_MS = 1800L

/**
 * 聊天视图。和 web 端同一套呈现:每条消息左侧一道色条区分角色,思考过程默认折叠,
 * 工具调用和结果默认折叠,摘要保留命令、文件及失败状态,展开后可查看完整内容。
 *
 * 读不到记录时**说清原因并给出回终端的出路**(协议 A9)—— 空白的聊天框会被读成
 * 「没有对话」,比一句解释更糟。
 */
@Composable
fun ChatView(
    chat: ChatUiState,
    onOpenTerminal: () -> Unit,
    modifier: Modifier = Modifier,
    onLoadOlder: () -> Unit = {},
    userOnly: Boolean = false,
    onUserOnlyChange: (Boolean) -> Unit = {},
) {
    val palette = LocalTermAppearance.current.uiPalette()
    val bg = palette.background.toOpaqueColor()
    val fg = palette.foreground.toOpaqueColor()

    CompositionLocalProvider(LocalContentColor provides fg) {
        Box(modifier.background(bg)) {
            val notice = chat.notice
            when {
                notice != null -> Fallback(noticeText(notice), onOpenTerminal, Modifier.fillMaxSize())
                chat.messages.isEmpty() && chat.loading -> Centered(Modifier.fillMaxSize()) {
                    CircularProgressIndicator()
                    Spacer(Modifier.height(12.dp))
                    Text(
                        stringResource(R.string.chat_loading),
                        style = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
                    )
                }
                chat.messages.isEmpty() && chat.olderCursor == null -> Centered(Modifier.fillMaxSize()) {
                    Text(
                        stringResource(R.string.chat_empty),
                        style = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
                    )
                }
                else -> key(chat.sessionId) {
                    Messages(
                        chat = chat,
                        onLoadOlder = onLoadOlder,
                        userOnly = userOnly,
                        onUserOnlyChange = onUserOnlyChange,
                        palette = palette,
                        modifier = Modifier.fillMaxSize(),
                    )
                }
            }
        }
    }
}

@Composable
private fun Messages(
    chat: ChatUiState,
    onLoadOlder: () -> Unit,
    userOnly: Boolean,
    onUserOnlyChange: (Boolean) -> Unit,
    palette: UiPalette,
    modifier: Modifier,
) {
    val messages = chat.messages
    val userPrompts = remember(messages) { Chat.userPrompts(messages) }
    val displayedMessages = if (userOnly) userPrompts else messages
    val trailingItemIndex = if (userOnly && userPrompts.isEmpty()) 2 else displayedMessages.size + 1

    // The trailing item anchors the viewport below even a very tall final message.
    val listState = rememberLazyListState(initialFirstVisibleItemIndex = trailingItemIndex)
    var historyPage by remember { mutableStateOf(chat.historyPage) }
    var historyAnchor by remember { mutableStateOf<Pair<String, Int>?>(null) }
    var visibleAnchor by remember { mutableStateOf<Pair<String, Int>?>(null) }
    val olderLoading by rememberUpdatedState(chat.olderLoading)
    val latestHistoryPage by rememberUpdatedState(chat.historyPage)
    // 贴着底部时才自动跟随;用户往上翻看历史时不该被拽回来。只在**用户滚动**时更新,
    // 否则新消息一来「不在底部了」会先于跟随生效,把自己关掉。
    var follow by remember { mutableStateOf(true) }
    var scrollingToBottom by remember { mutableStateOf(false) }
    var pendingJumpId by remember { mutableStateOf<String?>(null) }
    var highlightedId by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(listState) {
        snapshotFlow { Triple(listState.isScrollInProgress, listState.canScrollForward, scrollingToBottom) }
            .collect { (scrolling, canScrollForward, automatic) ->
                if (scrolling && !automatic) follow = !canScrollForward
            }
    }
    LaunchedEffect(listState) {
        snapshotFlow {
            if (!scrollingToBottom && historyPage == latestHistoryPage)
                listState.layoutInfo.visibleItemsInfo.firstOrNull { (it.key as? String)?.startsWith("message:") == true }
                    ?.let { it.key as String to it.offset }
            else null
        }.collect { anchor ->
            if (anchor != null) {
                visibleAnchor = anchor
                if (olderLoading && listState.isScrollInProgress) historyAnchor = anchor
            }
        }
    }
    LaunchedEffect(messages, chat.historyPage, userOnly) {
        if (historyPage != chat.historyPage) {
            historyPage = chat.historyPage
            if (follow && pendingJumpId == null) {
                scrollingToBottom = true
                try { listState.scrollToItem(trailingItemIndex) }
                finally { scrollingToBottom = false }
            } else {
                val anchor = historyAnchor ?: visibleAnchor
                if (anchor != null && chat.olderLoading && historyAnchor == null) {
                    historyAnchor = anchor
                }
                val index = anchor?.let { saved -> displayedMessages.indexOfFirst { "message:${it.id}" == saved.first } } ?: -1
                if (anchor != null && index >= 0) {
                    scrollingToBottom = true
                    try { listState.scrollToItem(index + 1, -anchor.second) }
                    finally { scrollingToBottom = false }
                }
            }
            return@LaunchedEffect
        }
        if (follow && pendingJumpId == null) {
            scrollingToBottom = true
            try { listState.scrollToItem(trailingItemIndex) }
            finally { scrollingToBottom = false }
        }
    }
    LaunchedEffect(chat.olderLoading) {
        if (!chat.olderLoading) historyAnchor = null
    }
    LaunchedEffect(pendingJumpId, userOnly, messages) {
        val targetId = pendingJumpId ?: return@LaunchedEffect
        if (userOnly) return@LaunchedEffect
        val index = messages.indexOfFirst { it.id == targetId }
        if (index < 0) {
            pendingJumpId = null
            return@LaunchedEffect
        }
        follow = false
        scrollingToBottom = true
        try {
            listState.scrollToItem(index + 1, scrollOffset = -48)
        } finally {
            scrollingToBottom = false
        }
        highlightedId = targetId
        pendingJumpId = null
    }
    LaunchedEffect(highlightedId) {
        if (highlightedId != null) {
            delay(JUMP_HIGHLIGHT_MS)
            highlightedId = null
        }
    }

    val scrollbarColor = palette.dim.toOpaqueColor()
    Column(modifier.fillMaxSize()) {
        if (userOnly) {
            FilterStatusBanner(
                count = userPrompts.size,
                palette = palette,
                onClear = { onUserOnlyChange(false) },
            )
        }
        LazyColumn(
            state = listState,
            modifier = Modifier
                .weight(1f)
                .fillMaxWidth()
                .chatScrollbar(listState, displayedMessages.size + 1, scrollbarColor),
            contentPadding = PaddingValues(horizontal = 12.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            item(key = "history-control") {
                if (chat.historySupported) Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                    if (chat.olderCursor != null) OutlinedButton(onClick = {
                        follow = false
                        historyAnchor = listState.layoutInfo.visibleItemsInfo
                            .firstOrNull { (it.key as? String)?.startsWith("message:") == true }
                            ?.let { it.key as String to it.offset }
                        onLoadOlder()
                    }, enabled = !chat.olderLoading) {
                        Text(
                            stringResource(when {
                                chat.olderLoading -> R.string.chat_loading_older
                                chat.olderFailed -> R.string.chat_older_failed
                                else -> R.string.chat_load_older
                            }),
                            fontFamily = FontFamily.Monospace,
                        )
                    } else Text(
                        stringResource(R.string.chat_beginning),
                        style = MaterialTheme.typography.labelMedium.copy(fontFamily = FontFamily.Monospace),
                        color = palette.dim.toOpaqueColor(),
                        modifier = Modifier.padding(vertical = 8.dp),
                    )
                }
            }
            if (userOnly && userPrompts.isEmpty()) {
                item(key = "filter-empty") {
                    EmptyFilterNotice(palette = palette, onClear = { onUserOnlyChange(false) })
                }
            } else if (userOnly) {
                items(userPrompts, key = { "message:${it.id}" }) { message ->
                    UserPromptFilterRow(
                        message = message,
                        palette = palette,
                        onClick = {
                            pendingJumpId = message.id
                            follow = false
                            onUserOnlyChange(false)
                        },
                    )
                }
            } else {
                items(messages, key = { "message:${it.id}" }) { message ->
                    MessageRow(
                        message = message,
                        palette = palette,
                        highlighted = highlightedId == message.id,
                    )
                }
            }
            item(key = "conversation-end") { Spacer(Modifier.height(1.dp)) }
        }
    }
}

@Composable
private fun FilterStatusBanner(
    count: Int,
    palette: UiPalette,
    onClear: () -> Unit,
) {
    Column(Modifier.fillMaxWidth().background(palette.panel.toOpaqueColor())) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 12.dp, vertical = 2.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                text = stringResource(R.string.chat_filter_banner, count),
                fontFamily = FontFamily.Monospace,
                fontSize = 12.sp,
                color = palette.dim.toOpaqueColor(),
                modifier = Modifier.weight(1f),
            )
            TextButton(onClick = onClear) {
                Text(
                    text = stringResource(R.string.chat_filter_clear),
                    fontFamily = FontFamily.Monospace,
                    fontSize = 12.sp,
                    color = palette.accent.toOpaqueColor(),
                )
            }
        }
        HorizontalDivider(thickness = 1.dp, color = palette.line.toOpaqueColor())
    }
}

@Composable
private fun EmptyFilterNotice(
    palette: UiPalette,
    onClear: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 28.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Text(
            text = stringResource(R.string.chat_filter_empty),
            fontFamily = FontFamily.Monospace,
            fontSize = 13.sp,
            color = palette.dim.toOpaqueColor(),
        )
        OutlinedButton(onClick = onClear) {
            Text(
                text = stringResource(R.string.chat_filter_clear),
                fontFamily = FontFamily.Monospace,
                fontSize = 12.sp,
            )
        }
    }
}

@Composable
private fun UserPromptFilterRow(
    message: ChatMessage,
    palette: UiPalette,
    onClick: () -> Unit,
) {
    val promptBlocks = remember(message) { message.userPromptTextBlocks() }
    val rowShape = RoundedCornerShape(topEnd = 6.dp, bottomEnd = 6.dp)
    Box(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 48.dp)
            .clip(rowShape)
            .background(palette.panel.toOpaqueColor().copy(alpha = 0.45f))
            .clickable(role = Role.Button, onClick = onClick)
            .drawBehind {
                val stripeW = 3.dp.toPx()
                drawRoundRect(
                    color = UserStripe,
                    topLeft = Offset.Zero,
                    size = Size(stripeW, size.height),
                    cornerRadius = CornerRadius(2.dp.toPx(), 2.dp.toPx()),
                )
            }
            .padding(start = 13.dp, end = 8.dp, top = 8.dp, bottom = 8.dp),
    ) {
        Row(
            Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.Top,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Column(
                Modifier.weight(1f),
                verticalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                promptBlocks.forEach { block ->
                    Text(
                        text = block.text,
                        fontFamily = FontFamily.Monospace,
                        fontSize = 13.sp,
                        lineHeight = 20.sp,
                        color = palette.foreground.toOpaqueColor(),
                    )
                }
            }
            Text(
                text = "↗",
                fontFamily = FontFamily.Monospace,
                fontSize = 12.sp,
                color = palette.dim.toOpaqueColor(),
                modifier = Modifier.clearAndSetSemantics { },
            )
        }
    }
}

/** Keep a scroll indicator visible whenever the conversation exceeds the viewport. */
private fun Modifier.chatScrollbar(state: LazyListState, messageCount: Int, color: Color): Modifier = drawWithContent {
    drawContent()
    if ((!state.canScrollBackward && !state.canScrollForward) || messageCount == 0) return@drawWithContent
    val layout = state.layoutInfo
    val visible = layout.visibleItemsInfo.filter { it.index < messageCount }
    val first = visible.firstOrNull() ?: return@drawWithContent
    val viewportStart = layout.viewportStartOffset
    val viewportEnd = layout.viewportEndOffset
    // Use visible fractions so a single message taller than the viewport still has a thumb.
    val visibleFraction = visible.sumOf { item ->
        val shown = (minOf(item.offset + item.size, viewportEnd) - maxOf(item.offset, viewportStart)).coerceAtLeast(0)
        shown.toDouble() / item.size.coerceAtLeast(1)
    }.toFloat()
    val firstFraction = (viewportStart - first.offset).coerceIn(0, first.size).toFloat() / first.size.coerceAtLeast(1)
    val progress = when {
        !state.canScrollBackward -> 0f
        !state.canScrollForward -> 1f
        else -> ((first.index + firstFraction) / (messageCount - visibleFraction).coerceAtLeast(0.001f)).coerceIn(0f, 1f)
    }
    val inset = 4.dp.toPx()
    val trackHeight = (size.height - inset * 2).coerceAtLeast(0f)
    if (trackHeight == 0f) return@drawWithContent
    val thumbHeight = (trackHeight * visibleFraction / messageCount).coerceIn(minOf(24.dp.toPx(), trackHeight), trackHeight)
    val width = 4.dp.toPx()
    val x = size.width - width - 2.dp.toPx()
    val radius = CornerRadius(width / 2)
    drawRoundRect(color.copy(alpha = 0.12f), Offset(x, inset), Size(width, trackHeight), radius)
    drawRoundRect(color.copy(alpha = 0.7f), Offset(x, inset + (trackHeight - thumbHeight) * progress), Size(width, thumbHeight), radius)
}

@Composable
private fun MessageRow(
    message: ChatMessage,
    palette: UiPalette,
    highlighted: Boolean,
) {
    val stripe = when (message.role) {
        ChatRole.User -> UserStripe
        ChatRole.Assistant -> palette.dim.toOpaqueColor().copy(alpha = 0.5f)
        ChatRole.System -> SystemStripe
    }
    val highlightBg by animateColorAsState(
        targetValue = if (highlighted) palette.accent.toOpaqueColor().copy(alpha = 0.22f) else Color.Transparent,
        animationSpec = tween(durationMillis = if (highlighted) 180 else 600),
        label = "jump-highlight",
    )
    Box(
        Modifier
            .fillMaxWidth()
            .background(highlightBg, RoundedCornerShape(topEnd = 6.dp, bottomEnd = 6.dp))
            .alpha(if (message.role == ChatRole.System) 0.8f else 1f)
            .drawBehind {
                val stripeW = 3.dp.toPx()
                drawRoundRect(
                    color = stripe,
                    topLeft = Offset.Zero,
                    size = Size(stripeW, size.height),
                    cornerRadius = CornerRadius(2.dp.toPx(), 2.dp.toPx()),
                )
            }
            .padding(start = 13.dp, top = 2.dp, bottom = 2.dp, end = 2.dp),
    ) {
        Column(
            Modifier.fillMaxWidth(),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            message.blocks.forEachIndexed { i, block ->
                BlockView(block, message.role, "${message.id}#$i", palette)
            }
        }
    }
}

@Composable
private fun BlockView(
    block: ChatBlock,
    role: ChatRole,
    stateKey: String,
    palette: UiPalette,
) {
    when (block) {
        is ChatBlock.Text -> {
            if (role == ChatRole.Assistant) {
                ChatMarkdown(block.text, palette)
            } else {
                SelectionContainer {
                    Text(
                        text = block.text,
                        fontFamily = FontFamily.Monospace,
                        fontSize = 13.sp,
                        lineHeight = 20.sp,
                        color = palette.foreground.toOpaqueColor(),
                    )
                }
            }
        }
        is ChatBlock.Thinking -> Disclosure(
            stateKey = stateKey,
            label = stringResource(R.string.chat_thinking),
            palette = palette,
            modifier = Modifier.alpha(0.8f),
        ) {
            SelectionContainer {
                Text(
                    text = block.text,
                    fontFamily = FontFamily.Monospace,
                    fontSize = 12.sp,
                    lineHeight = 18.sp,
                    color = palette.foreground.toOpaqueColor(),
                )
            }
        }
        is ChatBlock.ToolUse -> Disclosure(
            stateKey = stateKey,
            label = block.name,
            preview = excerpt(block.input),
            palette = palette,
        ) {
            ToolBody(block.input, palette)
        }
        is ChatBlock.ToolResult -> ToolResult(block, stateKey, palette)
    }
}

@Composable
private fun ToolResult(
    block: ChatBlock.ToolResult,
    stateKey: String,
    palette: UiPalette,
) {
    val bg = if (block.ok) {
        palette.panel2.toOpaqueColor()
    } else {
        palette.danger.toOpaqueColor().copy(alpha = 0.15f)
    }
    Disclosure(
        stateKey = stateKey,
        label = stringResource(R.string.chat_tool_result),
        preview = excerpt(block.text),
        error = !block.ok,
        palette = palette,
        modifier = Modifier
            .fillMaxWidth()
            .background(bg, RoundedCornerShape(4.dp))
            .padding(horizontal = 6.dp),
    ) {
        ToolBody(block.text, palette)
    }
}

private fun excerpt(text: String): String =
    text.lineSequence().firstOrNull { it.isNotBlank() }?.trim()?.take(160).orEmpty()

@Composable
private fun ToolBody(text: String, palette: UiPalette) {
    SelectionContainer {
        Text(
            text = text,
            fontSize = 12.sp,
            lineHeight = 18.sp,
            fontFamily = FontFamily.Monospace,
            color = palette.foreground.toOpaqueColor(),
        )
    }
}

@Composable
private fun Disclosure(
    stateKey: String,
    label: String,
    palette: UiPalette,
    preview: String = "",
    error: Boolean = false,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    var expanded by rememberSaveable(stateKey) { mutableStateOf(false) }
    val action = stringResource(if (expanded) R.string.chat_show_less else R.string.chat_show_more)
    val state = stringResource(if (expanded) R.string.chat_expanded else R.string.chat_collapsed)
    Column(modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 48.dp)
                .semantics { stateDescription = state }
                .clickable(role = Role.Button, onClickLabel = action) { expanded = !expanded }
                .padding(vertical = 6.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                (if (expanded) "▾ " else "▸ ") + label,
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.SemiBold,
                fontSize = 12.sp,
                color = palette.foreground.toOpaqueColor(),
            )
            if (error) {
                Text(
                    stringResource(R.string.chat_tool_result_error),
                    fontFamily = FontFamily.Monospace,
                    color = palette.danger.toOpaqueColor(),
                    fontSize = 11.sp,
                )
            }
            Text(
                preview,
                fontSize = 12.sp,
                fontFamily = FontFamily.Monospace,
                color = palette.dim.toOpaqueColor(),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
        }
        if (expanded) {
            Box(Modifier.padding(start = 12.dp, bottom = 6.dp)) { content() }
        }
    }
}

@Composable
private fun Fallback(text: String, onOpenTerminal: () -> Unit, modifier: Modifier) {
    Column(
        modifier
            .fillMaxSize()
            .padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Text(text, style = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace))
        OutlinedButton(onClick = onOpenTerminal) {
            Text(
                stringResource(R.string.chat_open_terminal_instead),
                fontFamily = FontFamily.Monospace,
            )
        }
    }
}

@Composable
private fun Centered(modifier: Modifier, content: @Composable () -> Unit) {
    Column(
        modifier
            .fillMaxSize()
            .padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) { content() }
}

/** `unsupported-agent` 带上是哪一家:「这个 AI 读不了」比「读不了」有用得多。 */
@Composable
private fun noticeText(notice: ChatNotice): String = when (notice) {
    ChatNotice.NoAgent -> stringResource(R.string.chat_fallback_no_agent)
    is ChatNotice.Server -> when (notice.reason) {
        ChatFallback.Disabled -> stringResource(R.string.chat_fallback_disabled)
        ChatFallback.NoTranscriptPath -> stringResource(R.string.chat_fallback_no_path)
        ChatFallback.UnsupportedAgent -> stringResource(R.string.chat_fallback_unsupported, notice.agentId)
        ChatFallback.Unreadable -> stringResource(R.string.chat_fallback_unreadable)
        ChatFallback.ParseFailed -> stringResource(R.string.chat_fallback_parse_failed)
    }
}
