package ai.eclosion.octoterm.android.push

import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage

class OctoMessagingService : FirebaseMessagingService() {
    override fun onNewToken(token: String) = PushRegistrar.onNewToken(applicationContext, token)

    override fun onMessageReceived(message: RemoteMessage) {
        if (!PushRegistrar.acceptsMessage(applicationContext, "fcm")) return
        val push = AgentPush.parse(message.data) ?: return
        AgentNotifications.handle(applicationContext, push)
    }
}
