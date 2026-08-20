use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics_simulator::sdl2::Keycode;
use embedded_graphics_simulator::{
    BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
};
use siwaj_core::render::{HEIGHT, TimeOfDay, View, WIDTH, render};
use siwaj_core::{Garment, RainOutlook};

fn view(garment: Garment, pop: u8, expected: bool, feels: f32) -> View {
    View {
        garment,
        feels_like_c: feels,
        rain: RainOutlook {
            pop_pct_next_hour: pop,
            rain_expected: expected,
        },
        rain_threshold_pct: 30,
        updated: TimeOfDay {
            hour: 14,
            minute: 32,
        },
        battery_pct: 87,
    }
}

fn cases() -> Vec<(&'static str, View)> {
    vec![
        (
            "1: jacket, -3C, rain 62%",
            view(Garment::Jacket, 62, true, -3.4),
        ),
        (
            "2: pullover, 11C, dry",
            view(Garment::Pullover, 10, false, 11.0),
        ),
        (
            "3: shirt, 18C, rain 45%",
            view(Garment::Shirt, 45, false, 17.5),
        ),
        (
            "4: t-shirt, 24C, dry",
            view(Garment::TShirt, 0, false, 24.0),
        ),
    ]
}

fn draw(case_view: &View) -> SimulatorDisplay<BinaryColor> {
    let fb = render(case_view);
    let mut sim = SimulatorDisplay::<BinaryColor>::new(Size::new(WIDTH, HEIGHT));
    fb.iter_pixels().for_each(|p| {
        p.draw(&mut sim).ok();
    });
    sim
}

const FRAME_MS: u64 = 50;
const CYCLE_MS: u64 = 4000;

fn main() {
    let scale: u32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .filter(|&s| (1..=8).contains(&s))
        .unwrap_or(3);
    let output_settings = OutputSettingsBuilder::new()
        .theme(BinaryColorTheme::Default)
        .scale(scale)
        .build();

    let cases = cases();
    let mut index = 0usize;
    let mut window = Window::new("siwaj [1-4 garment, q quit]", &output_settings);

    loop {
        window.update(&draw(&cases[index].1));
        let mut waited = 0u64;
        while waited < CYCLE_MS {
            for event in window.events() {
                match event {
                    SimulatorEvent::Quit => return,
                    SimulatorEvent::KeyDown { keycode, .. } => {
                        let digit = match keycode {
                            Keycode::Num1 => Some(1),
                            Keycode::Num2 => Some(2),
                            Keycode::Num3 => Some(3),
                            Keycode::Num4 => Some(4),
                            _ => None,
                        };
                        if let Some(n) = digit {
                            index = (n - 1) as usize;
                            waited = CYCLE_MS;
                        }
                    }
                    _ => {}
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(FRAME_MS));
            waited += FRAME_MS;
        }
        index = (index + 1) % cases.len();
    }
}
