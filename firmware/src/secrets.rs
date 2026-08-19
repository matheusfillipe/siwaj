use esp_idf_svc::nvs::{EspNvs, NvsDefault};

const NAMESPACE: &str = "secrets";

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
    pub fn get(&self, key: &str) -> Option<String> {
        let len = self.nvs.str_len(key).ok()??;
        let mut buf = vec![0u8; len];
        let s = self.nvs.get_str(key, &mut buf).ok()??;
        Some(s.to_string())
    }

    pub fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.nvs.set_str(key, value).map_err(|e| e.to_string())
    }

    pub fn del(&self, key: &str) -> Result<bool, String> {
        self.nvs.remove(key).map_err(|e| e.to_string())
    }

    pub fn keys(&self) -> Vec<String> {
        vec![
            "OPENWEATHER_API_KEY".to_string(),
            "WIFI_SSID".to_string(),
            "WIFI_PASS".to_string(),
        ]
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
            (Some("set"), Some(key), Some(val)) => match secrets.set(key, val) {
                Ok(()) => format!("OK set {key}"),
                Err(e) => format!("ERR {e}"),
            },
            (Some("get"), Some(key), None) => match secrets.get(key) {
                Some(v) => format!("OK {key}={v}"),
                None => format!("ERR {key} not set"),
            },
            (Some("del"), Some(key), None) => match secrets.del(key) {
                Ok(true) => format!("OK deleted {key}"),
                Ok(false) => format!("ERR {key} not set"),
                Err(e) => format!("ERR {e}"),
            },
            (Some("keys"), None, None) => {
                let mut out = String::from("OK");
                for k in secrets.keys() {
                    let known = secrets.get(&k).map(|_| "set").unwrap_or("unset");
                    out.push_str(&format!(" {k}={known}"));
                }
                out
            }
            _ => "ERR usage: set KEY VAL | get KEY | del KEY | keys".to_string(),
        };
        println!("{reply}");
    }
}
