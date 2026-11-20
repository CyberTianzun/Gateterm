package ai.eclosion.gateterm.android.ui.term

import android.content.ClipboardManager
import android.content.Context
import android.os.SystemClock
import android.view.inputmethod.InputMethodManager
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.PointerType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChange
import androidx.compose.ui.input.pointer.util.VelocityTracker
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalViewConfiguration
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
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
import ai.eclosion.gateterm.android.term.SelectionHandle
import ai.eclosion.gateterm.android.term.SingleFingerDragAction
import ai.eclosion.gateterm.android.term.TapAction
import ai.eclosion.gateterm.android.term.TermFrame
import ai.eclosion.gateterm.android.term.TermGeometry
import ai.eclosion.gateterm.android.term.TermSelection
import ai.eclosion.gateterm.android.term.TerminalKeyboard
import ai.eclosion.gateterm.android.term.TerminalSelectionHandles
import ai.eclosion.gateterm.android.term.TerminalTouchMode
import ai.eclosion.gateterm.android.term.TerminalTouchPolicy
import ai.eclosion.gateterm.android.term.TerminalView
import ai.eclosion.gateterm.android.term.TwoFingerMode
import ai.eclosion.gateterm.android.ui.components.AgentBrandIcon
import ai.eclosion.gateterm.android.ui.components.AppBarTitle
import ai.eclosion.gateterm.android.ui.connections.ChatUiState
import kotlin.math.abs
import kotlin.math.hypot
import kotlin.math.pow
import kotlin.math.roundToInt
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

