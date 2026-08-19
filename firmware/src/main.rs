use esp_idf_svc::hal::gpio::{PinDriver, Pull};
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::sys::{esp_deep_sleep, gpio_deep_sleep_hold_en, gpio_hold_en};

const VBAT_PWR: i32 = 17;
const WAKE_INTERVAL_SECS: u64 = 30 * 60;

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("siwaj awake");

    let peripherals = Peripherals::take().expect("peripherals");

    let boot_button =
        PinDriver::input(peripherals.pins.gpio0, Pull::Up).expect("boot button pin");

    let mut vbat_rail = PinDriver::output(peripherals.pins.gpio17).expect("vbat rail pin");
    vbat_rail.set_high().expect("vbat rail on");

    if boot_button.is_low() {
        log::info!("boot button held: entering config mode");
    } else {
        log::info!("weather cycle");
    }

    unsafe {
        gpio_hold_en(VBAT_PWR);
        gpio_deep_sleep_hold_en();
        esp_deep_sleep(WAKE_INTERVAL_SECS * 1_000_000);
    }
}
