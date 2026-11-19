package ai.eclosion.gateterm.android.ui.term

import android.content.ClipData
import android.content.ClipboardManager
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
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
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.platform.UriHandler
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.isSpecified
import androidx.compose.ui.unit.sp
import ai.eclosion.gateterm.android.R
import ai.eclosion.gateterm.android.appearance.UiPalette
import kotlinx.coroutines.delay
import uniffi.gateterm_cloud_ffi.FfiMdAlign
import uniffi.gateterm_cloud_ffi.FfiMdBlock
import uniffi.gateterm_cloud_ffi.FfiMdDocument
import uniffi.gateterm_cloud_ffi.FfiMdRun
import uniffi.gateterm_cloud_ffi.parseMarkdown

private const val MAX_DEPTH = 16
private const val COPY_FEEDBACK_MS = 1500L

internal fun Int.toOpaqueColor(): Color = Color(this or 0xFF000000.toInt())

internal fun sanitizeLinkUrl(raw: String?): String? {
    val trimmed = raw?.trim().orEmpty()
    if (trimmed.isEmpty()) return null
    if (trimmed.any { it.code <= 0x1f || it.code == 0x7f }) return null
    val scheme = listOf("https://", "http://", "mailto:")
        .firstOrNull { trimmed.startsWith(it, ignoreCase = true) }
        ?: return null
    if (trimmed.length <= scheme.length) return null
    return trimmed
}

internal fun headingFontSize(level: Int): TextUnit = when (level) {
    1 -> 15.sp
    2 -> 14.sp
    else -> 13.sp
}

private fun tableColumnAlignment(align: FfiMdAlign?): TextAlign = when (align) {
    FfiMdAlign.CENTER -> TextAlign.Center
    FfiMdAlign.RIGHT -> TextAlign.End
    FfiMdAlign.LEFT, FfiMdAlign.NONE, null -> TextAlign.Start
}

internal fun buildMarkdownAnnotatedString(
    runs: List<FfiMdRun>,
    palette: UiPalette,
    baseSize: TextUnit,
): AnnotatedString = buildAnnotatedString {
    val accentColor = palette.accent.toOpaqueColor()
    val codeFg = palette.mdCode.toOpaqueColor()
    val codeBg = palette.panel2.toOpaqueColor()
    val linkStyle = TextLinkStyles(
        style = SpanStyle(
            color = accentColor,
            textDecoration = TextDecoration.Underline,
        ),
    )
    var activeLink: String? = null
    for (run in runs) {
        if (run.text.isEmpty()) continue
        val safeLink = sanitizeLinkUrl(run.link)
        if (safeLink != activeLink) {
            if (activeLink != null) pop()
            if (safeLink != null) {
                pushLink(LinkAnnotation.Url(url = safeLink, styles = linkStyle))
            }
            activeLink = safeLink
        }
        val decoration = when {
            run.strikethrough && safeLink != null ->
                TextDecoration.combine(listOf(TextDecoration.LineThrough, TextDecoration.Underline))
            run.strikethrough -> TextDecoration.LineThrough
            else -> null
        }
        val style = SpanStyle(
            fontFamily = FontFamily.Monospace,
            fontWeight = if (run.bold) FontWeight.Bold else null,
            fontStyle = if (run.italic) FontStyle.Italic else null,
            textDecoration = decoration,
            color = if (run.code) codeFg else Color.Unspecified,
            background = if (run.code) codeBg else Color.Unspecified,
            fontSize = if (run.code && baseSize.isSpecified) (baseSize.value * 0.94f).sp else TextUnit.Unspecified,
        )
        withStyle(style) {
            append(run.text)
        }
    }
    if (activeLink != null) pop()
}

@Composable
fun ChatMarkdown(
    text: String,
    palette: UiPalette,
    modifier: Modifier = Modifier,
) {
    val doc = remember(text) { parseMarkdown(text) }
    if (doc.root.isEmpty()) return

    val baseUriHandler = LocalUriHandler.current
    val safeUriHandler = remember(baseUriHandler) {
        object : UriHandler {
            override fun openUri(uri: String) {
                if (sanitizeLinkUrl(uri) != null) {
                    runCatching { baseUriHandler.openUri(uri) }
                }
            }
        }
    }

    CompositionLocalProvider(LocalUriHandler provides safeUriHandler) {
        Column(
            modifier = modifier.fillMaxWidth(),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            MarkdownBlockList(
                doc = doc,
                indices = doc.root,
                palette = palette,
                depth = 0,
                listDepth = 0,
                strikeThrough = false,
            )
        }
    }
}

