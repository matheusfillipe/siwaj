#![allow(unknown_lints)]
#![allow(unexpected_cfgs)]

use esp_idf_svc::hal::peripherals::Peripherals;
#[cfg(esp32s3)]
use esp_idf_svc::hal::gpio::{PinDriver, Pull};

mod board;
#[cfg(esp32)]
mod frame;
mod net;
mod secrets;
mod server;
mod store;
mod weather;

#[cfg(esp32s3)]
const WAKE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// Budget for one weather cycle: wifi bring-up, sntp, the One Call fetch,
/// and the e-paper update, with margin. The watchdog forces deep sleep if a
/// hung step (stalled DHCP or TLS read) would otherwise drain the battery.
#[cfg(esp32s3)]
const CYCLE_BUDGET: std::time::Duration = std::time::Duration::from_secs(120);

// deep_sleep diverges, so the trailing Ok(()) is unreachable on esp32s3
#[cfg_attr(esp32s3, allow(unreachable_code))]
fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("siwaj awake");

    let peripherals = Peripherals::take()?;
    let sys_loop = esp_idf_svc::eventloop::EspSystemEventLoop::take()?;
    let nvs_partition = esp_idf_svc::nvs::EspDefaultNvsPartition::take()?;

    // stdin needs the UART0 driver attached to VFS; console output works without it
    // SAFETY: GPIO1/3 are the UART0 TX/RX pads and are handed straight to the
    // UART0 driver below; nothing else owns them at boot.
    let _uart0_driver = Box::leak(Box::new(
        esp_idf_svc::hal::uart::UartDriver::new(
            peripherals.uart0,
            unsafe { esp_idf_svc::hal::gpio::AnyOutputPin::steal(1) },
            unsafe { esp_idf_svc::hal::gpio::AnyInputPin::steal(3) },
            Option::<esp_idf_svc::hal::gpio::AnyInputPin>::None,
            Option::<esp_idf_svc::hal::gpio::AnyOutputPin>::None,
            &esp_idf_svc::hal::uart::config::Config::new()
                .baudrate(esp_idf_svc::hal::units::Hertz(115_200)),
        )
        .expect("uart0 driver"),
    ));
    // SAFETY: the UART0 driver was just initialized; this call only attaches
    // it to the VFS layer so stdin reads reach the driver. Called once.
    unsafe {
        esp_idf_svc::sys::esp_vfs_dev_uart_use_driver(
            esp_idf_svc::sys::uart_port_t_UART_NUM_0 as i32,
        )
    };

    let store = Box::leak(Box::new(store::take(nvs_partition.clone())?));
    let secrets_store = Box::leak(Box::new(secrets::take(nvs_partition.clone())?));
    secrets::spawn_repl(secrets_store);

    let configured = match store.load() {
        Ok(config) => config.is_some(),
        Err(e) => {
            log::error!("stored config unreadable: {e}; entering config mode");
            true
        }
    };

    #[cfg(esp32s3)]
    let hardware = {
        let Peripherals {
            pins,
            modem,
            spi2,
            adc1,
            ..
        } = peripherals;
        // SAFETY: GPIO17 (battery rail) is not consumed by any Peripherals
        // driver here, so this steal is the only owner.
        let mut vbat_rail = PinDriver::output(unsafe {
            esp_idf_svc::hal::gpio::Gpio17::steal()
        })?;
        vbat_rail.set_high()?;
        // SAFETY: raw RTC-register calls that latch GPIO17's current level
        // (just set high) through deep sleep; without the hold the rail
        // floats and the board never wakes on battery.
        unsafe {
            esp_idf_svc::sys::gpio_hold_en(board::VBAT_PWR_PIN);
            esp_idf_svc::sys::gpio_deep_sleep_hold_en();
        };
        Hardware {
            pins,
            modem,
            spi2,
            adc1,
        }
    };
    #[cfg(esp32)]
    let mac = peripherals.mac;

    #[cfg(esp32s3)]
    // SAFETY: GPIO0 (BOOT button) is read-only here; no other driver claims it.
    let boot_held = unsafe {
        use esp_idf_svc::hal::gpio::Gpio0;
        PinDriver::input(Gpio0::steal(), Pull::Up)?.is_low()
    };
    #[cfg(esp32)]
    let boot_held = false;

    let config_mode = !configured || boot_held;

    if config_mode {
        #[cfg(esp32)]
        run_config_mode(mac, sys_loop, store, secrets_store)?;
        #[cfg(esp32s3)]
        run_config_mode(
            hardware.modem,
            sys_loop,
            nvs_partition,
            store,
            secrets_store,
        )?;
        return Ok(());
    }

    #[cfg(esp32s3)]
    {
        // load once: the cycle and deep_sleep share the same config snapshot
        let config = match store.load() {
            Ok(config) => config,
            Err(e) => {
                log::error!("stored config unreadable: {e}");
                None
            }
        };
        // every wake must terminate in deep sleep; a failed cycle only costs
        // one stale frame, staying awake would cost the battery
        if let Some(config) = config.as_ref() {
            arm_cycle_watchdog(config.refresh_interval());
            if let Err(e) =
                run_weather_cycle(hardware, sys_loop, nvs_partition, config, secrets_store)
            {
                log::error!("weather cycle failed: {e}");
            }
        }
        let refresh = config
            .as_ref()
            .map(siwaj_core::Config::refresh_interval)
            .unwrap_or(WAKE_INTERVAL);
        deep_sleep(refresh);
    }
    #[cfg(esp32)]
    {
        // esp32 build exists for QEMU only: always the config-mode path,
        // which parks in the server loop and never returns
        run_config_mode(mac, sys_loop, store, secrets_store)?;
    }
    Ok(())
}