internal const val TOP_BAR_AUTO_HIDE_MS = 3000L
private val TOP_BAR_REVEAL_ZONE = 48.dp
private const val LONG_PRESS_MS = 380L

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
    touchMode: TerminalTouchMode,
    selection: TermSelection?,
    scrollOffset: Int,
    mouseTracking: Boolean,
    onBack: () -> Unit,
    onInput: (ByteArray) -> Unit,
    onProposeSize: (Int, Int) -> Unit,
    onScrollAt: (col: Int, row: Int, lines: Int) -> Unit,
    onSelection: (TermSelection?) -> Unit,
    onMouse: (col: Int, row: Int, pressed: Boolean, moving: Boolean) -> Unit,
    onCopy: () -> Unit,
    onPaste: () -> Unit,
    onJumpToBottom: () -> Unit,
    onFontSize: (Float) -> Unit,
    onTouchMode: (TerminalTouchMode) -> Unit,
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
    var topMenuExpanded by remember(activeId) { mutableStateOf(false) }
    var chatUserOnly by remember(activeId, chatOpen) { mutableStateOf(false) }
    var actionMenuOpen by remember(activeId) { mutableStateOf(false) }
    var menuAnchorCell by remember(activeId) { mutableStateOf<Pair<Int, Int>?>(null) }
    var menuSize by remember { mutableStateOf(IntSize.Zero) }
    var inertiaJob by remember(activeId) { mutableStateOf<Job?>(null) }
    val coroutineScope = rememberCoroutineScope()

    fun stopInertia() {
        inertiaJob?.cancel()
        inertiaJob = null
    }

    fun revealTopBar() {
        topBarVisible = true
        topBarRevealTick++
    }

    val keyboard = remember(activeId, serverOs, win32InputMode) { TerminalKeyboard(serverOs, win32InputMode) }
    var termView by remember { mutableStateOf<TerminalView?>(null) }
    var viewport by remember { mutableStateOf(IntSize.Zero) }
    val context = LocalContext.current
    val haptic = LocalHapticFeedback.current
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    val terminalHint = stringResource(R.string.term_keys_hint)
    val density = LocalDensity.current
    val touchSlop = LocalViewConfiguration.current.touchSlop
    val topRevealZonePx = with(density) { TOP_BAR_REVEAL_ZONE.toPx() }
    val handleDropPx = with(density) { SELECTION_HANDLE_DROP_DP.dp.toPx() }
    val handleRadiusPx = with(density) { SELECTION_HANDLE_RADIUS_DP.dp.toPx() }
    val handleHitRadiusPx = with(density) { 28.dp.toPx() }
    val menuGapPx = with(density) { 8.dp.toPx() }
    val loadedFont = remember(context, appearance.font) {
        TermFontLoader.load(context, appearance.font)
    }
    val paint = remember(appearance.fontSize, density.density, loadedFont, appearance.lineHeight, appearance.letterSpacing) {
        measureTerm(appearance.fontSize, density.density, loadedFont.typeface, loadedFont.advanceEm, appearance.lineHeight, appearance.letterSpacing)
    }

    // Keep latest state accessible inside long-lived pointerInput coroutine without restarting mid-gesture.
    val currentSelection by rememberUpdatedState(selection)
    val currentMouseTracking by rememberUpdatedState(mouseTracking)
    val currentTouchMode by rememberUpdatedState(touchMode)
    val currentFontSize by rememberUpdatedState(appearance.fontSize)
    val currentTopBarVisible by rememberUpdatedState(topBarVisible)
    val currentTopBarHeightPx by rememberUpdatedState(topBarHeightPx)
    val currentPaint by rememberUpdatedState(paint)
    val currentFrameCols by rememberUpdatedState(frame.cols)
    val currentFrameRows by rememberUpdatedState(frame.rows)
    val currentViewport by rememberUpdatedState(viewport)
    val currentTerm by rememberUpdatedState(term)
    val currentOnScrollAt by rememberUpdatedState(onScrollAt)
    val currentOnSelection by rememberUpdatedState(onSelection)
    val currentOnMouse by rememberUpdatedState(onMouse)
    val currentOnFontSize by rememberUpdatedState(onFontSize)

    fun startInertia(velocityY: Float, col: Int, row: Int) {
        stopInertia()
        val minStartVelocity = 200f * density.density
        val minStopVelocity = 40f * density.density
        if (abs(velocityY) <= minStartVelocity) return
        inertiaJob = coroutineScope.launch {
            var vel = velocityY
            var accum = 0f
            var lastNanos = 0L
            while (isActive && abs(vel) >= minStopVelocity) {
                val frameNanos = withFrameNanos { it }
                if (lastNanos == 0L) {
                    lastNanos = frameNanos
                    continue
                }
                val dt = ((frameNanos - lastNanos) / 1_000_000_000f).coerceIn(0f, 0.05f)
                lastNanos = frameNanos
                val (next, lines) = TermGeometry.scrollDelta(accum, vel * dt, currentPaint.cellH)
                accum = next
                if (lines != 0) currentOnScrollAt(col, row, lines)
                vel *= 0.94f.pow(dt * 60f)
            }
        }
    }

    fun clearModifiers() {
        ctrl = false; alt = false; shift = false
        termView?.softModifiers = KeyModifiers()
    }
    fun softModifiers() = KeyModifiers(shift = shift, ctrl = ctrl, alt = alt)
    fun releaseKeyboard() { termView?.releaseKeys(); clearModifiers() }
    fun dismissSelectionMenu() {
        actionMenuOpen = false
        menuAnchorCell = null
    }
    fun toggleChat() {
        if (!chatOpen) {
            // 进聊天前把键盘收掉:按住的修饰键要释放,软键盘也不该盖在聊天上
            stopInertia()
            dismissSelectionMenu()
            releaseKeyboard()
            termView?.windowToken?.let {
                context.getSystemService(InputMethodManager::class.java)?.hideSoftInputFromWindow(it, 0)
            }
        }
        onToggleChat()
    }
    LaunchedEffect(topBarVisible, topBarRevealTick, chatOpen, topMenuExpanded) {
        if (topBarVisible && !chatOpen && !topMenuExpanded) {
            delay(TOP_BAR_AUTO_HIDE_MS)
            topBarVisible = false
        }
    }
    LaunchedEffect(selection) {
        if (selection == null && menuAnchorCell == null) {
            actionMenuOpen = false
        }
    }
    DisposableEffect(keyboard, lifecycle) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_STOP) {
                stopInertia()
                onInput(keyboard.releaseAll())
                clearModifiers()
            }
        }
        lifecycle.addObserver(observer)
        onDispose {
            stopInertia()
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
    // 聊天视图里按返回 = 先退出用户消息筛选，再回终端；终端里先清选区再退出会话
    BackHandler {
        when {
            chatOpen && chatUserOnly -> chatUserOnly = false
            chatOpen -> onToggleChat()
            selection != null || actionMenuOpen -> {
                dismissSelectionMenu()
                onSelection(null)
            }
            else -> {
                stopInertia()
                releaseKeyboard()
                onBack()
            }
        }
    }
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
                        dismissSelectionMenu()
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
                onCopy = {
                    dismissSelectionMenu()
                    onCopy()
                },
                onPaste = {
                    dismissSelectionMenu()
                    onPaste()
                },
                onJumpToBottom = {
                    stopInertia()
                    onJumpToBottom()
                },
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
                        userOnly = chatUserOnly,
                        onToggleUserOnly = { chatUserOnly = !chatUserOnly },
                        onBack = { releaseKeyboard(); onBack() },
                        onToggleChat = ::toggleChat,
                    )
                    ChatView(
                        chat = chat,
                        onOpenTerminal = onToggleChat,
                        onLoadOlder = onLoadOlderChat,
                        userOnly = chatUserOnly,
                        onUserOnlyChange = { chatUserOnly = it },
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
                                dismissSelectionMenu()
                                onInput(bytes)
                            }
                        },
                        modifier = Modifier.fillMaxSize(),
                    )
                    Box(
                        Modifier
                            .fillMaxSize()
                            .pointerInput(activeId) {
                                fun gridOrigin(): Pair<Float, Float> = TermGeometry.origin(
                                    currentViewport.width.toFloat(),
                                    currentViewport.height.toFloat(),
                                    currentFrameCols,
                                    currentFrameRows,
                                    currentPaint.cellW,
                                    currentPaint.cellH,
                                )

                                fun cellOf(x: Float, y: Float): Pair<Int, Int>? {
                                    val (ox, oy) = gridOrigin()
                                    return TermGeometry.cellAt(
                                        x, y, ox, oy, currentPaint.cellW, currentPaint.cellH,
                                        currentFrameCols, currentFrameRows,
                                    )
                                }

                                fun cellOfClamped(x: Float, y: Float): Pair<Int, Int>? {
                                    val (ox, oy) = gridOrigin()
                                    return TermGeometry.cellAtClamped(
                                        x, y, ox, oy, currentPaint.cellW, currentPaint.cellH,
                                        currentFrameCols, currentFrameRows,
                                    )
                                }

                                awaitEachGesture {
                                    val down = awaitFirstDown(requireUnconsumed = false)
                                    stopInertia()
                                    val pointerId = down.id
                                    val start = down.position
                                    if (currentTopBarVisible && start.y < currentTopBarHeightPx) {
                                        return@awaitEachGesture
                                    }

                                    // 1. Selection handle dragging has highest priority when selection is active.
                                    val existingSel = currentSelection
                                    if (existingSel != null) {
                                        val (ox, oy) = gridOrigin()
                                        var activeHandle: SelectionHandle? = TerminalSelectionHandles.hitTestHandle(
                                            start.x,
                                            start.y,
                                            existingSel,
                                            ox,
                                            oy,
                                            currentPaint.cellW,
                                            currentPaint.cellH,
                                            handleDropPx,
                                            handleHitRadiusPx,
                                        )
                                        if (activeHandle != null) {
                                            actionMenuOpen = false
                                            var curSel = existingSel.normalized()
                                            down.consume()
                                            while (true) {
                                                val ev = awaitPointerEvent()
                                                val ch = ev.changes.firstOrNull { it.id == pointerId }
                                                if (ch == null || !ch.pressed) {
                                                    menuAnchorCell = null
                                                    actionMenuOpen = true
                                                    break
                                                }
                                                // Offset slightly upward so thumb does not cover the target row.
                                                val target = cellOfClamped(ch.position.x, ch.position.y - currentPaint.cellH * 0.8f)
                                                if (target != null) {
                                                    val (nextSel, nextHandle) = TerminalSelectionHandles.updateHandle(
                                                        curSel,
                                                        activeHandle!!,
                                                        target.first,
                                                        target.second,
                                                    )
                                                    curSel = nextSel
                                                    activeHandle = nextHandle
                                                    currentOnSelection(nextSel)
                                                }
                                                ch.consume()
                                            }
                                            return@awaitEachGesture
                                        }
                                    }

                                    val hadSelectionOnDown = existingSel != null || actionMenuOpen
                                    dismissSelectionMenu()
                                    val inTopRevealZone = !currentTopBarVisible && start.y <= topRevealZonePx
                                    if (currentTopBarVisible) {
                                        topBarVisible = false
                                    }

                                    val isHardwareMouse = down.type == PointerType.Mouse
                                    val anchor = cellOf(start.x, start.y)
                                    val clampedAnchor = anchor ?: cellOfClamped(start.x, start.y)
                                    var pastSlop = false
                                    var selecting = false
                                    var blankLongPress = false
                                    var draggedSelection = false
                                    var wordAnchor: TermSelection? = null
                                    var scrolling = false
                                    var mouseDragActive = false
                                    var twoFinger = false
                                    var twoFingerMode: TwoFingerMode? = null
                                    var initialPinchSpan = 1f
                                    var pinchBaseFont = currentFontSize
                                    var lastTwoFingerY = 0f
                                    var twoFingerPanTotal = 0f
                                    var twoFingerScrollCell: Pair<Int, Int>? = clampedAnchor
                                    var accum = 0f
                                    val velocityTracker = VelocityTracker()
                                    val longPressAt = SystemClock.uptimeMillis() + LONG_PRESS_MS

                                    while (true) {
                                        val remainingMs = longPressAt - SystemClock.uptimeMillis()
                                        val shouldWaitLongPress =
                                            !pastSlop && !selecting && !twoFinger && !inTopRevealZone && clampedAnchor != null
                                        val event = if (shouldWaitLongPress && remainingMs > 0L) {
                                            withTimeoutOrNull(remainingMs) { awaitPointerEvent() }
                                        } else if (shouldWaitLongPress) {
                                            null
                                        } else {
                                            awaitPointerEvent()
                                        }

                                        // Stationary long-press fired!
                                        if (event == null) {
                                            val target = clampedAnchor ?: continue
                                            selecting = true
                                            haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                                            val bounds = currentTerm.wordBoundsAt(target.first, target.second)
                                            if (bounds != null) {
                                                val w = TermSelection(bounds.first, target.second, bounds.second, target.second)
                                                wordAnchor = w
                                                blankLongPress = false
                                                currentOnSelection(w)
                                            } else {
                                                wordAnchor = null
                                                blankLongPress = true
                                                currentOnSelection(null)
                                            }
                                            continue
                                        }

                                        val pressed = event.changes.filter { it.pressed }
                                        if (pressed.size >= 2) {
                                            val p0 = pressed[0].position
                                            val p1 = pressed[1].position
                                            val span = hypot(p1.x - p0.x, p1.y - p0.y).coerceAtLeast(1f)
                                            val centroidX = (p0.x + p1.x) * 0.5f
                                            val centroidY = (p0.y + p1.y) * 0.5f
                                            if (!twoFinger) {
                                                twoFinger = true
                                                if (mouseDragActive && clampedAnchor != null) {
                                                    currentOnMouse(clampedAnchor.first, clampedAnchor.second, false, false)
                                                    mouseDragActive = false
                                                }
                                                if (selecting) {
                                                    selecting = false
                                                    currentOnSelection(null)
                                                }
                                                initialPinchSpan = span
                                                pinchBaseFont = currentFontSize
                                                lastTwoFingerY = centroidY
                                                twoFingerPanTotal = 0f
                                                twoFingerMode = null
                                                accum = 0f
                                                velocityTracker.resetTracking()
                                                twoFingerScrollCell = cellOfClamped(centroidX, centroidY) ?: clampedAnchor
                                            } else {
                                                val dy = centroidY - lastTwoFingerY
                                                lastTwoFingerY = centroidY
                                                twoFingerPanTotal += dy
                                                val scale = span / initialPinchSpan
                                                if (twoFingerMode == null) {
                                                    twoFingerMode = TerminalTouchPolicy.decideTwoFingerGesture(
                                                        scaleDelta = scale,
                                                        panDistancePx = abs(twoFingerPanTotal),
                                                        touchSlopPx = touchSlop,
                                                    )
                                                }
                                                when (twoFingerMode) {
                                                    TwoFingerMode.Pinch -> {
                                                        val nextSize = TerminalTouchPolicy.clampFontSize(pinchBaseFont, scale)
                                                        if (nextSize != currentFontSize) currentOnFontSize(nextSize)
                                                    }
                                                    TwoFingerMode.Scroll -> {
                                                        velocityTracker.addPosition(
                                                            pressed[0].uptimeMillis,
                                                            Offset(centroidX, centroidY),
                                                        )
                                                        val (next, lines) = TermGeometry.scrollDelta(
                                                            accum, dy, currentPaint.cellH,
                                                        )
                                                        accum = next
                                                        val sc = twoFingerScrollCell ?: clampedAnchor
                                                        if (lines != 0 && sc != null) {
                                                            currentOnScrollAt(sc.first, sc.second, lines)
                                                        }
                                                    }
                                                    null -> Unit
                                                }
                                            }
                                            event.changes.forEach { it.consume() }
                                            continue
                                        }

                                        // After a two-finger gesture, wait until all fingers lift without triggering 1-finger actions.
                                        if (twoFinger) {
                                            event.changes.forEach { it.consume() }
                                            if (pressed.isEmpty()) {
                                                if (twoFingerMode == TwoFingerMode.Scroll) {
                                                    val sc = twoFingerScrollCell ?: clampedAnchor
                                                    if (sc != null) {
                                                        startInertia(velocityTracker.calculateVelocity().y, sc.first, sc.second)
                                                    }
                                                }
                                                break
                                            }
                                            continue
                                        }

                                        val change = event.changes.firstOrNull { it.id == pointerId }
                                        if (change == null || !change.pressed) {
                                            val end = change?.position ?: start
                                            val endCell = cellOf(end.x, end.y) ?: anchor
                                            when {
                                                mouseDragActive -> {
                                                    val upCell = cellOfClamped(end.x, end.y) ?: clampedAnchor
                                                    if (upCell != null) currentOnMouse(upCell.first, upCell.second, false, false)
                                                }
                                                selecting -> {
                                                    if (blankLongPress && !draggedSelection) {
                                                        menuAnchorCell = clampedAnchor
                                                        actionMenuOpen = true
                                                    } else {
                                                        menuAnchorCell = null
                                                        actionMenuOpen = true
                                                    }
                                                }
                                                scrolling -> {
                                                    val sc = clampedAnchor
                                                    if (sc != null) {
                                                        startInertia(velocityTracker.calculateVelocity().y, sc.first, sc.second)
                                                    }
                                                }
                                                !pastSlop -> {
                                                    when (TerminalTouchPolicy.tapAction(
                                                        inTopRevealZone = inTopRevealZone,
                                                        hasSelection = hadSelectionOnDown,
                                                        mouseTracking = currentMouseTracking,
                                                    )) {
                                                        TapAction.RevealTopBar -> revealTopBar()
                                                        TapAction.DismissSelection -> {
                                                            dismissSelectionMenu()
                                                            currentOnSelection(null)
                                                        }
                                                        TapAction.MouseClick -> {
                                                            if (endCell != null) {
                                                                currentOnMouse(endCell.first, endCell.second, true, false)
                                                                currentOnMouse(endCell.first, endCell.second, false, false)
                                                            }
                                                        }
                                                        TapAction.FocusOnly -> {
                                                            termView?.showKeyboard()
                                                        }
                                                    }
                                                }
                                            }
                                            break
                                        }

                                        val dist = (change.position - start).getDistance()
                                        if (!pastSlop && dist > touchSlop) pastSlop = true
                                        val clampedCell = cellOfClamped(change.position.x, change.position.y)

                                        when {
                                            selecting && clampedCell != null -> {
                                                val w = wordAnchor
                                                if (w != null) {
                                                    currentOnSelection(
                                                        TerminalSelectionHandles.extendWordSelection(
                                                            w, clampedCell.first, clampedCell.second,
                                                        ),
                                                    )
                                                } else if (pastSlop && clampedAnchor != null) {
                                                    draggedSelection = true
                                                    currentOnSelection(
                                                        TermSelection(
                                                            clampedAnchor.first,
                                                            clampedAnchor.second,
                                                            clampedCell.first,
                                                            clampedCell.second,
                                                        ),
                                                    )
                                                }
                                            }
                                            pastSlop -> {
                                                val action = TerminalTouchPolicy.singleFingerDragAction(
                                                    mode = currentTouchMode,
                                                    mouseTracking = currentMouseTracking,
                                                    isHardwareMouse = isHardwareMouse,
                                                )
                                                if (action == SingleFingerDragAction.MouseDrag && clampedAnchor != null) {
                                                    if (!mouseDragActive) {
                                                        currentOnMouse(clampedAnchor.first, clampedAnchor.second, true, false)
                                                        mouseDragActive = true
                                                    }
                                                    if (clampedCell != null) {
                                                        currentOnMouse(clampedCell.first, clampedCell.second, true, true)
                                                    }
                                                } else {
                                                    if (!scrolling) {
                                                        scrolling = true
                                                        if (currentSelection != null) currentOnSelection(null)
                                                    }
                                                    velocityTracker.addPosition(change.uptimeMillis, change.position)
                                                    val (next, lines) = TermGeometry.scrollDelta(
                                                        accum,
                                                        change.positionChange().y,
                                                        currentPaint.cellH,
                                                    )
                                                    accum = next
                                                    val sc = clampedAnchor
                                                    if (lines != 0 && sc != null) {
                                                        currentOnScrollAt(sc.first, sc.second, lines)
                                                    }
                                                }
                                            }
                                        }
                                        change.consume()
                                    }
                                }
                            },
                    )
                    if (actionMenuOpen && viewport.width > 0 && viewport.height > 0) {
                        val effectiveAnchorSel = selection
                            ?: menuAnchorCell?.let { (c, r) -> TermSelection(c, r, c, r) }
                        if (effectiveAnchorSel != null) {
                            val norm = effectiveAnchorSel.normalized()
                            val lineRow = norm.y0
                            val lineBounds = term.lineBoundsAt(lineRow)
                            val canSelectLine = lineBounds != null && (
                                selection == null ||
                                    norm.y0 != norm.y1 ||
                                    norm.x0 != lineBounds.first ||
                                    norm.x1 != lineBounds.second
                                )
                            val linkCell = menuAnchorCell ?: (norm.x0 to norm.y0)
                            val hasLink = term.hyperlinkAt(linkCell.first, linkCell.second) != null
                            val canPaste = !reconnecting && clipboardHasText(context)
                            val canCopy = selection != null
                            if (canCopy || canPaste || canSelectLine || hasLink) {
                                val (ox, oy) = TermGeometry.origin(
                                    viewport.width.toFloat(),
                                    viewport.height.toFloat(),
                                    frame.cols,
                                    frame.rows,
                                    paint.cellW,
                                    paint.cellH,
                                )
                                val (mx, my) = TerminalSelectionHandles.menuAnchor(
                                    sel = norm,
                                    ox = ox,
                                    oy = oy,
                                    cellW = paint.cellW,
                                    cellH = paint.cellH,
                                    viewportW = viewport.width.toFloat(),
                                    viewportH = viewport.height.toFloat(),
                                    menuW = menuSize.width.toFloat().coerceAtLeast(1f),
                                    menuH = menuSize.height.toFloat().coerceAtLeast(1f),
                                    gapPx = menuGapPx,
                                    handleClearancePx = if (selection != null) handleDropPx + handleRadiusPx else 0f,
                                )
                                SelectionActionMenu(
                                    canCopy = canCopy,
                                    canPaste = canPaste,
                                    canSelectLine = canSelectLine,
                                    hasLink = hasLink,
                                    onCopy = {
                                        dismissSelectionMenu()
                                        onCopy()
                                    },
                                    onPaste = {
                                        dismissSelectionMenu()
                                        onSelection(null)
                                        onPaste()
                                    },
                                    onSelectLine = {
                                        val b = lineBounds ?: return@SelectionActionMenu
                                        menuAnchorCell = null
                                        onSelection(TermSelection(b.first, lineRow, b.second, lineRow))
                                    },
                                    onOpenLink = {
                                        TerminalView.openHyperlinkAt(context, term, linkCell.first, linkCell.second)
                                        dismissSelectionMenu()
                                        onSelection(null)
                                    },
                                    modifier = Modifier
                                        .offset { IntOffset(mx.roundToInt(), my.roundToInt()) }
                                        .onSizeChanged { menuSize = it },
                                )
                            }
                        }
                    }
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
                            touchMode = touchMode,
                            selectionAvailable = selection != null,
                            inputEnabled = !reconnecting,
                            menuExpanded = topMenuExpanded,
                            onMenuExpanded = { expanded ->
                                topMenuExpanded = expanded
                                if (expanded) revealTopBar()
                            },
                            onTouchMode = { mode ->
                                topMenuExpanded = false
                                onTouchMode(mode)
                            },
                            onCopy = {
                                topMenuExpanded = false
                                dismissSelectionMenu()
                                onCopy()
                            },
                            onPaste = {
                                topMenuExpanded = false
                                dismissSelectionMenu()
                                onPaste()
                            },
                            onBack = { stopInertia(); releaseKeyboard(); onBack() },
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

