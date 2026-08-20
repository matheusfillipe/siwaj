use anyhow::Context;
use embedded_svc::http::client::Client as HttpClient;
use esp_idf_svc::http::client::{Configuration as HttpConfig, EspHttpConnection};

use crate::secrets::{SecretKey, Secrets};

pub use siwaj_core::weather::{parse_geocode, parse_one_call, Snapshot};

fn http_get(url: &str) -> anyhow::Result<Vec<u8>> {
    let config = HttpConfig {
        buffer_size: Some(2048),
        buffer_size_tx: Some(1024),
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
    }
    Ok(body)
}

pub fn fetch(secrets: &Secrets, lat: f64, lon: f64) -> anyhow::Result<Snapshot> {
    let key = secrets
        .get(SecretKey::OpenWeatherApiKey)
        .context("no OPENWEATHER_API_KEY provisioned")?;
    let url = format!(
        "https://api.openweathermap.org/data/3.0/onecall?lat={lat}&lon={lon}&exclude=daily,alerts&units=metric&appid={key}"
    );
    let body = http_get(&url)?;
    parse_one_call(&body).map_err(anyhow::Error::msg)
}

pub fn geocode(secrets: &Secrets, city: &str) -> Option<(f64, f64)> {
    let key = secrets.get(SecretKey::OpenWeatherApiKey)?;
    let url = format!(
        "https://api.openweathermap.org/geo/1.0/direct?q={}&limit=1&appid={key}",
        siwaj_core::weather::urlencode(city)
    );
    let body = http_get(&url).ok()?;
    parse_geocode(&body)
}
