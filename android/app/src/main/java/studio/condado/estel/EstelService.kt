package studio.condado.estel

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.provider.Settings
import androidx.core.app.NotificationCompat
import java.time.LocalDate
import java.time.LocalTime

/**
 * Foreground service that keeps the circadian engine running.
 *
 * Tick every 60 seconds, recomputes [Target], attenuates by [Intensity],
 * then applies the warm-tint overlay.
 *
 * Platform rule: never surface medical/clinical/alarming content.
 */
class EstelService : Service() {

    private lateinit var overlay: OverlayManager
    private lateinit var cfg: Config
    private val handler = Handler(Looper.getMainLooper())

    private val tick = object : Runnable {
        override fun run() {
            applyTarget()
            handler.postDelayed(this, TICK_MS)
        }
    }

    override fun onCreate() {
        super.onCreate()
        cfg = Config.from(this)
        overlay = OverlayManager(this)
        ensureNotificationChannel()
        startForeground(NOTIF_ID, buildNotification())
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Re-read config on each start so intensity changes from MainActivity
        // take effect without restarting the service.
        cfg = Config.from(this)
        handler.removeCallbacks(tick)
        handler.post(tick)
        return START_STICKY
    }

    override fun onDestroy() {
        handler.removeCallbacks(tick)
        overlay.hide()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    // ── Private ──────────────────────────────────────────────────────────────

    private fun applyTarget() {
        // Re-read intensity each tick so changes in MainActivity are picked up.
        cfg = Config.from(this)

        if (!cfg.enabled) { overlay.hide(); return }

        val now    = LocalTime.now()
        val nowMin = now.hour * 60.0 + now.minute + now.second / 60.0

        val (sunrise, sunset) = solarTimes(cfg.latitude, cfg.longitude, LocalDate.now())
        val ctx = DayContext(
            sunriseMin = sunrise,
            sunsetMin  = sunset,
            wakeMin    = cfg.wakeMin.toDouble(),
            bedMin     = cfg.bedMin.toDouble(),
        )

        // Raw target from schedule, then attenuated by current intensity.
        val scheduled = DEFAULT_SCHEDULE.targetAt(nowMin, ctx)
        val target    = scheduled.attenuate(cfg.intensity.factor)

        val alpha = overlayAlpha(target.cctKelvin, target.brightness)
        val argb = cctToArgb(target.cctKelvin, alpha)

        if (Settings.canDrawOverlays(this)) {
            overlay.update(argb)
        }
    }

    private fun ensureNotificationChannel() {
        val nm = getSystemService(NOTIFICATION_SERVICE) as NotificationManager
        if (nm.getNotificationChannel(CHANNEL_ID) != null) return
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL_ID, getString(R.string.channel_name), NotificationManager.IMPORTANCE_MIN).apply {
                description = getString(R.string.channel_desc)
                setShowBadge(false)
            }
        )
    }

    private fun buildNotification() =
        NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_launcher)
            .setContentTitle(getString(R.string.app_name))
            .setContentText(getString(R.string.notif_running) + " • ${cfg.intensity.label}")
            .setOngoing(true)
            .setSilent(true)
            .setPriority(NotificationCompat.PRIORITY_MIN)
            .setContentIntent(
                PendingIntent.getActivity(
                    this, 0,
                    Intent(this, MainActivity::class.java),
                    PendingIntent.FLAG_IMMUTABLE,
                )
            )
            .build()

    companion object {
        private const val CHANNEL_ID = "estel_bg"
        private const val NOTIF_ID   = 1
        private const val TICK_MS    = 60_000L
    }
}

// ── Target attenuation (mirrors Rust Target::attenuate) ──────────────────────

private fun Target.attenuate(factor: Float): Target {
    val t = factor.coerceIn(0f, 1f)
    val neutralMired = 1_000_000f / 6500f
    val selfMired    = 1_000_000f / cctKelvin.coerceAtLeast(1f)
    val mired        = neutralMired + (selfMired - neutralMired) * t
    return Target(
        cctKelvin    = 1_000_000f / mired,
        brightness   = 1f + (brightness   - 1f) * t,
        noiseGain    = noiseGain * t,
        noise        = if (t >= 0.5f) noise else null,
    )
}
