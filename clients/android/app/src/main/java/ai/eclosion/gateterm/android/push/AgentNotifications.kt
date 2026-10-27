package ai.eclosion.gateterm.android.push

import ai.eclosion.gateterm.android.MainActivity
import ai.eclosion.gateterm.android.R
import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat

object AgentNotifications {
    const val CHANNEL = "agent_waiting"
    const val EXTRA_SERVER_ID = "gateterm.server_id"
    const val EXTRA_SESSION = "gateterm.session"
    const val EXTRA_AGENT_ID = "gateterm.agent_id"

    /** 按需创建(不用 Cloud 的用户不该在系统设置里看到这个渠道)。重复调用是幂等的。 */
    fun ensureChannel(context: Context) {
        val nm = context.getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL, context.getString(R.string.notif_channel_name), NotificationManager.IMPORTANCE_HIGH),
        )
    }

    /** 通知是否真能弹出来:App 级开关,以及(已创建时)agent_waiting 渠道没被用户关掉。 */
    fun enabled(context: Context): Boolean {
        val nm = NotificationManagerCompat.from(context)
        if (!nm.areNotificationsEnabled()) return false
        val channel = nm.getNotificationChannel(CHANNEL) ?: return true
        return channel.importance != NotificationManager.IMPORTANCE_NONE
    }

    fun cancel(context: Context, tag: String) {
        NotificationManagerCompat.from(context).cancel(tag, AgentPush.notificationId(tag))
    }

    fun handle(context: Context, push: AgentPush) {
        val nm = NotificationManagerCompat.from(context)
        val timeout = when (val display = push.display(System.currentTimeMillis() / 1000)) {
            AgentPush.Display.Cancel -> {
                nm.cancel(push.tag, push.notificationId)
                return
            }
            is AgentPush.Display.Post -> display.timeoutMillis
        }
        if (Build.VERSION.SDK_INT >= 33 &&
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) return
        ensureChannel(context)

        val name = agentDisplayName(push.agentId)
        val title = when (push.kind) {
            AgentPush.Kind.Permission -> context.getString(
                if (push.toolName in setOf("AskUserQuestion", "request_user_input")) R.string.notif_question_title
                else R.string.notif_permission_title, name)
            AgentPush.Kind.Done -> context.getString(R.string.notif_done_title, name)
            else -> context.getString(R.string.notif_waiting_title, name)
        }
        val body = push.toolName?.takeIf { push.kind == AgentPush.Kind.Permission }
            ?.let { context.getString(R.string.notif_permission_body, it) }
            ?: context.getString(R.string.notif_tap_to_open)
        val open = Intent(context, MainActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
            // 每个 tag 一个独立的 data Uri:PendingIntent 按 Intent.filterEquals 复用,request code
            // 只是 hashCode,碰撞时两条通知会共用一个 PendingIntent、跳到同一个会话。
            .setData(Uri.parse("gateterm://push/" + Uri.encode(push.tag)))
            .putExtra(EXTRA_SERVER_ID, push.serverId)
            .putExtra(EXTRA_SESSION, push.session)
            .putExtra(EXTRA_AGENT_ID, push.agentId)
        val pending = PendingIntent.getActivity(
            context, push.notificationId, open, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        // 不 setAutoCancel:点了但没能打开(没连过该服务器、会话已没了)时通知要留着;
        // 真打开会话后由 ConnectionViewModel 按 tag 撤回。
        val builder = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(title)
            .setContentText(body)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
            .setContentIntent(pending)
        timeout?.let { builder.setTimeoutAfter(it) }
        nm.notify(push.tag, push.notificationId, builder.build())
    }
}
