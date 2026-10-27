<h1 align="center">Estel</h1>

<p align="center">
  <b>English</b> · <a href="README.pt-BR.md">Português</a>
</p>

<p align="center">
  <b>Windows and Android app that adjusts screen color and brightness throughout the day</b>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/windows-rust-D4A24E?labelColor=171310" alt="Windows app in Rust">
  <img src="https://img.shields.io/badge/android-kotlin-D4A24E?labelColor=171310" alt="Android app in Kotlin">
  <img src="https://img.shields.io/badge/câmera-local-43A48E?labelColor=171310" alt="camera frames kept on the device">
  <img src="https://img.shields.io/badge/license-MIT-D4A24E?labelColor=171310" alt="MIT license">
</p>

<p align="center">
  <img src="assets/avatar-icon.png" width="180" alt="Estel avatar: character with black hair and green streaks">
</p>

<p align="center">
  <sub>Avatar inspired by the character from the <a href="https://github.com/DenisCDev/portfolio-site">DenisDev portfolio</a>.</sub>
</p>

The Windows interface uses the Fredoka and Lilita One fonts, distributed under
the SIL Open Font License in the files `assets/fredoka-OFL.txt` and
`assets/lilitaone-OFL.txt`.

Estel gradually changes the screen's color temperature and brightness on Windows
and Android. On Windows, you can choose gentle adjustments based on weather/window
or camera, and open a visual break or a grounding exercise. The Windows version
can also play pink or brown noise at low volume during the night. Everything runs
on the device, with no account and no collection of biometric data. City search
and weather lookup are online features described below.

Visual comfort choices vary from person to person. The support section
offers human contacts by country; the [evidence and limits](docs/COMFORT-EVIDENCE.md)
explain what was studied and what is still an engineering choice.

---

## Install (Windows)

You do not need to install Git or Rust, or open a terminal.

