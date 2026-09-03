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

    // The provisioner reaches the secrets REPL over whichever line carries
    // stdin, and that line differs per target: UART0 under the emulator, the
    // on-die USB-SERIAL-JTAG behind the board's USB-C. Either way a driver has
    // to be attached to VFS, because console output alone leaves stdin dead.
    #[cfg(esp32)]
    {
        // SAFETY: GPIO1/3 are the UART0 TX/RX pads and are handed straight to
        // the UART0 driver; nothing else owns them at boot.
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
        // SAFETY: the UART0 driver was just initialized; this call only
        // attaches it to the VFS layer so stdin reads reach it. Called once.
        unsafe {
            esp_idf_svc::sys::esp_vfs_dev_uart_use_driver(
                esp_idf_svc::sys::uart_port_t_UART_NUM_0 as i32,
            )
        };
    }
    #[cfg(esp32s3)]
    {
        let mut config = esp_idf_svc::sys::usb_serial_jtag_driver_config_t {
            tx_buffer_size: 1024,
            rx_buffer_size: 1024,
        };
        // SAFETY: installs the USB-SERIAL-JTAG driver and points VFS at it,
        // once, before any thread reads stdin. The config outlives the call.
        esp_idf_svc::sys::esp!(unsafe {
            esp_idf_svc::sys::usb_serial_jtag_driver_install(&mut config)
        })?;
        // SAFETY: the driver above is installed; this only routes stdin
        // through it.
        unsafe { esp_idf_svc::sys::esp_vfs_usb_serial_jtag_use_driver() };
    }

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

    // A press that ends deep sleep is over long before the CPU gets here, so
    // the latched wake cause is what reports it. Reading the pin alone would
    // only catch someone still holding the button. Both causes count: an RTC
    // pin wake is reported as EXT1 on some parts and GPIO on others.
    #[cfg(esp32s3)]
    // SAFETY: reads the wake cause the ROM latched at boot; no side effects.
    let wake_cause = unsafe { esp_idf_svc::sys::esp_sleep_get_wakeup_cause() };
    #[cfg(esp32s3)]
    let woke_on_button = wake_cause == esp_idf_svc::sys::esp_sleep_source_t_ESP_SLEEP_WAKEUP_EXT1
        || wake_cause == esp_idf_svc::sys::esp_sleep_source_t_ESP_SLEEP_WAKEUP_GPIO;
    #[cfg(esp32)]
    let (wake_cause, woke_on_button) = (0, false);

    let config_mode = !configured || boot_held || woke_on_button;
    log::info!(
        "wake_cause={wake_cause} configured={configured} boot_held={boot_held} woke_on_button={woke_on_button} -> {}",
        if config_mode { "config mode" } else { "weather cycle" }
    );

    if config_mode {
        #[cfg(esp32)]
        run_config_mode(mac, sys_loop, store, secrets_store)?;
        #[cfg(esp32s3)]
        run_config_mode(hardware, sys_loop, nvs_partition, store, secrets_store)?;
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
    #[cfg(esp32s3)] hardware: Hardware,
    sys_loop: esp_idf_svc::eventloop::EspSystemEventLoop,
    #[cfg(esp32s3)] nvs_partition: esp_idf_svc::nvs::EspNvsPartition<esp_idf_svc::nvs::NvsDefault>,
    store: &'static store::Store,
    secrets_store: &'static secrets::Secrets,
) -> anyhow::Result<()> {
    #[cfg(esp32s3)]
    let Hardware {
        pins,
        modem,
        spi2,
        adc1,
    } = hardware;
    // The mark goes up before the radio does. Nothing about it needs the
    // network, and painting after bring-up left the panel saying nothing for
    // the ten-odd seconds a join and a clock sync take, which reads as a device
    // that ignored the button.
    #[cfg(esp32s3)]
    let mut board = match board::Board::new(pins, spi2, adc1) {
        Ok(board) => Some(board),
        Err(e) => {
            // the page is the point of setup mode; losing the panel is worth
            // reporting but not worth refusing to serve over
            log::error!("config mode: panel unavailable: {e}");
            None
        }
    };
    #[cfg(esp32s3)]
    if let Some(board) = board.as_mut() {
        let mut setup = siwaj_core::render::View::offline(
            siwaj_core::render::TimeOfDay::from_unix(now_unix()),
            board.battery.pct(),
            board.battery.charging(),
        );
        setup.serving = true;
        if let Err(e) = board.draw(&setup) {
            log::error!("config mode: setup frame failed: {e}");
        }
    }

    // Past this point the radio is on, so nothing may return early: an error
    // that escapes here would abort into a reboot with wifi still up, and a
    // device that keeps rebooting into setup empties the battery in a day.
    #[cfg(esp32s3)]
    let net = match net::bring_up(
        modem,
        sys_loop,
        secrets_store.get(secrets::SecretKey::WifiSsid),
        secrets_store.get(secrets::SecretKey::WifiPass),
        nvs_partition,
        net::OnJoinFailure::ServeSetupNetwork,
    ) {
        Ok(net) => net,
        Err(e) => {
            log::error!("config mode: wifi bring-up failed: {e}");
            if let Some(board) = board.as_mut() {
                board.power_down();
            }
            deep_sleep(WAKE_INTERVAL);
        }
    };
    #[cfg(esp32)]
    let net = net::bring_up(mac, sys_loop)?;

    if !net::is_access_point(&net) {
        sync_time();
    }
    if let Some(ip) = net::ip_info(&net) {
        log::info!("config mode: serving on http://{}", ip.ip);
    }
    #[cfg(esp32)]
    frame::spawn_loop(store, secrets_store);
    #[cfg(esp32)]
    core::mem::forget(server::start_panel()?);
    core::mem::forget(net);

    #[cfg(esp32s3)]
    {
        // The panel holds whatever it was last given, so setup mode paints the
        // mark on the way in and paints it away on the way out. The board stays
        // up across the session and only sleeps once, at the end.
        if let Err(e) = serve_until_idle(
            store,
            secrets_store,
            board.as_mut().map(|board| &mut board.battery),
        ) {
            log::error!("config mode: server failed: {e}");
        }

        // Leaving setup repaints before the radio goes off: the weather the
        // device now has if it was configured, the plain offline face if the
        // setup was abandoned. Either way the mark goes with it.
        let config = store.load().ok().flatten();
        if let Some(board) = board.as_mut() {
            let leaving = match config.as_ref() {
                Some(config) => weather_view(
                    secrets_store,
                    config,
                    board.battery.pct(),
                    board.battery.charging(),
                ),
                None => siwaj_core::render::View::offline(
                    siwaj_core::render::TimeOfDay::from_unix(now_unix()),
                    board.battery.pct(),
                    board.battery.charging(),
                ),
            };
            if let Err(e) = board.draw(&leaving) {
                log::error!("leaving setup: panel repaint failed: {e}");
            }
            board.power_down();
        }
        let refresh = config
            .as_ref()
            .map(siwaj_core::Config::refresh_interval)
            .unwrap_or(WAKE_INTERVAL);
        deep_sleep(refresh);
    }
    #[cfg(esp32)]
    loop {
        serve_until_idle(store, secrets_store)?;
        wait_for_button();
    }
}

