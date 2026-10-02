//! Optional weather and room orientation input for the brightness fallback.

use std::io::Read;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Datelike, Local, Timelike, Utc};
use serde::{Deserialize, Serialize};

use crate::config::{Config, ScreenWindowRelation};

#[derive(Debug, Clone, Deserialize)]
pub struct Place {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub country: Option<String>,
    pub admin1: Option<String>,
}

impl Place {
    pub fn label(&self) -> String {
        [
            Some(self.name.as_str()),
            self.admin1.as_deref(),
            self.country.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ")
    }
}

#[derive(Deserialize)]
struct PlacesResponse {
    #[serde(default)]
    results: Vec<Place>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Weather {
    pub cloud_cover: f32,
    pub shortwave_radiation: f32,
    pub direct_radiation: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum WeatherPhase {
    Consulting,
    Ready(u8),
    Unavailable,
}

#[derive(Serialize, Deserialize)]
struct WeatherStatus {
    latitude: f64,
    longitude: f64,
    updated_at: u64,
    phase: WeatherPhase,
}

pub fn publish_status(config: &Config, phase: WeatherPhase) {
    let status = WeatherStatus {
        latitude: config.latitude,
        longitude: config.longitude,
        updated_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        phase,
    };
    let path = Config::config_path().with_file_name("weather-status.json");
    if let Err(error) = serde_json::to_vec(&status)
        .map_err(std::io::Error::other)
        .and_then(|data| crate::config::atomic_write(&path, &data))
    {
        tracing::warn!(%error, "não foi possível publicar o estado do clima");
    } else {
        #[cfg(windows)]
        crate::status::notify();
    }
}

pub fn status_label(config: &Config) -> String {
    if !config.weather_enabled {
        return "Consulta de clima desligada.".into();
    }
    if config.preserve_colors() {
        return "Clima em pausa enquanto as cores do monitor estão preservadas.".into();
    }
    let path = Config::config_path().with_file_name("weather-status.json");
    let status = std::fs::read(path)
        .ok()
        .and_then(|data| serde_json::from_slice::<WeatherStatus>(&data).ok());
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    match status {
        Some(status)
            if (status.latitude, status.longitude) == (config.latitude, config.longitude)
                && now.saturating_sub(status.updated_at) < 1_200 =>
        {
            match status.phase {
                WeatherPhase::Consulting => "Consultando o clima...".into(),
                WeatherPhase::Ready(clouds) => format!("Clima atualizado: {clouds}% de nuvens. A luz ambiente tem prioridade."),
                WeatherPhase::Unavailable => "Clima indisponível. Confira a conexão; o brilho segue o horário até a próxima consulta.".into(),
            }
        }
        _ => "Aguardando consulta de clima para este local...".into(),
    }
}

#[derive(Deserialize)]
struct WeatherResponse {
    current: Weather,
}

fn get_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .timeout_connect(Duration::from_secs(4))
        .timeout_read(Duration::from_secs(8))
        .timeout_write(Duration::from_secs(4))
        .user_agent("Estel/0.2.2 (https://github.com/DenisCDev/estel)")
        .build();
    let response = agent.get(url).call().map_err(|error| {
        tracing::warn!(%error, "consulta online indisponível");
        "O serviço online não respondeu. Confira a conexão e tente novamente.".to_owned()
    })?;
    let mut body = Vec::new();
    response
        .into_reader()
        .take(65_537)
        .read_to_end(&mut body)
        .map_err(|error| {
            tracing::warn!(%error, "não foi possível ler a resposta online");
            "Não foi possível ler a resposta online. Tente novamente.".to_owned()
        })?;
    if body.len() > 65_536 {
        return Err("A resposta online excedeu o tamanho esperado.".into());
    }
    serde_json::from_slice(&body).map_err(|error| {
        tracing::warn!(%error, "resposta online inválida");
        "O serviço online enviou dados inválidos. Tente novamente mais tarde.".to_owned()
    })
}

pub fn search_places(query: &str) -> Result<Vec<Place>, String> {
    let query = query.trim();
    if query.chars().count() < 3 || query.chars().count() > 100 {
        return Err("Digite entre 3 e 100 caracteres para buscar um lugar.".into());
    }
    let mut url = url::Url::parse("https://geocoding-api.open-meteo.com/v1/search")
        .map_err(|error| error.to_string())?;
    url.query_pairs_mut()
        .append_pair("name", query)
        .append_pair("count", "5")
        .append_pair("language", "pt");
    let response: PlacesResponse = get_json(url.as_str())?;
    Ok(response
        .results
        .into_iter()
        .filter(|place| {
            valid_coords(place.latitude, place.longitude)
                && !place.name.trim().is_empty()
                && place.name.chars().count() <= 200
        })
        .take(5)
        .collect())
}

pub fn current_weather(latitude: f64, longitude: f64) -> Result<Weather, String> {
    if !valid_coords(latitude, longitude) {
        return Err("Coordenadas inválidas para consultar o clima.".into());
    }
    let mut url = url::Url::parse("https://api.open-meteo.com/v1/forecast")
        .map_err(|error| error.to_string())?;
    url.query_pairs_mut()
        .append_pair("latitude", &latitude.to_string())
        .append_pair("longitude", &longitude.to_string())
        .append_pair(
            "current",
            "cloud_cover,shortwave_radiation,direct_radiation",
        );
    let response: WeatherResponse = get_json(url.as_str())?;
    let weather = response.current;
    if !weather.cloud_cover.is_finite()
        || !weather.shortwave_radiation.is_finite()
        || !weather.direct_radiation.is_finite()
        || !(0.0..=100.0).contains(&weather.cloud_cover)
        || weather.shortwave_radiation < 0.0
        || weather.direct_radiation < 0.0
    {
        return Err("O serviço de clima enviou valores inválidos.".into());
    }
    Ok(weather)
}

fn valid_coords(latitude: f64, longitude: f64) -> bool {
    latitude.is_finite()
        && longitude.is_finite()
        && (-90.0..=90.0).contains(&latitude)
        && (-180.0..=180.0).contains(&longitude)
}

/// Approximate outdoor daylight adjustment, used only without a valid camera reading.
pub fn fallback_brightness(
    scheduled: f32,
    weather: Weather,
    cfg: &Config,
    now: DateTime<Local>,
) -> f32 {
    let Some((sun_azimuth, sun_altitude)) = sun_position(cfg.latitude, cfg.longitude, now) else {
        return scheduled;
    };
    if sun_altitude <= 0.0 {
        return scheduled;
    }
    let daylight = (weather.shortwave_radiation / 700.0).clamp(0.0, 1.0);
    let diffuse = daylight * 0.07;
    let direct = if cfg.window_near {
        cfg.window_azimuth_deg
            .map(|window| {
                let separation = ((sun_azimuth - window + 180.0).rem_euclid(360.0) - 180.0).abs();
                if separation < 60.0 && sun_altitude > 5.0 {
                    let relation = match cfg.screen_window_relation {
                        ScreenWindowRelation::Front => 1.0,
                        ScreenWindowRelation::Side => 0.6,
                        ScreenWindowRelation::Back => 0.3,
                    };
                    (1.0 - separation / 60.0)
                        * (weather.direct_radiation / 600.0).clamp(0.0, 1.0)
                        * relation
                        * 0.18
                } else {
                    0.0
                }
            })
            .unwrap_or(0.0)
    } else {
        0.0
    };
    (scheduled + diffuse + direct).clamp(0.0, 1.0)
}

/// Approximate apparent solar azimuth clockwise from north and altitude.
fn sun_position(latitude: f64, longitude: f64, now: DateTime<Local>) -> Option<(f32, f32)> {
    if !valid_coords(latitude, longitude) {
        return None;
    }
    let utc = now.with_timezone(&Utc);
    let day = utc.ordinal() as f64;
    let hour = utc.hour() as f64 + utc.minute() as f64 / 60.0;
    let gamma = std::f64::consts::TAU / 365.0 * (day - 1.0 + (hour - 12.0) / 24.0);
    let declination = 0.006918 - 0.399912 * gamma.cos() + 0.070257 * gamma.sin()
        - 0.006758 * (2.0 * gamma).cos()
        + 0.000907 * (2.0 * gamma).sin()
        - 0.002697 * (3.0 * gamma).cos()
        + 0.00148 * (3.0 * gamma).sin();
    let equation = 229.18
        * (0.000075 + 0.001868 * gamma.cos()
            - 0.032077 * gamma.sin()
            - 0.014615 * (2.0 * gamma).cos()
            - 0.040849 * (2.0 * gamma).sin());
    let solar_minutes =
        (hour * 60.0 + utc.second() as f64 / 60.0 + equation + 4.0 * longitude).rem_euclid(1440.0);
    let hour_angle = (solar_minutes / 4.0 - 180.0).to_radians();
    let lat = latitude.to_radians();
    let altitude = (lat.sin() * declination.sin()
        + lat.cos() * declination.cos() * hour_angle.cos())
    .clamp(-1.0, 1.0)
    .asin();
    let azimuth = (hour_angle.sin())
        .atan2(hour_angle.cos() * lat.sin() - declination.tan() * lat.cos())
        .to_degrees()
        .rem_euclid(360.0);
    Some((
        ((azimuth + 180.0).rem_euclid(360.0)) as f32,
        altitude.to_degrees() as f32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn direct_sun_raises_no_camera_fallback_only_for_facing_window() {
        let now = Local
            .with_ymd_and_hms(2026, 3, 20, 12, 0, 0)
            .single()
            .unwrap();
        let mut cfg = Config {
            latitude: 0.0,
            longitude: 0.0,
            window_near: true,
            screen_window_relation: ScreenWindowRelation::Front,
            ..Config::default()
        };
        let azimuth = sun_position(0.0, 0.0, now).unwrap().0;
        cfg.window_azimuth_deg = Some(azimuth);
        let weather = Weather {
            cloud_cover: 0.0,
            shortwave_radiation: 700.0,
            direct_radiation: 600.0,
        };
        let aligned = fallback_brightness(0.5, weather, &cfg, now);
        cfg.window_azimuth_deg = Some((azimuth + 180.0).rem_euclid(360.0));
        let opposite = fallback_brightness(0.5, weather, &cfg, now);
        assert!(aligned > opposite);
        assert!(opposite > 0.5);
    }

    #[test]
    fn darkness_does_not_raise_brightness() {
        let weather = Weather {
            cloud_cover: 100.0,
            shortwave_radiation: 0.0,
            direct_radiation: 0.0,
        };
        assert_eq!(
            fallback_brightness(0.3, weather, &Config::default(), Local::now()),
            0.3
        );
    }

    #[test]
    fn stale_radiation_after_sunset_does_not_brighten_screen() {
        let now = Local
            .with_ymd_and_hms(2026, 3, 20, 22, 0, 0)
            .single()
            .unwrap();
        let config = Config {
            latitude: 0.0,
            longitude: 0.0,
            ..Config::default()
        };
        let weather = Weather {
            cloud_cover: 0.0,
            shortwave_radiation: 500.0,
            direct_radiation: 400.0,
        };
        assert!(sun_position(0.0, 0.0, now).unwrap().1 <= 0.0);
        assert_eq!(fallback_brightness(0.3, weather, &config, now), 0.3);
    }
}