1. [Download the Estel installer for Windows](https://github.com/DenisCDev/estel/releases/latest/download/Estel-Setup-x86_64.exe).
2. Open `Estel-Setup-x86_64.exe` and step through the installer.
3. At the end, leave **Configurar e abrir Estel** (Set up and open Estel) checked. The settings open and the icon
   appears next to the clock or inside the **Show hidden icons** arrow.

Starting with version 0.3.1, a new installation opens a guide covering location,
window, camera or light sensor, routine, and brightness limits. Screen and sound
adjustments wait until the guide is completed. Windows location and ambient light are
optional; you can use coordinates, search for a city, or keep the initial
São Paulo reference and review it later. The camera only captures when you choose
to use ambient light and request references, or when valid references already exist
for automatic adjustment.

For the window direction, use your phone's compass: point the top of the device from
inside the room out through the window and read the degrees, away from magnets and
metal objects. If the compass offers it, use true (geographic) north,
which is the reference for the sun's position. North is 0°, east 90°, south 180°, and west 270°. This is an estimate of the
window's orientation, not of the city's coordinates; leaving the direction undefined
is also valid. The guide explains the window's position relative to the screen.

Updates keep existing preferences. To repeat the questions,
open the panel and select **Configuração guiada · local, janela e câmera** (guided setup · location, window and camera).
If you close it before finishing a new setup, your choices are saved
and the guide reappears the next time the app opens.

The installer works per user, without asking for an administrator password. It creates a
Start menu shortcut and enables **Iniciar com o Windows** (Start with Windows) on the first installation;
this option can be turned off in the tray menu. The app can be removed
through Windows Settings. The app does not have a digital signature yet;
SmartScreen may show a warning and Smart App Control may block it from running.
Preparation for signed releases is described in [Windows signing](docs/WINDOWS-SIGNING.md).

If you prefer not to install, you can download
[`estel-portable-x86_64.exe`](https://github.com/DenisCDev/estel/releases/latest/download/estel-portable-x86_64.exe)
and open it directly. The portable version stores its settings in the same location as the
installed version.

In the tray:

- **Alta / Média / Suave** (High / Medium / Soft) — strength of color and sound, without changing the comfortable brightness; Medium is the default for new installations
- **Optional night noise** — pink or brown, with smooth transitions and a digital gain limit; the actual volume depends on the device
- **Pausar** (Pause) — restores the screen now, without closing
- **Configurações…** (Settings…) — wake, sleep, light, visual break and emotional support
- **Buscar atualização** (Check for update) — opens the settings and checks the version published on GitHub
- **Fechar Estel** (Close Estel) — restores gamma and backlight

The first run writes `%APPDATA%\condado\estel\config\config.toml`.
Writes replace the whole file only after they finish and keep
the previous preferences in `config.toml.bak`. If the main file becomes
corrupted, Estel restores that backup and preserves the corrupted content in
`config.toml.invalid`; without a valid backup, it warns you instead of writing defaults.
Changes made from the panel and the tray preserve the fields you did not edit.
The Start with Windows preference is also saved so that a registry entry that was removed or
points to an old path is repaired the next time the app opens.

On Windows 0.3.2, a separate process monitors Estel's startup and execution.
Startup has a 45-second deadline; if the main process
fails, it tries to reopen it up to three times, with increasing waits. **Fechar Estel** (Close Estel),
ending the session, and the `--quit` command shut down normally, without restarting.
Failures in the panel or in optional sensors do not close the main app.
The installer's registry entry includes `--startup`, like the entry created by the app.

Diagnostics are kept in the configuration folder: `launcher.log` records
launch attempts, `estel.log` records the main process, and the other
processes have separate logs (`settings.log`, `display.log`, `camera.log`,
`light-sensor.log`, and `diagnostics.log`). Each file holds up to 2 MiB plus a
`.log.1` backup, including failures that happen before the tray is created. If the recovery
attempts run out, a message explains how to find the logs.
Automatic startup depends on Windows running the registry entry when the session
starts; Windows may delay that. Recovery begins when the launcher
runs, and it does not override security blocks or an automatic startup
option disabled in Windows.

### Update or remove

Estel checks for a newer published version at startup and shows a
notice in the tray menu. In the settings, **Atualizar agora** (Update now) downloads the installer,
checks its size and SHA-256, and opens the Windows wizard. Installation only starts
when you proceed in the wizard, and it preserves your settings. To remove it,
open **Windows Settings → Apps → Installed apps**, search
for **Estel**, and choose **Uninstall**.

### Accurate colors and color blindness

In the settings, check **Trabalho com cores** (I work with color) or **Tenho daltonismo** (I am color blind) to preserve
the monitor's original colors. While either of these options is
on, Estel pauses the screen color and brightness adjustments; the optional sound can
keep playing. There is no diagnosis and no universal filter for color blindness. The sources and
limits of this choice are in [verified decisions](docs/VERIFIED-DECISIONS.md).

If the camera fails or its reading is rejected, Estel discards that reading
and gradually returns to the brightness calculated from the time of day and the optional weather;
automatic brightness changes are limited to 6 percentage points per adjustment.
Automatic color changes are also gradual. Changing only the camera
reading interval does not discard the current measurement. The tray menu shows when
the adjustment has fallen back to the time-of-day schedule. If another app is using the
camera, release it to try again; the Estel log records guidance
for this failure.

Each display picks its own adjustment: physical brightness via DDC/CI or WMI when
available, gamma only on confirmed SDR, and an overlay in all other cases.
Connecting or switching a display triggers a new detection. HDR and advanced
color management remain under Windows control. Brightness reduction via
overlay is black; warming via overlay is approximate and can lift
shadows. It is not equivalent to an HDR color transform.

Estel saves the original brightness and gamma per display identity before changing them. If a
monitor is missing at restore time, it keeps the record to try again.
Older sessions that never saved the original gamma cannot recover that
calibration: the panel reports the pending item and that adjustment is disabled.
Panel technology is only shown when the monitor reports it; OLED, IPS, VA,
mini-LED, or flicker are not inferred from the commercial name.
The decisions, alternatives, and sources are in [hardware adaptation](docs/WINDOWS-HARDWARE.md).

### Android

Open `android/` in Android Studio and run it on the device. On first launch,
grant the overlay permission — without it the warm layer does not appear. The
service applies only the visual adjustments; ambient sound is available in the
Windows version.

---

## What it does and why

| Adjustment | Goal | How it works |
|---|---|---|
| Color temperature | Adjust the screen's appearance throughout the day according to preference | Gradual curve in mired, with smooth transitions |
| Brightness | Avoid an uncomfortably bright or dark screen | DDC on the monitor or a dark layer on the laptop; personal adjustment remains important |
| Ambient light on Windows | Gently correct the base brightness using the approximate ambient brightness | Off in new configurations; requires two relative references, checks five stable frames, and corrects 35% of the difference from the time-of-day curve within personal limits |
| Weather and window on Windows | Estimate ambient brightness when the camera is not measuring | Optional solar radiation lookup and the approximate sun position adjust the time-of-day brightness; the window can be configured as facing, behind, or to the side of the screen |
| Visual stability | Avoid abrupt changes and flicker created by the app | No interface blinking and no software-simulated PWM |
| Optional sound on Windows | Offer a sound background for those who prefer it | Pink or brown noise, switching with gradual fade-out and fade-in; the digital limit does not measure the volume at the ears |
| Visual break and grounding on Windows | Offer simple actions when the person wants them | 20 s visual break and a sensory attention exercise; support contacts by country |

The color curve at High intensity (Medium and Soft reduce color and sound, without raising brightness):

| Phase | CCT | Brightness |
|---|---|---|
| Wake | ramp → 6500 K | rising |
| Day | 6500 K | personal ceiling, initially 85% on Windows |
| Early evening | 6500 → 3400 K | falling |
| Pre-sleep | 3400 → 2700 K | low |
| Night | 1900–2300 K | comfortable minimum |

Windows 11 gamma silently rejects aggressive ramps. Estel does not try to dim the screen via gamma below ~50 %: the rest goes to DDC (external monitor) or to the overlay (laptop / HDR).

On Windows 0.2.8, **Brilho mínimo** (Minimum brightness), **Máximo durante o dia** (Daytime maximum), and **Máximo à noite / descanso** (Night / rest maximum)
are preferences separate from color intensity. The rest ceiling starts at
25% (never below the chosen floor), from sunset and from three hours before
bedtime until wake-up, including routines that cross midnight or sleep
during the day. Color and brightness move gradually toward their targets; a transition can
take a few minutes. These are adjustable starting points, with no equivalence to lux
and no guarantee of clinical benefit. Android keeps its own implementation.

---

## What was left out

| Claim | Evidence | Decision |
|---|---|---|
| “Blue-blocking” glasses | May not reduce short-term fatigue; low certainty for this outcome (Cochrane 2023) | No |
| Binaural beats | No demonstrated benefit for this app | No |
| Blue color as a calming treatment | Color and emotion studies depend on context and do not test Estel | No therapeutic promise by hue |
| Therapeutic 432 Hz | Weak | No chime, no claim |
| Biometrics / closed loop | Out of scope + privacy | Never |

There is no account and no image collection. The optional online lookups send the typed
search or the coordinates to Open-Meteo; camera frames stay on the device.

### Ambient light from sensor or camera (Windows)

Ambient light adjustment is off in new installations. When enabled,
it can prefer the Windows light sensor; if that is not available,
it uses the calibrated camera and then time of day/weather. The sensor provides lux, while
the camera provides only relative brightness. If you use the webcam, capture **dark environment** and
then **bright environment**, keeping the camera and screen in the same position and using
diffuse ambient light. The steps can be done at different times while
the panel is open. Until the references are accepted, the alternative to the camera
is the enabled light sensor or time of day/weather.
Estel waits at least 500 ms after opening the camera and checks five frames,
averaging up to 8,000 pixels per frame. It discards the frames in memory
and closes access. The default
reading happens every 30 seconds, has a 5-second limit, and the result is
smoothed before changing the brightness. Estel negotiates supported resolution, format, and FPS,
prioritizing low capture traffic, and ties the references to the
profile used. Incompatible changes require a new calibration. Screens turned off,
session lock, and sleep stop new captures and discard readings
in progress; a helper that has already started can take up to five seconds to exit.

If the camera is unavailable, brightness follows the time-of-day curve with a
limited correction from solar radiation, if the weather lookup is enabled.
Changes made in the settings window are applied as soon as they are saved.

There is no recording, preview, or transmission of frames, and no identification of people, faces, eyes,
presence, or emotional state. A regular webcam is not a light meter: automatic
exposure and camera position change the reading. That is why the feature treats
image brightness as an experimental signal, without photometric calibration.
The relative references replace the fixed pixel conversion; they do not calibrate
exposure or turn pixels into lux. Pairs without enough contrast,
unstable/saturated frames, another device, or readings outside the references
are rejected. If automatic exposure hides the difference between environments,
use the time-of-day adjustment. Redo the references when you move the camera. The dark/bright
environment sliders still adjust the brightness output. With the camera active,
the estimated brightness partially corrects the time-of-day
curve, respecting the personal day and rest ceilings. Weather respects the
same ceilings, including before bedtime when the sun is still up. The camera takes
priority over the weather estimate; turning the option off restores the curve with
optional weather and does not open the camera. If you use Windows' native automatic brightness,
avoid two automatic controllers at the same time. To test only Windows brightness,
pause Estel's screen adjustments; this also pauses its color change.
Turning off only the camera keeps Estel's time-of-day brightness running.

The main process waits for Windows events instead of waking every 50 ms;
that short interval is reserved for audio transitions. Driver calls
run in a separate process with a deadline and a single pending intent. The panel
updates information on events, without reading the weather file on every frame.
The measurement protocol and its limits are in [Windows performance](docs/WINDOWS-PERFORMANCE.md).

### Location, weather, and orientation (Windows)

On first installation, the guide offers Windows location as an option;
the request only starts when you enable it. If you prefer, or if access is
denied, search for a city or neighborhood with the **Buscar** (Search) button,
choose the result, and check the point on the map. The search uses the
[Open-Meteo](https://open-meteo.com/en/docs/geocoding-api) geocoding service,
with data from GeoNames. Neighborhoods that are not registered may not appear; in that case,
adjust latitude and longitude manually after checking the point on the map.

Weather comes from [Open-Meteo](https://open-meteo.com/en/docs) and is queried at
most every 15 minutes, using the chosen coordinates. When the network fails,
Estel falls back to time-of-day brightness. The free service is intended for
[non-commercial use](https://open-meteo.com/en/terms); turn off **Usar clima** (Use weather)
if that condition does not apply. The service provides the data under CC BY 4.0.

If there is a window nearby, enter its orientation using your phone's compass and whether the
screen faces it, has its back to it, or is beside it. The calculation uses the approximate
sun position and direct radiation to estimate a limited brightness
increase. It does not measure or remove reflections, curtains, or artificial light in the room.
The camera, when it works, always takes priority over this estimate.
If you connect or reconnect the webcam with the panel open, use **Buscar câmeras
novamente** (Search for cameras again) in the ambient light section. The calibrated camera is located by
device identity even when Windows changes the order of the webcams.
Sunrise and sunset at the location still define the color curve.

The settings window uses four illustrated scenes derived from the original
mascot of [Denis's portfolio](https://github.com/DenisCDev/portfolio-site),
preserving the identity described in `astro/MASCOTES.md` in that repository.

---

## Honest limits

- Modest effects. The biggest gain is **removing** what triggers you, not adding something magical.
- The software does not change the panel's electrical modulation or measure its flicker.
- Individual variability is high. Everything important fits in the settings window.
- The ambient reading is an ergonomic aid, not a diagnosis of fatigue,
  anxiety, or eye health. Breaks, diffuse lighting, and reducing reflections
  remain important.
- Pink or brown noise has no proven general benefit for focus, anxiety,
  or sleep. Digital gain does not guarantee a safe decibel level in headphones.
- The grounding exercise and the support contacts are started by the person;
  the app does not detect or treat crises.

---

## Development

```powershell
# precisa de Rust apenas para desenvolver ou compilar o projeto
# engine pura, sem janela
cargo test

# app
cargo run --release
```

`install.ps1` is also meant for development: it compiles the local code and
installs that build in `%LOCALAPPDATA%\Estel`. For normal use, download the ready-made
installer from the section above.

The engine (`color`, `schedule`, `target`, `config`) does not call the operating system. The Windows host applies the `Target` through gamma, DDC, overlay, and audio. Android applies the same `Target` in an overlay.

Win32 API details: `docs/VERIFIED-DECISIONS.md`.

---

## References

Research updated on **2026-09-29**. The
[audit of recent studies and the algorithm](docs/RESEARCH-AUDIT-2026.md)
compares source quality, the adjustments with and without a webcam, and the
justified changes. The [evidence catalog](docs/COMFORT-EVIDENCE.md) details each feature.
Recent publication does not equal high-certainty evidence: none of the sources
below validates Estel's coefficients or an uncalibrated webcam as a light meter.

### Current basis and 2024–2026 updates

- [CIE PS 001:2024](https://www.cie.co.at/publications/cie-position-statement-integrative-lighting-recommending-proper-light-proper-time-3rd)
  — updated position on light at the right time and CIE S 026 metrology.
- [ISO/CIE 8995-1:2025](https://committee.iso.org/standard/76342.html?browse=tc)
  — lighting of work places; requirements for the environment, not an ideal screen percentage.
- [Spitschan et al., BMJ Public Health, 2025](https://doi.org/10.1136/bmjph-2025-003205)
  — Delphi consensus on communicating about light and health; not a trial of an app.
- [Spitschan et al., BMC Medicine, 2026](https://pubmed.ncbi.nlm.nih.gov/41612386/)
  — consensus on gaps in measurement, dose–response, and intervention efficacy.
- [Yang et al., 2026](https://doi.org/10.2150/jstl.IEIJ250000672)
  — review of light parameters and digital eye strain; mechanistic context, no validated curve for webcams.
- [Massa et al., 2025](https://pubmed.ncbi.nlm.nih.gov/42376338/)
  — trial of brightness and color software, with 47 participants who completed it; preliminary evidence.
- [Redondo et al., 2025](https://doi.org/10.1016/j.exer.2025.110463)
  — breaks during reading; does not establish a universal 20-second rule.
- [TFOS DEWS III, 2025](https://doi.org/10.1016/j.ajo.2025.05.039)
  — review of dry eye management; does not validate camera-based brightness control.
- [Xu et al., npj Digital Medicine, 2025](https://doi.org/10.1038/s41746-025-02053-8)
  — small trial of blink training in smartphone users with dry eye.
- [CIE 252:2024](https://www.cie.co.at/publications/assessment-discomfort-glare-daylight-buildings)
  — assessment of daylight discomfort glare; azimuth and weather alone do not measure reflections.
- [CIE 249:2022 and 2026 corrigendum](https://www.cie.co.at/publications/visual-aspects-time-modulated-lighting-systems)
  — temporal light modulation; requires hardware assessment.
- [Basner et al., SLEEP, 2026](https://doi.org/10.1093/sleep/zsag001)
  — controlled study of pink noise during sleep; does not justify automatic sound for sleeping.
- [Brown TM et al., PLOS Biology, 2022](https://doi.org/10.1371/journal.pbio.3001571)
  — melanopic EDI consensus, maintained and complemented by the 2024 CIE statement.
- [Singh S et al., Cochrane, 2023, CD013244](https://www.cochrane.org/evidence/CD013244_blue-light-filtering-spectacle-lenses-visual-performance-macular-back-part-eye-protection-and)
  — 17 trials of glasses, not of screen filters; certainty varies by outcome.
- [OSHA — computer workstation lighting](https://www.osha.gov/etools/computer-workstations/workstation-environment)
  — reflections, contrast, and visual fatigue.

### Context studies and historical references

- Wilkins AJ et al. *Lighting Research & Technology* 21(1):11–18, 1989 — flicker and headache
- [Hazell & Wilkins, 1990](https://doi.org/10.1017/S0033291700017098)
  — fluorescent lamps and agoraphobia; not a test of screen dimming.
- [Wilms L & Oberfeld D, 2018](https://doi.org/10.1007/s00426-017-0880-8)
  — brightness, saturation, and emotion in controlled stimuli, not anxiety treatment.
- [Weijs et al., 2023](https://pubmed.ncbi.nlm.nih.gov/37830019/)
  — color and arousal in VR; corrects the lead authorship previously attributed to Reutimann.
- [IEEE 1789-2015](https://standards.ieee.org/ieee/1789/4479/)
  — historical reference for LED modulation; **Inactive-Reserved since 2026-03-26**.
- [Blumenthal TD & Berg WK, 1986](https://doi.org/10.1111/j.1469-8986.1986.tb00682.x)
  — brief, intense acoustic stimuli; does not validate Estel's four-second fade.
- Sheedy JE et al. *Ergonomics* 48(9):1114–1128, 2005,
  [doi:10.1080/00140130500208414](https://doi.org/10.1080/00140130500208414)
  — luminance around the screen and visual adaptation
- [ISO/TR 9241-610:2022](https://www.iso.org/obp/ui/en/#iso:std:iso:tr:9241:-610:ed-1:v1:en)
  — impact of light and lighting on interactive systems
