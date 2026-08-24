package studio.condado.estel

import android.content.Context
import android.graphics.PixelFormat
import android.os.Build
import android.view.View
import android.view.WindowManager

/**
 * Manages the full-screen warm-tint overlay window.
 *
 * The overlay is a transparent colored View layered on top of all other windows
 * using TYPE_APPLICATION_OVERLAY. It doesn't intercept touch events.
 *
 * SYSTEM_ALERT_WINDOW permission must be granted before calling [show].
 */
class OverlayManager(private val context: Context) {

    private val wm = context.getSystemService(Context.WINDOW_SERVICE) as WindowManager
    private var overlayView: View? = null

    private val layoutParams = WindowManager.LayoutParams(
        WindowManager.LayoutParams.MATCH_PARENT,
        WindowManager.LayoutParams.MATCH_PARENT,
        WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
        WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE
                or WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE
                or WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN
                or WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS,
        PixelFormat.TRANSLUCENT,
    )

    /** Show or update the overlay with [argb] color. Call whenever [Target] changes. */
    fun update(argb: Int) {
        val alpha = (argb ushr 24) and 0xFF
        if (alpha == 0) {
            hide()
            return
        }

        val view = overlayView
        if (view == null) {
            val newView = View(context).apply { setBackgroundColor(argb) }
            wm.addView(newView, layoutParams)
            overlayView = newView
        } else {
            view.setBackgroundColor(argb)
        }
    }

    /** Remove the overlay from the screen. Safe to call when not shown. */
    fun hide() {
        overlayView?.let { wm.removeView(it) }
        overlayView = null
    }
}
