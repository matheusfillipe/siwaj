#[cfg(esp32)]
use esp_idf_svc::eth::{BlockingEth, EspEth, OpenEth};
#[cfg(esp32s3)]
use esp_idf_svc::wifi::{AccessPointConfiguration, AuthMethod, BlockingWifi, ClientConfiguration, Configuration as WifiConfiguration, EspWifi};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::ipv4::IpInfo;

#[cfg(esp32)]
pub struct NetUp {
    pub _eth: BlockingEth<EspEth<'static, OpenEth>>,
}

#[cfg(esp32s3)]
pub struct NetUp {
    pub wifi: BlockingWifi<EspWifi<'static>>,
}

#[cfg(esp32)]
pub fn bring_up(
    mac: esp_idf_svc::hal::mac::MAC<'static>,
    sys_loop: EspSystemEventLoop,
) -> Result<NetUp, anyhow::Error> {
    let driver = esp_idf_svc::eth::EthDriver::new_openeth(mac, sys_loop.clone())?;
    let eth = EspEth::wrap(driver)?;
    let mut eth = BlockingEth::wrap(eth, sys_loop)?;
    eth.start()?;
    eth.wait_netif_up()?;
    let ip_info = eth.eth().netif().get_ip_info()?;
    log::info!("openeth up: {ip_info:?}");
    Ok(NetUp { _eth: eth })
}

#[cfg(esp32s3)]
pub fn bring_up(
    modem: esp_idf_svc::hal::modem::Modem<'static>,
    sys_loop: EspSystemEventLoop,
    ssid: Option<String>,
    pass: Option<String>,
    nvs: esp_idf_svc::nvs::EspNvsPartition<esp_idf_svc::nvs::NvsDefault>,
) -> Result<NetUp, anyhow::Error> {
    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
    )?;

    let ap_mode = match (ssid, pass) {
        (Some(ssid), Some(pass)) if !ssid.is_empty() => {
            let conf = WifiConfiguration::Client(ClientConfiguration {
                ssid: ssid
                    .as_str()
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("ssid too long"))?,
                password: pass
                    .as_str()
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("password too long"))?,
                auth_method: AuthMethod::WPA2Personal,
                ..Default::default()
            });
            wifi.set_configuration(&conf)?;
            wifi.start()?;
            wifi.connect()?;
            wait_for_sta_ip(&wifi, 20)?;
            false
        }
        _ => {
            wifi.set_configuration(&WifiConfiguration::AccessPoint(AccessPointConfiguration {
                ssid: "siwaj".try_into()?,
                auth_method: AuthMethod::None,
                ..Default::default()
            }))?;
            wifi.start()?;
            wifi.wait_netif_up()?;
            log::info!("softAP 'siwaj' up (no wifi credentials provisioned)");
            true
        }
    };
    // the softAP branch never starts the STA interface, so read the AP netif there
    let netif = if ap_mode {
        wifi.wifi().ap_netif()
    } else {
        wifi.wifi().sta_netif()
    };
    let ip_info: IpInfo = netif.get_ip_info()?;
    log::info!("wifi up: {ip_info:?}");
    Ok(NetUp { wifi })
}

/// wait_netif_up blocks forever on a network that never delivers DHCP, so the
/// client path polls for an address with a deadline instead; a timed-out
/// bring_up is an error the cycle reports before sleeping.
#[cfg(esp32s3)]
fn wait_for_sta_ip(wifi: &BlockingWifi<EspWifi<'static>>, secs: u64) -> Result<(), anyhow::Error> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    loop {
        let has_ip = wifi
            .wifi()
            .sta_netif()
            .get_ip_info()
            .map(|info| info.ip != esp_idf_svc::ipv4::Ipv4Addr::new(0, 0, 0, 0))
            .unwrap_or(false);
        if wifi.is_connected().unwrap_or(false) && has_ip {
            return Ok(());
        }
        anyhow::ensure!(
            std::time::Instant::now() < deadline,
            "wifi did not connect within {secs}s"
        );
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

pub fn ip_info(net: &NetUp) -> Option<IpInfo> {
    #[cfg(esp32)]
    {
        net._eth.eth().netif().get_ip_info().ok()
    }
    #[cfg(esp32s3)]
    {
        net.wifi.wifi().sta_netif().get_ip_info().ok()
    }
}
