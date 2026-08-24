//! System tray icon, context menu, and HKCU autostart.

use muda::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

use crate::config::Intensity;

pub enum TrayAction {
    TogglePause,
    ToggleAutostart,
    ToggleNoise,
    OpenSettings,
    SetIntensity(Intensity),
    Quit,
}

pub struct Tray {
    icon: TrayIcon,
    pause: CheckMenuItem,
    autostart: CheckMenuItem,
    noise: CheckMenuItem,
    settings_id: MenuId,
    quit_id: MenuId,
    intensity_alta: CheckMenuItem,
    intensity_media: CheckMenuItem,
    intensity_suave: CheckMenuItem,
}

impl Tray {
    pub fn new(
        autostart_enabled: bool,
        intensity: Intensity,
        noise_enabled: bool,
    ) -> anyhow::Result<Self> {
        let icon = phial_icon()?;

        let intensity_alta = CheckMenuItem::new("Alta", true, intensity == Intensity::Alta, None);
        let intensity_media = CheckMenuItem::new("Média", true, intensity == Intensity::Media, None);
        let intensity_suave = CheckMenuItem::new("Suave", true, intensity == Intensity::Suave, None);

        let pause = CheckMenuItem::new("Pausar", true, false, None);
        let autostart = CheckMenuItem::new("Iniciar com o Windows", true, autostart_enabled, None);
        let noise = CheckMenuItem::new("Ruído noturno", true, noise_enabled, None);
        let settings = MenuItem::new("Configurações…", true, None);
        let quit = MenuItem::new("Fechar Estel", true, None);

        let settings_id = settings.id().clone();
        let quit_id = quit.id().clone();

        let menu = Menu::new();
        let _ = menu.append(&intensity_alta);
        let _ = menu.append(&intensity_media);
        let _ = menu.append(&intensity_suave);
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&noise);
        let _ = menu.append(&pause);
        let _ = menu.append(&autostart);
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&settings);
        let _ = menu.append(&quit);

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Estel")
            .with_icon(icon)
            .build()
            .map_err(|e| anyhow::anyhow!("falha ao criar o ícone da bandeja: {e}"))?;

        Ok(Tray {
            icon: tray,
            pause,
            autostart,
            noise,
            settings_id,
            quit_id,
            intensity_alta,
            intensity_media,
            intensity_suave,
        })
    }

    pub fn set_tooltip(&self, text: &str) {
        let _ = self.icon.set_tooltip(Some(text));
    }

    pub fn set_intensity(&self, intensity: Intensity) {
        self.intensity_alta.set_checked(intensity == Intensity::Alta);
        self.intensity_media.set_checked(intensity == Intensity::Media);
        self.intensity_suave.set_checked(intensity == Intensity::Suave);
    }

    pub fn set_paused(&self, paused: bool) {
        self.pause.set_checked(paused);
        self.pause.set_text(if paused { "Retomar" } else { "Pausar" });
    }

    pub fn set_autostart(&self, enabled: bool) {
        self.autostart.set_checked(enabled);
    }

    pub fn set_noise(&self, enabled: bool) {
        self.noise.set_checked(enabled);
    }

    pub fn poll(&self) -> Option<TrayAction> {
        let event = MenuEvent::receiver().try_recv().ok()?;
        let id = &event.id;

        if id == self.intensity_alta.id() {
            return Some(TrayAction::SetIntensity(Intensity::Alta));
        }
        if id == self.intensity_media.id() {
            return Some(TrayAction::SetIntensity(Intensity::Media));
        }
        if id == self.intensity_suave.id() {
            return Some(TrayAction::SetIntensity(Intensity::Suave));
        }
        if id == self.pause.id() {
            return Some(TrayAction::TogglePause);
        }
        if id == self.autostart.id() {
            return Some(TrayAction::ToggleAutostart);
        }
        if id == self.noise.id() {
            return Some(TrayAction::ToggleNoise);
        }
        if id == &self.settings_id {
            return Some(TrayAction::OpenSettings);
        }
        if id == &self.quit_id {
            return Some(TrayAction::Quit);
        }
        None
    }
}

pub struct Autostart(auto_launch::AutoLaunch);

impl Autostart {
    pub fn new() -> anyhow::Result<Self> {
        let exe = std::env::current_exe()?;
        let path = exe
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("caminho do executável não é UTF-8"))?;
        Ok(Autostart(auto_launch::AutoLaunch::new(
            "Estel",
            path,
            auto_launch::WindowsEnableMode::CurrentUser,
            &[] as &[&str],
        )))
    }

    pub fn is_enabled(&self) -> bool {
        self.0.is_enabled().unwrap_or(false)
    }

    pub fn toggle(&self) -> bool {
        if self.is_enabled() {
            let _ = self.0.disable();
            false
        } else {
            let _ = self.0.enable();
            true
        }
    }
}

