package ai.eclosion.gateterm.android.ui.onboarding

import ai.eclosion.gateterm.android.R
import ai.eclosion.gateterm.android.appearance.isLightTheme
import ai.eclosion.gateterm.android.onboarding.NormalizedRect
import ai.eclosion.gateterm.android.onboarding.OnboardingConfig
import ai.eclosion.gateterm.android.ui.theme.LocalTermAppearance
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.wrapContentWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.ui.zIndex
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowForward
import androidx.compose.material.icons.filled.Star
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

private val StageFocusBlueOnLight = Color(0xFF1F6FEB)

@Immutable
private data class StagePalette(
    val isLight: Boolean,
    val warmOrange: Color,
    val warmOrangePill: Color,
    val liveGreen: Color,
    val focusBlue: Color,
    val focusBluePill: Color,
)

@Composable
private fun rememberStagePalette(): StagePalette {
    val isLight = LocalTermAppearance.current.isLightTheme()
    return remember(isLight) {
        val blue = if (isLight) Color(0xFF0451A5) else StageFocusBlueOnLight
        StagePalette(
            isLight = isLight,
            warmOrange = if (isLight) Color(0xFFD94E22) else Color(0xFFF06A38),
            warmOrangePill = if (isLight) Color(0xFFD94E22) else Color(0xFFE45A2B),
            liveGreen = if (isLight) Color(0xFF107C10) else Color(0xFF23D18B),
            focusBlue = blue,
            focusBluePill = blue,
        )
    }
}

/**
 * Renders the visual stage matching [step] in `[0, OnboardingConfig.STEP_COUNT)`.
 */
@Composable
fun OnboardingVisualStage(
    step: Int,
    modifier: Modifier = Modifier,
) {
    val palette = rememberStagePalette()
    when (step) {
        0 -> ArchitectureStage(palette, modifier)
        1 -> InstallHostStage(palette, modifier)
        2 -> DesktopQrStage(palette, modifier)
        else -> ScanConnectStage(palette, modifier)
    }
}

@Composable
private fun StageFrame(
    palette: StagePalette,
    modifier: Modifier = Modifier,
    content: @Composable BoxScope.() -> Unit,
) {
    val primary = MaterialTheme.colorScheme.primary
    val orangeGlowAlpha = if (palette.isLight) 0.09f else 0.17f
    val blueGlowAlpha = if (palette.isLight) 0.09f else 0.19f
    val borderColor = MaterialTheme.colorScheme.outlineVariant.copy(
        alpha = if (palette.isLight) 0.65f else 0.85f,
    )
    Box(
        modifier = modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(24.dp))
            .background(MaterialTheme.colorScheme.surface)
            .border(1.dp, borderColor, RoundedCornerShape(24.dp))
            .drawBehind {
                val radius = size.maxDimension * 0.72f
                drawCircle(
                    brush = Brush.radialGradient(
                        colors = listOf(
                            palette.warmOrange.copy(alpha = orangeGlowAlpha),
                            Color.Transparent,
                        ),
                        center = Offset(size.width * 0.16f, size.height * 0.14f),
                        radius = radius,
                    ),
                    radius = radius,
                    center = Offset(size.width * 0.16f, size.height * 0.14f),
                )
                drawCircle(
                    brush = Brush.radialGradient(
                        colors = listOf(
                            primary.copy(alpha = blueGlowAlpha),
                            Color.Transparent,
                        ),
                        center = Offset(size.width * 0.84f, size.height * 0.86f),
                        radius = radius,
                    ),
                    radius = radius,
                    center = Offset(size.width * 0.84f, size.height * 0.86f),
                )
            }
            .padding(14.dp),
        contentAlignment = Alignment.Center,
        content = content,
    )
}

/**
 * Step 1 visual: Desktop-Web screenshot linked by a pulsing beam to an Android client preview.
 */
