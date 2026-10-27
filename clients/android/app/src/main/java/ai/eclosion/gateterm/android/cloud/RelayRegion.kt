package ai.eclosion.gateterm.android.cloud

import ai.eclosion.gateterm.android.R
import androidx.annotation.StringRes

enum class RelayRegion(val url: String, @StringRes val label: Int, val deployed: Boolean) {
    China("https://gateterm.signal.top/", R.string.cloud_region_china, true),
    NorthAmerica("https://gateterm.eclosion.ai/", R.string.cloud_region_north_america, false),
    Global("https://global.gateterm.eclosion.ai/", R.string.cloud_region_global, false);

    companion object {
        fun fromUrl(url: String): RelayRegion = entries.firstOrNull { it.url.trimEnd('/') == url.trim().trimEnd('/') } ?: China
    }
}
