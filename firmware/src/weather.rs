use esp_idf_svc::http::client::{Configuration as HttpConfig, EspHttpConnection};
use embedded_svc::http::client::Client as HttpClient;
use esp_idf_svc::io::Read;
use serde::Deserialize;

use crate::secrets::Secrets;

#[derive(Debug)]
pub struct Snapshot {
    pub feels_like_c: f32,
    pub minutely_mm: [f32; 60],
    pub hourly_pop: f32,
}

impl Default for Snapshot {
    fn default() -> Self {
        Snapshot {
            feels_like_c: 0.0,
            minutely_mm: [0.0; 60],
            hourly_pop: 0.0,
        }
    }
}

#[derive(Deserialize)]
struct OneCall {
    current: Current,
    minutely: Option<Vec<Minutely>>,
    hourly: Option<Vec<Hourly>>,
}

#[derive(Deserialize)]
struct Current {
    feels_like: f32,
}

#[derive(Deserialize)]
struct Minutely {
    precipitation: Option<f32>,
}

#[derive(Deserialize)]
struct Hourly {
    pop: Option<f32>,
}

#[derive(Deserialize)]
struct GeoHit {
    lat: f64,
    lon: f64,
}

fn http_get(url: &str) -> Result<Vec<u8>, String> {
    let config = HttpConfig {
        buffer_size: Some(2048),
        buffer_size_tx: Some(1024),
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        ..Default::default()
    };
    let mut client = HttpClient::wrap(EspHttpConnection::new(&config).map_err(|e| e.to_string())?);
    let mut request = client
        .get(url)
        .map_err(|e| e.to_string())?
        .submit()
        .map_err(|e| e.to_string())?;
    let status = request.status();
    if status != 200 {
        return Err(format!("request returned {status}"));
    }
    let mut body = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let n = request.read(&mut chunk).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Ok(body)
}

pub fn fetch(secrets: &Secrets, lat: f64, lon: f64) -> Result<Snapshot, String> {
    let key = secrets
        .get("OPENWEATHER_API_KEY")
        .ok_or("no OPENWEATHER_API_KEY provisioned")?;
    let url = format!(
        "https://api.openweathermap.org/data/3.0/onecall?lat={lat}&lon={lon}&exclude=daily,alerts&units=metric&appid={key}"
    );
    let body = http_get(&url)?;
    let parsed: OneCall = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    let mut snapshot = Snapshot {
        feels_like_c: parsed.current.feels_like,
        ..Default::default()
    };
    if let Some(minutely) = parsed.minutely {
        for (i, m) in minutely.iter().take(60).enumerate() {
            snapshot.minutely_mm[i] = m.precipitation.unwrap_or(0.0);
        }
    }
    if let Some(hourly) = parsed.hourly {
        snapshot.hourly_pop = hourly.first().and_then(|h| h.pop).unwrap_or(0.0);
    }
    Ok(snapshot)
}

pub fn geocode(secrets: &Secrets, city: &str) -> Option<(f64, f64)> {
    let key = secrets.get("OPENWEATHER_API_KEY")?;
    let url = format!(
        "https://api.openweathermap.org/geo/1.0/direct?q={}&limit=1&appid={key}",
        urlencode(city)
    );
    let body = http_get(&url).ok()?;
    let hits: Vec<GeoHit> = serde_json::from_slice(&body).ok()?;
    hits.first().map(|h| (h.lat, h.lon))
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