@Composable
private fun ArchitectureStage(
    palette: StagePalette,
    modifier: Modifier = Modifier,
) {
    val primary = MaterialTheme.colorScheme.primary
    val transition = rememberInfiniteTransition(label = "architecture_beam")
    val beamProgressState = transition.animateFloat(
        initialValue = 0f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(
            animation = tween(durationMillis = 2400, easing = LinearEasing),
            repeatMode = RepeatMode.Restart,
        ),
        label = "beam_progress",
    )

    StageFrame(
        palette = palette,
        modifier = modifier.heightIn(min = 264.dp),
    ) {
        Column(
            modifier = Modifier.fillMaxWidth(),
            verticalArrangement = Arrangement.spacedBy(14.dp, Alignment.CenterVertically),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            // Keep physical left-to-right stage ordering (desktop -> beam -> phone) even under RTL locales.
            CompositionLocalProvider(LocalLayoutDirection provides LayoutDirection.Ltr) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(top = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    DesktopTerminalWindow(
                        liveGreen = palette.liveGreen,
                        isLight = palette.isLight,
                        modifier = Modifier.weight(1.42f),
                    )
                    Box(
                        modifier = Modifier
                            .width(62.dp)
                            .zIndex(1f),
                        contentAlignment = Alignment.Center,
                    ) {
                        ConnectionBeam(
                            progressProvider = { beamProgressState.value },
                            liveGreen = palette.liveGreen,
                            accent = primary,
                            modifier = Modifier
                                .fillMaxWidth()
                                .height(52.dp),
                        )
                        // Floating network badge centered over the beam without overlapping terminal content.
                        Surface(
                            shape = RoundedCornerShape(50),
                            color = if (palette.isLight) Color.White else Color(0xFF1F242E),
                            border = BorderStroke(1.dp, primary.copy(alpha = 0.45f)),
                            shadowElevation = if (palette.isLight) 3.dp else 0.dp,
                            modifier = Modifier
                                .wrapContentWidth(unbounded = true)
                                .widthIn(max = 82.dp)
                                .offset(y = (-22).dp),
                        ) {
                            Text(
                                text = stringResource(R.string.onboarding_step_1_link_label),
                                fontFamily = FontFamily.Monospace,
                                fontWeight = FontWeight.SemiBold,
                                fontSize = 8.5.sp,
                                lineHeight = 10.5.sp,
                                color = primary,
                                textAlign = TextAlign.Center,
                                maxLines = 2,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.padding(horizontal = 6.dp, vertical = 2.5.dp),
                            )
                        }
                    }
                    MiniaturePhoneFrame(
                        liveGreen = palette.liveGreen,
                        accent = primary,
                        isLight = palette.isLight,
                        syncedLabel = stringResource(R.string.onboarding_step_1_phone_synced),
                        modifier = Modifier.weight(0.95f),
                    )
                }
            }

            FlowRow(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally),
                verticalArrangement = Arrangement.spacedBy(6.dp, Alignment.CenterVertically),
            ) {
                HostChip(
                    label = "Gateterm-Desktop-Web",
                    dotColor = palette.liveGreen,
                    highlighted = true,
                    isLight = palette.isLight,
                )
                HostChip(
                    label = "Gateterm-Server",
                    dotColor = MaterialTheme.colorScheme.onSurfaceVariant,
                    highlighted = false,
                    isLight = palette.isLight,
                )
            }
        }
    }
}

