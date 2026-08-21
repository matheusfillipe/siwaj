#[cfg(esp32s3)]
mod device {
    use std::sync::Arc;

    use esp_idf_svc::hal::adc::oneshot::config::{AdcChannelConfig, Calibration};
    use esp_idf_svc::hal::adc::oneshot::{AdcChannelDriver, AdcDriver};
    use esp_idf_svc::hal::adc::attenuation;
    use esp_idf_svc::hal::delay::Ets;
    use esp_idf_svc::hal::gpio::{Gpio14, PinDriver};
    use esp_idf_svc::hal::spi::{SpiConfig, SpiDeviceDriver, SpiDriver, SpiDriverConfig};
    use esp_idf_svc::hal::units::Hertz;
    use epd_waveshare::epd1in54_v2::Epd1in54;
    use epd_waveshare::prelude::*;
    use siwaj_core::render::View;

    pub const VBAT_PWR_PIN: i32 = 17;

    pub struct Board {
        epd: Epd1in54<
            SpiDeviceDriver<'static, SpiDriver<'static>>,
            PinDriver<'static, esp_idf_svc::hal::gpio::Input>,
            PinDriver<'static, esp_idf_svc::hal::gpio::Output>,
            PinDriver<'static, esp_idf_svc::hal::gpio::Output>,
            Ets,
        >,
        pub battery: Battery,
        spi: SpiDeviceDriver<'static, SpiDriver<'static>>,
        delay: Ets,
        epd_power: PinDriver<'static, esp_idf_svc::hal::gpio::Output>,
    }

    impl Board {
        pub fn new(
            pins: esp_idf_svc::hal::gpio::Pins,
            spi2: esp_idf_svc::hal::spi::SPI2<'static>,
            adc1: esp_idf_svc::hal::adc::ADC1<'static>,
        ) -> Result<Self, anyhow::Error> {
            use esp_idf_svc::hal::gpio::Pins;

            let Pins {
                gpio4,
                gpio6,
                gpio8,
                gpio9,
                gpio10,
                gpio11,
                gpio12,
                gpio13,
                ..
            } = pins;

            let busy = PinDriver::input(gpio8, esp_idf_svc::hal::gpio::Pull::Floating)?;
            let dc = PinDriver::output(gpio10)?;
            let rst = PinDriver::output(gpio9)?;
            let mut epd_power = PinDriver::output(gpio6)?;
            epd_power.set_low()?; // e-paper rail: active-low

            let driver = SpiDriver::new(
                spi2,
                gpio12,
                gpio13,
                Option::<Gpio14<'static>>::None,
                &SpiDriverConfig::new(),
            )?;
            let mut spi = SpiDeviceDriver::new(
                driver,
                Some(gpio11),
                &SpiConfig::new().baudrate(Hertz(10_000_000)),
            )?;

            let mut delay = Ets;
            let epd = Epd1in54::new(&mut spi, busy, dc, rst, &mut delay, None)?;
            let battery = Battery::new(adc1, gpio4)?;
            Ok(Board {
                epd,
                battery,
                spi,
                delay,
                epd_power,
            })
        }

        pub fn draw(&mut self, view: &View) -> Result<(), anyhow::Error> {
            let fb = siwaj_core::render::render(view);
            self.epd
                .update_frame(&mut self.spi, fb.buffer(), &mut self.delay)?;
            self.epd.display_frame(&mut self.spi, &mut self.delay)?;
            Ok(())
        }

        // both rails are active-low: high = off, minimum sleep current
        /// Sleeping the panel belongs here rather than at the end of a draw:
        /// a cycle can paint more than once, and the controller only needs to
        /// go down when the board does.
        pub fn power_down(&mut self) {
            let _ = self.epd.sleep(&mut self.spi, &mut self.delay);
            let _ = self.epd_power.set_high();
            // SAFETY: GPIO42 (audio amp rail) is owned by no other driver; a
            // momentary steal to latch it off is the only use.
            if let Ok(mut audio) = PinDriver::output(unsafe {
                esp_idf_svc::hal::gpio::AnyOutputPin::steal(42)
            }) {
                let _ = audio.set_high();
            }
        }
    }

    pub struct Battery {
        channel:
            AdcChannelDriver<'static, esp_idf_svc::hal::adc::ADCCH3<esp_idf_svc::hal::adc::ADCU1>, Arc<AdcDriver<'static, esp_idf_svc::hal::adc::ADCU1>>>,
    }

    impl Battery {
        pub fn new(adc1: esp_idf_svc::hal::adc::ADC1<'static>, gpio4: esp_idf_svc::hal::gpio::Gpio4<'static>) -> Result<Self, anyhow::Error> {
            let driver = Arc::new(AdcDriver::new(adc1)?);
            let config = AdcChannelConfig {
                attenuation: attenuation::DB_12,
                calibration: Calibration::Curve,
                ..Default::default()
            };
            let channel = AdcChannelDriver::new(driver, gpio4, &config)?;
            Ok(Battery { channel })
        }

        /// Rail voltage a discharging cell cannot reach, so anything at or
        /// above it means the charger is driving the rail. Tune against the
        /// board: too low and a fresh cell reads as plugged in forever.
        const EXTERNAL_POWER: f32 = 4.2;

        pub fn volts(&mut self) -> Option<f32> {
            match self.channel.read() {
                Ok(mv) => Some(mv as f32 * 2.0 / 1000.0),
                Err(e) => {
                    log::warn!("battery adc read failed: {e}");
                    None
                }
            }
        }

        /// True only while the rail sits above what the cell alone can supply.
        /// A charge that has not yet pushed the rail that high is
        /// indistinguishable from running on battery at this sense point.
        pub fn charging(&mut self) -> bool {
            self.volts().is_some_and(|v| v >= Self::EXTERNAL_POWER)
        }

        pub fn pct(&mut self) -> Option<u8> {
            let volts = self.volts()?;
            const FULL: f32 = 4.12;
            const EMPTY: f32 = 3.0;
            if volts <= EMPTY {
                return Some(0);
            }
            if volts >= FULL {
                return Some(100);
            }
            Some(((volts - EMPTY) / (FULL - EMPTY) * 100.0) as u8)
        }
    }
}

#[cfg(esp32s3)]
pub use device::{Board, VBAT_PWR_PIN};
