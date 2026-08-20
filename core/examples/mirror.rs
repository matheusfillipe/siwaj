use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics_simulator::sdl2::Keycode;
use embedded_graphics_simulator::{
    BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
};
use siwaj_core::render::{Framebuffer, HEIGHT, WIDTH, decode_bmp};

/// Live mirror of the emulated device's e-paper: polls GET /api/frame.bmp
/// (make qemu-run first) and shows the exact frame the firmware rendered,
/// fetched with the stored config. r refreshes now, q quits.
fn main() {
    let addr = std::env::var("SIWAJ_DEVICE_ADDR").unwrap_or_else(|_| "127.0.0.1:47652".into());
    let scale: u32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .filter(|&s| (1..=8).contains(&s))
        .unwrap_or(3);
    let output_settings = OutputSettingsBuilder::new()
        .theme(BinaryColorTheme::Default)
        .scale(scale)
        .build();

    let mut window = Window::new("siwaj live display [r refresh, q quit]", &output_settings);
    let mut display = SimulatorDisplay::<BinaryColor>::new(Size::new(WIDTH, HEIGHT));
    let mut last_attempt = std::time::Instant::now() - Duration::from_secs(600);
    const INTERVAL: Duration = Duration::from_secs(60);

    loop {
        if last_attempt.elapsed() >= INTERVAL {
            last_attempt = std::time::Instant::now();
            match fetch_frame(&addr) {
                Ok(fb) => {
                    display = draw(fb);
                    println!("frame updated");
                }
                Err(e) => println!("{e}"),
            }
        }

        window.update(&display);
        for event in window.events() {
            match event {
                SimulatorEvent::Quit => return,
                SimulatorEvent::KeyDown { keycode, .. } => match keycode {
                    Keycode::Q => return,
                    Keycode::R => last_attempt -= INTERVAL,
                    _ => {}
                },
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn fetch_frame(addr: &str) -> Result<Framebuffer, String> {
    let mut stream =
        TcpStream::connect(addr).map_err(|e| format!("connect {addr}: {e} (is qemu-run up?)"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(90)))
        .map_err(|e| format!("socket: {e}"))?;
    // HTTP/1.0 so the server closes the connection instead of chunking
    stream
        .write_all(b"GET /api/frame.bmp HTTP/1.0\r\nHost: siwaj\r\n\r\n")
        .map_err(|e| format!("send: {e}"))?;
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| format!("read: {e}"))?;
    let header_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("malformed http response")?;
    decode_bmp(&raw[header_end + 4..])
        .ok_or("no frame (device configured? one call plan activated?)".to_string())
}

fn draw(fb: Framebuffer) -> SimulatorDisplay<BinaryColor> {
    let mut sim = SimulatorDisplay::<BinaryColor>::new(Size::new(WIDTH, HEIGHT));
    fb.iter_pixels().for_each(|p| {
        p.draw(&mut sim).ok();
    });
    sim
}
