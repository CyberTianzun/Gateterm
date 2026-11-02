package ai.eclosion.gateterm.android.ui.term

import android.os.SystemClock
import android.view.inputmethod.InputMethodManager
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.Scaffold
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChange
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalViewConfiguration
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import ai.eclosion.gateterm.android.R
import ai.eclosion.gateterm.android.appearance.TermAppearance
import ai.eclosion.gateterm.android.appearance.TermFontLoader
import ai.eclosion.gateterm.android.term.AgentKeyLayout
import ai.eclosion.gateterm.android.term.AlacrittyTerm
import ai.eclosion.gateterm.android.term.KeyModifiers
import ai.eclosion.gateterm.android.term.TermFrame
import ai.eclosion.gateterm.android.term.TermGeometry
import ai.eclosion.gateterm.android.term.TermSelection
import ai.eclosion.gateterm.android.term.TerminalKeyboard
import ai.eclosion.gateterm.android.term.TerminalView
import ai.eclosion.gateterm.android.ui.components.AgentBrandIcon
import ai.eclosion.gateterm.android.ui.components.AppBarTitle
import ai.eclosion.gateterm.android.ui.connections.ChatUiState
import kotlinx.coroutines.delay

internal const val TOP_BAR_AUTO_HIDE_MS = 3000L
private val TOP_BAR_REVEAL_ZONE = 48.dp

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TerminalScreen(
    title: String,
    activeId: Long,
    reconnecting: Boolean,
    serverOs: String,
    win32InputMode: Boolean,
    term: AlacrittyTerm,
    frame: TermFrame,
    generation: Long,
    appearance: TermAppearance,
    extraKeyRows: Int,
    agentId: String?,
    agentKeyLayouts: Map<String, AgentKeyLayout>,
    selection: TermSelection?,
    scrollOffset: Int,
    mouseTracking: Boolean,
    onBack: () -> Unit,
    onInput: (ByteArray) -> Unit,
    onProposeSize: (Int, Int) -> Unit,
    onScroll: (Int) -> Unit,
    onSelection: (TermSelection?) -> Unit,
    onMouse: (col: Int, row: Int, pressed: Boolean, moving: Boolean) -> Unit,
    onCopy: () -> Unit,
    onPaste: () -> Unit,
    onJumpToBottom: () -> Unit,
    chat: ChatUiState,
    chatOpen: Boolean,
    chatAvailable: Boolean,
    onToggleChat: () -> Unit,
    onLoadOlderChat: () -> Unit,
) {
    var ctrl by remember(activeId) { mutableStateOf(false) }
    var alt by remember(activeId) { mutableStateOf(false) }
    var shift by remember(activeId) { mutableStateOf(false) }
    var topBarVisible by remember(activeId) { mutableStateOf(true) }
    var topBarRevealTick by remember(activeId) { mutableIntStateOf(0) }
    var topBarHeightPx by remember { mutableIntStateOf(0) }
    fun revealTopBar() {
        topBarVisible = true
        topBarRevealTick++
    }
    val keyboard = remember(activeId, serverOs, win32InputMode) { TerminalKeyboard(serverOs, win32InputMode) }
    var termView by remember { mutableStateOf<TerminalView?>(null) }
    var viewport by remember { mutableStateOf(IntSize.Zero) }
    val context = LocalContext.current
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    val terminalHint = stringResource(R.string.term_keys_hint)
    val density = LocalDensity.current
    val touchSlop = LocalViewConfiguration.current.touchSlop
    val topRevealZonePx = with(density) { TOP_BAR_REVEAL_ZONE.toPx() }
    val loadedFont = remember(context, appearance.font) {
        TermFontLoader.load(context, appearance.font)
    }
    val paint = remember(appearance.fontSize, density.density, loadedFont, appearance.lineHeight, appearance.letterSpacing) {
        measureTerm(appearance.fontSize, density.density, loadedFont.typeface, loadedFont.advanceEm, appearance.lineHeight, appearance.letterSpacing)
    }
    fun clearModifiers() {
        ctrl = false; alt = false; shift = false
        termView?.softModifiers = KeyModifiers()
    }
    fun softModifiers() = KeyModifiers(shift = shift, ctrl = ctrl, alt = alt)
    fun releaseKeyboard() { termView?.releaseKeys(); clearModifiers() }
    fun toggleChat() {
        if (!chatOpen) {
            // 进聊天前把键盘收掉:按住的修饰键要释放,软键盘也不该盖在聊天上
            releaseKeyboard()
            termView?.windowToken?.let {
                context.getSystemService(InputMethodManager::class.java)?.hideSoftInputFromWindow(it, 0)
            }
        }
        onToggleChat()
    }
    LaunchedEffect(topBarVisible, topBarRevealTick, chatOpen) {
        if (topBarVisible && !chatOpen) {
            delay(TOP_BAR_AUTO_HIDE_MS)
            topBarVisible = false
        }
    }
    DisposableEffect(keyboard, lifecycle) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_STOP) {
                onInput(keyboard.releaseAll())
                clearModifiers()
            }
        }
        lifecycle.addObserver(observer)
        onDispose {
            lifecycle.removeObserver(observer)
            // This callback captures the old session, so releases never go to the new session.
            onInput(keyboard.releaseAll())
        }
    }
    LaunchedEffect(keyboard, reconnecting) {
        if (reconnecting) { keyboard.reset(); clearModifiers() }
    }
    LaunchedEffect(agentId) { clearModifiers() }
    LaunchedEffect(generation) {
        TerminalView.drainClipboardToSystem(context, term)
    }
    // 聊天视图里按返回 = 回终端,不是直接退出会话
    BackHandler { if (chatOpen) onToggleChat() else { releaseKeyboard(); onBack() } }
    Scaffold(
        bottomBar = {
            if (!chatOpen) ExtraKeysBar(
                rows = extraKeyRows,
                sessionId = activeId,
                agentId = agentId,
                agentLayouts = agentKeyLayouts,
                ctrl = ctrl,
                alt = alt,
                shift = shift,
                enabled = !reconnecting,
                copyEnabled = selection != null,
                scrollOffset = scrollOffset,
                onCtrl = {
                    ctrl = it
                    termView?.softModifiers = softModifiers()
                },
                onAlt = {
                    alt = it
                    termView?.softModifiers = softModifiers()
                },
                onShift = {
                    shift = it
                    termView?.softModifiers = softModifiers()
                },
                onKey = { stroke ->
                    if (!reconnecting) {
                        topBarVisible = false
                        val modifiers = stroke.modifiers + softModifiers()
                        onInput(keyboard.tap(
                            stroke.copy(modifiers = modifiers),
                            term.applicationCursor,
                            term.modifyOtherKeys,
                            kittyKeyboardFlags = term.kittyKeyboardFlags,
                        ))
                        clearModifiers()
                    }
                },
                onShowKeyboard = { termView?.showKeyboard() },
                onCopy = onCopy,
                onPaste = onPaste,
                onJumpToBottom = onJumpToBottom,
                modifier = Modifier
                    .fillMaxWidth()
                    .navigationBarsPadding(),
            )
        },
    ) { padding ->
        Box(Modifier.fillMaxSize()) {
            Column(
                Modifier
                    .fillMaxSize()
                    .padding(padding),
            ) {
                if (reconnecting) {
                    LinearProgressIndicator(Modifier.fillMaxWidth())
                }
                if (chatOpen) {
                    TerminalTopBar(
                        title = title,
                        agentId = agentId,
                        chatOpen = true,
                        chatAvailable = chatAvailable,
                        onBack = { releaseKeyboard(); onBack() },
                        onToggleChat = ::toggleChat,
                    )
                    ChatView(
                        chat = chat,
                        onOpenTerminal = onToggleChat,
                        onLoadOlder = onLoadOlderChat,
                        modifier = Modifier
                            .fillMaxWidth()
                            .weight(1f)
                            .navigationBarsPadding(),
                    )
                } else Box(
                    Modifier
                        .fillMaxWidth()
                        .weight(1f)
                        .onSizeChanged { viewport = it },
                ) {
                    val committed = remember(generation) { frame }
                    TerminalCanvas(
                        frame = committed,
                        appearance = appearance,
                        selection = selection,
                        paint = paint,
                        onProposeSize = onProposeSize,
                        modifier = Modifier.fillMaxSize(),
                    )
                    AndroidView(
                        factory = { ctx ->
                            TerminalView(ctx).also { view ->
                                view.applicationCursor = term.applicationCursor
                                view.modifyOtherKeys = term.modifyOtherKeys
                                termView = view
                            }
                        },
                        update = { view ->
                            if (view.keyboard !== keyboard) view.releaseKeys()
                            view.keyboard = keyboard
                            view.term = term
                            view.cellWidthPx = paint.cellW
                            view.cellHeightPx = paint.cellH
                            view.inputEnabled = !reconnecting
                            view.applicationCursor = term.applicationCursor
                            view.modifyOtherKeys = term.modifyOtherKeys
                            view.contentDescription = terminalHint
                            view.softModifiers = softModifiers()
                            view.onModifiersConsumed = ::clearModifiers
                            view.onInput = { bytes ->
                                topBarVisible = false
                                onInput(bytes)
                            }
                        },
                        modifier = Modifier.fillMaxSize(),
                    )
                    Box(
                        Modifier
                            .fillMaxSize()
                            .pointerInput(mouseTracking, paint, frame.cols, frame.rows, viewport) {
                                val cols = frame.cols
                                val rows = frame.rows
                                val (ox, oy) = TermGeometry.origin(
                                    viewport.width.toFloat(),
                                    viewport.height.toFloat(),
                                    cols,
                                    rows,
                                    paint.cellW,
                                    paint.cellH,
                                )
                                fun cellOf(x: Float, y: Float) = TermGeometry.cellAt(
                                    x, y, ox, oy, paint.cellW, paint.cellH, cols, rows,
                                )
                                awaitEachGesture {
                                    val down = awaitFirstDown(requireUnconsumed = false)
                                    val pointerId = down.id
                                    val start = down.position
                                    if (topBarVisible && start.y < topBarHeightPx) {
                                        return@awaitEachGesture
                                    }
                                    val inTopRevealZone = !topBarVisible && start.y <= topRevealZonePx
                                    if (topBarVisible) {
                                        topBarVisible = false
                                    }
                                    var pastSlop = false
                                    var selecting = false
                                    var twoFinger = false
                                    var accum = 0f
                                    var mouseDown = false
                                    val anchor = cellOf(start.x, start.y)
                                    val longPressAt = SystemClock.uptimeMillis() + 380L
                                    if (mouseTracking && anchor != null && !inTopRevealZone) {
                                        mouseDown = true
                                        onMouse(anchor.first, anchor.second, true, false)
                                    }
                                    while (true) {
                                        val event = awaitPointerEvent()
                                        val pressed = event.changes.filter { it.pressed }
                                        if (pressed.size >= 2) {
                                            twoFinger = true
                                            if (mouseDown && anchor != null) {
                                                onMouse(anchor.first, anchor.second, false, false)
                                                mouseDown = false
                                            }
                                            val dy = pressed[0].positionChange().y
                                            val (next, lines) = TermGeometry.scrollDelta(accum, dy, paint.cellH)
                                            accum = next
                                            if (lines != 0) onScroll(lines)
                                            event.changes.forEach { it.consume() }
                                            continue
                                        }
                                        val change = event.changes.firstOrNull { it.id == pointerId }
                                        if (change == null || !change.pressed) {
                                            val end = change?.position ?: start
                                            val cell = cellOf(end.x, end.y) ?: anchor
                                            when {
                                                mouseDown && cell != null -> onMouse(cell.first, cell.second, false, false)
                                                selecting && anchor != null && cell != null ->
                                                    onSelection(TermSelection(anchor.first, anchor.second, cell.first, cell.second))
                                                !pastSlop && !twoFinger -> {
                                                    if (inTopRevealZone) {
                                                        revealTopBar()
                                                    } else {
                                                        onSelection(null)
                                                        termView?.showKeyboard()
                                                    }
                                                }
                                            }
                                            break
                                        }
                                        val dist = (change.position - start).getDistance()
                                        if (!pastSlop && dist > touchSlop) pastSlop = true
                                        val cell = cellOf(change.position.x, change.position.y)
                                        if (!selecting && !pastSlop &&
                                            SystemClock.uptimeMillis() >= longPressAt && anchor != null
                                        ) {
                                            selecting = true
                                            if (mouseDown) {
                                                onMouse(anchor.first, anchor.second, false, false)
                                                mouseDown = false
                                            }
                                            onSelection(TermSelection(anchor.first, anchor.second, anchor.first, anchor.second))
                                        }
                                        when {
                                            selecting && anchor != null && cell != null ->
                                                onSelection(TermSelection(anchor.first, anchor.second, cell.first, cell.second))
                                            mouseTracking && mouseDown && cell != null && pastSlop ->
                                                onMouse(cell.first, cell.second, true, true)
                                            !mouseTracking && !selecting && pastSlop -> {
                                                val (next, lines) = TermGeometry.scrollDelta(
                                                    accum,
                                                    change.positionChange().y,
                                                    paint.cellH,
                                                )
                                                accum = next
                                                if (lines != 0) onScroll(lines)
                                            }
                                        }
                                        change.consume()
                                    }
                                }
                            },
                    )
                    androidx.compose.animation.AnimatedVisibility(
                        visible = topBarVisible,
                        enter = slideInVertically { -it } + fadeIn(),
                        exit = slideOutVertically { -it } + fadeOut(),
                        modifier = Modifier
                            .align(Alignment.TopCenter)
                            .onSizeChanged { topBarHeightPx = it.height },
                    ) {
                        TerminalTopBar(
                            title = title,
                            agentId = agentId,
                            chatOpen = false,
                            chatAvailable = chatAvailable,
                            onBack = { releaseKeyboard(); onBack() },
                            onToggleChat = ::toggleChat,
                        )
                    }
                }
                DisposableEffect(Unit) {
                    val imm = context.getSystemService(InputMethodManager::class.java)
                    onDispose {
                        termView?.windowToken?.let { imm?.hideSoftInputFromWindow(it, 0) }
                    }
                }
            }
            if (!chatOpen && !topBarVisible) {
                Box(
                    Modifier
                        .fillMaxWidth()
                        .height(padding.calculateTopPadding())
                        .align(Alignment.TopCenter)
                        .pointerInput(Unit) {
                            awaitEachGesture {
                                val down = awaitFirstDown(requireUnconsumed = false)
                                while (true) {
                                    val event = awaitPointerEvent()
                                    val change = event.changes.firstOrNull { it.id == down.id }
                                    if (change == null || !change.pressed) {
                                        revealTopBar()
                                        break
                                    }
                                }
                            }
                        },
                )
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TerminalTopBar(
    title: String,
    agentId: String?,
    chatOpen: Boolean,
    chatAvailable: Boolean,
    onBack: () -> Unit,
    onToggleChat: () -> Unit,
) {
    TopAppBar(
        windowInsets = WindowInsets(0, 0, 0, 0),
        title = {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                if (!agentId.isNullOrBlank()) {
                    AgentBrandIcon(agentId = agentId)
                }
                AppBarTitle(
                    text = title,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        },
        navigationIcon = {
            IconButton(onClick = onBack) {
                Icon(
                    Icons.AutoMirrored.Filled.ArrowBack,
                    contentDescription = stringResource(R.string.action_back),
                )
            }
        },
        actions = {
            // 只有本会话里有读得了记录的 agent 才给入口;已经在聊天里时回终端永远可以
            if (chatOpen || chatAvailable) {
                val desc = stringResource(
                    if (chatOpen) R.string.chat_back_to_terminal_desc else R.string.chat_open_desc,
                )
                IconButton(onClick = onToggleChat) {
                    Icon(painterResource(if (chatOpen) R.drawable.ic_terminal else R.drawable.ic_chat), desc)
                }
            }
        },
    )
}
