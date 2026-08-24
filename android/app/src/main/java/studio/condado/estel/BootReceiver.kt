package studio.condado.estel

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import androidx.core.content.ContextCompat

/** Starts EstelService after device boot (or after app update). */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val action = intent.action ?: return
        if (action != Intent.ACTION_BOOT_COMPLETED && action != Intent.ACTION_MY_PACKAGE_REPLACED) return

        val cfg = Config.from(context)
        if (!cfg.enabled) return

        ContextCompat.startForegroundService(context, Intent(context, EstelService::class.java))
    }
}
