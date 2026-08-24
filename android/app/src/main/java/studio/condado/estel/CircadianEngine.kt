package studio.condado.estel

import java.time.LocalDate
import java.time.ZoneId
import kotlin.math.*

/**
 * Pure circadian schedule engine — mirrors the Rust `color`, `schedule`, and `target` modules.
 *
 * Platform rule (non-negotiable): never produce medical/clinical/alarming output.
 * Estel is adjunctive comfort only.
 */

// ── Color temperature → RGB ──────────────────────────────────────────────────

/** Tanner Helland. `t = kelvin / 100` — dividing by 1000 painted a red wash at noon. */
fun cctToRgb(kelvin: Float): Triple<Float, Float, Float> {
    val t = kelvin.coerceIn(1000f, 40000f) / 100f

    val r = if (t <= 66f) 255f
            else 329.69873f * (t - 60f).pow(-0.13320476f)

    val g = if (t <= 66f) 99.4708f * ln(t) - 161.11957f
            else 288.12217f * (t - 60f).pow(-0.07551485f)

    val b = when {
        t >= 66f -> 255f
        t <= 19f -> 0f
        else -> 138.51773f * ln(t - 10f) - 305.04477f
    }

    return Triple(
        (r / 255f).coerceIn(0f, 1f),
        (g / 255f).coerceIn(0f, 1f),
        (b / 255f).coerceIn(0f, 1f),
    )
}

/** Convert CCT to an ARGB int suitable for Android Color usage (alpha=alpha). */
fun cctToArgb(cct: Float, alpha: Int = 255): Int {
    val (r, g, b) = cctToRgb(cct)
    return android.graphics.Color.argb(
        alpha,
        (r * 255).toInt(),
        (g * 255).toInt(),
        (b * 255).toInt(),
    )
}

// ── Schedule ─────────────────────────────────────────────────────────────────

enum class NoiseColor { PINK, BROWN }

data class Target(
    val cctKelvin: Float,
    val brightness: Float,      // 0..1
    val noiseGain: Float,       // 0..1 — Estel's own noise, not OS volume
    val noise: NoiseColor?,
)

enum class AnchorKind { ABSOLUTE, WAKE_OFFSET, BED_OFFSET }

data class Keypoint(
    val anchorKind: AnchorKind,
    val anchorOffset: Int,      // minutes from anchor
    val cctKelvin: Float,
    val brightness: Float,
    val noiseGain: Float,
    val noise: NoiseColor?,
)

data class DayContext(
    val sunriseMin: Double,
    val sunsetMin: Double,
    val wakeMin: Double,
    val bedMin: Double,
)

private const val DAY_MIN = 1440.0

fun Keypoint.resolvedMin(ctx: DayContext): Double = when (anchorKind) {
    AnchorKind.ABSOLUTE -> anchorOffset.toDouble().mod(DAY_MIN)
    AnchorKind.WAKE_OFFSET -> (ctx.wakeMin + anchorOffset).mod(DAY_MIN)
    AnchorKind.BED_OFFSET -> (ctx.bedMin + anchorOffset).mod(DAY_MIN)
}

/** Mired-space interpolation: lerp in 1/K space, then invert. */
private fun lerpCct(a: Float, b: Float, t: Float): Float {
    val mA = 1_000_000f / a.coerceAtLeast(1f)
    val mB = 1_000_000f / b.coerceAtLeast(1f)
    return 1_000_000f / (mA + (mB - mA) * t)
}

/**
 * Overlay alpha: warmth as CCT drops, dim as brightness drops.
 * Day at 6500 K is invisible. Mirrors Rust `overlay::overlay_alpha` with
 * `ddc_active = false` (Android has no DDC).
 */
fun overlayAlpha(cct: Float, brightness: Float): Int {
    val startK = 5500f
    val floorK = 1900f
    val warm = if (cct >= startK) 0f else {
        val t = ((startK - cct) / (startK - floorK)).coerceIn(0f, 1f)
        val s = t * t * (3f - 2f * t)
        s * 70f
    }
    val dimT = (1f - brightness.coerceIn(0f, 1f)).coerceIn(0f, 1f)
    val dimS = dimT * dimT * (3f - 2f * dimT)
    val dim = dimS * 90f
    return (warm + dim).coerceAtMost(140f).toInt()
}

/** Smoothstep easing: 3t²-2t³ */
private fun smoothstep(t: Float): Float = t * t * (3f - 2f * t)

