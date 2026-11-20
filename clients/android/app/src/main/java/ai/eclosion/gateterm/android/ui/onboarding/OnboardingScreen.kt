package ai.eclosion.gateterm.android.ui.onboarding

import ai.eclosion.gateterm.android.R
import ai.eclosion.gateterm.android.appearance.isLightTheme
import ai.eclosion.gateterm.android.onboarding.OnboardingConfig
import ai.eclosion.gateterm.android.onboarding.OnboardingGate
import ai.eclosion.gateterm.android.onboarding.OnboardingLinks
import ai.eclosion.gateterm.android.ui.theme.LocalTermAppearance
import androidx.activity.compose.BackHandler
import androidx.annotation.StringRes
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
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
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.ArrowForward
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.Button
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

@Immutable
private data class OnboardingStepCopy(
    @StringRes val badgeRes: Int,
    @StringRes val titleRes: Int,
    @StringRes val bodyRes: Int,
)

private val ONBOARDING_STEPS = listOf(
    OnboardingStepCopy(
        badgeRes = R.string.onboarding_step_1_badge,
        titleRes = R.string.onboarding_step_1_title,
        bodyRes = R.string.onboarding_step_1_body,
    ),
    OnboardingStepCopy(
        badgeRes = R.string.onboarding_step_2_badge,
        titleRes = R.string.onboarding_step_2_title,
        bodyRes = R.string.onboarding_step_2_body,
    ),
    OnboardingStepCopy(
        badgeRes = R.string.onboarding_step_3_badge,
        titleRes = R.string.onboarding_step_3_title,
        bodyRes = R.string.onboarding_step_3_body,
    ),
    OnboardingStepCopy(
        badgeRes = R.string.onboarding_step_4_badge,
        titleRes = R.string.onboarding_step_4_title,
        bodyRes = R.string.onboarding_step_4_body,
    ),
)

private val EMPHASIZED_BODY_TOKENS = listOf(
    "Gateterm-Desktop-Web",
    "Gateterm-Server",
    "gateterm-desktop-web",
    "gateterm-server",
    "0.0.0.0",
)

/**
 * Four-step editorial onboarding carousel explaining host architecture, installation, QR setup, and pairing.
 */
@Composable
fun OnboardingScreen(
    isFirstLaunch: Boolean,
    onCopyReleaseUrl: () -> Unit,
    onScanNow: () -> Unit,
    onFinish: () -> Unit,
) {
    val pagerState = rememberPagerState(pageCount = { OnboardingConfig.STEP_COUNT })
    val scope = rememberCoroutineScope()
    val isLight = LocalTermAppearance.current.isLightTheme()
    val isLastStep = pagerState.currentPage == OnboardingConfig.STEP_COUNT - 1

    BackHandler {
        val previous = OnboardingGate.previousPageOrExit(pagerState.currentPage)
        if (previous != null) {
            scope.launch { pagerState.animateScrollToPage(previous) }
        } else {
            onFinish()
        }
    }

    Surface(
        modifier = Modifier.fillMaxSize(),
        color = MaterialTheme.colorScheme.background,
    ) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .statusBarsPadding()
                .navigationBarsPadding(),
            contentAlignment = Alignment.TopCenter,
        ) {
            Column(
                modifier = Modifier
                    .widthIn(max = 480.dp)
                    .fillMaxSize(),
            ) {
                OnboardingTopBar(
                    isFirstLaunch = isFirstLaunch,
                    isLastStep = isLastStep,
                    onDismiss = onFinish,
                )

                HorizontalPager(
                    state = pagerState,
                    modifier = Modifier
                        .fillMaxWidth()
                        .weight(1f),
                ) { page ->
                    OnboardingPageContent(
                        page = page,
                        step = ONBOARDING_STEPS[page],
                        isLight = isLight,
                        onCopyReleaseUrl = onCopyReleaseUrl,
                    )
                }

                OnboardingBottomBar(
                    currentPage = pagerState.currentPage,
                    isFirstLaunch = isFirstLaunch,
                    onPrevious = {
                        val previous = OnboardingGate.previousPageOrExit(pagerState.currentPage)
                        if (previous != null) {
                            scope.launch { pagerState.animateScrollToPage(previous) }
                        }
                    },
                    onNext = {
                        val next = (pagerState.currentPage + 1).coerceAtMost(OnboardingConfig.STEP_COUNT - 1)
                        scope.launch { pagerState.animateScrollToPage(next) }
                    },
                    onScanNow = onScanNow,
                    onFinish = onFinish,
                )
            }
        }
    }
}

