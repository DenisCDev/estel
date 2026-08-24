package studio.condado.estel

import android.content.Context
import android.content.SharedPreferences
import androidx.core.content.edit

/** How strongly circadian effects are applied. Mirrors Rust `Intensity`. */
enum class Intensity(val factor: Float, val label: String) {
    /** Full schedule — maximum benefit. */
    ALTA(1.0f, "Alta"),
    /** 60 % — casual gaming, films. CCT ~3100 K at bedtime, brightness ~50 %. */
    MEDIA(0.6f, "Média"),
    /** 30 % — competitive gaming. CCT ~4200 K at bedtime, brightness ~75 %. Noise off. */
    SUAVE(0.3f, "Suave");

    companion object {
        fun fromKey(key: String) = entries.firstOrNull { it.name == key } ?: ALTA
    }
}

/** Persisted config — SharedPreferences-backed. Same fields as Rust Config. */
class Config(private val prefs: SharedPreferences) {

    var enabled: Boolean
        get() = prefs.getBoolean(KEY_ENABLED, true)
        set(v) = prefs.edit { putBoolean(KEY_ENABLED, v) }

    var latitude: Double
        get() = prefs.getString(KEY_LAT, null)?.toDoubleOrNull() ?: -23.55
        set(v) = prefs.edit { putString(KEY_LAT, v.toString()) }

    var longitude: Double
        get() = prefs.getString(KEY_LON, null)?.toDoubleOrNull() ?: -46.63
        set(v) = prefs.edit { putString(KEY_LON, v.toString()) }

    /** Wake time as minutes since midnight. */
    var wakeMin: Int
        get() = prefs.getInt(KEY_WAKE, 7 * 60)
        set(v) = prefs.edit { putInt(KEY_WAKE, v) }

    /** Bed time as minutes since midnight. */
    var bedMin: Int
        get() = prefs.getInt(KEY_BED, 23 * 60)
        set(v) = prefs.edit { putInt(KEY_BED, v) }

    /** Max overlay alpha (0..255). Scaled by intensity at runtime. */
    var maxAlpha: Int
        get() = prefs.getInt(KEY_ALPHA, 60)
        set(v) = prefs.edit { putInt(KEY_ALPHA, v.coerceIn(0, 180)) }

    /** Volume 0..1 for notification chimes. */
    var maxVolume: Float
        get() = prefs.getString(KEY_VOL, null)?.toFloatOrNull() ?: 0.7f
        set(v) = prefs.edit { putString(KEY_VOL, v.coerceIn(0f, 1f).toString()) }

    /** Effect intensity — Alta/Media/Suave. */
    var intensity: Intensity
        get() = Intensity.fromKey(prefs.getString(KEY_INTENSITY, null) ?: Intensity.ALTA.name)
        set(v) = prefs.edit { putString(KEY_INTENSITY, v.name) }

    companion object {
        private const val PREFS_NAME   = "estel_config"
        private const val KEY_ENABLED   = "enabled"
        private const val KEY_LAT       = "latitude"
        private const val KEY_LON       = "longitude"
        private const val KEY_WAKE      = "wake_min"
        private const val KEY_BED       = "bed_min"
        private const val KEY_ALPHA     = "max_alpha"
        private const val KEY_VOL       = "max_volume"
        private const val KEY_INTENSITY = "intensity"

        fun from(context: Context): Config =
            Config(context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE))
    }
}
