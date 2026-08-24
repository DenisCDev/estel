package studio.condado.estel

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.Settings
import android.view.View
import android.widget.SeekBar
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat
import studio.condado.estel.databinding.ActivityMainBinding

/**
 * Settings screen. No biometrics, no health metrics, no medical framing.
 */
class MainActivity : AppCompatActivity() {

    private lateinit var binding: ActivityMainBinding
    private lateinit var cfg: Config
    private var bound = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        binding = ActivityMainBinding.inflate(layoutInflater)
        setContentView(binding.root)
        cfg = Config.from(this)
        bindControls()
    }

    override fun onResume() {
        super.onResume()
        refresh()
        if (cfg.enabled && Settings.canDrawOverlays(this)) {
            startServiceIfNeeded()
        }
    }

    private fun bindControls() {
        if (bound) return
        bound = true

        binding.switchEnabled.setOnCheckedChangeListener { _, checked ->
            cfg.enabled = checked
            updateServiceState(checked)
        }

        binding.btnOverlayPermission.setOnClickListener {
            startActivity(
                Intent(
                    Settings.ACTION_MANAGE_OVERLAY_PERMISSION,
                    Uri.parse("package:$packageName"),
                )
            )
        }

        val intensityButtons = listOf(
            binding.btnIntensityAlta to Intensity.ALTA,
            binding.btnIntensityMedia to Intensity.MEDIA,
            binding.btnIntensitySuave to Intensity.SUAVE,
        )
        intensityButtons.forEach { (btn, level) ->
            btn.setOnClickListener {
                cfg.intensity = level
                refreshIntensity(intensityButtons)
                if (cfg.enabled) startServiceIfNeeded()
            }
        }

        binding.editLat.setOnFocusChangeListener { _, hasFocus ->
            if (!hasFocus) commitCoord(isLat = true)
        }
        binding.editLon.setOnFocusChangeListener { _, hasFocus ->
            if (!hasFocus) commitCoord(isLat = false)
        }

        binding.seekWake.max = 1439
        binding.seekWake.setOnSeekBarChangeListener(
            minuteListener(binding.labelWake) { cfg.wakeMin = it }
        )
        binding.seekBed.max = 1439
        binding.seekBed.setOnSeekBarChangeListener(
            minuteListener(binding.labelBed) { cfg.bedMin = it }
        )
    }

    private fun refresh() {
        binding.switchEnabled.isChecked = cfg.enabled

        val hasOverlay = Settings.canDrawOverlays(this)
        binding.btnOverlayPermission.visibility = if (hasOverlay) View.GONE else View.VISIBLE

        refreshIntensity(
            listOf(
                binding.btnIntensityAlta to Intensity.ALTA,
                binding.btnIntensityMedia to Intensity.MEDIA,
                binding.btnIntensitySuave to Intensity.SUAVE,
            )
        )

        if (!binding.editLat.hasFocus()) {
            binding.editLat.setText(cfg.latitude.toString())
        }
        if (!binding.editLon.hasFocus()) {
            binding.editLon.setText(cfg.longitude.toString())
        }

        binding.seekWake.progress = cfg.wakeMin
        binding.labelWake.text = formatMinutes(cfg.wakeMin)
        binding.seekBed.progress = cfg.bedMin
        binding.labelBed.text = formatMinutes(cfg.bedMin)
    }

    private fun refreshIntensity(
        buttons: List<Pair<com.google.android.material.button.MaterialButton, Intensity>>,
    ) {
        val current = cfg.intensity
        buttons.forEach { (btn, level) ->
            btn.isSelected = level == current
            btn.alpha = if (level == current) 1.0f else 0.5f
        }
    }

    private fun commitCoord(isLat: Boolean) {
        val field = if (isLat) binding.editLat else binding.editLon
        val raw = field.text?.toString().orEmpty()
        val parsed = parseCoord(raw)
        val ok = parsed != null && if (isLat) parsed in -90.0..90.0 else parsed in -180.0..180.0
        if (ok && parsed != null) {
            if (isLat) cfg.latitude = parsed else cfg.longitude = parsed
            field.setText(parsed.toString())
            binding.labelCoordError.visibility = View.GONE
        } else {
            field.setText(if (isLat) cfg.latitude.toString() else cfg.longitude.toString())
            binding.labelCoordError.visibility = View.VISIBLE
        }
    }

    /** Accepts point or comma decimals (pt-BR). */
    private fun parseCoord(text: String): Double? =
        text.trim().replace(',', '.').toDoubleOrNull()

    private fun formatMinutes(min: Int): String =
        "%02d:%02d".format(min / 60, min % 60)

    private fun minuteListener(
        label: android.widget.TextView,
        save: (Int) -> Unit,
    ) = object : SeekBar.OnSeekBarChangeListener {
        override fun onProgressChanged(sb: SeekBar, v: Int, fromUser: Boolean) {
            if (fromUser) {
                label.text = formatMinutes(v)
                save(v)
            }
        }
        override fun onStartTrackingTouch(sb: SeekBar) = Unit
        override fun onStopTrackingTouch(sb: SeekBar) = Unit
    }

    private fun startServiceIfNeeded() {
        ContextCompat.startForegroundService(
            this,
            Intent(this, EstelService::class.java),
        )
    }

    private fun updateServiceState(enable: Boolean) {
        val intent = Intent(this, EstelService::class.java)
        if (enable) ContextCompat.startForegroundService(this, intent)
        else stopService(intent)
    }
}