@Composable
private fun MarkdownBlockList(
    doc: FfiMdDocument,
    indices: List<UInt>,
    palette: UiPalette,
    depth: Int,
    listDepth: Int,
    strikeThrough: Boolean,
) {
    if (depth >= MAX_DEPTH - 1) {
        MarkdownFlattenedBlocks(doc = doc, indices = indices, palette = palette, strikeThrough = strikeThrough)
        return
    }
    indices.forEach { idx ->
        val block = doc.blocks.getOrNull(idx.toInt()) ?: return@forEach
        MarkdownBlock(
            doc = doc,
            block = block,
            palette = palette,
            depth = depth,
            listDepth = listDepth,
            strikeThrough = strikeThrough,
        )
    }
}

internal fun flattenDeepBlocks(
    indices: List<UInt>,
    doc: FfiMdDocument,
    inheritedStrikeThrough: Boolean,
): List<Pair<Int, Boolean>> {
    val result = mutableListOf<Pair<Int, Boolean>>()
    val stack = ArrayDeque<Pair<Int, Boolean>>(indices.size)
    for (i in indices.indices.reversed()) {
        stack.addLast(indices[i].toInt() to inheritedStrikeThrough)
    }
    while (stack.isNotEmpty()) {
        val (blockIndex, strikeThrough) = stack.removeLast()
        val block = doc.blocks.getOrNull(blockIndex) ?: continue
        when (block) {
            is FfiMdBlock.BlockQuote -> {
                for (i in block.children.indices.reversed()) {
                    stack.addLast(block.children[i].toInt() to strikeThrough)
                }
            }
            is FfiMdBlock.ListBlock -> {
                for (i in block.items.indices.reversed()) {
                    val item = block.items[i]
                    val childStrike = strikeThrough || (item.checked == true)
                    for (j in item.children.indices.reversed()) {
                        stack.addLast(item.children[j].toInt() to childStrike)
                    }
                }
            }
            is FfiMdBlock.Heading,
            is FfiMdBlock.Paragraph,
            is FfiMdBlock.CodeBlock,
            is FfiMdBlock.Table,
            FfiMdBlock.ThematicBreak -> result.add(blockIndex to strikeThrough)
        }
    }
    return result
}

@Composable
private fun MarkdownFlattenedBlocks(
    doc: FfiMdDocument,
    indices: List<UInt>,
    palette: UiPalette,
    strikeThrough: Boolean,
) {
    val flattened = remember(doc, indices, strikeThrough) {
        flattenDeepBlocks(indices, doc, strikeThrough)
    }
    flattened.forEach { (blockIndex, leafStrike) ->
        val block = doc.blocks.getOrNull(blockIndex) ?: return@forEach
        MarkdownLeafBlock(block = block, palette = palette, strikeThrough = leafStrike)
    }
}

@Composable
private fun MarkdownLeafBlock(
    block: FfiMdBlock,
    palette: UiPalette,
    strikeThrough: Boolean,
) {
    when (block) {
        is FfiMdBlock.Heading -> MarkdownHeading(block, palette, strikeThrough)
        is FfiMdBlock.Paragraph -> MarkdownParagraph(block.runs, palette, strikeThrough)
        is FfiMdBlock.CodeBlock -> MarkdownCodeBlock(block, palette)
        is FfiMdBlock.Table -> MarkdownTable(block, palette)
        FfiMdBlock.ThematicBreak -> HorizontalDivider(
            thickness = 1.dp,
            color = palette.line.toOpaqueColor(),
            modifier = Modifier.padding(vertical = 4.dp),
        )
        is FfiMdBlock.BlockQuote, is FfiMdBlock.ListBlock -> Unit
    }
}

@Composable
private fun MarkdownBlock(
    doc: FfiMdDocument,
    block: FfiMdBlock,
    palette: UiPalette,
    depth: Int,
    listDepth: Int,
    strikeThrough: Boolean,
) {
    when (block) {
        is FfiMdBlock.Heading,
        is FfiMdBlock.Paragraph,
        is FfiMdBlock.CodeBlock,
        is FfiMdBlock.Table,
        FfiMdBlock.ThematicBreak -> MarkdownLeafBlock(block, palette, strikeThrough)
        is FfiMdBlock.BlockQuote -> MarkdownBlockQuote(doc, block, palette, depth, listDepth, strikeThrough)
        is FfiMdBlock.ListBlock -> MarkdownList(doc, block, palette, depth, listDepth, strikeThrough)
    }
}

