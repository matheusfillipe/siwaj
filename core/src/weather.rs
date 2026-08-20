//! OpenWeather One Call 4.0 payload parsing, host-testable and shared with
//! the firmware. The firmware keeps only the HTTP transport shell.
//!
//! 4.0 splits what one request used to return across per-resolution timeline
//! endpoints, so a cycle reads the hourly timeline for the temperature, the
//! rain probability and the zone offset, then the 1-minute timeline for the
//! precipitation trace. Both fold into one `Snapshot`.

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
struct Timeline<T> {
    data: Vec<T>,
    #[serde(default)]
    timezone_offset: i32,
}

#[derive(Deserialize)]
struct HourStep {
    feels_like: f32,
    pop: Option<f32>,
}

#[derive(Deserialize)]
struct MinuteStep {
    precipitation: Option<f32>,
}

#[derive(Deserialize)]
struct GeoHit {
    lat: f64,
    lon: f64,
    country: Option<String>,
    state: Option<String>,
}

/// Where a city name resolved to. `state` and `country` are what let someone
/// tell their Springfield from the other one, so they travel with the fix
/// rather than being dropped at the parser.
#[derive(Debug, Clone, PartialEq)]
pub struct GeoMatch {
    pub lat: f64,
    pub lon: f64,
    pub region: Option<String>,
    pub country: Option<String>,
}

/// Parses a `onecall/timeline/1h` body into the part of a snapshot that does
/// not involve precipitation. `data[0]` is the current hour bucket, so its
/// `feels_like` is the hour's value rather than an instantaneous reading.
/// Err carries a short reason suitable for logs.
pub fn parse_hourly(body: &[u8]) -> Result<Snapshot, String> {
    let parsed: Timeline<HourStep> =
        serde_json::from_slice(body).map_err(|e| format!("hourly timeline payload: {e}"))?;
    let current = parsed
        .data
        .first()
        .ok_or_else(|| "hourly timeline carries no steps".to_string())?;
    Ok(Snapshot {
        feels_like_c: current.feels_like,
        next_hour_pop_frac: current.pop.unwrap_or(0.0),
        timezone_offset_secs: parsed.timezone_offset,
        ..Default::default()
    })
}

/// Folds a `onecall/timeline/1min` body into a snapshot. A short trace pads
/// with zero: fewer steps means no forecast for those minutes, not rain.
pub fn merge_minutely(snapshot: &mut Snapshot, body: &[u8]) -> Result<(), String> {
    let parsed: Timeline<MinuteStep> =
        serde_json::from_slice(body).map_err(|e| format!("minutely timeline payload: {e}"))?;
    for (slot, step) in snapshot.minutely_mm.iter_mut().zip(parsed.data) {
        *slot = step.precipitation.unwrap_or(0.0);
    }
    Ok(())
}

