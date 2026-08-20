//! OpenWeather One Call payload parsing, host-testable and shared with the
//! firmware. The firmware keeps only the HTTP transport shell.

use serde::Deserialize;

use crate::RainOutlook;

pub const MINUTELY_LEN: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snapshot {
    pub feels_like_c: f32,
    pub minutely_mm: [f32; MINUTELY_LEN],
    pub next_hour_pop_frac: f32,
    pub timezone_offset_secs: i32,
}

impl Default for Snapshot {
    fn default() -> Self {
        Snapshot {
            feels_like_c: 0.0,
            minutely_mm: [0.0; MINUTELY_LEN],
            next_hour_pop_frac: 0.0,
            timezone_offset_secs: 0,
        }
    }
}

impl Snapshot {
    pub fn rain_outlook(&self) -> RainOutlook {
        RainOutlook::from_one_call(self.next_hour_pop_frac, &self.minutely_mm)
    }
}

#[derive(Deserialize)]
struct OneCall {
    current: Current,
    minutely: Option<Vec<Minutely>>,
    hourly: Option<Vec<Hourly>>,
    #[serde(default)]
    timezone_offset: i32,
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

/// Parses a One Call response body. Err carries a short reason suitable for logs.
pub fn parse_one_call(body: &[u8]) -> Result<Snapshot, String> {
    let parsed: OneCall =
        serde_json::from_slice(body).map_err(|e| format!("one call payload: {e}"))?;
    let mut snapshot = Snapshot {
        feels_like_c: parsed.current.feels_like,
        timezone_offset_secs: parsed.timezone_offset,
        ..Default::default()
    };
    if let Some(minutely) = parsed.minutely {
        for (slot, entry) in snapshot.minutely_mm.iter_mut().zip(minutely) {
            *slot = entry.precipitation.unwrap_or(0.0);
        }
    }
    if let Some(hourly) = parsed.hourly {
        snapshot.next_hour_pop_frac = hourly.first().and_then(|h| h.pop).unwrap_or(0.0);
    }
    Ok(snapshot)
}

/// Parses a geocoding /direct response body, returning the first hit.
/// Err carries a short reason suitable for logs and 422 bodies.
pub fn parse_geocode(body: &[u8]) -> Result<(f64, f64), String> {
    let hits: Vec<GeoHit> =
        serde_json::from_slice(body).map_err(|e| format!("geocode payload: {e}"))?;
    let Some(first) = hits.first() else {
        return Err("no match for that city name".to_string());
    };
    Ok((first.lat, first.lon))
}

/// Percent-encodes a city name for a query string.
pub fn urlencode(s: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn full_payload() -> serde_json::Value {
        serde_json::json!({
            "timezone_offset": 3600,
            "current": {"feels_like": 17.5},
            "minutely": (0..60).map(|i| serde_json::json!({"precipitation": if i == 5 { 0.4 } else { 0.0 }})).collect::<Vec<_>>(),
            "hourly": [{"pop": 0.25}, {"pop": 0.9}]
        })
    }

    #[test]
    fn parses_full_payload() {
        let body = serde_json::to_vec(&full_payload()).unwrap();
        let snap = parse_one_call(&body).unwrap();
        assert_eq!(snap.feels_like_c, 17.5);
        assert_eq!(snap.timezone_offset_secs, 3600);
        assert_eq!(snap.next_hour_pop_frac, 0.25);
        assert_eq!(snap.minutely_mm[5], 0.4);
        assert_eq!(snap.minutely_mm.len(), 60);
        let rain = snap.rain_outlook();
        assert_eq!(rain.pop_pct_next_hour, 25);
        assert!(rain.rain_expected);
    }

    #[test]
    fn missing_minutely_and_hourly_defaults_to_zero() {
        let body = br#"{"current": {"feels_like": 3.0}}"#;
        let snap = parse_one_call(body).unwrap();
        assert_eq!(snap.feels_like_c, 3.0);
        assert_eq!(snap.next_hour_pop_frac, 0.0);
        assert!(snap.minutely_mm.iter().all(|&mm| mm == 0.0));
    }

    #[test]
    fn short_minutely_list_pads_with_zero() {
        let body = br#"{"current": {"feels_like": 1.0}, "minutely": [{"precipitation": 0.7}]}"#;
        let snap = parse_one_call(body).unwrap();
        assert_eq!(snap.minutely_mm[0], 0.7);
        assert!(snap.minutely_mm[1..].iter().all(|&mm| mm == 0.0));
        assert!(snap.rain_outlook().rain_expected);
    }

    #[test]
    fn null_precipitation_reads_as_zero() {
        let body = br#"{"current": {"feels_like": 1.0}, "minutely": [{"precipitation": null}]}"#;
        let snap = parse_one_call(body).unwrap();
        assert_eq!(snap.minutely_mm[0], 0.0);
        assert!(!snap.rain_outlook().rain_expected);
    }

    #[test]
    fn malformed_body_is_rejected() {
        assert!(parse_one_call(b"not json").is_err());
        assert!(parse_one_call(br#"{"no_current": true}"#).is_err());
    }

    #[test]
    fn geocode_takes_first_hit() {
        let body = br#"[{"lat": 52.5, "lon": 13.4}, {"lat": 1.0, "lon": 2.0}]"#;
        assert_eq!(parse_geocode(body), Ok((52.5, 13.4)));
    }

    #[test]
    fn geocode_distinguishes_failure_modes() {
        assert_eq!(
            parse_geocode(b"[]"),
            Err("no match for that city name".to_string())
        );
        assert!(parse_geocode(b"{}").is_err());
        assert!(parse_geocode(b"").is_err());
    }

    #[test]
    fn urlencode_encodes_reserved_characters() {
        assert_eq!(urlencode("Berlin"), "Berlin");
        assert_eq!(urlencode("São Paulo"), "S%C3%A3o%20Paulo");
        assert_eq!(urlencode("a/b~c"), "a%2Fb~c");
    }
}
