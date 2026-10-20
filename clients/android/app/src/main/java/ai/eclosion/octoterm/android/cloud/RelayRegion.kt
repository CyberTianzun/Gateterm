package ai.eclosion.octoterm.android.cloud

import ai.eclosion.octoterm.android.R
import androidx.annotation.StringRes

enum class RelayRegion(val url: String, @StringRes val label: Int, val deployed: Boolean) {
    China("https://octoterm.signal.top/", R.string.cloud_region_china, true),
    NorthAmerica("https://octoterm.eclosion.ai/", R.string.cloud_region_north_america, false),
    Global("https://global.octoterm.eclosion.ai/", R.string.cloud_region_global, false);

    companion object {
        fun fromUrl(url: String): RelayRegion = entries.firstOrNull { it.url.trimEnd('/') == url.trim().trimEnd('/') } ?: China
    }
}