/// Parses a geocoding /direct response body, returning the first hit.
/// Err carries a short reason suitable for logs and 422 bodies.
pub fn parse_geocode(body: &[u8]) -> Result<GeoMatch, String> {
    let hits: Vec<GeoHit> =
        serde_json::from_slice(body).map_err(|e| format!("geocode payload: {e}"))?;
    let Some(first) = hits.into_iter().next() else {
        return Err("no match for that city name".to_string());
    };
    Ok(GeoMatch {
        lat: first.lat,
        lon: first.lon,
        region: first.state,
        country: first.country,
    })
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

    /// Shaped after a live `onecall/timeline/1h` response, trimmed to the
    /// fields the parser names.
    fn hourly_payload() -> serde_json::Value {
        serde_json::json!({
            "lat": 52.2096,
            "lon": 7.1886,
            "timezone": "Europe/Berlin",
            "timezone_offset": 3600,
            "data": [
                {"dt": 1787252400, "temp": 18.0, "feels_like": 17.5, "humidity": 85, "pop": 0.25},
                {"dt": 1787256000, "temp": 17.0, "feels_like": 16.4, "humidity": 88, "pop": 0.9}
            ]
        })
    }

    fn minutely_payload(wet_minute: usize) -> serde_json::Value {
        serde_json::json!({
            "timezone_offset": 3600,
            "data": (0..60)
                .map(|i| serde_json::json!({
                    "dt": 1787252940 + i * 60,
                    "precipitation": if i == wet_minute as i64 { 0.4 } else { 0.0 }
                }))
                .collect::<Vec<_>>()
        })
    }

    #[test]
    fn parses_both_timelines_into_one_snapshot() {
        let mut snap = parse_hourly(&serde_json::to_vec(&hourly_payload()).unwrap()).unwrap();
        assert_eq!(snap.feels_like_c, 17.5);
        assert_eq!(snap.timezone_offset_secs, 3600);
        assert_eq!(snap.next_hour_pop_frac, 0.25);
        assert!(snap.minutely_mm.iter().all(|&mm| mm == 0.0));

        merge_minutely(
            &mut snap,
            &serde_json::to_vec(&minutely_payload(5)).unwrap(),
        )
        .unwrap();
        assert_eq!(snap.minutely_mm[5], 0.4);
        assert_eq!(snap.minutely_mm.len(), MINUTELY_LEN);
        assert_eq!(snap.feels_like_c, 17.5, "merging must not disturb the hour");

        let rain = snap.rain_outlook();
        assert_eq!(rain.pop_pct_next_hour, 25);
        assert!(rain.rain_expected);
    }

    #[test]
    fn hourly_without_pop_reads_as_zero() {
        let body = br#"{"timezone_offset": 0, "data": [{"feels_like": 3.0}]}"#;
        let snap = parse_hourly(body).unwrap();
        assert_eq!(snap.feels_like_c, 3.0);
        assert_eq!(snap.next_hour_pop_frac, 0.0);
        assert!(!snap.rain_outlook().rain_expected);
    }

    #[test]
    fn short_minutely_trace_pads_with_zero() {
        let mut snap = Snapshot::default();
        merge_minutely(&mut snap, br#"{"data": [{"precipitation": 0.7}]}"#).unwrap();
        assert_eq!(snap.minutely_mm[0], 0.7);
        assert!(snap.minutely_mm[1..].iter().all(|&mm| mm == 0.0));
        assert!(snap.rain_outlook().rain_expected);
    }

    #[test]
    fn null_precipitation_reads_as_zero() {
        let mut snap = Snapshot::default();
        merge_minutely(&mut snap, br#"{"data": [{"precipitation": null}]}"#).unwrap();
        assert_eq!(snap.minutely_mm[0], 0.0);
        assert!(!snap.rain_outlook().rain_expected);
    }

    #[test]
    fn malformed_bodies_are_rejected() {
        assert!(parse_hourly(b"not json").is_err());
        assert!(
            parse_hourly(br#"{"data": []}"#).is_err(),
            "no steps is an error, not a zeroed frame"
        );
        assert!(parse_hourly(br#"{"no_data": true}"#).is_err());
        assert!(merge_minutely(&mut Snapshot::default(), b"not json").is_err());
    }

    #[test]
    fn a_failed_merge_leaves_the_hourly_half_intact() {
        let mut snap = parse_hourly(&serde_json::to_vec(&hourly_payload()).unwrap()).unwrap();
        assert!(merge_minutely(&mut snap, b"{{{").is_err());
        assert_eq!(snap.feels_like_c, 17.5);
        assert_eq!(snap.next_hour_pop_frac, 0.25);
    }

    #[test]
    fn geocode_takes_first_hit_with_its_region() {
        let body = br#"[
            {"lat": 52.5, "lon": 13.4, "country": "DE", "state": "Berlin"},
            {"lat": 1.0, "lon": 2.0}
        ]"#;
        assert_eq!(
            parse_geocode(body),
            Ok(GeoMatch {
                lat: 52.5,
                lon: 13.4,
                region: Some("Berlin".to_string()),
                country: Some("DE".to_string()),
            })
        );
    }

    #[test]
    fn geocode_tolerates_a_hit_without_a_region() {
        let body = br#"[{"lat": 1.5, "lon": 2.5}]"#;
        let hit = parse_geocode(body).unwrap();
        assert_eq!((hit.lat, hit.lon), (1.5, 2.5));
        assert_eq!((hit.region, hit.country), (None, None));
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
