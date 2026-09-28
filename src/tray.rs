//! System tray icon, context menu, and HKCU autostart.

use muda::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

use crate::config::Intensity;

pub enum TrayAction {
    TogglePause,
    ToggleAutostart,
    ToggleNoise,
    PreviewNight,
    OpenSettings,
    CheckUpdates,
    SetIntensity(Intensity),
    Quit,
}

pub struct Tray {
    icon: TrayIcon,
    pause: CheckMenuItem,
    autostart: CheckMenuItem,
    ambient_status: MenuItem,
    weather_status: MenuItem,
    noise: CheckMenuItem,
    noise_status: MenuItem,
    preview_id: MenuId,
    settings_id: MenuId,
    updates_id: MenuId,
    updates: MenuItem,
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
        ambient_enabled: bool,
    ) -> anyhow::Result<Self> {
        let icon = avatar_icon()?;

        let intensity_alta = CheckMenuItem::new("Alta", true, intensity == Intensity::Alta, None);
        let intensity_media =
            CheckMenuItem::new("Média", true, intensity == Intensity::Media, None);
        let intensity_suave =
            CheckMenuItem::new("Suave", true, intensity == Intensity::Suave, None);

        let pause = CheckMenuItem::new("Pausar", true, false, None);
        let autostart = CheckMenuItem::new("Iniciar com o Windows", true, autostart_enabled, None);
        let ambient_status = MenuItem::new(
            if ambient_enabled {
                "Luz ambiente: aguardando câmera"
            } else {
                "Luz ambiente: desligada"
            },
            false,
            None,
        );
        let weather_status = MenuItem::new("Clima: aguardando consulta", false, None);
        let noise = CheckMenuItem::new("Ruído noturno", true, noise_enabled, None);
        let noise_status = MenuItem::new(
            if noise_enabled {
                "Ruído: programado para a noite"
            } else {
                "Ruído: desligado"
            },
            false,
            None,
        );
        let preview = MenuItem::new("Prévia noturna (20 s; som se ativado)", true, None);
        let settings = MenuItem::new("Configurações…", true, None);
        let updates = MenuItem::new(
            format!("Buscar atualização · v{}", env!("CARGO_PKG_VERSION")),
            true,
            None,
        );
        let quit = MenuItem::new("Fechar Estel", true, None);

        let preview_id = preview.id().clone();
        let settings_id = settings.id().clone();
        let updates_id = updates.id().clone();
        let quit_id = quit.id().clone();

        let menu = Menu::new();
        let _ = menu.append(&intensity_alta);
        let _ = menu.append(&intensity_media);
        let _ = menu.append(&intensity_suave);
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&noise);
        let _ = menu.append(&noise_status);
        let _ = menu.append(&preview);
        let _ = menu.append(&pause);
        let _ = menu.append(&autostart);
        let _ = menu.append(&ambient_status);
        let _ = menu.append(&weather_status);
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&settings);
        let _ = menu.append(&updates);
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
            ambient_status,
            weather_status,
            noise,
            noise_status,
            preview_id,
            settings_id,
            updates_id,
            updates,
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
        self.intensity_alta
            .set_checked(intensity == Intensity::Alta);
        self.intensity_media
            .set_checked(intensity == Intensity::Media);
        self.intensity_suave
            .set_checked(intensity == Intensity::Suave);
    }

    pub fn set_paused(&self, paused: bool) {
        self.pause.set_checked(paused);
        self.pause
            .set_text(if paused { "Retomar" } else { "Pausar" });
    }

    pub fn set_autostart(&self, enabled: bool) {
        self.autostart.set_checked(enabled);
    }

    pub fn set_ambient_status(&self, status: &str) {
        self.ambient_status.set_text(status);
    }

    pub fn set_weather_status(&self, status: &str) {
        self.weather_status.set_text(status);
    }

    pub fn set_noise(&self, enabled: bool) {
        self.noise.set_checked(enabled);
        self.noise_status.set_text(if enabled {
            "Ruído: programado para a noite"
        } else {
            "Ruído: desligado"
        });
    }

    pub fn set_noise_status(&self, status: &str) {
        self.noise_status.set_text(status);
    }

    pub fn set_update_available(&self, version: &str) {
        self.updates
            .set_text(format!("Atualização {version} disponível…"));
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
        if id == &self.preview_id {
            return Some(TrayAction::PreviewNight);
        }
        if id == &self.settings_id {
            return Some(TrayAction::OpenSettings);
        }
        if id == &self.updates_id {
            return Some(TrayAction::CheckUpdates);
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

    pub fn toggle(&self) -> anyhow::Result<bool> {
        let was_enabled = self.0.is_enabled()?;
        if was_enabled {
            self.0.disable()?;
        } else {
            self.0.enable()?;
        }

        let enabled = self.0.is_enabled()?;
        if enabled == was_enabled {
            anyhow::bail!("o Windows não confirmou a alteração do início automático");
        }
        Ok(enabled)
    }
}

fn avatar_icon() -> anyhow::Result<tray_icon::Icon> {
    let image = image::load_from_memory_with_format(
        include_bytes!("../assets/avatar-icon.png"),
        image::ImageFormat::Png,
    )?
    .resize_exact(32, 32, image::imageops::FilterType::Lanczos3)
    .into_rgba8();
    tray_icon::Icon::from_rgba(image.into_raw(), 32, 32).map_err(|error| anyhow::anyhow!("{error}"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn avatar_has_transparent_corners() {
        let image = image::load_from_memory_with_format(
            include_bytes!("../assets/avatar-icon.png"),
            image::ImageFormat::Png,
        )
        .unwrap()
        .into_rgba8();
        assert_eq!(image.get_pixel(0, 0)[3], 0);
        assert!(image.get_pixel(image.width() / 2, image.height() / 2)[3] > 0);
    }
}
