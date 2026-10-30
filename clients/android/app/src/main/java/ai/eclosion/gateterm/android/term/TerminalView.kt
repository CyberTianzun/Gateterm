package ai.eclosion.gateterm.android.term

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Path
import android.graphics.Rect
import android.graphics.RectF
import android.net.Uri
import android.os.Build
import android.text.InputType
import android.util.AttributeSet
import android.view.KeyEvent
import android.view.KeyCharacterMap
import android.view.MotionEvent
import android.view.View
import android.view.WindowInsets
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager
import java.nio.ByteBuffer
import uniffi.gateterm_cloud_ffi.FfiClipboardEvent
import uniffi.gateterm_cloud_ffi.FfiViewportImage

class TerminalView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
) : View(context, attrs) {
    var applicationCursor: Boolean = false
    var modifyOtherKeys: Int = 0
    var term: AlacrittyTerm? = null
    var cellWidthPx: Float = 8f
    var cellHeightPx: Float = 16f

    var onInput: (ByteArray) -> Unit = {}
    var keyboard: TerminalKeyboard = TerminalKeyboard("")
    var softModifiers: KeyModifiers = KeyModifiers()
    var onModifiersConsumed: () -> Unit = {}
    var inputEnabled: Boolean = true
    private var composing = false
    private var deadAccent = 0
    private val textKeys = mutableSetOf<Long>()
    private val layerPaint = Paint(Paint.ANTI_ALIAS_FLAG)

    init {
        isFocusable = true
        isFocusableInTouchMode = true
        isClickable = true
        keepScreenOn = true
        setBackgroundColor(android.graphics.Color.TRANSPARENT)
        setWillNotDraw(true)
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val t = term ?: return
        val cols = t.cols
        val rows = t.rows
        if (cols <= 0 || rows <= 0) return
        val (ox, oy) = TermGeometry.origin(
            width.toFloat(),
            height.toFloat(),
            cols,
            rows,
            cellWidthPx,
            cellHeightPx,
        )
        val images = t.visibleImages()
        drawViewportImages(canvas, images, { it < 0 }, ox, oy, cellWidthPx, cellHeightPx, layerPaint)
        val snap = t.snapshot()
        val density = resources.displayMetrics.density.coerceAtLeast(1f)
        for (y in 0 until rows) {
            val row = snap.getOrNull(y) ?: continue
            val top = oy + y * cellHeightPx
            for (x in 0 until cols) {
                val cell = row.getOrNull(x) ?: continue
                if (cell.width == 0 || !hasFancyUnderline(cell.attrs)) continue
                drawFancyUnderline(
                    canvas,
                    cell.attrs,
                    ox + x * cellWidthPx,
                    top,
                    cellWidthPx * cell.width.coerceAtLeast(1),
                    cellHeightPx,
                    (cell.fg and 0x00FFFFFF) or 0xFF000000.toInt(),
                    density,
                    layerPaint,
                )
            }
        }
        drawViewportImages(canvas, images, { it >= 0 }, ox, oy, cellWidthPx, cellHeightPx, layerPaint)
    }

    fun showKeyboard() {
        isFocusable = true
        isFocusableInTouchMode = true
        requestFocus()
        val imm = context.getSystemService(InputMethodManager::class.java) ?: return
        imm.viewClicked(this)
        imm.restartInput(this)
        post {
            requestFocus()
            imm.showSoftInput(this, InputMethodManager.SHOW_IMPLICIT)
            if (Build.VERSION.SDK_INT >= 30) {
                windowInsetsController?.show(WindowInsets.Type.ime())
            }
        }
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        // 滚动、选区和鼠标上报在 Compose 层处理。这个 View 只负责输入法。
        return false
    }

    override fun performClick(): Boolean {
        super.performClick()
        return true
    }

    override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection {
        // TYPE_NULL 在 Gboard / 三星键盘上经常干脆不弹盘。VISIBLE_PASSWORD
        // 是终端里常用的「能弹盘、少联想」折中（Termux 同款）。
        outAttrs.inputType = InputType.TYPE_CLASS_TEXT or
            InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD or
            InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
        outAttrs.imeOptions = EditorInfo.IME_FLAG_NO_FULLSCREEN or
            EditorInfo.IME_FLAG_NO_EXTRACT_UI or
            EditorInfo.IME_ACTION_NONE
        return object : BaseInputConnection(this, true) {
            override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
                if (inputEnabled && !text.isNullOrEmpty()) {
                    val value = text.toString()
                    val asciiKey = value.length == 1 && value[0].code in 32..126
                    val modifiers = if (composing && !asciiKey) KeyModifiers() else softModifiers
                    emit(keyboard.committedText(value, modifiers, modifyOtherKeys))
                    if ((!composing || asciiKey) && value.length == 1 && value[0].code < 128 &&
                        TerminalKey.fromAscii(value[0]) != null
                    ) consumeModifiers()
                }
                finishComposingText()
                editable?.clear()
                return true
            }

            override fun setComposingText(text: CharSequence?, newCursorPosition: Int): Boolean {
                composing = !text.isNullOrEmpty()
                return super.setComposingText(text, newCursorPosition)
            }

            override fun finishComposingText(): Boolean {
                composing = false
                return super.finishComposingText()
            }

            override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
                if (composing) return super.deleteSurroundingText(beforeLength, afterLength)
                if (inputEnabled) {
                    repeat(beforeLength.coerceIn(0, 200_000)) {
                        emit(keyboard.tap(
                            KeyStroke(TerminalKey.Backspace, modifiers = softModifiers),
                            modifyOtherKeys = modifyOtherKeys,
                        ))
                    }
                    repeat(afterLength.coerceIn(0, 200_000)) {
                        emit(keyboard.tap(KeyStroke(TerminalKey.Delete, modifiers = softModifiers)))
                    }
                    consumeModifiers()
                }
                return true
            }

            override fun deleteSurroundingTextInCodePoints(beforeLength: Int, afterLength: Int): Boolean =
                if (composing) super.deleteSurroundingTextInCodePoints(beforeLength, afterLength)
                else deleteSurroundingText(beforeLength, afterLength)

            override fun sendKeyEvent(event: KeyEvent): Boolean {
                return this@TerminalView.dispatchKeyEvent(event)
            }
        }
    }

    override fun onCheckIsTextEditor(): Boolean = true

    // Capture hardware shortcuts before an IME turns them into an unmodified text commit.
    override fun onKeyPreIme(keyCode: Int, event: KeyEvent): Boolean {
        val key = AndroidKeyMapper.key(keyCode)
        val shortcut = event.isCtrlPressed || event.isAltPressed ||
            (event.isShiftPressed && key?.character in listOf(0, 8, 9, 13, 27))
        if (!composing && !event.isMetaPressed && key != null &&
            event.flags and KeyEvent.FLAG_SOFT_KEYBOARD == 0 &&
            (shortcut || keyboard.hasPressed(AndroidKeyMapper.id(event)))
        ) {
            return when (event.action) {
                KeyEvent.ACTION_DOWN -> onKeyDown(keyCode, event)
                KeyEvent.ACTION_UP -> onKeyUp(keyCode, event)
                else -> super.onKeyPreIme(keyCode, event)
            }
        }
        return super.onKeyPreIme(keyCode, event)
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        if (event.action != KeyEvent.ACTION_DOWN) return super.onKeyDown(keyCode, event)
        if (event.isMetaPressed) return super.onKeyDown(keyCode, event)
        var key = AndroidKeyMapper.key(keyCode)
        val id = AndroidKeyMapper.id(event)
        if (!inputEnabled) return key != null || super.onKeyDown(keyCode, event)
        if (composing) return super.onKeyDown(keyCode, event)
        val altGr = event.isCtrlPressed && event.metaState and KeyEvent.META_ALT_RIGHT_ON != 0
        val meta = if (altGr) event.metaState else event.metaState and
            (KeyEvent.META_CTRL_MASK or KeyEvent.META_ALT_MASK).inv()
        var unicode = event.getUnicodeChar(meta)
        if (unicode and KeyCharacterMap.COMBINING_ACCENT != 0) {
            deadAccent = unicode and KeyCharacterMap.COMBINING_ACCENT_MASK
            textKeys.add(id)
            return true
        }
        if (deadAccent != 0 && unicode != 0) {
            val composed = KeyEvent.getDeadChar(deadAccent, unicode)
            val text = if (composed != 0) String(Character.toChars(composed)) else
                String(Character.toChars(deadAccent)) + String(Character.toChars(unicode))
            deadAccent = 0
            textKeys.add(id)
            emit(text.toByteArray(Charsets.UTF_8))
            return true
        }
        if (altGr && unicode != 0) {
            textKeys.add(id)
            emit(String(Character.toChars(unicode)).toByteArray(Charsets.UTF_8))
            return true
        }
        if (key == null) {
            key = if (unicode in 1..127) TerminalKey.fromAscii(unicode.toChar()) else null
            if (key == null) {
                if (unicode == 0) return super.onKeyDown(keyCode, event)
                textKeys.add(id)
                emit(String(Character.toChars(unicode)).toByteArray(Charsets.UTF_8))
                return true
            }
        }
        if (unicode == 0 && key.character != 0 && key !in TerminalKey.Numpad0..TerminalKey.Numpad9 &&
            key != TerminalKey.NumpadDecimal
        ) unicode = key.character
        val modifiers = AndroidKeyMapper.modifiers(event) + softModifiers
        emit(keyboard.keyDown(
            id, KeyStroke(key, unicode, modifiers), applicationCursor, modifyOtherKeys,
        ))
        if (key !in TerminalKey.ShiftLeft..TerminalKey.ScrollLock) consumeModifiers()
        return true
    }

    override fun onKeyUp(keyCode: Int, event: KeyEvent): Boolean {
        val id = AndroidKeyMapper.id(event)
        val textKey = textKeys.remove(id)
        val bytes = keyboard.keyUp(id, AndroidKeyMapper.modifiers(event))
            ?: return if (textKey) true else super.onKeyUp(keyCode, event)
        if (inputEnabled) emit(bytes)
        return true
    }

    fun releaseKeys() {
        if (inputEnabled) emit(keyboard.releaseAll()) else keyboard.reset()
        textKeys.clear()
        deadAccent = 0
        consumeModifiers()
    }

    override fun onFocusChanged(gainFocus: Boolean, direction: Int, previouslyFocusedRect: Rect?) {
        if (!gainFocus) releaseKeys()
        super.onFocusChanged(gainFocus, direction, previouslyFocusedRect)
    }

    override fun onWindowFocusChanged(hasWindowFocus: Boolean) {
        if (!hasWindowFocus) releaseKeys()
        super.onWindowFocusChanged(hasWindowFocus)
    }

    override fun onDetachedFromWindow() {
        releaseKeys()
        super.onDetachedFromWindow()
    }

    private fun emit(bytes: ByteArray) { if (bytes.isNotEmpty()) onInput(bytes) }

    private fun consumeModifiers() {
        softModifiers = KeyModifiers()
        onModifiersConsumed()
    }

    companion object {
        fun hasFancyUnderline(attrs: Int): Boolean {
            return (attrs and (Attr.DOUBLE_UNDERLINE or Attr.UNDERCURL or Attr.DOTTED_UNDERLINE or Attr.DASHED_UNDERLINE or Attr.HYPERLINK)) != 0
        }

        fun drawViewportImages(
            canvas: Canvas,
            images: List<FfiViewportImage>,
            zFilter: (Int) -> Boolean,
            ox: Float,
            oy: Float,
            cellW: Float,
            cellH: Float,
            paint: Paint,
        ) {
            for (img in images) {
                if (!zFilter(img.zIndex)) continue
                val w = img.widthPx.toInt()
                val h = img.heightPx.toInt()
                if (w <= 0 || h <= 0 || img.rgba.size < w * h * 4) continue
                val bmp = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
                bmp.copyPixelsFromBuffer(ByteBuffer.wrap(img.rgba))
                val left = ox + img.col.toFloat() * cellW
                val top = oy + img.row.toFloat() * cellH
                val right = left + img.cols.coerceAtLeast(1u).toFloat() * cellW
                val bottom = top + img.rows.coerceAtLeast(1u).toFloat() * cellH
                canvas.drawBitmap(bmp, null, RectF(left, top, right, bottom), paint)
                bmp.recycle()
            }
        }

        fun drawFancyUnderline(
            canvas: Canvas,
            attrs: Int,
            left: Float,
            top: Float,
            width: Float,
            cellH: Float,
            color: Int,
            density: Float,
            paint: Paint,
        ) {
            val stroke = density.coerceAtLeast(1f)
            val bottom = top + cellH
            paint.color = color
            paint.strokeWidth = stroke
            when {
                (attrs and Attr.DOUBLE_UNDERLINE) != 0 -> {
                    paint.style = Paint.Style.FILL
                    canvas.drawRect(left, bottom - stroke * 3f, left + width, bottom - stroke * 2f, paint)
                    canvas.drawRect(left, bottom - stroke, left + width, bottom, paint)
                }
                (attrs and Attr.UNDERCURL) != 0 -> {
                    paint.style = Paint.Style.STROKE
                    val waveY = bottom - stroke * 2f
                    val step = (stroke * 3f).coerceAtLeast(3f)
                    val path = Path()
                    path.moveTo(left, waveY)
                    var x = left
                    var up = true
                    while (x < left + width) {
                        val nextX = (x + step).coerceAtMost(left + width)
                        val midX = (x + nextX) * 0.5f
                        val ctrlY = if (up) waveY - stroke * 1.5f else waveY + stroke * 1.5f
                        path.quadTo(midX, ctrlY, nextX, waveY)
                        x = nextX
                        up = !up
                    }
                    canvas.drawPath(path, paint)
                    paint.style = Paint.Style.FILL
                }
                (attrs and Attr.DOTTED_UNDERLINE) != 0 -> {
                    paint.style = Paint.Style.FILL
                    val dot = (stroke * 2f).coerceAtLeast(2f)
                    val y0 = bottom - stroke * 2f
                    var x = left
                    while (x < left + width) {
                        val x1 = (x + dot).coerceAtMost(left + width)
                        canvas.drawRect(x, y0, x1, y0 + stroke, paint)
                        x += dot * 2f
                    }
                }
                (attrs and Attr.DASHED_UNDERLINE) != 0 -> {
                    paint.style = Paint.Style.FILL
                    val dash = (stroke * 4f).coerceAtLeast(4f)
                    val gap = (stroke * 2f).coerceAtLeast(2f)
                    val y0 = bottom - stroke * 2f
                    var x = left
                    while (x < left + width) {
                        val x1 = (x + dash).coerceAtMost(left + width)
                        canvas.drawRect(x, y0, x1, y0 + stroke, paint)
                        x += dash + gap
                    }
                }
                (attrs and Attr.HYPERLINK) != 0 && (attrs and Attr.UNDERLINE) == 0 -> {
                    paint.style = Paint.Style.FILL
                    val dot = (stroke * 2f).coerceAtLeast(2f)
                    val y0 = bottom - stroke * 1.5f
                    var x = left
                    while (x < left + width) {
                        val x1 = (x + dot).coerceAtMost(left + width)
                        canvas.drawRect(x, y0, x1, y0 + stroke, paint)
                        x += dot * 2f
                    }
                }
            }
        }

        fun drainClipboardToSystem(context: Context, term: AlacrittyTerm) {
            val events = term.drainClipboardEvents()
            if (events.isEmpty()) return
            val cm = context.getSystemService(ClipboardManager::class.java) ?: return
            for (ev in events) {
                when (ev) {
                    is FfiClipboardEvent.Write -> {
                        if (ev.mime == "text/plain" || ev.mime.startsWith("text/plain;")) {
                            val text = ev.data.toString(Charsets.UTF_8)
                            cm.setPrimaryClip(ClipData.newPlainText("gateterm", text))
                        }
                    }
                    is FfiClipboardEvent.ReadRequest -> {}
                }
            }
        }

        fun openHyperlinkAt(context: Context, term: AlacrittyTerm, col: Int, row: Int): Boolean {
            val url = term.hyperlinkAt(col, row) ?: return false
            return runCatching {
                val intent = Intent(Intent.ACTION_VIEW, Uri.parse(url)).apply {
                    addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                }
                context.startActivity(intent)
                true
            }.getOrDefault(false)
        }
    }
}