@Composable
private fun DesktopTerminalWindow(
    liveGreen: Color,
    isLight: Boolean,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier = modifier,
        shape = RoundedCornerShape(12.dp),
        color = Color(0xFF15181D),
        border = BorderStroke(
            1.dp,
            if (isLight) Color(0xFF2D333B) else Color.White.copy(alpha = 0.14f),
        ),
        shadowElevation = if (isLight) 6.dp else 2.dp,
    ) {
        Column {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .background(Color(0xFF1D2128))
                    .padding(horizontal = 8.dp, vertical = 5.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                TrafficLightDots(dotSize = 6.dp)
                Text(
                    text = "Desktop-Web",
                    fontFamily = FontFamily.Monospace,
                    fontSize = 9.5.sp,
                    fontWeight = FontWeight.Medium,
                    color = Color(0xFFAEB8C4),
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Image(
                painter = painterResource(R.drawable.tutorial_web_terminal),
                contentDescription = null,
                contentScale = ContentScale.Crop,
                modifier = Modifier
                    .fillMaxWidth()
                    .aspectRatio(800f / 500f),
            )
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .background(Color(0xFF101317))
                    .padding(horizontal = 8.dp, vertical = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(5.dp),
            ) {
                Box(
                    modifier = Modifier
                        .size(6.dp)
                        .clip(CircleShape)
                        .background(liveGreen),
                )
                Text(
                    text = stringResource(R.string.onboarding_step_1_host_status),
                    fontSize = 9.sp,
                    color = Color(0xFFC8D3DF),
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

@Composable
private fun ConnectionBeam(
    progressProvider: () -> Float,
    liveGreen: Color,
    accent: Color,
    modifier: Modifier = Modifier,
) {
    Canvas(modifier = modifier) {
        val progress = progressProvider()
        val y = size.height * 0.5f
        val startX = 2.dp.toPx()
        val endX = size.width - 2.dp.toPx()
        drawLine(
            brush = Brush.horizontalGradient(
                colors = listOf(
                    liveGreen.copy(alpha = 0.55f),
                    accent.copy(alpha = 0.65f),
                ),
                startX = startX,
                endX = endX,
            ),
            start = Offset(startX, y),
            end = Offset(endX, y),
            strokeWidth = 2.dp.toPx(),
            cap = StrokeCap.Round,
        )
        val orbX = startX + (endX - startX) * progress
        drawCircle(
            color = accent.copy(alpha = 0.25f),
            radius = 7.dp.toPx(),
            center = Offset(orbX, y),
        )
        drawCircle(
            color = liveGreen,
            radius = 3.2.dp.toPx(),
            center = Offset(orbX, y),
        )
    }
}

@Composable
private fun MiniaturePhoneFrame(
    liveGreen: Color,
    accent: Color,
    isLight: Boolean,
    syncedLabel: String,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier = modifier,
        shape = RoundedCornerShape(16.dp),
        color = Color(0xFF191D24),
        border = BorderStroke(
            1.5.dp,
            if (isLight) Color(0xFF333A45) else Color.White.copy(alpha = 0.16f),
        ),
        shadowElevation = if (isLight) 6.dp else 2.dp,
    ) {
        Column(
            modifier = Modifier.padding(horizontal = 6.dp, vertical = 6.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Box(
                modifier = Modifier
                    .width(20.dp)
                    .height(3.dp)
                    .clip(CircleShape)
                    .background(Color.White.copy(alpha = 0.18f)),
            )
            Spacer(Modifier.height(5.dp))
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(10.dp))
                    .background(Color(0xFF101318))
                    .padding(horizontal = 6.dp, vertical = 7.dp),
                verticalArrangement = Arrangement.spacedBy(5.dp),
            ) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    Box(
                        modifier = Modifier
                            .size(5.dp)
                            .clip(CircleShape)
                            .background(liveGreen),
                    )
                    Text(
                        text = syncedLabel,
                        fontSize = 8.sp,
                        fontWeight = FontWeight.Medium,
                        color = liveGreen,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                Text(
                    text = "~/octoterm $",
                    fontFamily = FontFamily.Monospace,
                    fontSize = 7.5.sp,
                    lineHeight = 9.5.sp,
                    color = accent,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                TerminalBarLine(widthFraction = 0.85f, color = Color(0xFFD5DCE6))
                TerminalBarLine(widthFraction = 0.65f, color = liveGreen.copy(alpha = 0.85f))
                TerminalBarLine(widthFraction = 0.72f, color = Color(0xFF8B96A8))
            }
        }
    }
}

@Composable
private fun HostChip(
    label: String,
    dotColor: Color,
    highlighted: Boolean,
    isLight: Boolean,
) {
    val primary = MaterialTheme.colorScheme.primary
    val bg = when {
        highlighted -> primary.copy(alpha = if (isLight) 0.10f else 0.16f)
        else -> MaterialTheme.colorScheme.background.copy(alpha = if (isLight) 0.85f else 0.55f)
    }
    val border = when {
        highlighted -> primary.copy(alpha = 0.38f)
        else -> MaterialTheme.colorScheme.outlineVariant
    }
    Surface(
        shape = RoundedCornerShape(50),
        color = bg,
        border = BorderStroke(1.dp, border),
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 9.dp, vertical = 5.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(5.dp),
        ) {
            Box(
                modifier = Modifier
                    .size(5.dp)
                    .clip(CircleShape)
                    .background(dotColor),
            )
            Text(
                text = label,
                fontFamily = FontFamily.Monospace,
                fontSize = 10.sp,
                fontWeight = if (highlighted) FontWeight.SemiBold else FontWeight.Medium,
                color = if (highlighted) primary else MaterialTheme.colorScheme.onSurface,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

/**
 * Step 2 visual: Browser frame mirroring GitHub Releases with `gateterm-desktop-web` highlighted.
 */
@Composable
private fun InstallHostStage(
    palette: StagePalette,
    modifier: Modifier = Modifier,
) {
    val accentBlue = if (palette.isLight) Color(0xFF3B8EEA) else MaterialTheme.colorScheme.primary

    StageFrame(
        palette = palette,
        modifier = modifier.heightIn(min = 264.dp),
    ) {
        Box(
            modifier = Modifier.fillMaxWidth(),
            contentAlignment = Alignment.Center,
        ) {
            Surface(
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(14.dp),
                color = Color(0xFF15181E),
                border = BorderStroke(
                    1.dp,
                    if (palette.isLight) Color(0xFF2D333B) else Color.White.copy(alpha = 0.14f),
                ),
                shadowElevation = if (palette.isLight) 6.dp else 2.dp,
            ) {
                Column(modifier = Modifier.fillMaxWidth()) {
                    // Dark browser toolbar
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .background(Color(0xFF1D222B))
                            .padding(horizontal = 10.dp, vertical = 7.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        TrafficLightDots(dotSize = 6.dp)
                        Row(
                            modifier = Modifier
                                .weight(1f)
                                .clip(RoundedCornerShape(50))
                                .background(Color(0xFF12151B))
                                .padding(horizontal = 10.dp, vertical = 4.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(5.dp),
                        ) {
                            Box(
                                modifier = Modifier
                                    .size(5.dp)
                                    .clip(CircleShape)
                                    .background(palette.liveGreen),
                            )
                            Text(
                                text = "github.com / Gateterm / Releases",
                                fontFamily = FontFamily.Monospace,
                                fontSize = 9.5.sp,
                                color = Color(0xFFC0CAD6),
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                    }

                    // Release assets cards
                    Column(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(horizontal = 12.dp, vertical = 12.dp),
                        verticalArrangement = Arrangement.spacedBy(10.dp),
                    ) {
                        Box(modifier = Modifier.fillMaxWidth()) {
                            Surface(
                                modifier = Modifier.fillMaxWidth(),
                                shape = RoundedCornerShape(12.dp),
                                color = Color(0xFF1B2433),
                                border = BorderStroke(1.5.dp, accentBlue),
                            ) {
                                Column(
                                    modifier = Modifier.padding(
                                        start = 12.dp,
                                        end = 12.dp,
                                        top = 14.dp,
                                        bottom = 11.dp,
                                    ),
                                    verticalArrangement = Arrangement.spacedBy(4.dp),
                                ) {
                                    Row(
                                        verticalAlignment = Alignment.CenterVertically,
                                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                                    ) {
                                        Icon(
                                            imageVector = Icons.Filled.Star,
                                            contentDescription = null,
                                            tint = palette.warmOrange,
                                            modifier = Modifier.size(14.dp),
                                        )
                                        Text(
                                            text = "gateterm-desktop-web",
                                            fontFamily = FontFamily.Monospace,
                                            fontWeight = FontWeight.Bold,
                                            fontSize = 12.5.sp,
                                            color = Color(0xFFF3F6FA),
                                            maxLines = 1,
                                            overflow = TextOverflow.Ellipsis,
                                        )
                                    }
                                    Text(
                                        text = stringResource(R.string.onboarding_step_2_desktop_desc),
                                        fontSize = 11.sp,
                                        color = Color(0xFFB7C4D4),
                                        maxLines = 2,
                                        overflow = TextOverflow.Ellipsis,
                                    )
                                }
                            }

                            CalloutPill(
                                text = stringResource(R.string.onboarding_step_2_callout),
                                backgroundColor = palette.warmOrangePill,
                                contentColor = Color.White,
                                maxLines = 2,
                                modifier = Modifier
                                    .widthIn(max = 152.dp)
                                    .align(Alignment.TopEnd)
                                    .offset(x = 4.dp, y = (-10).dp),
                            )
                        }

                        Surface(
                            modifier = Modifier.fillMaxWidth(),
                            shape = RoundedCornerShape(12.dp),
                            color = Color(0xFF191D24),
                            border = BorderStroke(1.dp, Color.White.copy(alpha = 0.10f)),
                        ) {
                            Column(
                                modifier = Modifier.padding(horizontal = 12.dp, vertical = 10.dp),
                                verticalArrangement = Arrangement.spacedBy(4.dp),
                            ) {
                                Row(
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                                ) {
                                    Text(
                                        text = ">_",
                                        fontFamily = FontFamily.Monospace,
                                        fontWeight = FontWeight.Bold,
                                        fontSize = 11.sp,
                                        color = Color(0xFF8B97A8),
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                    )
                                    Text(
                                        text = "gateterm-server",
                                        fontFamily = FontFamily.Monospace,
                                        fontWeight = FontWeight.SemiBold,
                                        fontSize = 12.sp,
                                        color = Color(0xFFDCE3EC),
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                    )
                                }
                                Text(
                                    text = stringResource(R.string.onboarding_step_2_server_desc),
                                    fontSize = 11.sp,
                                    color = Color(0xFF93A0B2),
                                    maxLines = 2,
                                    overflow = TextOverflow.Ellipsis,
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}

/**
 * Step 3 visual: Framed localized desktop Settings screenshot with normalized focus overlays.
 */
@Composable
private fun DesktopQrStage(
    palette: StagePalette,
    modifier: Modifier = Modifier,
) {
    val transition = rememberInfiniteTransition(label = "desktop_qr_rings")
    val pulseState = transition.animateFloat(
        initialValue = 0f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(
            animation = tween(durationMillis = 2200, easing = LinearEasing),
            repeatMode = RepeatMode.Reverse,
        ),
        label = "ring_pulse",
    )

    StageFrame(
        palette = palette,
        modifier = modifier,
    ) {
        Surface(
            modifier = Modifier
                .widthIn(max = 356.dp)
                .fillMaxWidth()
                .align(Alignment.Center),
            shape = RoundedCornerShape(14.dp),
            color = Color(0xFFF7F7F7),
            border = BorderStroke(
                1.dp,
                if (palette.isLight) Color(0xFFD0D5DD) else Color.White.copy(alpha = 0.22f),
            ),
            shadowElevation = if (palette.isLight) 6.dp else 10.dp,
        ) {
            Column(modifier = Modifier.fillMaxWidth()) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(Color(0xFFECEEF2))
                        .padding(horizontal = 10.dp, vertical = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    TrafficLightDots(dotSize = 6.dp)
                    Text(
                        text = stringResource(R.string.onboarding_step_3_window_title),
                        fontSize = 10.5.sp,
                        fontWeight = FontWeight.SemiBold,
                        color = Color(0xFF2D333B),
                        textAlign = TextAlign.Center,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier
                            .weight(1f)
                            .padding(end = 24.dp),
                    )
                }

                // Pin screenshot overlay coordinates and callouts to physical LTR to match the raster image and Canvas rings.
                CompositionLocalProvider(LocalLayoutDirection provides LayoutDirection.Ltr) {
                    BoxWithConstraints(
                        modifier = Modifier
                            .fillMaxWidth()
                            .aspectRatio(OnboardingConfig.DESKTOP_SETTINGS_ASPECT_RATIO),
                    ) {
                        val stageWidth = maxWidth
                        val stageHeight = maxHeight

                        Image(
                            painter = painterResource(R.drawable.tutorial_desktop_settings),
                            contentDescription = stringResource(R.string.onboarding_step_3_screenshot_desc),
                            contentScale = ContentScale.FillBounds,
                            modifier = Modifier.fillMaxSize(),
                        )

                        Canvas(modifier = Modifier.fillMaxSize()) {
                            val pulse = pulseState.value
                            drawFocusRing(
                                rect = OnboardingConfig.DESKTOP_SETTINGS_LISTEN_RECT,
                                color = palette.warmOrange,
                                pulse = pulse,
                            )
                            drawFocusRing(
                                rect = OnboardingConfig.DESKTOP_SETTINGS_QR_RECT,
                                color = palette.focusBlue,
                                pulse = 1f - pulse,
                            )
                        }

                        // Callout (1) sits right next to DESKTOP_SETTINGS_LISTEN_RECT without touching 0.0.0.0 : 7683.
                        Box(
                            modifier = Modifier
                                .offset(
                                    x = stageWidth * 0.600f,
                                    y = stageHeight * 0.052f,
                                )
                                .width(stageWidth * 0.392f),
                            contentAlignment = Alignment.CenterStart,
                        ) {
                            CalloutPill(
                                text = stringResource(R.string.onboarding_step_3_callout_listen),
                                backgroundColor = palette.warmOrangePill,
                                contentColor = Color.White,
                                compact = true,
                                maxLines = 2,
                            )
                        }

                        // Callout (2) sits in the blank bottom-right zone and grows upward without clipping at high fontScale.
                        Box(
                            modifier = Modifier
                                .align(Alignment.BottomStart)
                                .padding(
                                    start = stageWidth * 0.405f,
                                    end = stageWidth * 0.02f,
                                    bottom = stageHeight * 0.045f,
                                ),
                            contentAlignment = Alignment.CenterStart,
                        ) {
                            CalloutPill(
                                text = stringResource(R.string.onboarding_step_3_callout_qr),
                                backgroundColor = palette.focusBluePill,
                                contentColor = Color.White,
                                compact = true,
                                maxLines = 2,
                            )
                        }
                    }
                }
            }
        }
    }
}

private fun DrawScope.drawFocusRing(
    rect: NormalizedRect,
    color: Color,
    pulse: Float,
) {
    val left = size.width * rect.left
    val top = size.height * rect.top
    val width = size.width * rect.width
    val height = size.height * rect.height
    val corner = CornerRadius(6.dp.toPx(), 6.dp.toPx())

    drawRoundRect(
        color = color.copy(alpha = 0.07f + 0.05f * pulse),
        topLeft = Offset(left, top),
        size = Size(width, height),
        cornerRadius = corner,
    )
    drawRoundRect(
        color = color.copy(alpha = 0.22f + 0.10f * pulse),
        topLeft = Offset(left - 2.dp.toPx(), top - 2.dp.toPx()),
        size = Size(width + 4.dp.toPx(), height + 4.dp.toPx()),
        cornerRadius = CornerRadius(8.dp.toPx(), 8.dp.toPx()),
        style = Stroke(width = 4.dp.toPx()),
    )
    drawRoundRect(
        color = color.copy(alpha = 0.90f),
        topLeft = Offset(left, top),
        size = Size(width, height),
        cornerRadius = corner,
        style = Stroke(width = 2.dp.toPx()),
    )
}

/**
 * Step 4 visual: QR viewfinder with sweeping scan laser transitioning into mobile terminal UI.
 */
@Composable
private fun ScanConnectStage(
    palette: StagePalette,
    modifier: Modifier = Modifier,
) {
    val primary = MaterialTheme.colorScheme.primary
    val transition = rememberInfiniteTransition(label = "qr_laser")
    val scanPositionState = transition.animateFloat(
        initialValue = 0.08f,
        targetValue = 0.92f,
        animationSpec = infiniteRepeatable(
            animation = tween(durationMillis = 2400, easing = LinearEasing),
            repeatMode = RepeatMode.Reverse,
        ),
        label = "scan_line_y",
    )

    StageFrame(
        palette = palette,
        modifier = modifier.heightIn(min = 264.dp),
    ) {
        // Keep scan-to-terminal diagram flow and POSIX shell prompts left-to-right under RTL locales.
        CompositionLocalProvider(LocalLayoutDirection provides LayoutDirection.Ltr) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .height(IntrinsicSize.Min)
                    .heightIn(min = 236.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                // Left: QR scanner viewfinder card
                Surface(
                    modifier = Modifier
                        .weight(1f)
                        .fillMaxHeight(),
                    shape = RoundedCornerShape(16.dp),
                    color = Color(0xFF11151B),
                    border = BorderStroke(1.dp, palette.liveGreen.copy(alpha = 0.35f)),
                    shadowElevation = if (palette.isLight) 6.dp else 2.dp,
                ) {
                    Column(
                        modifier = Modifier
                            .fillMaxSize()
                            .padding(10.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.SpaceBetween,
                    ) {
                        Box(
                            modifier = Modifier
                                .widthIn(max = 106.dp)
                                .fillMaxWidth()
                                .aspectRatio(1f)
                                .padding(2.dp),
                            contentAlignment = Alignment.Center,
                        ) {
                            Box(
                                modifier = Modifier
                                    .fillMaxSize()
                                    .padding(10.dp)
                                    .clip(RoundedCornerShape(8.dp))
                                    .background(Color.White)
                                    .padding(7.dp),
                            ) {
                                SampleQrMatrix(modifier = Modifier.fillMaxSize())
                            }
                            Canvas(modifier = Modifier.fillMaxSize()) {
                                drawViewfinderCorners(
                                    color = palette.liveGreen,
                                    cornerLength = 15.dp.toPx(),
                                    strokeWidth = 2.5.dp.toPx(),
                                )
                                val laserY = size.height * scanPositionState.value
                                val trailHeight = 20.dp.toPx()
                                drawRect(
                                    brush = Brush.verticalGradient(
                                        colors = listOf(
                                            Color.Transparent,
                                            palette.liveGreen.copy(alpha = 0.28f),
                                            Color.Transparent,
                                        ),
                                        startY = (laserY - trailHeight * 0.5f).coerceAtLeast(0f),
                                        endY = (laserY + trailHeight * 0.5f).coerceAtMost(size.height),
                                    ),
                                    topLeft = Offset(4.dp.toPx(), (laserY - trailHeight * 0.5f).coerceAtLeast(0f)),
                                    size = Size(size.width - 8.dp.toPx(), trailHeight),
                                )
                                drawLine(
                                    color = palette.liveGreen,
                                    start = Offset(4.dp.toPx(), laserY),
                                    end = Offset(size.width - 4.dp.toPx(), laserY),
                                    strokeWidth = 2.dp.toPx(),
                                    cap = StrokeCap.Round,
                                )
                            }
                        }

                        Spacer(Modifier.height(6.dp))

                        Text(
                            text = stringResource(R.string.onboarding_step_4_auto_fill),
                            fontSize = 10.sp,
                            lineHeight = 13.sp,
                            fontWeight = FontWeight.Medium,
                            color = Color(0xFFD2DCE8),
                            textAlign = TextAlign.Center,
                            maxLines = 3,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }

                // Center: Flow arrow
                Surface(
                    shape = CircleShape,
                    color = primary.copy(alpha = if (palette.isLight) 0.12f else 0.20f),
                    border = BorderStroke(1.dp, primary.copy(alpha = 0.35f)),
                    modifier = Modifier.size(28.dp),
                ) {
                    Box(contentAlignment = Alignment.Center) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Filled.ArrowForward,
                            contentDescription = null,
                            tint = primary,
                            modifier = Modifier.size(15.dp),
                        )
                    }
                }

                // Right: Connected Gateterm Android terminal preview
                Surface(
                    modifier = Modifier
                        .weight(1f)
                        .fillMaxHeight(),
                    shape = RoundedCornerShape(18.dp),
                    color = Color(0xFF191D24),
                    border = BorderStroke(
                        1.5.dp,
                        if (palette.isLight) Color(0xFF313844) else Color.White.copy(alpha = 0.16f),
                    ),
                    shadowElevation = if (palette.isLight) 6.dp else 2.dp,
                ) {
                    Column(
                        modifier = Modifier
                            .fillMaxSize()
                            .padding(6.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                    ) {
                        Box(
                            modifier = Modifier
                                .width(24.dp)
                                .height(3.dp)
                                .clip(CircleShape)
                                .background(Color.White.copy(alpha = 0.18f)),
                        )
                        Spacer(Modifier.height(6.dp))
                        Column(
                            modifier = Modifier
                                .fillMaxSize()
                                .clip(RoundedCornerShape(11.dp))
                                .background(Color(0xFF101318))
                                .padding(6.dp),
                            verticalArrangement = Arrangement.SpaceBetween,
                        ) {
                            Column(verticalArrangement = Arrangement.spacedBy(5.dp)) {
                                Row(
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(5.dp),
                                ) {
                                    Box(
                                        modifier = Modifier
                                            .size(6.dp)
                                            .clip(CircleShape)
                                            .background(palette.liveGreen),
                                    )
                                    Text(
                                        text = "~/octoterm",
                                        fontFamily = FontFamily.Monospace,
                                        fontWeight = FontWeight.SemiBold,
                                        fontSize = 9.sp,
                                        color = Color(0xFFE3EAF3),
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                    )
                                }
                                Text(
                                    text = "$ gateterm",
                                    fontFamily = FontFamily.Monospace,
                                    fontSize = 8.5.sp,
                                    color = Color(0xFF8AB4F8),
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                                Text(
                                    text = "✓ ${stringResource(R.string.connection_connected)}",
                                    fontFamily = FontFamily.Monospace,
                                    fontSize = 8.5.sp,
                                    color = palette.liveGreen,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                                TerminalBarLine(widthFraction = 0.78f, color = Color(0xFFB8C4D4))
                                Row(
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(3.dp),
                                ) {
                                    Text(
                                        text = "$",
                                        fontFamily = FontFamily.Monospace,
                                        fontSize = 8.5.sp,
                                        color = palette.liveGreen,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                    )
                                    Box(
                                        modifier = Modifier
                                            .width(5.dp)
                                            .height(9.dp)
                                            .background(Color(0xFFDCE4EE)),
                                    )
                                }
                            }

                            Spacer(Modifier.height(6.dp))

                            // Signature Gateterm terminal shortcut key bar
                            FlowRow(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.spacedBy(2.dp, Alignment.CenterHorizontally),
                                verticalArrangement = Arrangement.spacedBy(3.dp),
                            ) {
                                listOf("ESC", "TAB", "CTRL", "↑↓").forEach { key ->
                                    Box(
                                        modifier = Modifier
                                            .clip(RoundedCornerShape(4.dp))
                                            .background(Color(0xFF212630))
                                            .padding(horizontal = 3.5.dp, vertical = 2.5.dp),
                                        contentAlignment = Alignment.Center,
                                    ) {
                                        Text(
                                            text = key,
                                            fontFamily = FontFamily.Monospace,
                                            fontWeight = FontWeight.SemiBold,
                                            fontSize = 6.5.sp,
                                            color = Color(0xFFC8D2E0),
                                            softWrap = false,
                                            maxLines = 1,
                                            overflow = TextOverflow.Ellipsis,
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

private fun DrawScope.drawViewfinderCorners(
    color: Color,
    cornerLength: Float,
    strokeWidth: Float,
) {
    val half = strokeWidth / 2f
    val r = cornerLength
    val w = size.width
    val h = size.height
    val path = Path().apply {
        // Top-left
        moveTo(half, r)
        lineTo(half, half)
        lineTo(r, half)
        // Top-right
        moveTo(w - r, half)
        lineTo(w - half, half)
        lineTo(w - half, r)
        // Bottom-right
        moveTo(w - half, h - r)
        lineTo(w - half, h - half)
        lineTo(w - r, h - half)
        // Bottom-left
        moveTo(r, h - half)
        lineTo(half, h - half)
        lineTo(half, h - r)
    }
    drawPath(
        path = path,
        color = color,
        style = Stroke(width = strokeWidth, cap = StrokeCap.Round),
    )
}

@Composable
private fun SampleQrMatrix(modifier: Modifier = Modifier) {
    Canvas(modifier = modifier) {
        val grid = SAMPLE_QR_ROWS.size
        val cell = size.minDimension / grid
        val moduleColor = Color(0xFF14171C)
        for (row in 0 until grid) {
            val bits = SAMPLE_QR_ROWS[row]
            for (col in 0 until grid) {
                if (bits[col] == '#') {
                    drawRect(
                        color = moduleColor,
                        topLeft = Offset(col * cell, row * cell),
                        size = Size(cell + 0.4f, cell + 0.4f),
                    )
                }
            }
        }
    }
}

@Composable
private fun CalloutPill(
    text: String,
    backgroundColor: Color,
    contentColor: Color,
    modifier: Modifier = Modifier,
    compact: Boolean = false,
    maxLines: Int = 1,
) {
    Surface(
        modifier = modifier,
        shape = RoundedCornerShape(50),
        color = backgroundColor,
        border = BorderStroke(1.dp, Color.White.copy(alpha = 0.45f)),
        shadowElevation = 4.dp,
    ) {
        Text(
            text = text,
            color = contentColor,
            fontSize = if (compact) 8.8.sp else 9.5.sp,
            lineHeight = if (compact) 10.8.sp else 11.5.sp,
            fontWeight = FontWeight.SemiBold,
            maxLines = maxLines,
            overflow = TextOverflow.Ellipsis,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(
                horizontal = if (compact) 6.dp else 8.dp,
                vertical = if (compact) 3.dp else 3.5.dp,
            ),
        )
    }
}

@Composable
private fun TrafficLightDots(dotSize: Dp) {
    Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        listOf(Color(0xFFFF5F56), Color(0xFFFFBD2E), Color(0xFF27C93F)).forEach { color ->
            Box(
                modifier = Modifier
                    .size(dotSize)
                    .clip(CircleShape)
                    .background(color),
            )
        }
    }
}

@Composable
private fun TerminalBarLine(widthFraction: Float, color: Color) {
    Box(
        modifier = Modifier
            .fillMaxWidth(widthFraction)
            .height(3.5.dp)
            .clip(CircleShape)
            .background(color),
    )
}

// Deterministic 15x15 decorative QR glyph with 3 authentic finder squares for the scanner stage.
private val SAMPLE_QR_ROWS = listOf(
    "#####.##..#####",
    "#...#..#..#...#",
    "#.#.#.#.#.#.#.#",
    "#...#..##.#...#",
    "#####.#.#.#####",
    "......#........",
    ".###.##.#.###.#",
    "#.##..###.##...",
    "..##.#.##..###.",
    "......#.#..#.#.",
    "#####..###.#..#",
    "#...#.#..#..###",
    "#.#.#..###.#.#.",
    "#...#.#.#..##.#",
    "#####..#.##...#",
)
