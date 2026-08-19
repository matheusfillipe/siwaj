use siwaj_core::{Config, CONFIG_SCHEMA_VERSION};

use esp_idf_svc::nvs::{EspNvs, NvsDefault};

const NAMESPACE: &str = "siwaj";
const KEY_CONFIG: &str = "config";

pub struct Store {
    nvs: EspNvs<NvsDefault>,
}

pub fn take(
    partition: esp_idf_svc::nvs::EspNvsPartition<NvsDefault>,
) -> Result<Store, esp_idf_svc::sys::EspError> {
    let nvs = EspNvs::new(partition, NAMESPACE, true)?;
    Ok(Store { nvs })
}

impl Store {
    pub fn load(&self) -> Option<Config> {
        let len = self.nvs.blob_len(KEY_CONFIG).ok()??;
        let mut buf = vec![0u8; len];
        let _ = self.nvs.get_blob(KEY_CONFIG, &mut buf).ok()??;
        let raw = serde_json::from_slice::<serde_json::Value>(&buf).ok()?;
        siwaj_core::migrate(raw).ok()
    }

    pub fn save(&self, config: &Config) -> Result<(), String> {
        config
            .validate()
            .map_err(|e| format!("invalid config: {e}"))?;
        let buf = serde_json::to_vec(config).map_err(|e| e.to_string())?;
        self.nvs
            .set_blob(KEY_CONFIG, &buf)
            .map_err(|e| e.to_string())
    }

    pub fn next_revision(&self, current: Option<&Config>) -> u32 {
        current.map(|c| c.revision.wrapping_add(1)).unwrap_or(1)
    }

    pub fn now_unix(&self) -> u32 {
        let secs = unsafe { esp_idf_svc::sys::time(std::ptr::null_mut()) };
        u32::try_from(secs).unwrap_or(0)
    }

    pub fn config_from_submit(
        &self,
        submit: siwaj_core::ConfigSubmit,
        geocode: Option<(f64, f64)>,
    ) -> Result<Config, String> {
        let current = self.load();
        let (lat, lon) = geocode.unwrap_or((0.0, 0.0));
        let config = Config {
            schema_version: CONFIG_SCHEMA_VERSION,
            revision: self.next_revision(current.as_ref()),
            date_modified_unix: self.now_unix(),
            thresholds: submit.thresholds,
            rain_threshold_pct: submit.rain_threshold_pct,
            refresh_minutes: submit.refresh_minutes,
            location: siwaj_core::Location {
                name: submit.location_name,
                lat,
                lon,
            },
        };
        config
            .validate()
            .map_err(|e| format!("invalid config: {e}"))?;
        Ok(config)
    }
}
