package ai.eclosion.gateterm.android.ui.components

import ai.eclosion.gateterm.android.R
import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.layout.Column
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource

@Composable
fun PrivacyLinks() {
    val context = LocalContext.current
    Column {
        listOf(
            R.string.privacy_policy to "https://gateterm.signal.top/privacy",
            R.string.account_deletion_information to "https://gateterm.signal.top/account-deletion",
            R.string.aliyun_privacy_policy to "https://terms.aliyun.com/legal-agreement/terms/suit_bu1_ali_cloud/suit_bu1_ali_cloud202112071754_83380.html",
        ).forEach { (label, url) ->
            TextButton(onClick = {
                runCatching { context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url))) }
            }) { Text(stringResource(label)) }
        }
    }
}
