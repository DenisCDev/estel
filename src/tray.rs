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
        let icon = amber_icon()?;

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

fn amber_icon() -> anyhow::Result<tray_icon::Icon> {
    const SZ: u32 = 32;
    let mut rgba = vec![0u8; (SZ * SZ * 4) as usize];
    let c = SZ as f32 / 2.0;
    for y in 0..SZ {
        for x in 0..SZ {
            let dx = x as f32 - c;
            let dy = y as f32 - c;
            if (dx * dx + dy * dy).sqrt() < c - 1.0 {
                let i = ((y * SZ + x) * 4) as usize;
                rgba[i] = 255;
                rgba[i + 1] = 140;
                rgba[i + 3] = 255;
            }
        }
    }
    tray_icon::Icon::from_rgba(rgba, SZ, SZ).map_err(|e| anyhow::anyhow!("{e}"))
}