/** Interpolate between two keypoints at fractional position t ∈ [0,1]. */
private fun lerp(a: Keypoint, b: Keypoint, t: Float): Target {
    val s = smoothstep(t.coerceIn(0f, 1f))
    return Target(
        cctKelvin = lerpCct(a.cctKelvin, b.cctKelvin, s),
        brightness = a.brightness + (b.brightness - a.brightness) * s,
        noiseGain = a.noiseGain + (b.noiseGain - a.noiseGain) * s,
        noise = if (s < 0.5f) a.noise else b.noise,
    )
}

/**
 * Compute the ambient [Target] at [nowMin] minutes since midnight.
 * Mirrors `Schedule::target_at` from the Rust engine.
 */
fun List<Keypoint>.targetAt(nowMin: Double, ctx: DayContext): Target {
    if (isEmpty()) return Target(6500f, 1f, 0f, null)

    val resolved = map { it to it.resolvedMin(ctx) }
        .sortedBy { it.second }

    // Find surrounding keypoints (with wraparound across midnight)
    val n = resolved.size
    var hi = resolved.indexOfFirst { it.second > nowMin }
    if (hi == -1) hi = 0  // wrapped past last — next is first

    val loIdx = (hi - 1 + n) % n
    val (kA, tA) = resolved[loIdx]
    val (kB, tB) = resolved[hi]

    val span = if (tB > tA) tB - tA
               else tB + DAY_MIN - tA    // crosses midnight

    val elapsed = if (nowMin >= tA) nowMin - tA
                  else nowMin + DAY_MIN - tA

    val t = (elapsed / span).toFloat()
    return lerp(kA, kB, t)
}

// ── Default schedule ─────────────────────────────────────────────────────────

/** Same default curve as config.rs in the Rust crate. */
val DEFAULT_SCHEDULE: List<Keypoint> = listOf(
    Keypoint(AnchorKind.BED_OFFSET, 120, 1900f, 0.0f, 0.10f, NoiseColor.BROWN),
    Keypoint(AnchorKind.WAKE_OFFSET, 0, 3400f, 0.45f, 0.70f, null),
    Keypoint(AnchorKind.WAKE_OFFSET, 120, 6500f, 0.90f, 0.90f, null),
    Keypoint(AnchorKind.BED_OFFSET, -300, 6500f, 0.85f, 0.85f, null),
    Keypoint(AnchorKind.BED_OFFSET, -180, 3400f, 0.55f, 0.50f, null),
    Keypoint(AnchorKind.BED_OFFSET, -60, 2700f, 0.35f, 0.25f, NoiseColor.PINK),
    Keypoint(AnchorKind.BED_OFFSET, 0, 2300f, 0.18f, 0.15f, NoiseColor.PINK),
)

// ── Sunrise/sunset ────────────────────────────────────────────────────────────

/**
 * Approximate sunrise and sunset times in minutes-since-midnight (local time).
 * Uses the NOAA simplified solar position algorithm.
 * Falls back to civil twilight (06:00 / 18:00) on error.
 */
fun solarTimes(lat: Double, lon: Double, date: LocalDate): Pair<Double, Double> {
    return try {
        val dayOfYear = date.dayOfYear
        val fractionalYear = 2 * PI / 365 * (dayOfYear - 1)
        val eqTime = 229.18 * (0.000075 + 0.001868 * cos(fractionalYear)
                - 0.032077 * sin(fractionalYear) - 0.014615 * cos(2 * fractionalYear)
                - 0.04089 * sin(2 * fractionalYear))
        val decl = 0.006918 - 0.399912 * cos(fractionalYear) + 0.070257 * sin(fractionalYear) -
                0.006758 * cos(2 * fractionalYear) + 0.000907 * sin(2 * fractionalYear) -
                0.002697 * cos(3 * fractionalYear) + 0.00148 * sin(3 * fractionalYear)

        val latRad = Math.toRadians(lat)
        val zenithRad = Math.toRadians(90.833)
        val ha = acos(
            cos(zenithRad) / (cos(latRad) * cos(decl)) - tan(latRad) * tan(decl)
        )
        val haDeg = Math.toDegrees(ha)

        val timeZoneOffsetH = ZoneId.systemDefault().rules
            .getOffset(date.atStartOfDay().atZone(ZoneId.systemDefault()).toInstant())
            .totalSeconds / 3600.0

        val sunrise = 720 - 4 * (lon + haDeg) - eqTime + timeZoneOffsetH * 60
        val sunset  = 720 - 4 * (lon - haDeg) - eqTime + timeZoneOffsetH * 60

        Pair(sunrise.mod(DAY_MIN), sunset.mod(DAY_MIN))
    } catch (e: Exception) {
        Pair(6.0 * 60, 18.0 * 60)   // civil twilight fallback
    }
}
