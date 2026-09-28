//! One-time location lookup through Windows, used only while settings are open.

use std::thread;
use std::time::{Duration, Instant};

use windows::Devices::Geolocation::{GeolocationAccessStatus, Geolocator};
use windows::Foundation::TimeSpan;
use windows::core::RuntimeType;
use windows_future::{AsyncStatus, IAsyncOperation};

const TICKS_PER_SECOND: i64 = 10_000_000;

/// Call on the foreground settings UI thread so Windows can ask for consent.
pub fn request_access() -> Result<IAsyncOperation<GeolocationAccessStatus>, String> {
    Geolocator::RequestAccessAsync()
        .map_err(|error| format!("O Windows não iniciou a solicitação de localização ({error})."))
}

/// Resolve consent and one coarse location reading on a worker thread.
pub fn resolve(access: IAsyncOperation<GeolocationAccessStatus>) -> Result<(f64, f64), String> {
    let permission = wait_for(&access, Duration::from_secs(30))?;
    if permission != GeolocationAccessStatus::Allowed {
        return Err("O acesso à localização foi negado pelo Windows.".into());
    }
    let locator = Geolocator::new()
        .map_err(|error| format!("O Windows não iniciou o serviço de localização ({error})."))?;
    let operation = locator
        .GetGeopositionAsyncWithAgeAndTimeout(
            TimeSpan {
                Duration: 60 * TICKS_PER_SECOND,
            },
            TimeSpan {
                Duration: 10 * TICKS_PER_SECOND,
            },
        )
        .map_err(|error| format!("O Windows não iniciou a leitura da localização ({error})."))?;
    let position = wait_for(&operation, Duration::from_secs(12))?
        .Coordinate()
        .and_then(|coordinate| coordinate.Point())
        .and_then(|point| point.Position())
        .map_err(|error| format!("O Windows retornou uma localização inválida ({error})."))?;
    if !position.Latitude.is_finite()
        || !position.Longitude.is_finite()
        || !(-90.0..=90.0).contains(&position.Latitude)
        || !(-180.0..=180.0).contains(&position.Longitude)
    {
        return Err("O Windows retornou coordenadas inválidas.".into());
    }
    Ok((position.Latitude, position.Longitude))
}

fn wait_for<T: RuntimeType + 'static>(
    operation: &IAsyncOperation<T>,
    timeout: Duration,
) -> Result<T, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let status = operation
            .Status()
            .map_err(|error| format!("O Windows não concluiu a localização ({error})."))?;
        if status == AsyncStatus::Completed {
            return operation
                .GetResults()
                .map_err(|error| format!("O Windows não forneceu a localização ({error})."));
        }
        if status == AsyncStatus::Error || status == AsyncStatus::Canceled {
            return Err("O Windows cancelou a leitura da localização.".into());
        }
        if Instant::now() >= deadline {
            let _ = operation.Cancel();
            return Err("A busca pela localização demorou demais.".into());
        }
        thread::sleep(Duration::from_millis(100));
    }
}
