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
    /// Only one of the two interfaces is ever started, and reading the address
    /// off the other one yields 0.0.0.0.
    access_point: bool,
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

/// What a caller wants when the stored credentials do not get onto the network.
#[cfg(esp32s3)]
pub enum OnJoinFailure {
    /// Serve the setup network, so the credentials that failed can be corrected
    /// from the page. Without this a wrong password leaves the device
    /// unreachable over the air for good.
    ServeSetupNetwork,
    /// Report the failure. A weather cycle has nothing to serve and its caller
    /// sleeps instead.
    GiveUp,
}

#[cfg(esp32s3)]
pub fn bring_up(
    modem: esp_idf_svc::hal::modem::Modem<'static>,
    sys_loop: EspSystemEventLoop,
    ssid: Option<String>,
    pass: Option<String>,
    nvs: esp_idf_svc::nvs::EspNvsPartition<esp_idf_svc::nvs::NvsDefault>,
    on_failure: OnJoinFailure,
) -> Result<NetUp, anyhow::Error> {
    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
    )?;

    let ap_mode = match (ssid, pass) {
        (Some(ssid), Some(pass)) if !ssid.is_empty() => match join(&mut wifi, &ssid, &pass) {
            Ok(()) => false,
            Err(e) => match on_failure {
                OnJoinFailure::GiveUp => return Err(e),
                OnJoinFailure::ServeSetupNetwork => {
                    log::warn!("joining '{ssid}' failed: {e:#}");
                    log_networks_in_range(&mut wifi);
                    serve_setup_network(&mut wifi)?;
                    true
                }
            },
        },
        _ => {
            serve_setup_network(&mut wifi)?;
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
    Ok(NetUp {
        wifi,
        access_point: ap_mode,
    })
}

#[cfg(esp32s3)]
fn join(
    wifi: &mut BlockingWifi<EspWifi<'static>>,
    ssid: &str,
    pass: &str,
) -> Result<(), anyhow::Error> {
    let conf = WifiConfiguration::Client(ClientConfiguration {
        ssid: ssid
            .try_into()
            .map_err(|_| anyhow::anyhow!("ssid too long"))?,
        password: pass
            .try_into()
            .map_err(|_| anyhow::anyhow!("password too long"))?,
        auth_method: AuthMethod::WPA2Personal,
        ..Default::default()
    });
    wifi.set_configuration(&conf)?;
    wifi.start()?;
    wifi.connect()?;
    wait_for_sta_ip(wifi, 20)
}

/// A join that times out without ever reaching authentication means the radio
/// never found that name, which is indistinguishable from a wrong password
/// unless the alternatives are named. The 2.4GHz-only radio makes this worth
/// printing: a 5GHz network is simply absent here however strong it looks on a
/// phone. Runs while the station interface is still up, before it is torn down
/// for the setup network.
#[cfg(esp32s3)]
fn log_networks_in_range(wifi: &mut BlockingWifi<EspWifi<'static>>) {
    match wifi.scan() {
        Ok(found) if found.is_empty() => log::warn!("scan saw no 2.4GHz networks at all"),
        Ok(found) => {
            for ap in found.iter() {
                log::info!(
                    "in range: '{}' ch{} {}dBm {:?}",
                    ap.ssid,
                    ap.channel,
                    ap.signal_strength,
                    ap.auth_method
                );
            }
        }
        Err(e) => log::warn!("scan failed: {e}"),
    }
}

/// The device's own network, named for what it is there to do. The radio has
/// to be stopped first: a failed join leaves the station interface running,
/// and reconfiguring underneath it is refused.
#[cfg(esp32s3)]
fn serve_setup_network(wifi: &mut BlockingWifi<EspWifi<'static>>) -> Result<(), anyhow::Error> {
    let _ = wifi.disconnect();
    let _ = wifi.stop();
    wifi.set_configuration(&WifiConfiguration::AccessPoint(AccessPointConfiguration {
        ssid: "siwaj".try_into()?,
        auth_method: AuthMethod::None,
        ..Default::default()
    }))?;
    wifi.start()?;
    wifi.wait_netif_up()?;
    log::info!("setup network 'siwaj' up");
    Ok(())
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
        let netif = if net.access_point {
            net.wifi.wifi().ap_netif()
        } else {
            net.wifi.wifi().sta_netif()
        };
        netif.get_ip_info().ok()
    }
}

/// True while the device is its own network. Nothing upstream is reachable
/// then, so work that needs the internet is skipped rather than timed out.
pub fn is_access_point(net: &NetUp) -> bool {
    #[cfg(esp32)]
    {
        let _ = net;
        false
    }
    #[cfg(esp32s3)]
    {
        net.access_point
    }
}
