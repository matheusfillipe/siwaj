use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use siwaj_core::render::{FRAME_BYTES, Framebuffer};

use crate::secrets::Secrets;
use crate::store::Store;

/// The sense lines QEMU has no hardware for. This lives with the emulator's
/// frame loop rather than in the shared contract: the device reads the real
/// board and never takes a reading from the network, so nothing about this
/// shape belongs in what ships to it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SimInputs {
    pub charging: bool,
}

pub struct Frame {
    pub bytes: [u8; FRAME_BYTES],
    /// False when the cycle fell back to the offline face, which looks like a
    /// valid frame on the wire and would otherwise hide a dead weather fetch.
    pub live: bool,
}

static LAST: Mutex<Option<Frame>> = Mutex::new(None);
/// Stands in for the charger sense the emulator has no hardware for, so the
/// charging frame can be exercised without a bench supply.
static CHARGING: AtomicBool = AtomicBool::new(false);
static REDRAW_PENDING: AtomicBool = AtomicBool::new(false);
/// Whether the config page is up, so the mirror shows the same mark the panel
/// would carry while the device is serving.
static SERVING: AtomicBool = AtomicBool::new(false);

const POLL: Duration = Duration::from_secs(1);

pub fn set_charging(on: bool) {
    CHARGING.store(on, Ordering::Relaxed);
    REDRAW_PENDING.store(true, Ordering::Relaxed);
}

pub fn charging() -> bool {
    CHARGING.load(Ordering::Relaxed)
}

pub fn is_serving() -> bool {
    SERVING.load(Ordering::Relaxed)
}

pub fn set_serving(on: bool) {
    SERVING.store(on, Ordering::Relaxed);
    REDRAW_PENDING.store(true, Ordering::Relaxed);
}

/// Borrowed rather than copied out: the frame is 5000 bytes and the httpd
/// task it gets written from has a 12KB stack.
pub fn last() -> MutexGuard<'static, Option<Frame>> {
    LAST.lock().expect("frame lock")
}

/// The emulator has no panel, so this stands in for the device's weather
/// cycle: same fetch, same view, published to memory. A changed revision
/// re-renders at once, and a cycle that fell back to offline retries on the
/// short interval so the display recovers without a config edit.
pub fn spawn_loop(store: &'static Store, secrets: &'static Secrets) {
    std::thread::Builder::new()
        .name("frame".to_string())
        .stack_size(32768)
        .spawn(move || {
            let booted = Instant::now();
            let mut rendered: Option<(u32, Instant, bool)> = None;
            let mut showing: Option<siwaj_core::render::View> = None;
            loop {
                match store.load() {
                    Ok(Some(config)) => {
                        let switched = REDRAW_PENDING.swap(false, Ordering::Relaxed);
                        if due(rendered, &config) {
                            let view = crate::weather_view(
                                secrets,
                                &config,
                                simulated_battery(booted),
                                charging(),
                            );
                            let mut view = view;
                            view.serving = SERVING.load(Ordering::Relaxed);
                            let live = !view.offline;
                            publish(&siwaj_core::render::render(&view), live);
                            rendered = Some((config.revision, Instant::now(), live));
                            showing = Some(view);
                        } else if switched {
                            // a flipped switch changes only what the frame
                            // draws, so redraw the weather already in hand
                            // rather than spending a fetch on it
                            if let Some(view) = showing.as_mut() {
                                view.battery_pct = simulated_battery(booted);
                                view.charging = charging();
                                view.serving = SERVING.load(Ordering::Relaxed);
                                publish(&siwaj_core::render::render(view), !view.offline);
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(e) => log::error!("stored config unreadable: {e:#}"),
                }
                std::thread::sleep(POLL);
            }
        })
        .expect("spawn frame loop");
}

fn publish(fb: &Framebuffer, live: bool) {
    let mut bytes = [0u8; FRAME_BYTES];
    bytes.copy_from_slice(fb.buffer());
    *last() = Some(Frame { bytes, live });
}

/// QEMU emulates no ADC, so the emulator stands one in: a drain from full at
/// 1% per minute, which walks the gauge through every level a real board
/// would reach. The device reads `Board::battery` instead.
fn simulated_battery(booted: Instant) -> Option<u8> {
    let drained = booted.elapsed().as_secs() / 60;
    Some(100u64.saturating_sub(drained) as u8)
}

fn due(rendered: Option<(u32, Instant, bool)>, config: &siwaj_core::Config) -> bool {
    match rendered {
        Some((revision, at, live)) => {
            revision != config.revision || at.elapsed() >= config.next_fetch_delay(live)
        }
        None => true,
    }
}