/// Holds the config port open until the page goes quiet. Serving is what
/// costs the battery in this mode, so the window is idle-based rather than
/// fixed: an active setup keeps it alive, an abandoned one lets it close.
fn serve_until_idle(
    store: &'static store::Store,
    secrets_store: &'static secrets::Secrets,
    #[cfg(esp32s3)] mut battery: Option<&mut board::Battery>,
) -> anyhow::Result<()> {
    let server = server::start(store, secrets_store)?;
    touch();
    #[cfg(esp32)]
    frame::set_serving(true);
    // read per tick, not once: a window saved during this session has to take
    // effect now, or the countdown the page shows disagrees with the loop
    while idle_for() < awake_window(store) {
        #[cfg(esp32s3)]
        if let Some(battery) = battery.as_mut() {
            publish_battery_mv(battery.millivolts());
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    drop(server);
    #[cfg(esp32)]
    frame::set_serving(false);
    log::info!(
        "config mode idle for {}s; stopping the server",
        awake_window(store).as_secs()
    );
    Ok(())
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
        net::OnJoinFailure::GiveUp,
    )?;
    sync_time();

    let view = weather_view(
        secrets_store,
        config,
        board.battery.pct(),
        board.battery.charging(),
    );
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
    charging: bool,
) -> siwaj_core::render::View {
    match weather::fetch(secrets, config.location.lat, config.location.lon) {
        Ok(snapshot) => siwaj_core::render::View::from_snapshot(
            &snapshot,
            config,
            battery_pct,
            charging,
            now_unix(),
        ),
        Err(e) => {
            log::warn!("weather fetch failed: {e}; drawing offline frame");
            siwaj_core::render::View::offline(
                siwaj_core::render::TimeOfDay::from_unix(now_unix()),
                battery_pct,
                charging,
            )
        }
    }
}

/// When config mode last saw a request. An untouched device stops serving and
/// lets the radio go quiet; the page keeps the window open while someone is
/// editing by refetching its config as they work.
static LAST_REQUEST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// A device with no readable config still has to close its window, so the
/// contract default stands in until there is a stored answer.
fn awake_window(store: &store::Store) -> std::time::Duration {
    store
        .load()
        .ok()
        .flatten()
        .map(|config| config.awake_window())
        .unwrap_or(siwaj_core::CONFIG_MODE_IDLE)
}

pub(crate) fn touch() {
    *LAST_REQUEST.lock().expect("activity lock") = Some(std::time::Instant::now());
}

/// The ADC belongs to the board, which the HTTP handlers do not hold, so the
/// serve loop leaves its latest reading here for them.
static BATTERY_MV: std::sync::Mutex<Option<u32>> = std::sync::Mutex::new(None);

#[cfg(esp32s3)]
fn publish_battery_mv(reading: Option<u32>) {
    *BATTERY_MV.lock().expect("battery lock") = reading;
}

pub(crate) fn battery_mv() -> Option<u32> {
    *BATTERY_MV.lock().expect("battery lock")
}

pub(crate) fn idle_for() -> std::time::Duration {
    LAST_REQUEST
        .lock()
        .expect("activity lock")
        .map(|at| at.elapsed())
        .unwrap_or_default()
}

/// The emulator cannot deep sleep under QEMU, so it stands the wake-up in:
/// the serial `button` command plays the part of the BOOT button the real
/// device reads from GPIO0 at boot.
#[cfg(esp32)]
static BUTTON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The real device reads BOOT once, at boot, so a press while it is already
/// serving cannot be a wake. Latching one anyway would sit there and cancel
/// the next sleep the instant the window ran out; counting it as use is both
/// harmless and what someone pressing it probably meant.
#[cfg(esp32)]
pub(crate) fn press_button() {
    if frame::is_serving() {
        touch();
        return;
    }
    BUTTON.store(true, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(esp32)]
pub(crate) fn force_sleep() {
    *LAST_REQUEST.lock().expect("activity lock") =
        std::time::Instant::now().checked_sub(siwaj_core::CONFIG_MODE_IDLE);
}

#[cfg(esp32)]
fn wait_for_button() {
    while !BUTTON.swap(false, std::sync::atomic::Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    log::info!("button pressed: serving config mode again");
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

/// GPIO0 carries the BOOT button and is one of the S3's RTC pins, so it can
/// pull the chip out of deep sleep. ext1 goes on working with the RTC
/// peripherals powered down, and the board's own pull-up on that line (the one
/// that makes a normal boot possible) holds it high, so no internal pull has to
/// be kept alive to hold the level.
#[cfg(esp32s3)]
const WAKE_BUTTON_MASK: u64 = 1 << 0;

#[cfg(esp32s3)]
fn deep_sleep(duration: std::time::Duration) -> ! {
    log::info!("deep sleeping for {}s", duration.as_secs());
    // SAFETY: arms one RTC-capable pin as a wake source. The call only writes
    // RTC wake configuration, takes no ownership, and is safe to repeat.
    unsafe {
        esp_idf_svc::sys::esp_sleep_enable_ext1_wakeup_io(
            WAKE_BUTTON_MASK,
            esp_idf_svc::sys::esp_sleep_ext1_wakeup_mode_t_ESP_EXT1_WAKEUP_ANY_LOW,
        );
    }
    // SAFETY: terminal call; the SoC enters deep sleep and never returns, so
    // no state after this point is observable.
    unsafe { esp_idf_svc::sys::esp_deep_sleep(duration.as_micros() as u64) }
}
