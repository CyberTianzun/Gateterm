package ai.eclosion.gateterm.android.ui.cloud

import ai.eclosion.gateterm.android.R
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AccountCircle
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp

@Composable
fun CloudAccountButton(email: String?, onClick: () -> Unit) {
    val signedIn = email != null
    val tint = if (signedIn) MaterialTheme.colorScheme.primary else LocalContentColor.current
    val description = if (email == null) stringResource(R.string.cloud_not_connected)
        else stringResource(R.string.cloud_connected_as, email)
    IconButton(onClick = onClick) {
        Box(Modifier.size(30.dp)) {
            Icon(Icons.Filled.AccountCircle, description, tint = tint, modifier = Modifier.size(26.dp))
            Icon(
                if (signedIn) Icons.Filled.Check else Icons.Filled.Close,
                contentDescription = null,
                tint = tint,
                modifier = Modifier.align(Alignment.BottomEnd).size(15.dp)
                    .background(MaterialTheme.colorScheme.surface, CircleShape).padding(1.dp),
            )
        }
    }
}
