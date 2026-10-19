package ai.eclosion.octoterm.android

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.core.view.WindowCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import ai.eclosion.octoterm.android.appearance.isLightTheme
import ai.eclosion.octoterm.android.push.AgentNotifications
import ai.eclosion.octoterm.android.push.NotificationOpen
import ai.eclosion.octoterm.android.ui.connections.ConnectionViewModel
import ai.eclosion.octoterm.android.ui.theme.OctotermTheme

class MainActivity : ComponentActivity() {
    private val connectionViewModel: ConnectionViewModel by viewModels { ConnectionViewModel.Factory }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        if (savedInstanceState == null) handleIntent(intent)
        setContent {
            val state by connectionViewModel.state.collectAsStateWithLifecycle()
            val prefersDark = isSystemInDarkTheme()
            LaunchedEffect(prefersDark) {
                connectionViewModel.onSystemThemeChanged(prefersDark)
            }
            SideEffect {
                WindowCompat.getInsetsController(window, window.decorView).apply {
                    isAppearanceLightStatusBars = state.appearance.isLightTheme()
                    isAppearanceLightNavigationBars = state.appearance.isLightTheme()
                }
            }
            OctotermTheme(appearance = state.appearance) {
                OctotermApp(connectionViewModel)
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleIntent(intent)
    }

    private fun handleIntent(intent: Intent?) {
        intent ?: return
        val target = NotificationOpen.from(
            flags = intent.flags,
            serverId = intent.getStringExtra(AgentNotifications.EXTRA_SERVER_ID),
            session = intent.getLongExtra(AgentNotifications.EXTRA_SESSION, -1L),
            agentId = intent.getStringExtra(AgentNotifications.EXTRA_AGENT_ID),
        ) ?: return
        // 只处理一次:配置变更重建时不要重复跳转
        intent.removeExtra(AgentNotifications.EXTRA_SERVER_ID)
        connectionViewModel.openFromNotification(target.serverId, target.session, target.agentId)
    }
}
