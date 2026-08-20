use anyhow::{Context, Result};
use siwaj_core::Config;

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
    /// Ok(None): unconfigured (no stored blob). Err: a blob exists but cannot
    /// be parsed or migrated; callers should log and enter config mode.
    pub fn load(&self) -> Result<Option<Config>> {
        let Some(len) = self.nvs.blob_len(KEY_CONFIG).map_err(anyhow::Error::msg)? else {
            return Ok(None);
        };
        let mut buf = vec![0u8; len];
        self.nvs
            .get_blob(KEY_CONFIG, &mut buf)
            .map_err(anyhow::Error::msg)?;
        let raw = serde_json::from_slice::<serde_json::Value>(&buf)
            .context("stored config is not valid JSON")?;
        siwaj_core::migrate(raw)
            .map(Some)
            .context("stored config failed migration")
    }

    pub fn save(&self, config: &Config) -> Result<()> {
        config.validate().context("invalid config")?;
        let buf = serde_json::to_vec(config).map_err(anyhow::Error::msg)?;
        self.nvs
            .set_blob(KEY_CONFIG, &buf)
            .map_err(anyhow::Error::msg)
    }
}
