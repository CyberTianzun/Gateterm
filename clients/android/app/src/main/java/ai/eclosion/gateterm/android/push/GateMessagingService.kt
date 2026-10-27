package ai.eclosion.gateterm.android.push

import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage

class GateMessagingService : FirebaseMessagingService() {
    override fun onNewToken(token: String) = PushRegistrar.onNewToken(applicationContext, token)

    override fun onMessageReceived(message: RemoteMessage) {
        if (!PushRegistrar.acceptsMessage(applicationContext, "fcm")) return
        val push = AgentPush.parse(message.data) ?: return
        AgentNotifications.handle(applicationContext, push)
    }
}
