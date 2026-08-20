use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics_simulator::sdl2::Keycode;
use embedded_graphics_simulator::{
    BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
};
use siwaj_core::render::{FRAME_BYTES, Framebuffer, HEIGHT, WIDTH};

/// Polling costs the device one memory read, so it is cheap enough to keep
/// the window in step with a config change instead of a refresh interval.
const INTERVAL: Duration = Duration::from_secs(1);

/// Live mirror of the emulated device's e-paper: polls GET /api/frame (make
/// qemu-run first) and shows the bytes the device last rendered. Polling is
/// free; the device decides when to fetch weather again.
/// r refreshes now, q quits.
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
    let mut last_attempt = Instant::now() - INTERVAL;
    let mut last_face = String::new();

    loop {
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
        if last_attempt.elapsed() >= INTERVAL {
            last_attempt = Instant::now();
            match fetch_frame(&addr) {
                Ok((fb, face)) => {
                    if face != last_face {
                        println!("{face} frame");
                        last_face = face;
                    }
                    display = draw(&fb);
                }
                Err(e) => println!("{e}"),
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The esp httpd refuses HTTP/1.0 outright and never closes a 1.1 connection,
/// so the terminating chunk is the only end-of-response signal available.
fn fetch_frame(addr: &str) -> Result<(Framebuffer, String), String> {
    let mut stream =
        TcpStream::connect(addr).map_err(|e| format!("connect {addr}: {e} (is qemu-run up?)"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("socket: {e}"))?;
    stream
        .write_all(b"GET /api/frame HTTP/1.1\r\nHost: siwaj\r\n\r\n")
        .map_err(|e| format!("send: {e}"))?;

    let mut raw = Vec::with_capacity(FRAME_BYTES + 512);
    let mut buf = [0u8; 4096];
    while !raw.ends_with(b"0\r\n\r\n") {
        let n = stream.read(&mut buf).map_err(|e| format!("read: {e}"))?;
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
    }

    let header_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("malformed http response")?;
    let head = String::from_utf8_lossy(&raw[..header_end]).to_ascii_lowercase();
    let tail = &raw[header_end + 4..];
    // the handlers chunk, but httpd's own errors carry a Content-Length
    let body = if head.contains("transfer-encoding: chunked") {
        dechunk(tail).ok_or("malformed chunked body")?
    } else {
        tail.to_vec()
    };
    let code = status_code(&raw).ok_or("no status line")?;
    if code != 200 {
        return Err(format!(
            "device says {code}: {}",
            String::from_utf8_lossy(&body).trim()
        ));
    }
    let face = if head.contains("x-siwaj-frame: offline") {
        "offline"
    } else {
        "live"
    };
    let fb = Framebuffer::from_bytes(&body).ok_or_else(|| {
        format!(
            "expected {FRAME_BYTES} frame bytes, device sent {}",
            body.len()
        )
    })?;
    Ok((fb, face.to_string()))
}

fn dechunk(mut body: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let eol = body.windows(2).position(|w| w == b"\r\n")?;
        let size =
            usize::from_str_radix(std::str::from_utf8(&body[..eol]).ok()?.trim(), 16).ok()?;
        if size == 0 {
            return Some(out);
        }
        out.extend_from_slice(body.get(eol + 2..eol + 2 + size)?);
        body = body.get(eol + 2 + size + 2..)?;
    }
}

fn status_code(raw: &[u8]) -> Option<u16> {
    let line = raw.split(|&b| b == b'\n').next()?;
    std::str::from_utf8(line)
        .ok()?
        .split_ascii_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

fn draw(fb: &Framebuffer) -> SimulatorDisplay<BinaryColor> {
    let mut sim = SimulatorDisplay::<BinaryColor>::new(Size::new(WIDTH, HEIGHT));
    fb.iter_pixels().for_each(|p| {
        p.draw(&mut sim).ok();
    });
    sim
}