@Composable
private fun MarkdownHeading(
    block: FfiMdBlock.Heading,
    palette: UiPalette,
    strikeThrough: Boolean,
) {
    val level = block.level.toInt().coerceIn(1, 6)
    val size = headingFontSize(level)
    val lineHeight = (size.value * 1.45f).sp
    val annotated = remember(block.runs, palette, size) {
        buildMarkdownAnnotatedString(block.runs, palette, size)
    }
    Column(Modifier.fillMaxWidth()) {
        Row(Modifier.fillMaxWidth()) {
            Text(
                text = "#".repeat(level) + " ",
                fontFamily = FontFamily.Monospace,
                fontSize = size,
                lineHeight = lineHeight,
                color = palette.dim.toOpaqueColor(),
            )
            SelectionContainer(Modifier.weight(1f)) {
                Text(
                    text = annotated,
                    style = TextStyle(
                        fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Bold,
                        fontSize = size,
                        lineHeight = lineHeight,
                        color = palette.mdHeading.toOpaqueColor(),
                        textDecoration = if (strikeThrough) TextDecoration.LineThrough else null,
                    ),
                )
            }
        }
        if (level <= 2) {
            Spacer(Modifier.height(3.dp))
            HorizontalDivider(thickness = 1.dp, color = palette.line.toOpaqueColor())
        }
    }
}

@Composable
private fun MarkdownParagraph(
    runs: List<FfiMdRun>,
    palette: UiPalette,
    strikeThrough: Boolean,
) {
    val size = 13.sp
    val annotated = remember(runs, palette, size) {
        buildMarkdownAnnotatedString(runs, palette, size)
    }
    SelectionContainer {
        Text(
            text = annotated,
            style = TextStyle(
                fontFamily = FontFamily.Monospace,
                fontSize = size,
                lineHeight = 20.sp,
                color = palette.foreground.toOpaqueColor(),
                textDecoration = if (strikeThrough) TextDecoration.LineThrough else null,
            ),
        )
    }
}

@Composable
private fun MarkdownCodeBlock(
    block: FfiMdBlock.CodeBlock,
    palette: UiPalette,
) {
    val shape = RoundedCornerShape(6.dp)
    val borderColor = palette.line.toOpaqueColor()
    val context = LocalContext.current
    var copied by remember(block.code) { mutableStateOf(false) }

    LaunchedEffect(copied) {
        if (copied) {
            delay(COPY_FEEDBACK_MS)
            copied = false
        }
    }

    Column(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(palette.panel.toOpaqueColor())
            .border(1.dp, borderColor, shape),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .background(palette.panel2.toOpaqueColor())
                .padding(horizontal = 10.dp, vertical = 5.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = block.lang?.lowercase().orEmpty(),
                fontFamily = FontFamily.Monospace,
                fontSize = 11.sp,
                color = palette.dim.toOpaqueColor(),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            val copyShape = RoundedCornerShape(4.dp)
            val copyTint = (if (copied) palette.accent else palette.dim).toOpaqueColor()
            val copyBorder = (if (copied) palette.accent else palette.line).toOpaqueColor()
            Text(
                text = stringResource(if (copied) R.string.chat_copied_code else R.string.chat_copy_code),
                fontFamily = FontFamily.Monospace,
                fontSize = 11.sp,
                color = copyTint,
                modifier = Modifier
                    .clip(copyShape)
                    .background(palette.background.toOpaqueColor())
                    .border(1.dp, copyBorder, copyShape)
                    .clickable(role = Role.Button) {
                        val cm = context.getSystemService(ClipboardManager::class.java)
                        if (cm != null) {
                            cm.setPrimaryClip(ClipData.newPlainText("code", block.code))
                            copied = true
                        }
                    }
                    .padding(horizontal = 8.dp, vertical = 3.dp),
            )
        }
        HorizontalDivider(thickness = 1.dp, color = borderColor)
        Box(
            Modifier
                .fillMaxWidth()
                .horizontalScroll(rememberScrollState())
                .padding(horizontal = 12.dp, vertical = 10.dp),
        ) {
            SelectionContainer {
                Text(
                    text = block.code,
                    fontFamily = FontFamily.Monospace,
                    fontSize = 12.5.sp,
                    lineHeight = 19.sp,
                    color = palette.foreground.toOpaqueColor(),
                    softWrap = false,
                )
            }
        }
    }
}