/// 32×32 raster of `assets/icon-phial.svg`: a short lidded jar with a teal halo.
fn phial_icon() -> anyhow::Result<tray_icon::Icon> {
    const SZ: u32 = 32;
    const SAMPLES: u32 = 3;
    let mut rgba = vec![0u8; (SZ * SZ * 4) as usize];
    for y in 0..SZ {
        for x in 0..SZ {
            let mut acc = [0.0_f32; 4];
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let u = (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / SZ as f32;
                    let v = (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / SZ as f32;
                    let p = phial_pixel(u, v);
                    acc[0] += p[0];
                    acc[1] += p[1];
                    acc[2] += p[2];
                    acc[3] += p[3];
                }
            }
            let n = (SAMPLES * SAMPLES) as f32;
            let i = ((y * SZ + x) * 4) as usize;
            let a = (acc[3] / n).clamp(0.0, 255.0);
            // Premultiplied-looking RGB: keep color even when a < 255.
            rgba[i] = (acc[0] / n).clamp(0.0, 255.0) as u8;
            rgba[i + 1] = (acc[1] / n).clamp(0.0, 255.0) as u8;
            rgba[i + 2] = (acc[2] / n).clamp(0.0, 255.0) as u8;
            rgba[i + 3] = a as u8;
        }
    }
    tray_icon::Icon::from_rgba(rgba, SZ, SZ).map_err(|e| anyhow::anyhow!("{e}"))
}

/// Unit square, y down. A short round jar with a lid — reads at 32 px.
/// Starlight inside, teal halo. Not a teardrop (that looked like a pin/bulb).
fn phial_pixel(u: f32, v: f32) -> [f32; 4] {
    let x = (u - 0.50) * 2.0;
    let y = (v - 0.50) * 2.0;

    let d_body = sd_jar(x, y);
    let d_lid = sd_lid(x, y);
    let d_shape = d_body.min(d_lid);

    let glow = smoothstep(0.50, 0.0, d_shape + 0.22);
    let fill = smoothstep(0.035, -0.02, d_shape);
    let core = (-(x * x * 7.0 + (y - 0.18).powi(2) * 6.0)).exp();
    let highlight = smoothstep(0.14, 0.0, (x + 0.18).abs() + (y - 0.10).abs() * 0.5);

    let mut r = 50.0 * glow + 175.0 * fill + 80.0 * core + 28.0 * highlight * fill;
    let mut g = 200.0 * glow + 220.0 * fill + 45.0 * core + 22.0 * highlight * fill;
    let mut b = 195.0 * glow + 215.0 * fill + 18.0 * core + 32.0 * highlight * fill;
    if d_lid < 0.02 {
        r = r * 0.78 + 210.0 * fill;
        g = g * 0.82 + 212.0 * fill;
        b = b * 0.88 + 220.0 * fill;
    }
    let a = (glow * 130.0 + fill * 255.0).clamp(0.0, 255.0);
    [r.min(255.0), g.min(255.0), b.min(255.0), a]
}

fn sd_jar(x: f32, y: f32) -> f32 {
    // Wide squat body — a pote, not a bottle.
    sd_ellipse(x, y - 0.16, 0.56, 0.58)
}

fn sd_lid(x: f32, y: f32) -> f32 {
    let plate = sd_round_box(x, y + 0.50, 0.40, 0.09, 0.04);
    let knob = (x * x + (y + 0.68).powi(2)).sqrt() - 0.11;
    plate.min(knob)
}

fn sd_ellipse(x: f32, y: f32, rx: f32, ry: f32) -> f32 {
    (x / rx).powi(2) + (y / ry).powi(2) - 1.0
}

fn sd_round_box(x: f32, y: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let qx = x.abs() - hx + r;
    let qy = y.abs() - hy + r;
    qx.max(qy).min(0.0) + (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() - r
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phial_is_not_an_orange_disc() {
        let body = phial_pixel(0.50, 0.58);
        assert!(body[3] > 180.0, "body should be opaque, alpha {}", body[3]);
        assert!(
            body[1] + body[2] > body[0] * 1.4,
            "glass is teal/silver, not orange: r={} g={} b={}",
            body[0], body[1], body[2]
        );
        let corner = phial_pixel(0.02, 0.02);
        assert!(corner[3] < 20.0, "corners stay transparent");
    }
}
