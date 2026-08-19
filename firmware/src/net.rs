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

    match (ssid, pass) {
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
            wifi.wait_netif_up()?;
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
        }
    }
    let ip_info: IpInfo = wifi.wifi().sta_netif().get_ip_info()?;
    log::info!("wifi up: {ip_info:?}");
    Ok(NetUp { wifi })
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