@Composable
private fun MarkdownBlockQuote(
    doc: FfiMdDocument,
    block: FfiMdBlock.BlockQuote,
    palette: UiPalette,
    depth: Int,
    listDepth: Int,
    strikeThrough: Boolean,
) {
    if (depth >= MAX_DEPTH - 1) {
        MarkdownFlattenedBlocks(doc, block.children, palette, strikeThrough)
        return
    }
    val barColor = palette.accent.toOpaqueColor()
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .alpha(0.85f)
            .drawBehind {
                val barWidth = 2.dp.toPx()
                drawRoundRect(
                    color = barColor,
                    topLeft = Offset.Zero,
                    size = Size(barWidth, size.height),
                    cornerRadius = CornerRadius(barWidth / 2f, barWidth / 2f),
                )
            }
            .padding(start = 12.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        MarkdownBlockList(doc, block.children, palette, depth + 1, listDepth, strikeThrough)
    }
}

@Composable
private fun MarkdownList(
    doc: FfiMdDocument,
    block: FfiMdBlock.ListBlock,
    palette: UiPalette,
    depth: Int,
    listDepth: Int,
    strikeThrough: Boolean,
) {
    if (depth >= MAX_DEPTH - 1) {
        Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            block.items.forEach { item ->
                MarkdownFlattenedBlocks(doc, item.children, palette, strikeThrough || (item.checked == true))
            }
        }
        return
    }
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(start = if (listDepth == 0) 2.dp else 10.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        block.items.forEachIndexed { idx, item ->
            val isCheckedTask = item.checked == true
            val marker = when {
                item.checked != null -> if (isCheckedTask) "[x]" else "[ ]"
                block.ordered -> "${block.start + idx.toULong()}."
                else -> when (listDepth % 3) {
                    0 -> "•"
                    1 -> "◦"
                    else -> "▪"
                }
            }
            val markerColor = (if (isCheckedTask) palette.accent else palette.dim).toOpaqueColor()
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(
                    text = marker,
                    fontFamily = FontFamily.Monospace,
                    fontSize = 13.sp,
                    lineHeight = 20.sp,
                    color = markerColor,
                )
                Column(
                    modifier = Modifier
                        .weight(1f)
                        .alpha(if (isCheckedTask) 0.72f else 1f),
                    verticalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    val firstIdx = item.children.firstOrNull()
                    val firstBlock = firstIdx?.let { doc.blocks.getOrNull(it.toInt()) }
                    if (firstBlock is FfiMdBlock.Paragraph) {
                        MarkdownBlock(
                            doc = doc,
                            block = firstBlock,
                            palette = palette,
                            depth = depth + 1,
                            listDepth = listDepth + 1,
                            strikeThrough = strikeThrough || isCheckedTask,
                        )
                        if (item.children.size > 1) {
                            MarkdownBlockList(
                                doc = doc,
                                indices = item.children.drop(1),
                                palette = palette,
                                depth = depth + 1,
                                listDepth = listDepth + 1,
                                strikeThrough = strikeThrough,
                            )
                        }
                    } else {
                        MarkdownBlockList(
                            doc = doc,
                            indices = item.children,
                            palette = palette,
                            depth = depth + 1,
                            listDepth = listDepth + 1,
                            strikeThrough = strikeThrough || isCheckedTask,
                        )
                    }
                }
            }
        }
    }
}

private class TableGridMetrics {
    var colWidths: IntArray = IntArray(0)
    var rowHeights: IntArray = IntArray(0)
}

