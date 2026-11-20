package ai.eclosion.gateterm.android.onboarding

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.content.Intent
import android.net.Uri
import java.util.Locale

/**
 * Safe external link and system share-sheet dispatchers for onboarding.
 */
object OnboardingLinks {
    fun sharePayload(
        shareTemplate: String,
        url: String = OnboardingConfig.RELEASES_URL,
    ): String {
        val formatted = runCatching {
            String.format(Locale.ROOT, shareTemplate, url)
        }.getOrNull()
        return when {
            formatted != null && formatted.contains(url) -> formatted
            else -> "$shareTemplate $url".trim()
        }
    }

    fun dispatchSafely(action: () -> Unit): Boolean =
        runCatching(action).isSuccess

    fun openReleasesUrl(
        context: Context,
        url: String = OnboardingConfig.RELEASES_URL,
    ): Boolean = dispatchSafely {
        val intent = Intent(Intent.ACTION_VIEW, Uri.parse(url)).apply {
            if (!context.hasActivityContext()) {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
        }
        context.startActivity(intent)
    }

    fun shareReleasesUrl(
        context: Context,
        shareText: String,
        chooserTitle: String,
    ): Boolean = dispatchSafely {
        val sendIntent = Intent(Intent.ACTION_SEND).apply {
            type = "text/plain"
            putExtra(Intent.EXTRA_TEXT, shareText)
        }
        val chooser = Intent.createChooser(sendIntent, chooserTitle).apply {
            if (!context.hasActivityContext()) {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
        }
        context.startActivity(chooser)
    }

    private tailrec fun Context.hasActivityContext(): Boolean = when (this) {
        is Activity -> true
        is ContextWrapper -> baseContext.hasActivityContext()
        else -> false
    }
}
