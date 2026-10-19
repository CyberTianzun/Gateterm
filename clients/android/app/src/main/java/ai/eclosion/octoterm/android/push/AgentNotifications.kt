package ai.eclosion.octoterm.android.push

import ai.eclosion.octoterm.android.MainActivity
import ai.eclosion.octoterm.android.R
import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat

object AgentNotifications {
    const val CHANNEL = "agent_waiting"
    const val EXTRA_SERVER_ID = "octoterm.server_id"
    const val EXTRA_SESSION = "octoterm.session"

    fun ensureChannel(context: Context) {
        if (Build.VERSION.SDK_INT < 26) return
        val nm = context.getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL, context.getString(R.string.notif_channel_name), NotificationManager.IMPORTANCE_HIGH),
        )
    }

    fun handle(context: Context, push: AgentPush) {
        val nm = NotificationManagerCompat.from(context)
        if (push.kind == AgentPush.Kind.Resolved) {
            nm.cancel(push.tag, push.notificationId)
            return
        }
        if (Build.VERSION.SDK_INT >= 33 &&
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) return

        val name = agentDisplayName(push.agentId)
        val title = when (push.kind) {
            AgentPush.Kind.Permission -> context.getString(R.string.notif_permission_title, name)
            else -> context.getString(R.string.notif_waiting_title, name)
        }
        val body = push.toolName?.takeIf { push.kind == AgentPush.Kind.Permission }
            ?.let { context.getString(R.string.notif_permission_body, it) }
            ?: context.getString(R.string.notif_tap_to_open)
        val open = Intent(context, MainActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
            .putExtra(EXTRA_SERVER_ID, push.serverId)
            .putExtra(EXTRA_SESSION, push.session)
        val pending = PendingIntent.getActivity(
            context, push.notificationId, open, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val builder = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(title)
            .setContentText(body)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
            .setAutoCancel(true)
            .setContentIntent(pending)
        push.timeoutMillis(System.currentTimeMillis() / 1000)?.let { builder.setTimeoutAfter(it) }
        nm.notify(push.tag, push.notificationId, builder.build())
    }
}
