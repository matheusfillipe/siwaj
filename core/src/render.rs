use core::convert::Infallible;

use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::{FONT_6X12, FONT_10X20};
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::rectangle::Rectangle;
use embedded_graphics::primitives::{Circle, Line, PrimitiveStyle};
use embedded_graphics::text::Text;

use crate::{Garment, RainOutlook};

pub const WIDTH: u32 = 200;
pub const HEIGHT: u32 = 200;
const BUF_LEN: usize = (WIDTH * HEIGHT / 8) as usize;

const ON: PrimitiveStyle<BinaryColor> = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
const THICK: PrimitiveStyle<BinaryColor> = PrimitiveStyle::with_stroke(BinaryColor::On, 3);
const FILL: PrimitiveStyle<BinaryColor> = PrimitiveStyle::with_fill(BinaryColor::On);
const SLEEVE: PrimitiveStyle<BinaryColor> = PrimitiveStyle::with_stroke(BinaryColor::On, 9);
const CARVE: PrimitiveStyle<BinaryColor> = PrimitiveStyle::with_fill(BinaryColor::Off);
const CARVE_S1: PrimitiveStyle<BinaryColor> = PrimitiveStyle::with_stroke(BinaryColor::Off, 1);
const CARVE_S2: PrimitiveStyle<BinaryColor> = PrimitiveStyle::with_stroke(BinaryColor::Off, 2);
const CARVE_S3: PrimitiveStyle<BinaryColor> = PrimitiveStyle::with_stroke(BinaryColor::Off, 3);

pub struct Framebuffer {
    buffer: [u8; BUF_LEN],
}

impl Framebuffer {
    pub fn new() -> Framebuffer {
        Framebuffer {
            buffer: [0xFF; BUF_LEN],
        }
    }

    pub fn buffer(&self) -> &[u8] {
        &self.buffer
    }

    pub fn iter_pixels(&self) -> impl Iterator<Item = Pixel<BinaryColor>> + '_ {
        let buf = &self.buffer;
        (0..HEIGHT).flat_map(move |y| {
            (0..WIDTH).map(move |x| {
                let (index, bit) = bit_position(x, y);
                let on = (buf[index] >> bit) & 1 == 0;
                Pixel(Point::new(x as i32, y as i32), BinaryColor::from(on))
            })
        })
    }
}

impl Default for Framebuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// Single source of the packing convention: 1 bit per pixel, MSB first,
/// 0 = black (drawn), 1 = white (background).
fn bit_position(x: u32, y: u32) -> (usize, u32) {
    ((y * WIDTH / 8 + x / 8) as usize, 7 - (x % 8))
}

impl DrawTarget for Framebuffer {
    type Color = BinaryColor;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(coord, color) in pixels {
            let (x, y) = (coord.x as u32, coord.y as u32);
            if x >= WIDTH || y >= HEIGHT {
                continue;
            }
            let (index, bit) = bit_position(x, y);
            match color {
                BinaryColor::On => self.buffer[index] &= !(1 << bit),
                BinaryColor::Off => self.buffer[index] |= 1 << bit,
            }
        }
        Ok(())
    }
}