@Composable
private fun SelectionActionMenu(
    canCopy: Boolean,
    canPaste: Boolean,
    canSelectLine: Boolean,
    hasLink: Boolean,
    onCopy: () -> Unit,
    onPaste: () -> Unit,
    onSelectLine: () -> Unit,
    onOpenLink: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier = modifier,
        shape = RoundedCornerShape(12.dp),
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
        tonalElevation = 4.dp,
        shadowElevation = 6.dp,
        border = BorderStroke(0.5.dp, MaterialTheme.colorScheme.outlineVariant),
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 4.dp, vertical = 2.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            val btnPadding = PaddingValues(horizontal = 10.dp, vertical = 4.dp)
            if (canCopy) {
                TextButton(onClick = onCopy, contentPadding = btnPadding) {
                    Text(stringResource(R.string.term_copy))
                }
            }
            if (canPaste) {
                TextButton(onClick = onPaste, contentPadding = btnPadding) {
                    Text(stringResource(R.string.term_paste))
                }
            }
            if (canSelectLine) {
                TextButton(onClick = onSelectLine, contentPadding = btnPadding) {
                    Text(stringResource(R.string.term_select_line))
                }
            }
            if (hasLink) {
                TextButton(onClick = onOpenLink, contentPadding = btnPadding) {
                    Text(stringResource(R.string.term_open_link))
                }
            }
        }
    }
}

