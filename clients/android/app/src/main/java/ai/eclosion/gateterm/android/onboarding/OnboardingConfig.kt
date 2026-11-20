package ai.eclosion.gateterm.android.onboarding

/**
 * Axis-aligned rectangle in normalized `[0, 1]` image coordinates.
 */
data class NormalizedRect(
    val left: Float,
    val top: Float,
    val width: Float,
    val height: Float,
) {
    val right: Float get() = left + width
    val bottom: Float get() = top + height

    fun isValid(): Boolean =
        left >= 0f && top >= 0f && width > 0f && height > 0f && right <= 1f && bottom <= 1f

    fun overlaps(other: NormalizedRect): Boolean =
        left < other.right && right > other.left && top < other.bottom && bottom > other.top
}

/**
 * Constants and highlight geometry for the first-launch onboarding tutorial.
 */
object OnboardingConfig {
    const val CURRENT_VERSION = 1
    const val RELEASES_URL = "https://github.com/CyberTianzun/Gateterm/releases/latest"
    const val RELEASES_DISPLAY_URL = "github.com/CyberTianzun/Gateterm/releases/latest"
    const val STEP_COUNT = 4
    const val DESKTOP_SETTINGS_ASPECT_RATIO = 1120f / 880f

    // Pixel coordinates locked to the deterministic 1120x880 desktop settings illustration.
    val DESKTOP_SETTINGS_LISTEN_RECT = NormalizedRect(
        left = 0.215f,
        top = 0.068f,
        width = 0.375f,
        height = 0.072f,
    )

    val DESKTOP_SETTINGS_QR_RECT = NormalizedRect(
        left = 0.048f,
        top = 0.480f,
        width = 0.340f,
        height = 0.432f,
    )
}
