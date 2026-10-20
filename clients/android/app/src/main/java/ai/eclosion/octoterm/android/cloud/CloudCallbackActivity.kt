package ai.eclosion.octoterm.android.cloud

import ai.eclosion.octoterm.android.MainActivity
import android.app.Activity
import android.content.Intent
import android.os.Bundle

/** Return from the browser to the existing application task. Auth proof is validated by the ViewModel. */
class CloudCallbackActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val callback = intent?.data
        if (callback != null && isCloudCallback(callback.toString())) {
            startActivity(Intent(this, MainActivity::class.java).apply {
                action = Intent.ACTION_VIEW
                data = callback
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP
            })
        }
        finish()
    }
}