private fun clipboardHasText(context: Context): Boolean = runCatching {
    val cm = context.getSystemService(ClipboardManager::class.java) ?: return false
    cm.hasPrimaryClip() && (cm.primaryClip?.itemCount ?: 0) > 0 &&
        !cm.primaryClip?.getItemAt(0)?.coerceToText(context).isNullOrEmpty()
}.getOrDefault(false)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TerminalTopBar(
    title: String,
    agentId: String?,
    chatOpen: Boolean,
    chatAvailable: Boolean,
    onBack: () -> Unit,
    onToggleChat: () -> Unit,
    userOnly: Boolean = false,
    onToggleUserOnly: () -> Unit = {},
    touchMode: TerminalTouchMode = TerminalTouchMode.Default,
    selectionAvailable: Boolean = false,
    inputEnabled: Boolean = true,
    menuExpanded: Boolean = false,
    onMenuExpanded: (Boolean) -> Unit = {},
    onTouchMode: (TerminalTouchMode) -> Unit = {},
    onCopy: () -> Unit = {},
    onPaste: () -> Unit = {},
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
            if (chatOpen) {
                IconButton(onClick = onToggleUserOnly) {
                    Icon(
                        painterResource(R.drawable.ic_filter),
                        contentDescription = stringResource(R.string.chat_filter_user),
                        tint = if (userOnly) MaterialTheme.colorScheme.primary else LocalContentColor.current,
                    )
                }
            }
            // 只有本会话里有读得了记录的 agent 才给入口;已经在聊天里时回终端永远可以
            if (chatOpen || chatAvailable) {
                val desc = stringResource(
                    if (chatOpen) R.string.chat_back_to_terminal_desc else R.string.chat_open_desc,
                )
                IconButton(onClick = onToggleChat) {
                    Icon(painterResource(if (chatOpen) R.drawable.ic_terminal else R.drawable.ic_chat), desc)
                }
            }
            if (!chatOpen) {
                Box {
                    IconButton(onClick = { onMenuExpanded(true) }) {
                        Icon(
                            Icons.Default.MoreVert,
                            contentDescription = stringResource(R.string.term_more),
                        )
                    }
                    DropdownMenu(
                        expanded = menuExpanded,
                        onDismissRequest = { onMenuExpanded(false) },
                    ) {
                        TerminalTouchMode.entries.forEach { mode ->
                            DropdownMenuItem(
                                text = {
                                    Text(
                                        stringResource(
                                            when (mode) {
                                                TerminalTouchMode.Touch -> R.string.settings_touch_mode_touch
                                                TerminalTouchMode.Mouse -> R.string.settings_touch_mode_mouse
                                            },
                                        ),
                                    )
                                },
                                trailingIcon = if (touchMode == mode) {
                                    { Icon(Icons.Default.Check, contentDescription = null) }
                                } else null,
                                onClick = { onTouchMode(mode) },
                            )
                        }
                        HorizontalDivider()
                        DropdownMenuItem(
                            text = { Text(stringResource(R.string.term_copy)) },
                            enabled = selectionAvailable,
                            onClick = onCopy,
                        )
                        DropdownMenuItem(
                            text = { Text(stringResource(R.string.term_paste)) },
                            enabled = inputEnabled,
                            onClick = onPaste,
                        )
                    }
                }
            }
        },
    )
}
