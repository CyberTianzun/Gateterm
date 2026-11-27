package ai.eclosion.gateterm.android.update

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri

object StoreLinks {
    const val PACKAGE_ID = "ai.eclosion.gateterm.android"

    fun marketUri(packageId: String = PACKAGE_ID): String =
        "market://details?id=$packageId"

    fun webUrl(packageId: String = PACKAGE_ID): String =
        "https://play.google.com/store/apps/details?id=$packageId"

    fun openGooglePlay(context: Context, packageId: String = PACKAGE_ID) {
        try {
            context.startActivity(viewIntent(context, marketUri(packageId)))
        } catch (_: ActivityNotFoundException) {
            runCatching {
                context.startActivity(viewIntent(context, webUrl(packageId)))
            }
        }
    }

    private fun viewIntent(context: Context, url: String): Intent =
        Intent(Intent.ACTION_VIEW, Uri.parse(url)).apply {
            if (context !is Activity) addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        }
}
