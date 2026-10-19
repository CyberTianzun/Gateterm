package ai.eclosion.octoterm.android.push

import android.content.Intent

/** 点通知带进 MainActivity 的跳转目标。纯数据,便于单测。 */
data class NotificationOpen(val serverId: String, val session: Long, val agentId: String?) {
    companion object {
        /**
         * 从 Intent 的 flags 与 extras 取出目标。从"最近任务"重新进入时系统会把当初的 Intent 原样再送一遍,
         * 那不是用户在点通知,必须忽略。
         */
        fun from(flags: Int, serverId: String?, session: Long, agentId: String?): NotificationOpen? {
            if ((flags and Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY) != 0) return null
            if (serverId.isNullOrEmpty() || session < 0) return null
            return NotificationOpen(serverId, session, agentId)
        }
    }
}
