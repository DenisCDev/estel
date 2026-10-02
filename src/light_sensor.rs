//! One-shot illuminance readings. The caller isolates driver I/O in a timed helper.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows::Devices::Sensors::LightSensor;
use windows::Win32::Foundation::E_POINTER;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightSample {
    pub device_id: String,
    pub illuminance_lux: f32,
}

impl LightSample {
    pub fn is_valid(&self) -> bool {
        !self.device_id.is_empty()
            && self.device_id.len() <= 4096
            && self.illuminance_lux.is_finite()
            && (0.0..=200_000.0).contains(&self.illuminance_lux)
    }

    pub fn brightness(&self, low: f32, high: f32) -> f32 {
        // A comfort heuristic within the user's bounds, not a photometric target.
        let fraction = (self.illuminance_lux.ln_1p() / 1000.0_f32.ln_1p()).clamp(0.0, 1.0);
        low + (high - low) * fraction
    }
}

pub fn sample() -> Result<Option<LightSample>, String> {
    let _runtime = Runtime::start()?;
    let sensor = match LightSensor::GetDefault() {
        Ok(sensor) => sensor,
        // The WinRT projection represents a successful null return as E_POINTER.
        Err(error) if error.code() == E_POINTER => return Ok(None),
        Err(error) => return Err(format!("O sensor de luz não está disponível ({error}).")),
    };
    let interval = sensor
        .MinimumReportInterval()
        .map_err(|error| error.to_string())?;
    sensor
        .SetReportInterval(interval.max(1000))
        .map_err(|error| error.to_string())?;
    let sensor = SensorSession(sensor);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match sensor.0.GetCurrentReading() {
            Ok(reading) => {
                let timestamp = reading.Timestamp().map_err(|error| error.to_string())?;
                if reading_is_fresh(timestamp.UniversalTime, SystemTime::now()) {
                    let sample = LightSample {
                        device_id: sensor
                            .0
                            .DeviceId()
                            .map_err(|error| error.to_string())?
                            .to_string(),
                        illuminance_lux: reading
                            .IlluminanceInLux()
                            .map_err(|error| error.to_string())?,
                    };
                    return sample
                        .is_valid()
                        .then_some(Some(sample))
                        .ok_or_else(|| "O sensor de luz retornou uma medida inválida.".to_owned());
                }
            }
            Err(error) if error.code() == E_POINTER => {}
            Err(error) => {
                return Err(format!(
                    "O sensor de luz não forneceu uma leitura ({error})."
                ));
            }
        }
        if Instant::now() >= deadline {
            return Err("O sensor de luz não forneceu uma leitura atual em 3 segundos.".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn reading_is_fresh(timestamp: i64, now: SystemTime) -> bool {
    const WINDOWS_EPOCH_SECONDS: i128 = 11_644_473_600;
    let Ok(elapsed) = now.duration_since(UNIX_EPOCH) else {
        return false;
    };
    let now_ticks = (elapsed.as_secs() as i128 + WINDOWS_EPOCH_SECONDS) * 10_000_000
        + elapsed.subsec_nanos() as i128 / 100;
    (-10_000_000..=30 * 10_000_000).contains(&(now_ticks - timestamp as i128))
}

struct Runtime;

impl Runtime {
    fn start() -> Result<Self, String> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|error| error.to_string())?;
        Ok(Self)
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

struct SensorSession(LightSensor);

impl Drop for SensorSession {
    fn drop(&mut self) {
        if let Err(error) = self.0.SetReportInterval(0) {
            tracing::warn!(%error, "light sensor interval release failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedicated_sensor_readings_reject_invalid_values_and_respect_personal_bounds() {
        let mut sample = LightSample {
            device_id: "als".into(),
            illuminance_lux: 0.0,
        };
        assert!(sample.is_valid());
        assert_eq!(sample.brightness(0.25, 0.85), 0.25);
        sample.illuminance_lux = 100_000.0;
        assert_eq!(sample.brightness(0.25, 0.85), 0.85);
        for value in [f32::NAN, f32::INFINITY, -1.0, 200_001.0] {
            sample.illuminance_lux = value;
            assert!(!sample.is_valid());
        }
    }

    #[test]
    fn stale_or_future_sensor_readings_are_rejected() {
        let now = UNIX_EPOCH + Duration::from_secs(1_000);
        let ticks = (11_644_473_600_i64 + 1_000) * 10_000_000;
        assert!(reading_is_fresh(ticks, now));
        assert!(!reading_is_fresh(ticks - 31 * 10_000_000, now));
        assert!(!reading_is_fresh(ticks + 2 * 10_000_000, now));
    }
}
