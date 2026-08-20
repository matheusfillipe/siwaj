use esp_idf_svc::nvs::{EspNvs, NvsDefault};

const NAMESPACE: &str = "secrets";

/// The closed set of provisionable secrets. NVS keys are capped at 15
/// characters, so each variant maps to a short storage key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKey {
    OpenWeatherApiKey,
    WifiSsid,
    WifiPass,
}

impl SecretKey {
    pub const ALL: [SecretKey; 3] = [
        SecretKey::OpenWeatherApiKey,
        SecretKey::WifiSsid,
        SecretKey::WifiPass,
    ];

    fn nvs_key(self) -> &'static str {
        match self {
            SecretKey::OpenWeatherApiKey => "ow_key",
            SecretKey::WifiSsid => "wifi_ssid",
            SecretKey::WifiPass => "wifi_pass",
        }
    }

    fn env_name(self) -> &'static str {
        match self {
            SecretKey::OpenWeatherApiKey => "OPENWEATHER_API_KEY",
            SecretKey::WifiSsid => "WIFI_SSID",
            SecretKey::WifiPass => "WIFI_PASS",
        }
    }

    fn from_env_name(name: &str) -> Option<SecretKey> {
        Self::ALL
            .into_iter()
            .find(|key| key.env_name() == name)
    }
}

pub struct Secrets {
    nvs: EspNvs<NvsDefault>,
}

pub fn take(
    partition: esp_idf_svc::nvs::EspNvsPartition<NvsDefault>,
) -> Result<Secrets, esp_idf_svc::sys::EspError> {
    let nvs = EspNvs::new(partition, NAMESPACE, true)?;
    Ok(Secrets { nvs })
}

impl Secrets {
    /// None means the secret is unset. NVS-level failures are logged and also
    /// surface as None so callers keep a single fallback path.
    pub fn get(&self, key: SecretKey) -> Option<String> {
        let nvs_key = key.nvs_key();
        let len = match self.nvs.str_len(nvs_key) {
            Ok(Some(len)) => len,
            Ok(None) => return None,
            Err(e) => {
                log::error!("nvs read of {nvs_key} failed: {e}");
                return None;
            }
        };
        let mut buf = vec![0u8; len];
        match self.nvs.get_str(nvs_key, &mut buf) {
            Ok(Some(s)) => Some(s.to_string()),
            Ok(None) => None,
            Err(e) => {
                log::error!("nvs read of {nvs_key} failed: {e}");
                None
            }
        }
    }

    pub fn set(&self, key: SecretKey, value: &str) -> anyhow::Result<()> {
        self.nvs.set_str(key.nvs_key(), value).map_err(anyhow::Error::msg)
    }

    pub fn del(&self, key: SecretKey) -> anyhow::Result<bool> {
        self.nvs.remove(key.nvs_key()).map_err(anyhow::Error::msg)
    }
}

pub fn spawn_repl(secrets: &'static Secrets) {
    std::thread::Builder::new()
        .name("repl".to_string())
        .stack_size(4096)
        .spawn(move || repl_loop(secrets))
        .expect("spawn repl");
}

fn repl_loop(secrets: &'static Secrets) {
    use std::io::BufRead;
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(3, ' ');
        let reply = match (parts.next(), parts.next(), parts.next()) {
            (Some("set"), Some(name), Some(val)) => match SecretKey::from_env_name(name) {
                Some(key) => match secrets.set(key, val) {
                    Ok(()) => format!("OK set {name}"),
                    Err(e) => format!("ERR {e}"),
                },
                None => format!("ERR unknown key {name}"),
            },
            (Some("get"), Some(name), None) => match SecretKey::from_env_name(name) {
                Some(key) => match secrets.get(key) {
                    Some(v) => format!("OK {name}={v}"),
                    None => format!("ERR {name} not set"),
                },
                None => format!("ERR unknown key {name}"),
            },
            (Some("del"), Some(name), None) => match SecretKey::from_env_name(name) {
                Some(key) => match secrets.del(key) {
                    Ok(true) => format!("OK deleted {name}"),
                    Ok(false) => format!("ERR {name} not set"),
                    Err(e) => format!("ERR {e}"),
                },
                None => format!("ERR unknown key {name}"),
            },
            (Some("keys"), None, None) => {
                let mut out = String::from("OK");
                for key in SecretKey::ALL {
                    let known = if secrets.get(key).is_some() {
                        "set"
                    } else {
                        "unset"
                    };
                    out.push_str(&format!(" {}={known}", key.env_name()));
                }
                out
            }
            // The emulator has no BOOT button and cannot deep sleep under
            // QEMU, so the serial line stands in for both. Serial stays up
            // when the HTTP server is down, which is the whole point.
            #[cfg(esp32)]
            (Some("button"), None, None) => {
                crate::press_button();
                "OK button".to_string()
            }
            #[cfg(esp32)]
            (Some("sleep"), None, None) => {
                crate::force_sleep();
                "OK sleeping".to_string()
            }
            _ => "ERR usage: set KEY VAL | get KEY | del KEY | keys".to_string(),
        };
        println!("{reply}");
    }
}