@Composable
private fun OnboardingTopBar(
    isFirstLaunch: Boolean,
    isLastStep: Boolean,
    onDismiss: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 52.dp)
            .padding(horizontal = 20.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Box(
                modifier = Modifier
                    .size(30.dp)
                    .clip(RoundedCornerShape(8.dp))
                    .background(Color.Black)
                    .border(1.dp, MaterialTheme.colorScheme.outlineVariant, RoundedCornerShape(8.dp)),
                contentAlignment = Alignment.Center,
            ) {
                Image(
                    painter = painterResource(R.mipmap.ic_launcher_foreground),
                    contentDescription = null,
                    contentScale = ContentScale.Crop,
                    modifier = Modifier
                        .fillMaxSize()
                        .scale(1.26f),
                )
            }
            Text(
                text = stringResource(R.string.app_name),
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
                color = MaterialTheme.colorScheme.onBackground,
            )
        }

        if (!isFirstLaunch) {
            TextButton(onClick = onDismiss) {
                Icon(
                    imageVector = Icons.Filled.Close,
                    contentDescription = null,
                    modifier = Modifier.size(16.dp),
                )
                Spacer(Modifier.width(4.dp))
                Text(stringResource(R.string.onboarding_close))
            }
        } else {
            AnimatedVisibility(
                visible = !isLastStep,
                enter = fadeIn(),
                exit = fadeOut(),
            ) {
                TextButton(onClick = onDismiss) {
                    Text(stringResource(R.string.onboarding_skip))
                }
            }
        }
    }
}

@Composable
private fun OnboardingPageContent(
    page: Int,
    step: OnboardingStepCopy,
    isLight: Boolean,
    onCopyReleaseUrl: () -> Unit,
) {
    val bodyText = stringResource(step.bodyRes)
    val emphasisColor = MaterialTheme.colorScheme.onSurface
    val emphasizedBody = remember(bodyText, emphasisColor) {
        buildEmphasizedBodyText(
            text = bodyText,
            tokens = EMPHASIZED_BODY_TOKENS,
            emphasisStyle = SpanStyle(
                fontWeight = FontWeight.SemiBold,
                color = emphasisColor,
            ),
        )
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp, vertical = 4.dp),
    ) {
        OnboardingVisualStage(
            step = page,
            modifier = Modifier.fillMaxWidth(),
        )

        Spacer(Modifier.height(18.dp))

        StepBadge(
            text = stringResource(step.badgeRes),
            isLight = isLight,
        )

        Spacer(Modifier.height(10.dp))

        Text(
            text = stringResource(step.titleRes),
            style = MaterialTheme.typography.headlineSmall,
            fontWeight = FontWeight.Bold,
            color = MaterialTheme.colorScheme.onBackground,
        )

        Spacer(Modifier.height(8.dp))

        Text(
            text = emphasizedBody,
            style = MaterialTheme.typography.bodyMedium,
            lineHeight = 22.sp,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )

        if (page == 1) {
            Spacer(Modifier.height(14.dp))
            ReleasesActionCard(onCopyReleaseUrl = onCopyReleaseUrl)
        }

        Spacer(Modifier.height(10.dp))
    }
}

@Composable
private fun StepBadge(
    text: String,
    isLight: Boolean,
) {
    val primary = MaterialTheme.colorScheme.primary
    Surface(
        shape = RoundedCornerShape(50),
        color = primary.copy(alpha = if (isLight) 0.10f else 0.16f),
        border = BorderStroke(1.dp, primary.copy(alpha = 0.28f)),
    ) {
        Text(
            text = text,
            fontFamily = FontFamily.Monospace,
            fontSize = 11.sp,
            fontWeight = FontWeight.SemiBold,
            letterSpacing = 0.4.sp,
            color = primary,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.padding(horizontal = 11.dp, vertical = 5.dp),
        )
    }
}

