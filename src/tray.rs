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

/// 32×32 raster of `assets/icon-phial.svg`: Galadriel's phial, not an orange blob.
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

/// Unit square, y down. Inspired by the Phial of Galadriel card art:
/// teardrop vial, silver cap, white-gold core, teal halo.
fn phial_pixel(u: f32, v: f32) -> [f32; 4] {
    let x = (u - 0.50) * 2.0;
    let y = (v - 0.50) * 2.0;

    let d_body = sd_teardrop(x, y + 0.08);
    let d_cap = sd_cap(x, y + 0.08);
    let d_shape = d_body.min(d_cap);

    let glow = smoothstep(0.55, 0.0, d_shape + 0.28);
    let fill = smoothstep(0.04, -0.02, d_shape);
    let core = (-((x * x) * 9.0 + (y - 0.05).powi(2) * 4.5)).exp();
    let highlight = smoothstep(0.12, 0.0, (x + 0.16).abs() + (y + 0.05).abs() * 0.4);

    let mut r = 55.0 * glow + 185.0 * fill + 70.0 * core + 25.0 * highlight * fill;
    let mut g = 195.0 * glow + 225.0 * fill + 40.0 * core + 20.0 * highlight * fill;
    let mut b = 190.0 * glow + 220.0 * fill + 15.0 * core + 30.0 * highlight * fill;
    if d_cap < 0.02 {
        r = r * 0.85 + 200.0 * fill;
        g = g * 0.88 + 205.0 * fill;
        b = b * 0.92 + 215.0 * fill;
    }
    let a = (glow * 140.0 + fill * 255.0).clamp(0.0, 255.0);
    [r.min(255.0), g.min(255.0), b.min(255.0), a]
}

fn sd_teardrop(x: f32, y: f32) -> f32 {
    let bulb = (x * x + (y + 0.10).powi(2)).sqrt() - 0.36;
    let t = ((y + 0.08) / 0.96).clamp(0.0, 1.0);
    let hw = 0.36 * (1.0 - t * t);
    let taper = if y < -0.08 {
        1.0
    } else if y <= 0.88 {
        x.abs() - hw
    } else {
        (x * x + (y - 0.88).powi(2)).sqrt()
    };
    bulb.min(taper)
}

fn sd_cap(x: f32, y: f32) -> f32 {
    let cy = y + 0.62;
    let dome = ((x * x) * 1.4 + cy * cy).sqrt() - 0.20;
    let neck = {
        let nx = x.abs() - 0.11;
        let ny = (y + 0.42).abs() - 0.10;
        nx.max(ny)
    };
    dome.min(neck)
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
        let body = phial_pixel(0.50, 0.55);
        assert!(body[3] > 180.0, "body should be opaque, alpha {}", body[3]);
        let glow = phial_pixel(0.72, 0.52);
        assert!(
            glow[1] > glow[0] && glow[2] > glow[0],
            "halo is teal, not orange: r={} g={} b={}",
            glow[0], glow[1], glow[2]
        );
        let corner = phial_pixel(0.02, 0.02);
        assert!(corner[3] < 20.0, "corners stay transparent");
    }
}
