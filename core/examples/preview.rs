use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics_simulator::{
    BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
};
use siwaj_core::render::{HEIGHT, View, WIDTH, render};
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
        updated: (14, 32),
        battery_pct: 87,
    }
}

fn main() {
    let output_settings = OutputSettingsBuilder::new()
        .theme(BinaryColorTheme::Default)
        .scale(2)
        .build();
    let mut window = Window::new("siwaj preview", &output_settings);

    let cases = vec![
        ("jacket, rain", view(Garment::Jacket, 62, true, -3.4)),
        ("pullover", view(Garment::Pullover, 10, false, 11.0)),
        ("shirt", view(Garment::Shirt, 45, false, 17.5)),
        ("t-shirt", view(Garment::TShirt, 0, false, 24.0)),
    ];

    let mut index = 0;
    'outer: loop {
        let (_, case_view) = &cases[index];
        let fb = render(case_view);
        let mut sim = SimulatorDisplay::<BinaryColor>::new(Size::new(WIDTH, HEIGHT));
        fb.iter_pixels().for_each(|p| {
            p.draw(&mut sim).ok();
        });
        window.update(&sim);
        for _ in 0..30 {
            if window.events().any(|e| e == SimulatorEvent::Quit) {
                break 'outer;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        index = (index + 1) % cases.len();
    }
}