fn run_config_mode(
    #[cfg(esp32)] mac: esp_idf_svc::hal::mac::MAC<'static>,
    #[cfg(esp32s3)] modem: esp_idf_svc::hal::modem::Modem<'static>,
    sys_loop: esp_idf_svc::eventloop::EspSystemEventLoop,
    #[cfg(esp32s3)] nvs_partition: esp_idf_svc::nvs::EspNvsPartition<esp_idf_svc::nvs::NvsDefault>,
    store: &'static store::Store,
    secrets_store: &'static secrets::Secrets,
) -> anyhow::Result<()> {
    #[cfg(esp32s3)]
    let net = net::bring_up(
        modem,
        sys_loop,
        secrets_store.get(secrets::SecretKey::WifiSsid),
        secrets_store.get(secrets::SecretKey::WifiPass),
        nvs_partition,
    )?;
    #[cfg(esp32)]
    let net = net::bring_up(mac, sys_loop)?;

    sync_time();
    if let Some(ip) = net::ip_info(&net) {
        log::info!("config mode: serving on http://{}", ip.ip);
    }
    #[cfg(esp32)]
    frame::spawn_loop(store, secrets_store);
    let server = server::start(store, secrets_store)?;
    core::mem::forget(net);
    core::mem::forget(server);
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

/// The esp32s3 peripherals the weather cycle consumes, grouped so the cycle
/// signature stays small.
#[cfg(esp32s3)]
struct Hardware {
    pins: esp_idf_svc::hal::gpio::Pins,
    spi2: esp_idf_svc::hal::spi::SPI2<'static>,
    adc1: esp_idf_svc::hal::adc::ADC1<'static>,
    modem: esp_idf_svc::hal::modem::Modem<'static>,
}

#[cfg(esp32s3)]
fn run_weather_cycle(
    hardware: Hardware,
    sys_loop: esp_idf_svc::eventloop::EspSystemEventLoop,
    nvs_partition: esp_idf_svc::nvs::EspNvsPartition<esp_idf_svc::nvs::NvsDefault>,
    config: &siwaj_core::Config,
    secrets_store: &'static secrets::Secrets,
) -> anyhow::Result<()> {
    let Hardware {
        pins,
        spi2,
        adc1,
        modem,
    } = hardware;
    let mut board = board::Board::new(pins, spi2, adc1)?;

    // the fetch needs the radio and a valid wall clock; both only exist after
    // the network is up, and neither survives deep sleep
    let _net = net::bring_up(
        modem,
        sys_loop,
        secrets_store.get(secrets::SecretKey::WifiSsid),
        secrets_store.get(secrets::SecretKey::WifiPass),
        nvs_partition,
    )?;
    sync_time();

    let view = weather_view(secrets_store, config, board.battery.pct());
    board.draw(&view)?;
    board.power_down();
    Ok(())
}

/// One cycle's display state: the live One Call fetch, or the offline face
/// when it fails. `View::offline` marks which one the caller got.
pub(crate) fn weather_view(
    secrets: &secrets::Secrets,
    config: &siwaj_core::Config,
    battery_pct: Option<u8>,
) -> siwaj_core::render::View {
    match weather::fetch(secrets, config.location.lat, config.location.lon) {
        Ok(snapshot) => {
            siwaj_core::render::View::from_snapshot(&snapshot, config, battery_pct, now_unix())
        }
        Err(e) => {
            log::warn!("weather fetch failed: {e}; drawing offline frame");
            siwaj_core::render::View::offline(
                siwaj_core::render::TimeOfDay::from_unix(now_unix()),
                battery_pct,
            )
        }
    }
}

pub(crate) fn now_unix() -> u32 {
    // SAFETY: libc time(NULL) with a null out-pointer is the documented call.
    let secs = unsafe { esp_idf_svc::sys::time(std::ptr::null_mut()) };
    u32::try_from(secs).unwrap_or(0)
}

fn sync_time() {
    use esp_idf_svc::sntp::{EspSntp, SyncStatus};

    let sntp = match EspSntp::new_default() {
        Ok(s) => s,
        Err(e) => {
            log::warn!("sntp init failed: {e}");
            return;
        }
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while std::time::Instant::now() < deadline {
        if sntp.get_sync_status() == SyncStatus::Completed {
            log::info!("sntp synced");
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    log::warn!("sntp sync timeout; timestamps will be wrong this cycle");
}

#[cfg(esp32s3)]
fn arm_cycle_watchdog(refresh: std::time::Duration) {
    std::thread::Builder::new()
        .name("watchdog".to_string())
        .stack_size(2048)
        .spawn(move || {
            std::thread::sleep(CYCLE_BUDGET);
            log::error!(
                "weather cycle overran {}s; forcing deep sleep",
                CYCLE_BUDGET.as_secs()
            );
            deep_sleep(refresh);
        })
        .expect("spawn watchdog");
}

#[cfg(esp32s3)]
fn deep_sleep(duration: std::time::Duration) -> ! {
    log::info!("deep sleeping for {}s", duration.as_secs());
    // SAFETY: terminal call; the SoC enters deep sleep and never returns, so
    // no state after this point is observable.
    unsafe { esp_idf_svc::sys::esp_deep_sleep(duration.as_micros() as u64) }
}