impl OriginDimensions for Framebuffer {
    fn size(&self) -> Size {
        Size::new(WIDTH, HEIGHT)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeOfDay {
    pub hour: u8,
    pub minute: u8,
}

impl TimeOfDay {
    /// UTC wall time of a unix timestamp.
    pub fn from_unix(unix: u32) -> TimeOfDay {
        let secs_of_day = unix % (24 * 3600);
        TimeOfDay {
            hour: (secs_of_day / 3600) as u8,
            minute: ((secs_of_day % 3600) / 60) as u8,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub garment: Garment,
    pub feels_like_c: f32,
    pub rain: RainOutlook,
    pub rain_threshold_pct: u8,
    pub updated: TimeOfDay,
    /// None when the ADC read failed; drawn as an empty battery with a "--%"
    /// label rather than a fabricated charge level.
    pub battery_pct: Option<u8>,
    pub offline: bool,
}

impl View {
    /// The frame for a failed weather cycle: the safe garment, the clock, and
    /// the real battery, with no invented weather numbers.
    pub fn offline(updated: TimeOfDay, battery_pct: Option<u8>) -> View {
        View {
            garment: Garment::Jacket,
            feels_like_c: 0.0,
            rain: RainOutlook {
                pop_pct_next_hour: 0,
                rain_expected: false,
            },
            rain_threshold_pct: 0,
            updated,
            battery_pct,
            offline: true,
        }
    }

    /// The frame for a successful One Call fetch; the single mapping from a
    /// weather snapshot to display state, shared by the device cycle and the
    /// /api/frame.bmp debug preview.
    pub fn from_snapshot(
        snapshot: &crate::weather::Snapshot,
        config: &crate::Config,
        battery_pct: Option<u8>,
        now_unix: u32,
    ) -> View {
        View {
            garment: Garment::from_feels_like(snapshot.feels_like_c, &config.thresholds),
            feels_like_c: snapshot.feels_like_c,
            rain: snapshot.rain_outlook(),
            rain_threshold_pct: config.rain_threshold_pct,
            updated: TimeOfDay::from_unix(
                now_unix.wrapping_add(snapshot.timezone_offset_secs as u32),
            ),
            battery_pct,
            offline: false,
        }
    }
}

struct Bounds {
    x0: i32,
    y0: i32,
    w: i32,
    h: i32,
}

fn px(b: &Bounds, fx: f32, fy: f32) -> Point {
    Point::new(
        b.x0 + (b.w as f32 * fx) as i32,
        b.y0 + (b.h as f32 * fy) as i32,
    )
}

// Garments are filled silhouettes; necklines, ribs and seams are carved back
// out with white (background) primitives, which reads far better at 200x200
// than stroked outlines.
fn draw_jacket<D: DrawTarget<Color = BinaryColor>>(d: &mut D, b: &Bounds) -> Result<(), D::Error> {
    Circle::new(px(b, 0.5, 0.18), 15)
        .into_styled(THICK)
        .draw(d)?;
    Rectangle::with_corners(px(b, 0.32, 0.10), px(b, 0.68, 0.96))
        .into_styled(FILL)
        .draw(d)?;
    Line::new(px(b, 0.34, 0.14), px(b, 0.08, 0.66))
        .into_styled(SLEEVE)
        .draw(d)?;
    Line::new(px(b, 0.66, 0.14), px(b, 0.92, 0.66))
        .into_styled(SLEEVE)
        .draw(d)?;
    Line::new(px(b, 0.40, 0.10), px(b, 0.5, 0.30))
        .into_styled(CARVE_S3)
        .draw(d)?;
    Line::new(px(b, 0.60, 0.10), px(b, 0.5, 0.30))
        .into_styled(CARVE_S3)
        .draw(d)?;
    Line::new(px(b, 0.5, 0.30), px(b, 0.5, 0.96))
        .into_styled(CARVE_S2)
        .draw(d)?;
    Line::new(px(b, 0.36, 0.72), px(b, 0.46, 0.72))
        .into_styled(CARVE_S1)
        .draw(d)?;
    Line::new(px(b, 0.54, 0.72), px(b, 0.64, 0.72))
        .into_styled(CARVE_S1)
        .draw(d)?;
    Ok(())
}

fn draw_pullover<D: DrawTarget<Color = BinaryColor>>(
    d: &mut D,
    b: &Bounds,
) -> Result<(), D::Error> {
    Rectangle::with_corners(px(b, 0.34, 0.08), px(b, 0.66, 0.96))
        .into_styled(FILL)
        .draw(d)?;
    Line::new(px(b, 0.36, 0.12), px(b, 0.12, 0.68))
        .into_styled(SLEEVE)
        .draw(d)?;
    Line::new(px(b, 0.64, 0.12), px(b, 0.88, 0.68))
        .into_styled(SLEEVE)
        .draw(d)?;
    Rectangle::with_corners(px(b, 0.41, 0.04), px(b, 0.59, 0.12))
        .into_styled(CARVE)
        .draw(d)?;
    Line::new(px(b, 0.36, 0.86), px(b, 0.64, 0.86))
        .into_styled(CARVE_S1)
        .draw(d)?;
    Line::new(px(b, 0.36, 0.91), px(b, 0.64, 0.91))
        .into_styled(CARVE_S1)
        .draw(d)?;
    Ok(())
}

fn draw_shirt<D: DrawTarget<Color = BinaryColor>>(d: &mut D, b: &Bounds) -> Result<(), D::Error> {
    Rectangle::with_corners(px(b, 0.36, 0.10), px(b, 0.64, 0.96))
        .into_styled(FILL)
        .draw(d)?;
    Line::new(px(b, 0.38, 0.14), px(b, 0.15, 0.62))
        .into_styled(SLEEVE)
        .draw(d)?;
    Line::new(px(b, 0.62, 0.14), px(b, 0.85, 0.62))
        .into_styled(SLEEVE)
        .draw(d)?;
    Line::new(px(b, 0.41, 0.10), px(b, 0.5, 0.26))
        .into_styled(CARVE_S3)
        .draw(d)?;
    Line::new(px(b, 0.59, 0.10), px(b, 0.5, 0.26))
        .into_styled(CARVE_S3)
        .draw(d)?;
    Line::new(px(b, 0.5, 0.26), px(b, 0.5, 0.96))
        .into_styled(CARVE_S1)
        .draw(d)?;
    Ok(())
}

fn draw_tshirt<D: DrawTarget<Color = BinaryColor>>(d: &mut D, b: &Bounds) -> Result<(), D::Error> {
    Rectangle::with_corners(px(b, 0.34, 0.12), px(b, 0.66, 0.96))
        .into_styled(FILL)
        .draw(d)?;
    Line::new(px(b, 0.36, 0.18), px(b, 0.14, 0.36))
        .into_styled(SLEEVE)
        .draw(d)?;
    Line::new(px(b, 0.64, 0.18), px(b, 0.86, 0.36))
        .into_styled(SLEEVE)
        .draw(d)?;
    Rectangle::with_corners(px(b, 0.40, 0.07), px(b, 0.60, 0.16))
        .into_styled(CARVE)
        .draw(d)?;
    Ok(())
}

fn draw_garment<D: DrawTarget<Color = BinaryColor>>(
    d: &mut D,
    garment: Garment,
) -> Result<(), D::Error> {
    let b = Bounds {
        x0: 45,
        y0: 6,
        w: 110,
        h: 84,
    };
    match garment {
        Garment::Jacket => draw_jacket(d, &b),
        Garment::Pullover => draw_pullover(d, &b),
        Garment::Shirt => draw_shirt(d, &b),
        Garment::TShirt => draw_tshirt(d, &b),
    }
}

const PATTERNS: [&[u8]; 12] = [
    &[0b111, 0b101, 0b101, 0b101, 0b111],
    &[0b010, 0b110, 0b010, 0b010, 0b111],
    &[0b111, 0b001, 0b111, 0b100, 0b111],
    &[0b111, 0b001, 0b011, 0b001, 0b111],
    &[0b101, 0b101, 0b111, 0b001, 0b001],
    &[0b111, 0b100, 0b111, 0b001, 0b111],
    &[0b111, 0b100, 0b111, 0b101, 0b111],
    &[0b111, 0b001, 0b001, 0b010, 0b010],
    &[0b111, 0b101, 0b111, 0b101, 0b111],
    &[0b111, 0b101, 0b111, 0b001, 0b111],
    &[0b000, 0b000, 0b111, 0b000, 0b000],
    &[0b11, 0b11],
];

/// Glyph lookup for the big custom font; None for unsupported characters.
fn glyph(ch: char) -> Option<&'static [u8]> {
    match ch {
        '0'..='9' => Some(PATTERNS[ch as usize - '0' as usize]),
        '-' => Some(PATTERNS[10]),
        '\u{00b0}' => Some(PATTERNS[11]),
        _ => None,
    }
}

/// Column count comes from the pattern itself (longest row in bits), so the
/// degree sign's narrow glyph stays a data property instead of a special case.
fn glyph_cols(ch: char) -> usize {
    glyph(ch).map_or(3, |pattern| {
        pattern
            .iter()
            .map(|row| 8 - row.leading_zeros())
            .max()
            .unwrap_or(3) as usize
    })
}

fn draw_big_text<D: DrawTarget<Color = BinaryColor>>(
    d: &mut D,
    origin: Point,
    text: &str,
    cell: i32,
) -> Result<(), D::Error> {
    let mut x = origin.x;
    for ch in text.chars() {
        let Some(pattern) = glyph(ch) else {
            continue;
        };
        let pattern_cols = glyph_cols(ch) as i32;
        for (ry, row) in pattern.iter().enumerate() {
            for rx in 0..pattern_cols {
                if (row >> (pattern_cols - 1 - rx)) & 1 == 1 {
                    let top_left = Point::new(x + rx * cell, origin.y + ry as i32 * cell);
                    let rect = Rectangle::with_corners(
                        top_left,
                        top_left + Size::new(cell as u32 - 1, cell as u32 - 1),
                    );
                    rect.into_styled(FILL).draw(d)?;
                }
            }
        }
        x += pattern_cols * cell + cell;
    }
    Ok(())
}

fn big_text_width(text: &str, cell: i32) -> i32 {
    let mut w = 0;
    for ch in text.chars().filter(|ch| glyph(*ch).is_some()) {
        w += glyph_cols(ch) as i32 * cell + cell;
    }
    w - cell
}

fn centered_small<D: DrawTarget<Color = BinaryColor>>(
    d: &mut D,
    cx: i32,
    y: i32,
    text: &str,
) -> Result<(), D::Error> {
    let style = MonoTextStyle::new(&FONT_6X12, BinaryColor::On);
    let bb = Text::new(text, Point::zero(), style).bounding_box();
    let x = cx - bb.size.width as i32 / 2;
    Text::new(text, Point::new(x, y), style).draw(d).map(|_| ())
}

fn draw_rain_badge<D: DrawTarget<Color = BinaryColor>>(
    d: &mut D,
    view: &View,
) -> Result<(), D::Error> {
    let (bx, by) = (8, 156);
    Circle::new(Point::new(bx + 6, by + 6), 6)
        .into_styled(FILL)
        .draw(d)?;
    Circle::new(Point::new(bx + 14, by + 3), 7)
        .into_styled(FILL)
        .draw(d)?;
    Rectangle::with_corners(Point::new(bx + 1, by + 6), Point::new(bx + 21, by + 13))
        .into_styled(FILL)
        .draw(d)?;
    if view.rain.rain_expected || view.rain.is_risk(view.rain_threshold_pct) {
        for i in 0..3 {
            let x = bx + 3 + i * 7;
            Line::new(Point::new(x, by + 18), Point::new(x - 3, by + 24))
                .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
                .draw(d)?;
        }
    }
    let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    Text::new(
        &format!("{}%", view.rain.pop_pct_next_hour),
        Point::new(bx + 26, by + 20),
        style,
    )
    .draw(d)
    .map(|_| ())
}

fn draw_battery<D: DrawTarget<Color = BinaryColor>>(
    d: &mut D,
    pct: Option<u8>,
) -> Result<(), D::Error> {
    let (bx, by) = (160, 162);
    Rectangle::with_corners(Point::new(bx, by), Point::new(bx + 27, by + 12))
        .into_styled(ON)
        .draw(d)?;
    Rectangle::with_corners(Point::new(bx + 28, by + 3), Point::new(bx + 30, by + 9))
        .into_styled(FILL)
        .draw(d)?;
    if let Some(pct) = pct {
        let inner = (24 * pct.min(100) as u32) / 100;
        if inner > 0 {
            Rectangle::with_corners(
                Point::new(bx + 2, by + 2),
                Point::new(bx + 1 + inner as i32, by + 10),
            )
            .into_styled(FILL)
            .draw(d)?;
        }
    }
    Ok(())
}

fn draw_updated<D: DrawTarget<Color = BinaryColor>>(
    d: &mut D,
    updated: TimeOfDay,
) -> Result<(), D::Error> {
    let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    let text = format!("{:02}:{:02}", updated.hour, updated.minute);
    let bb = Text::new(&text, Point::zero(), style).bounding_box();
    let x = WIDTH as i32 / 2 - bb.size.width as i32 / 2;
    Text::new(&text, Point::new(x, 176), style)
        .draw(d)
        .map(|_| ())
}

pub fn render(view: &View) -> Framebuffer {
    let mut fb = Framebuffer::new();
    render_to(&mut fb, view);
    fb
}

/// Fixed 54-byte header for the 24-bit bottom-up BMP this module emits.
pub fn bmp_header() -> [u8; 54] {
    let row = WIDTH as usize * 3;
    let data_len = row * HEIGHT as usize;
    let mut out = [0u8; 54];
    out[0..2].copy_from_slice(b"BM");
    out[2..6].copy_from_slice(&((54 + data_len) as u32).to_le_bytes());
    out[10..14].copy_from_slice(&54u32.to_le_bytes());
    out[14..18].copy_from_slice(&40u32.to_le_bytes());
    out[18..22].copy_from_slice(&(WIDTH as i32).to_le_bytes());
    out[22..26].copy_from_slice(&(HEIGHT as i32).to_le_bytes());
    out[26..28].copy_from_slice(&1u16.to_le_bytes());
    out[28..30].copy_from_slice(&24u16.to_le_bytes());
    out[34..38].copy_from_slice(&(data_len as u32).to_le_bytes());
    out[38..42].copy_from_slice(&2835i32.to_le_bytes());
    out[42..46].copy_from_slice(&2835i32.to_le_bytes());
    out
}

/// Writes BMP pixel row `y_from_bottom` (0 = last image row) into `out`,
/// which must be exactly WIDTH*3 bytes; lets emitters stream instead of
/// holding the full image in RAM.
pub fn bmp_row(fb: &Framebuffer, y_from_bottom: usize, out: &mut [u8]) {
    let y = HEIGHT as usize - 1 - y_from_bottom;
    let row = out.chunks_exact_mut(3);
    for (x, px) in row.enumerate() {
        let (index, bit) = bit_position(x as u32, y as u32);
        let on = (fb.buffer[index] >> bit) & 1 == 0;
        let v = if on { 0x00 } else { 0xFF };
        px.copy_from_slice(&[v, v, v]);
    }
}

/// Encodes a frame as a 24-bit bottom-up BMP, the one image format every
/// browser renders and a device can emit without a compressor.
pub fn bmp(fb: &Framebuffer) -> Vec<u8> {
    let mut out = Vec::with_capacity(54 + WIDTH as usize * 3 * HEIGHT as usize);
    out.extend_from_slice(&bmp_header());
    let mut row = vec![0u8; WIDTH as usize * 3];
    for y in 0..HEIGHT as usize {
        bmp_row(fb, y, &mut row);
        out.extend_from_slice(&row);
    }
    out
}

/// Decodes the 24-bit BMP emitted by `bmp` back into a frame; host-side only
/// (live display mirror and tests). The device encodes, never decodes, so
/// this is compiled out of firmware builds.
#[cfg(any(test, feature = "preview"))]
pub fn decode_bmp(body: &[u8]) -> Option<Framebuffer> {
    if body.len() < 54 || &body[0..2] != b"BM" {
        return None;
    }
    let width = u32::from_le_bytes(body[18..22].try_into().ok()?) as usize;
    let height = u32::from_le_bytes(body[22..26].try_into().ok()?) as usize;
    if width != WIDTH as usize || height != HEIGHT as usize {
        return None;
    }
    let data = body.get(54..)?;
    let mut fb = Framebuffer::new();
    for row in 0..height {
        let y = height - 1 - row;
        for x in 0..width {
            let px = data.get(row * width * 3 + x * 3..row * width * 3 + x * 3 + 3)?;
            if px == [0x00, 0x00, 0x00] {
                let (index, bit) = bit_position(x as u32, y as u32);
                fb.buffer[index] &= !(1 << bit);
            }
        }
    }
    Some(fb)
}

pub fn render_to<D: DrawTarget<Color = BinaryColor>>(d: &mut D, view: &View) {
    draw_garment(d, view.garment).ok();
    if view.offline {
        centered_small(d, WIDTH as i32 / 2, 108, "offline").ok();
    } else {
        let rounded = view.feels_like_c.round() as i32;
        let temp = format!("{rounded}\u{00b0}");
        let cell = 9;
        let w = big_text_width(&temp, cell);
        draw_big_text(d, Point::new((WIDTH as i32 - w) / 2, 96), &temp, cell).ok();
        centered_small(d, WIDTH as i32 / 2, 150, "feels like").ok();
        draw_rain_badge(d, view).ok();
    }
    draw_battery(d, view.battery_pct).ok();
    draw_updated(d, view.updated).ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Framebuffer {
        let mut view = crate::render::View::offline(
            TimeOfDay {
                hour: 14,
                minute: 32,
            },
            Some(87),
        );
        view.garment = crate::Garment::Pullover;
        render(&view)
    }

    #[test]
    fn bmp_decode_round_trips() {
        let fb = sample();
        let back = decode_bmp(&bmp(&fb)).unwrap();
        assert_eq!(back.buffer(), fb.buffer());
    }

    #[test]
    fn bmp_decode_rejects_wrong_shapes() {
        assert!(decode_bmp(b"").is_none());
        assert!(decode_bmp(b"BM").is_none());
        let mut tiny = vec![0u8; 54];
        tiny[0..2].copy_from_slice(b"BM");
        tiny[18..22].copy_from_slice(&100u32.to_le_bytes());
        tiny[22..26].copy_from_slice(&100u32.to_le_bytes());
        assert!(decode_bmp(&tiny).is_none());
    }
}