@Composable
private fun MarkdownTable(
    block: FfiMdBlock.Table,
    palette: UiPalette,
) {
    val colCount = maxOf(
        block.headers.size,
        block.rows.maxOfOrNull { it.size } ?: 0,
        block.alignments.size,
    )
    if (colCount == 0) return

    val totalRows = 1 + block.rows.size
    val metrics = remember(block) { TableGridMetrics() }
    val headerBg = palette.panel.toOpaqueColor()
    val borderColor = palette.line.toOpaqueColor()
    val density = LocalDensity.current
    val minColPx = with(density) { 48.dp.roundToPx() }
    val maxColPx = with(density) { 280.dp.roundToPx() }
    val strokePx = with(density) { 1.dp.toPx() }

    Box(
        Modifier
            .fillMaxWidth()
            .horizontalScroll(rememberScrollState()),
    ) {
        SelectionContainer {
            Layout(
                modifier = Modifier
                    .drawBehind {
                        val headerH = metrics.rowHeights.firstOrNull() ?: return@drawBehind
                        drawRect(
                            color = headerBg,
                            topLeft = Offset.Zero,
                            size = Size(size.width, headerH.toFloat()),
                        )
                    }
                    .drawWithContent {
                        drawContent()
                        val cols = metrics.colWidths
                        val rows = metrics.rowHeights
                        if (cols.isEmpty() || rows.isEmpty()) return@drawWithContent
                        val half = strokePx / 2f
                        var x = 0f
                        drawLine(borderColor, Offset(half, 0f), Offset(half, size.height), strokePx)
                        for (w in cols) {
                            x += w
                            val lineX = (x - half).coerceAtLeast(half)
                            drawLine(borderColor, Offset(lineX, 0f), Offset(lineX, size.height), strokePx)
                        }
                        var y = 0f
                        drawLine(borderColor, Offset(0f, half), Offset(size.width, half), strokePx)
                        for (h in rows) {
                            y += h
                            val lineY = (y - half).coerceAtLeast(half)
                            drawLine(borderColor, Offset(0f, lineY), Offset(size.width, lineY), strokePx)
                        }
                    },
                content = {
                    for (c in 0 until colCount) {
                        val cell = block.headers.getOrNull(c)
                        val align = tableColumnAlignment(block.alignments.getOrNull(c))
                        TableCellView(
                            runs = cell?.runs.orEmpty(),
                            palette = palette,
                            align = align,
                            isHeader = true,
                        )
                    }
                    for (row in block.rows) {
                        for (c in 0 until colCount) {
                            val cell = row.getOrNull(c)
                            val align = tableColumnAlignment(block.alignments.getOrNull(c))
                            TableCellView(
                                runs = cell?.runs.orEmpty(),
                                palette = palette,
                                align = align,
                                isHeader = false,
                            )
                        }
                    }
                },
            ) { measurables, constraints ->
                val colWidths = IntArray(colCount) { minColPx }
                for (r in 0 until totalRows) {
                    for (c in 0 until colCount) {
                        val m = measurables[r * colCount + c]
                        val natural = m.maxIntrinsicWidth(Constraints.Infinity).coerceIn(minColPx, maxColPx)
                        if (natural > colWidths[c]) colWidths[c] = natural
                    }
                }
                val placeables = Array(totalRows * colCount) { idx ->
                    val c = idx % colCount
                    measurables[idx].measure(Constraints(minWidth = colWidths[c], maxWidth = colWidths[c]))
                }
                val rowHeights = IntArray(totalRows) { r ->
                    (0 until colCount).maxOf { c -> placeables[r * colCount + c].height }
                }
                metrics.colWidths = colWidths
                metrics.rowHeights = rowHeights
                val totalWidth = colWidths.sum().coerceAtLeast(constraints.minWidth)
                val totalHeight = rowHeights.sum().coerceAtLeast(constraints.minHeight)
                layout(totalWidth, totalHeight) {
                    var y = 0
                    for (r in 0 until totalRows) {
                        var x = 0
                        for (c in 0 until colCount) {
                            placeables[r * colCount + c].placeRelative(x, y)
                            x += colWidths[c]
                        }
                        y += rowHeights[r]
                    }
                }
            }
        }
    }
}

@Composable
private fun TableCellView(
    runs: List<FfiMdRun>,
    palette: UiPalette,
    align: TextAlign,
    isHeader: Boolean,
) {
    val size = 12.5.sp
    val annotated = remember(runs, palette, size) {
        buildMarkdownAnnotatedString(runs, palette, size)
    }
    Box(Modifier.padding(horizontal = 10.dp, vertical = 6.dp)) {
        Text(
            text = annotated,
            modifier = Modifier.fillMaxWidth(),
            style = TextStyle(
                fontFamily = FontFamily.Monospace,
                fontWeight = if (isHeader) FontWeight.Bold else FontWeight.Normal,
                fontSize = size,
                lineHeight = 18.sp,
                color = (if (isHeader) palette.mdHeading else palette.foreground).toOpaqueColor(),
                textAlign = align,
            ),
        )
    }
}
