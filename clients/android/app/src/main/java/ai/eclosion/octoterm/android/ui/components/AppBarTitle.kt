package ai.eclosion.octoterm.android.ui.components

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.sp

@Composable
fun AppBarTitle(
    text: String,
    modifier: Modifier = Modifier,
    maxLines: Int = Int.MAX_VALUE,
    overflow: TextOverflow = TextOverflow.Clip,
) {
    val titleStyle = MaterialTheme.typography.titleLarge
    Text(
        text = text,
        modifier = modifier,
        style = titleStyle.copy(fontSize = (titleStyle.fontSize.value - 3).sp),
        maxLines = maxLines,
        overflow = overflow,
    )
}
