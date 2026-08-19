#![allow(unknown_lints)]
#![allow(unexpected_cfgs)]

use esp_idf_svc::hal::gpio::{PinDriver, Pull};
use esp_idf_svc::hal::peripherals::Peripherals;

mod board;
mod net;
mod secrets;
mod server;
mod store;
mod weather;

const WAKE_INTERVAL_SECS: u64 = 30 * 60;

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("siwaj awake");

    let mut peripherals = Peripherals::take()?;
    let sys_loop = esp_idf_svc::eventloop::EspSystemEventLoop::take()?;
    let nvs_partition = esp_idf_svc::nvs::EspDefaultNvsPartition::take()?;

    let store = Box::leak(Box::new(store::take(nvs_partition.clone())?));
    let secrets_store = Box::leak(Box::new(secrets::take(nvs_partition.clone())?));
    secrets::spawn_repl(secrets_store);

    let configured = store.load().is_some();

    #[cfg(esp32s3)]
    let (pins, modem, spi2, adc1) = {
        let Peripherals {
            pins,
            modem,
            spi2,
            adc1,
            ..
        } = peripherals;
        let mut vbat_rail = PinDriver::output(unsafe {
            esp_idf_svc::hal::gpio::Gpio17::steal()
        })?;
        vbat_rail.set_high()?;
        unsafe {
            esp_idf_svc::sys::gpio_hold_en(board::VBAT_PWR_PIN);
            esp_idf_svc::sys::gpio_deep_sleep_hold_en();
        };
        (pins, modem, spi2, adc1)
    };
    #[cfg(esp32)]
    let mac = peripherals.mac;

    #[cfg(esp32s3)]
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
        run_config_mode(modem, sys_loop, nvs_partition, store, secrets_store)?;
        return Ok(());
    }

    #[cfg(esp32s3)]
    {
        run_weather_cycle(pins, spi2, adc1, store, secrets_store)?;
        deep_sleep(store);
    }
    #[cfg(esp32)]
    {
        // esp32 build exists for QEMU only: always the config-mode path
        let _ = &nvs_partition;
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
        secrets_store.get("WIFI_SSID"),
        secrets_store.get("WIFI_PASS"),
        nvs_partition,
    )?;
    #[cfg(esp32)]
    let net = net::bring_up(mac, sys_loop)?;

    if let Some(ip) = net::ip_info(&net) {
        log::info!("config mode: serving on http://{}", ip.ip);
    }
    let server = server::start(store, secrets_store)?;
    core::mem::forget(net);
    core::mem::forget(server);
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

#[cfg(esp32s3)]
fn run_weather_cycle(
    pins: esp_idf_svc::hal::gpio::Pins,
    spi2: esp_idf_svc::hal::spi::SPI2<'static>,
    adc1: esp_idf_svc::hal::adc::ADC1<'static>,
    store: &'static store::Store,
    secrets_store: &'static secrets::Secrets,
) -> anyhow::Result<()> {
    let config = store
        .load()
        .ok_or_else(|| anyhow::anyhow!("no config for weather cycle"))?;
    let mut board = board::Board::new(pins, spi2, adc1)?;

    let view = match weather::fetch(secrets_store, config.location.lat, config.location.lon) {
        Ok(snapshot) => {
            let rain = siwaj_core::RainOutlook::from_one_call(
                snapshot.hourly_pop,
                &snapshot.minutely_mm,
            );
            siwaj_core::render::View {
                garment: siwaj_core::Garment::from_feels_like(
                    snapshot.feels_like_c,
                    &config.thresholds,
                ),
                feels_like_c: snapshot.feels_like_c,
                rain,
                rain_threshold_pct: config.rain_threshold_pct,
                updated: hhmm(store.now_unix()),
                battery_pct: board.battery.pct(),
            }
        }
        Err(e) => {
            log::warn!("weather fetch failed: {e}; keeping display as-is");
            siwaj_core::render::View {
                garment: siwaj_core::Garment::Jacket,
                feels_like_c: 0.0,
                rain: siwaj_core::RainOutlook {
                    pop_pct_next_hour: 0,
                    rain_expected: false,
                },
                rain_threshold_pct: config.rain_threshold_pct,
                updated: hhmm(store.now_unix()),
                battery_pct: board.battery.pct(),
            }
        }
    };
    board.draw(&view)?;
    Ok(())
}

#[cfg(esp32s3)]
fn hhmm(unix: u32) -> (u8, u8) {
    let secs_of_day = unix % (24 * 3600);
    ((secs_of_day / 3600) as u8, ((secs_of_day % 3600) / 60) as u8)
}

#[cfg(esp32s3)]
fn deep_sleep(store: &'static store::Store) -> ! {
    let secs = store
        .load()
        .map(|c| c.refresh_minutes as u64 * 60)
        .unwrap_or(WAKE_INTERVAL_SECS);
    log::info!("deep sleeping for {secs}s");
    unsafe { esp_idf_svc::sys::esp_deep_sleep((secs * 1_000_000) as u64) }
}
