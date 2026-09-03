//! Shared contract for siwaj: the user configuration types, the clothing and
//! rain decision logic, the OpenWeather payload parsing, and the e-paper
//! render pipeline. Host-testable; the firmware and the generated TypeScript
//! bindings (ts-rs, camelCase wire format) both consume this crate.

use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub mod render;
pub mod weather;

pub const CONFIG_SCHEMA_VERSION: u16 = 3;
pub const REFRESH_MINUTES_DEFAULT: u16 = 30;
pub const RAIN_THRESHOLD_PCT_DEFAULT: u8 = 30;

/// The range the thresholds may occupy. These are the points where a garment
/// starts being needed, not the temperatures a place can reach, so the window
/// is narrower than any real climate. The config page draws its axis to the
/// same bounds, and this check is what keeps the two from drifting.
pub const THRESHOLD_MIN_C: f32 = -10.0;
pub const THRESHOLD_MAX_C: f32 = 30.0;

const REFRESH_MINUTES_MIN: u16 = 5;
const REFRESH_MINUTES_MAX: u16 = 240;
const MINUTELY_RAIN_MM_THRESHOLD: f32 = 0.1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
/// `region` and `country` come back from geocoding and exist so the page can
/// show which of the world's several Springfields it actually resolved to.
pub struct Location {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub region: Option<String>,
    pub country: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct Thresholds {
    pub low_c: f32,
    pub high_c: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Config {
    pub schema_version: u16,
    pub revision: u32,
    pub date_modified_unix: u32,
    pub thresholds: Thresholds,
    pub rain_threshold_pct: u8,
    pub refresh_minutes: u16,
    /// How long the config page stays served after the last request. Short
    /// values are what make the sleep path testable without a ten minute wait.
    pub awake_minutes: u16,
    pub location: Location,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct ConfigSubmit {
    pub thresholds: Thresholds,
    pub rain_threshold_pct: u8,
    pub refresh_minutes: u16,
    pub awake_minutes: u16,
    pub location_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConfigState {
    pub configured: bool,
    pub revision: u32,
    pub config: Option<Config>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WeatherProbe {
    pub feels_like_c: f32,
    pub next_hour_pop_frac: f32,
    pub max_minutely_mm: f32,
    pub timezone_offset_secs: i32,
}

/// How long config mode keeps serving after the last request, when there is
/// no stored config to say otherwise. Long enough to finish a setup, short
/// enough that a stray button press does not hold the radio on until the
/// battery is flat.
pub const AWAKE_MINUTES_DEFAULT: u16 = 10;
const AWAKE_MINUTES_MIN: u16 = 1;
const AWAKE_MINUTES_MAX: u16 = 60;

pub const CONFIG_MODE_IDLE: core::time::Duration =
    core::time::Duration::from_secs(AWAKE_MINUTES_DEFAULT as u64 * 60);

/// What the config page needs to say whether the device is about to drop off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DeviceStatus {
    pub seconds_until_sleep: u32,
    /// Cell voltage as the board reads it, for comparing against a meter at the
    /// battery terminals. Absent until a reading has been taken, and on a build
    /// with no battery sense line.
    pub battery_mv: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum Garment {
    Jacket,
    Pullover,
    Shirt,
}

impl Garment {
    pub fn from_feels_like(feels_like_c: f32, t: &Thresholds) -> Garment {
        if feels_like_c < t.low_c {
            Garment::Jacket
        } else if feels_like_c < t.high_c {
            Garment::Pullover
        } else {
            Garment::Shirt
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RainOutlook {
    pub pop_pct_next_hour: u8,
    pub rain_expected: bool,
}

impl RainOutlook {
    pub fn from_one_call(hourly_pop: f32, minutely_precip_mm: &[f32]) -> RainOutlook {
        let pop = hourly_pop.clamp(0.0, 1.0) * 100.0;
        RainOutlook {
            pop_pct_next_hour: pop.round() as u8,
            rain_expected: minutely_precip_mm
                .iter()
                .any(|&mm| mm >= MINUTELY_RAIN_MM_THRESHOLD),
        }
    }

    pub fn is_risk(&self, rain_threshold_pct: u8) -> bool {
        self.rain_expected || self.pop_pct_next_hour >= rain_threshold_pct
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    ThresholdOrder,
    ThresholdRange,
    RainThreshold,
    RefreshWindow,
    AwakeWindow,
    Latitude,
    Longitude,
    SchemaVersion,
    UnsupportedSchema(u16),
    Malformed,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::ThresholdOrder => write!(f, "thresholds must satisfy low < high"),
            ConfigError::ThresholdRange => write!(
                f,
                "thresholds must be within {THRESHOLD_MIN_C}..={THRESHOLD_MAX_C} C"
            ),
            ConfigError::RainThreshold => write!(f, "rain threshold must be 0..=100"),
            ConfigError::RefreshWindow => write!(f, "refresh minutes must be 5..=240"),
            ConfigError::AwakeWindow => write!(
                f,
                "awake minutes must be {AWAKE_MINUTES_MIN}..={AWAKE_MINUTES_MAX}"
            ),
            ConfigError::Latitude => write!(f, "latitude must be -90..=90"),
            ConfigError::Longitude => write!(f, "longitude must be -180..=180"),
            ConfigError::SchemaVersion => write!(f, "missing or invalid schemaVersion"),
            ConfigError::UnsupportedSchema(v) => write!(f, "unsupported schema version {v}"),
            ConfigError::Malformed => write!(f, "config payload does not match the schema"),
        }
    }
}

impl Error for ConfigError {}

/// How soon to re-fetch after a cycle that fell back to the offline frame.
/// Short enough that a device recovers on its own once the network or the
/// upstream plan comes back.
pub const OFFLINE_RETRY: core::time::Duration = core::time::Duration::from_secs(60);

impl Config {
    pub fn refresh_interval(&self) -> core::time::Duration {
        core::time::Duration::from_secs(self.refresh_minutes as u64 * 60)
    }

    pub fn awake_window(&self) -> core::time::Duration {
        core::time::Duration::from_secs(self.awake_minutes as u64 * 60)
    }

    pub fn next_fetch_delay(&self, last_was_live: bool) -> core::time::Duration {
        if last_was_live {
            self.refresh_interval()
        } else {
            OFFLINE_RETRY
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.thresholds.low_c >= self.thresholds.high_c {
            return Err(ConfigError::ThresholdOrder);
        }
        let within = |c: f32| (THRESHOLD_MIN_C..=THRESHOLD_MAX_C).contains(&c);
        if !within(self.thresholds.low_c) || !within(self.thresholds.high_c) {
            return Err(ConfigError::ThresholdRange);
        }
        if self.rain_threshold_pct > 100 {
            return Err(ConfigError::RainThreshold);
        }
        if !(REFRESH_MINUTES_MIN..=REFRESH_MINUTES_MAX).contains(&self.refresh_minutes) {
            return Err(ConfigError::RefreshWindow);
        }
        if !(AWAKE_MINUTES_MIN..=AWAKE_MINUTES_MAX).contains(&self.awake_minutes) {
            return Err(ConfigError::AwakeWindow);
        }
        if !(-90.0..=90.0).contains(&self.location.lat) {
            return Err(ConfigError::Latitude);
        }
        if !(-180.0..=180.0).contains(&self.location.lon) {
            return Err(ConfigError::Longitude);
        }
        if self.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedSchema(self.schema_version));
        }
        Ok(())
    }

    pub fn example() -> Config {
        Config {
            schema_version: CONFIG_SCHEMA_VERSION,
            revision: 0,
            date_modified_unix: 0,
            thresholds: Thresholds {
                low_c: 8.0,
                high_c: 18.0,
            },
            rain_threshold_pct: RAIN_THRESHOLD_PCT_DEFAULT,
            refresh_minutes: REFRESH_MINUTES_DEFAULT,
            awake_minutes: AWAKE_MINUTES_DEFAULT,
            location: Location {
                name: "Example City".to_string(),
                lat: 52.52,
                lon: 13.405,
                region: None,
                country: None,
            },
        }
    }
}

pub fn migrate(raw: serde_json::Value) -> Result<Config, ConfigError> {
    let version = raw
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        .ok_or(ConfigError::SchemaVersion)?;
    if u16::try_from(version) != Ok(CONFIG_SCHEMA_VERSION) {
        return Err(ConfigError::UnsupportedSchema(
            u16::try_from(version).unwrap_or(u16::MAX),
        ));
    }
    let config: Config = serde_json::from_value(raw).map_err(|_| ConfigError::Malformed)?;
    config.validate()?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::time::Duration;
    use serde_json::json;

    fn valid() -> Config {
        Config::example()
    }

    #[test]
    fn fetch_delay_backs_off_only_while_offline() {
        let mut config = valid();
        config.refresh_minutes = 30;
        assert_eq!(config.refresh_interval(), Duration::from_secs(1800));
        assert_eq!(config.next_fetch_delay(true), Duration::from_secs(1800));
        assert_eq!(config.next_fetch_delay(false), OFFLINE_RETRY);
        assert!(OFFLINE_RETRY < config.refresh_interval());
    }

    #[test]
    fn garment_boundaries() {
        let t = Thresholds {
            low_c: 10.0,
            high_c: 22.0,
        };
        assert_eq!(Garment::from_feels_like(9.9, &t), Garment::Jacket);
        assert_eq!(Garment::from_feels_like(10.0, &t), Garment::Pullover);
        assert_eq!(Garment::from_feels_like(21.9, &t), Garment::Pullover);
        assert_eq!(Garment::from_feels_like(22.0, &t), Garment::Shirt);
    }

    #[test]
    fn rain_outlook_from_one_call() {
        let empty = RainOutlook::from_one_call(0.25, &[0.0; 60]);
        assert_eq!(empty.pop_pct_next_hour, 25);
        assert!(!empty.rain_expected);

        let wet = RainOutlook::from_one_call(0.25, &[0.0, 0.3, 0.0]);
        assert!(wet.rain_expected);

        let clamped = RainOutlook::from_one_call(1.4, &[]);
        assert_eq!(clamped.pop_pct_next_hour, 100);
    }

    #[test]
    fn rain_risk_semantics() {
        let dry = RainOutlook {
            pop_pct_next_hour: 20,
            rain_expected: false,
        };
        assert!(!dry.is_risk(30));
        assert!(dry.is_risk(20));
        let spotted = RainOutlook {
            pop_pct_next_hour: 5,
            rain_expected: true,
        };
        assert!(spotted.is_risk(30));
    }

    #[test]
    fn validate_accepts_example() {
        assert_eq!(valid().validate(), Ok(()));
    }

    #[test]
    fn validate_rejects_disordered_thresholds() {
        let mut c = valid();
        c.thresholds = Thresholds {
            low_c: 30.0,
            high_c: 10.0,
        };
        assert_eq!(c.validate(), Err(ConfigError::ThresholdOrder));
    }

    #[test]
    fn validate_rejects_thresholds_outside_the_axis() {
        let mut c = valid();
        c.thresholds = Thresholds {
            low_c: THRESHOLD_MIN_C - 0.5,
            high_c: 10.0,
        };
        assert_eq!(c.validate(), Err(ConfigError::ThresholdRange));

        let mut c = valid();
        c.thresholds = Thresholds {
            low_c: 10.0,
            high_c: THRESHOLD_MAX_C + 0.5,
        };
        assert_eq!(c.validate(), Err(ConfigError::ThresholdRange));
    }

    #[test]
    fn a_submit_carrying_a_foreign_field_is_refused() {
        // the four-garment page sent midC; taking the fields that happen to
        // line up would silently redefine what highC means
        let stale = json!({
            "thresholds": {"lowC": 8.0, "midC": 15.0, "highC": 21.0},
            "rainThresholdPct": 30,
            "refreshMinutes": 30,
            "locationName": "Berlin",
        });
        assert!(serde_json::from_value::<ConfigSubmit>(stale).is_err());
    }

    #[test]
    fn validate_rejects_out_of_range_fields() {
        let mut c = valid();
        c.rain_threshold_pct = 101;
        assert_eq!(c.validate(), Err(ConfigError::RainThreshold));

        let mut c = valid();
        c.refresh_minutes = 4;
        assert_eq!(c.validate(), Err(ConfigError::RefreshWindow));

        let mut c = valid();
        c.refresh_minutes = 241;
        assert_eq!(c.validate(), Err(ConfigError::RefreshWindow));

        let mut c = valid();
        c.location.lat = 91.0;
        assert_eq!(c.validate(), Err(ConfigError::Latitude));

        let mut c = valid();
        c.location.lon = -181.0;
        assert_eq!(c.validate(), Err(ConfigError::Longitude));
    }

    #[test]
    fn serde_uses_camel_case() {
        let c = valid();
        let raw = serde_json::to_value(&c).unwrap();
        assert!(raw.get("rainThresholdPct").is_some());
        assert!(raw.get("schemaVersion").is_some());
        assert!(raw.get("dateModifiedUnix").is_some());
        let round: Config = serde_json::from_value(raw).unwrap();
        assert_eq!(round, c);
    }

    #[test]
    fn migrate_round_trips_valid_config() {
        let raw = serde_json::to_value(valid()).unwrap();
        assert_eq!(migrate(raw).unwrap(), valid());
    }

    #[test]
    fn migrate_rejects_unknown_and_missing_schema() {
        let mut raw = serde_json::to_value(valid()).unwrap();
        raw["schemaVersion"] = json!(99);
        assert_eq!(migrate(raw), Err(ConfigError::UnsupportedSchema(99)));

        let mut raw = serde_json::to_value(valid()).unwrap();
        raw.as_object_mut().unwrap().remove("schemaVersion");
        assert_eq!(migrate(raw), Err(ConfigError::SchemaVersion));

        let bad = json!({"schemaVersion": CONFIG_SCHEMA_VERSION, "hello": "world"});
        assert_eq!(migrate(bad), Err(ConfigError::Malformed));
    }

    #[test]
    fn migrate_rejects_an_older_schema() {
        let old = json!({
            "schemaVersion": 1,
            "revision": 4,
            "dateModifiedUnix": 0,
            "thresholds": {"lowC": 8.0, "midC": 15.0, "highC": 21.0},
            "rainThresholdPct": 30,
            "refreshMinutes": 30,
            "location": {"name": "Berlin", "lat": 52.5, "lon": 13.4},
        });
        assert_eq!(migrate(old), Err(ConfigError::UnsupportedSchema(1)));
    }
}