@Composable
private fun ReleasesActionCard(
    onCopyReleaseUrl: () -> Unit,
) {
    val context = LocalContext.current
    val shareTemplate = stringResource(R.string.onboarding_share_text)
    val shareTitle = stringResource(R.string.onboarding_share_link)
    var copiedRecently by remember { mutableStateOf(false) }
    val triggerCopy: () -> Unit = {
        copiedRecently = true
        onCopyReleaseUrl()
    }

    LaunchedEffect(copiedRecently) {
        if (copiedRecently) {
            delay(1500L)
            copiedRecently = false
        }
    }

    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(16.dp),
        color = MaterialTheme.colorScheme.surface,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
    ) {
        Column(modifier = Modifier.fillMaxWidth()) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable(
                        onClickLabel = stringResource(R.string.onboarding_open_link),
                    ) {
                        if (!OnboardingLinks.openReleasesUrl(context)) {
                            triggerCopy()
                        }
                    }
                    .padding(horizontal = 14.dp, vertical = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                Text(
                    text = OnboardingConfig.RELEASES_DISPLAY_URL,
                    fontFamily = FontFamily.Monospace,
                    fontSize = 12.sp,
                    fontWeight = FontWeight.Medium,
                    color = MaterialTheme.colorScheme.primary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                Icon(
                    painter = painterResource(R.drawable.ic_open_in_new),
                    contentDescription = stringResource(R.string.onboarding_open_link),
                    tint = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.size(18.dp),
                )
            }

            HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.6f))

            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 12.dp, vertical = 10.dp),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                FilledTonalButton(
                    onClick = triggerCopy,
                    shape = RoundedCornerShape(50),
                    contentPadding = PaddingValues(horizontal = 14.dp, vertical = 8.dp),
                    modifier = Modifier.weight(1f),
                ) {
                    if (copiedRecently) {
                        Icon(
                            imageVector = Icons.Filled.Check,
                            contentDescription = null,
                            modifier = Modifier.size(16.dp),
                        )
                    } else {
                        Icon(
                            painter = painterResource(R.drawable.ic_content_copy),
                            contentDescription = null,
                            modifier = Modifier.size(16.dp),
                        )
                    }
                    Spacer(Modifier.width(6.dp))
                    Text(
                        text = stringResource(R.string.onboarding_copy_link),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }

                OutlinedButton(
                    onClick = {
                        val payload = OnboardingLinks.sharePayload(shareTemplate)
                        if (!OnboardingLinks.shareReleasesUrl(context, payload, shareTitle)) {
                            triggerCopy()
                        }
                    },
                    shape = RoundedCornerShape(50),
                    contentPadding = PaddingValues(horizontal = 14.dp, vertical = 8.dp),
                    modifier = Modifier.weight(1f),
                ) {
                    Icon(
                        imageVector = Icons.Filled.Share,
                        contentDescription = null,
                        modifier = Modifier.size(16.dp),
                    )
                    Spacer(Modifier.width(6.dp))
                    Text(
                        text = stringResource(R.string.onboarding_share_link),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
    }
}

@Composable
private fun OnboardingBottomBar(
    currentPage: Int,
    isFirstLaunch: Boolean,
    onPrevious: () -> Unit,
    onNext: () -> Unit,
    onScanNow: () -> Unit,
    onFinish: () -> Unit,
) {
    val isLastStep = currentPage == OnboardingConfig.STEP_COUNT - 1

    Box(
        modifier = Modifier
            .fillMaxWidth()
            .animateContentSize()
            .padding(horizontal = 20.dp, vertical = 14.dp),
    ) {
        if (!isLastStep) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                PageIndicator(
                    pageCount = OnboardingConfig.STEP_COUNT,
                    currentPage = currentPage,
                )

                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    if (currentPage > 0) {
                        TextButton(onClick = onPrevious) {
                            Icon(
                                imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                                contentDescription = null,
                                modifier = Modifier.size(16.dp),
                            )
                            Spacer(Modifier.width(4.dp))
                            Text(stringResource(R.string.onboarding_prev))
                        }
                    }

                    Button(
                        onClick = onNext,
                        shape = RoundedCornerShape(50),
                        contentPadding = PaddingValues(horizontal = 18.dp, vertical = 10.dp),
                    ) {
                        Text(
                            text = stringResource(R.string.onboarding_next),
                            fontWeight = FontWeight.SemiBold,
                        )
                        Spacer(Modifier.width(6.dp))
                        Icon(
                            imageVector = Icons.AutoMirrored.Filled.ArrowForward,
                            contentDescription = null,
                            modifier = Modifier.size(16.dp),
                        )
                    }
                }
            }
        } else {
            Column(
                modifier = Modifier.fillMaxWidth(),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Button(
                    onClick = onScanNow,
                    shape = RoundedCornerShape(50),
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = 48.dp),
                ) {
                    Icon(
                        painter = painterResource(R.drawable.ic_qr_code_scanner),
                        contentDescription = null,
                        modifier = Modifier.size(18.dp),
                    )
                    Spacer(Modifier.width(8.dp))
                    Text(
                        text = stringResource(R.string.onboarding_scan_now),
                        fontWeight = FontWeight.SemiBold,
                    )
                }

                TextButton(
                    onClick = onFinish,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(
                        text = stringResource(
                            if (isFirstLaunch) R.string.onboarding_enter_app else R.string.onboarding_done,
                        ),
                    )
                }
            }
        }
    }
}

@Composable
private fun PageIndicator(
    pageCount: Int,
    currentPage: Int,
) {
    Row(
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        for (index in 0 until pageCount) {
            val active = index == currentPage
            val width by animateDpAsState(
                targetValue = if (active) 22.dp else 6.dp,
                label = "onboarding_indicator_width",
            )
            Box(
                modifier = Modifier
                    .height(6.dp)
                    .width(width)
                    .clip(CircleShape)
                    .background(
                        if (active) MaterialTheme.colorScheme.primary
                        else MaterialTheme.colorScheme.outlineVariant,
                    ),
            )
        }
    }
}

private fun buildEmphasizedBodyText(
    text: String,
    tokens: List<String>,
    emphasisStyle: SpanStyle,
): AnnotatedString = buildAnnotatedString {
    append(text)
    val orderedTokens = tokens.sortedByDescending { it.length }
    var cursor = 0
    while (cursor < text.length) {
        val nextMatch = orderedTokens
            .mapNotNull { token ->
                val index = text.indexOf(token, startIndex = cursor)
                if (index >= 0) index to token else null
            }
            .minWithOrNull(compareBy<Pair<Int, String>> { it.first }.thenByDescending { it.second.length })
            ?: break
        val (start, token) = nextMatch
        addStyle(emphasisStyle, start, start + token.length)
        cursor = start + token.length
    }
}
