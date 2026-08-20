use anyhow::Context;
use embedded_svc::http::client::Client as HttpClient;
use esp_idf_svc::http::client::{Configuration as HttpConfig, EspHttpConnection};

use crate::secrets::{SecretKey, Secrets};

pub use siwaj_core::weather::{GeoMatch, Snapshot, merge_minutely, parse_geocode, parse_hourly};

const ONE_CALL: &str = "https://api.openweathermap.org/data/4.0/onecall";

fn http_get(url: &str) -> anyhow::Result<Vec<u8>> {
    let config = HttpConfig {
        buffer_size: Some(2048),
        buffer_size_tx: Some(1024),
        timeout: Some(core::time::Duration::from_secs(20)),
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        ..Default::default()
    };
    let mut client = HttpClient::wrap(EspHttpConnection::new(&config)?);
    let mut request = client.get(url)?.submit()?;
    anyhow::ensure!(request.status() == 200, "request returned {}", request.status());
    let mut body = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let n = request.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
        anyhow::ensure!(body.len() <= 128 * 1024, "response larger than 128KiB");
    }
    Ok(body)
}

/// One Call 4.0 serves each resolution from its own endpoint, so a cycle
/// spends two requests: the hourly timeline carries the temperature, the rain
/// probability and the zone offset, the 1-minute timeline the precipitation
/// trace that decides rain within the hour.
pub fn fetch(secrets: &Secrets, lat: f64, lon: f64) -> anyhow::Result<Snapshot> {
    let key = secrets
        .get(SecretKey::OpenWeatherApiKey)
        .context("no OPENWEATHER_API_KEY provisioned")?;
    let hourly = http_get(&format!(
        "{ONE_CALL}/timeline/1h?lat={lat}&lon={lon}&units=metric&appid={key}"
    ))
    .context("hourly timeline")?;
    let mut snapshot = parse_hourly(&hourly).map_err(anyhow::Error::msg)?;
    let minutely = http_get(&format!(
        "{ONE_CALL}/timeline/1min?lat={lat}&lon={lon}&appid={key}"
    ))
    .context("minutely timeline")?;
    merge_minutely(&mut snapshot, &minutely).map_err(anyhow::Error::msg)?;
    Ok(snapshot)
}

pub fn geocode(secrets: &Secrets, city: &str) -> anyhow::Result<GeoMatch> {
    let key = secrets
        .get(SecretKey::OpenWeatherApiKey)
        .context("no OPENWEATHER_API_KEY provisioned")?;
    let url = format!(
        "https://api.openweathermap.org/geo/1.0/direct?q={}&limit=1&appid={key}",
        siwaj_core::weather::urlencode(city)
    );
    let body = http_get(&url)?;
    parse_geocode(&body).map_err(anyhow::Error::msg)
}
