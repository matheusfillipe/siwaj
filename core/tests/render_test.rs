use std::env;
use std::fs;
use std::path::PathBuf;

use siwaj_core::render::{Framebuffer, TimeOfDay, View, render};
use siwaj_core::{Garment, RainOutlook, Thresholds};

const SNAPSHOT_DIR: &str = "tests/snapshots";

fn view(garment: Garment, rain: RainOutlook, feels_like: f32) -> View {
    View {
        garment,
        feels_like_c: feels_like,
        rain,
        rain_threshold_pct: 30,
        updated: TimeOfDay {
            hour: 14,
            minute: 32,
        },
        battery_pct: Some(87),
        charging: false,
        serving: false,
        offline: false,
    }
}

fn rain(pct: u8, expected: bool) -> RainOutlook {
    RainOutlook {
        pop_pct_next_hour: pct,
        rain_expected: expected,
    }
}

fn cases() -> Vec<(&'static str, View)> {
    let t = Thresholds {
        low_c: 8.0,
        high_c: 21.0,
    };
    let mut battery_unknown = view(Garment::from_feels_like(11.0, &t), rain(10, false), 11.0);
    battery_unknown.battery_pct = None;
    vec![
        (
            "jacket_rain",
            view(Garment::from_feels_like(-3.4, &t), rain(62, true), -3.4),
        ),
        (
            "pullover_dry",
            view(Garment::from_feels_like(11.0, &t), rain(10, false), 11.0),
        ),
        (
            "shirt_rain",
            view(Garment::from_feels_like(24.0, &t), rain(45, false), 24.0),
        ),
        ("battery_unknown", battery_unknown),
        ("serving", {
            let mut serving = view(Garment::from_feels_like(11.0, &t), rain(10, false), 11.0);
            serving.serving = true;
            serving
        }),
        (
            "offline",
            View::offline(TimeOfDay { hour: 9, minute: 5 }, Some(41), false),
        ),
    ]
}

#[test]
fn snapshots_match_fixtures() {
    let update = env::var("UPDATE_SNAPSHOTS").is_ok();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(SNAPSHOT_DIR);
    fs::create_dir_all(&dir).unwrap();
    for (name, view) in cases() {
        let fb: Framebuffer = render(&view);
        let path = dir.join(format!("{name}.bin"));
        if update {
            fs::write(&path, fb.buffer()).unwrap();
            continue;
        }
        let expected = fs::read(&path).unwrap_or_else(|_| {
            panic!("missing fixture {name}; run `make core-snapshots` to (re)generate it")
        });
        assert_eq!(
            fb.buffer(),
            expected.as_slice(),
            "render output for {name} differs from fixture; if intentional run `make core-snapshots`"
        );
    }
}

#[test]
fn framebuffer_starts_white() {
    let fb = Framebuffer::new();
    assert!(fb.buffer().iter().all(|&b| b == 0xFF));
    assert_eq!(fb.buffer().len(), siwaj_core::render::FRAME_BYTES);
}

#[test]
fn temp_rounds_and_signs() {
    let t = Thresholds {
        low_c: 8.0,
        high_c: 21.0,
    };
    let v = view(Garment::from_feels_like(11.6, &t), rain(0, false), 11.6);
    let fb = render(&v);
    assert!(fb.buffer().iter().any(|&b| b != 0xFF));
}

#[test]
fn time_of_day_from_unix() {
    use siwaj_core::render::TimeOfDay;
    assert_eq!(TimeOfDay::from_unix(0), TimeOfDay { hour: 0, minute: 0 });
    assert_eq!(
        TimeOfDay::from_unix(24 * 3600 + 3661),
        TimeOfDay { hour: 1, minute: 1 }
    );
}

#[test]
fn view_from_snapshot_maps_every_field() {
    use siwaj_core::render::{TimeOfDay, View};
    use siwaj_core::weather::Snapshot;

    let config = siwaj_core::Config::example();
    let snapshot = Snapshot {
        feels_like_c: 17.5,
        minutely_mm: {
            let mut mm = [0.0_f32; siwaj_core::weather::MINUTELY_LEN];
            mm[5] = 0.4;
            mm
        },
        next_hour_pop_frac: 0.25,
        timezone_offset_secs: 3600,
    };
    let v = View::from_snapshot(&snapshot, &config, Some(87), false, 7_200);
    assert_eq!(
        v.garment,
        Garment::Pullover,
        "17.5C sits under the example high of 18C"
    );
    assert_eq!(v.feels_like_c, 17.5);
    assert_eq!(v.rain.pop_pct_next_hour, 25);
    assert!(v.rain.rain_expected);
    assert_eq!(v.rain_threshold_pct, config.rain_threshold_pct);
    assert_eq!(v.updated, TimeOfDay { hour: 3, minute: 0 });
    assert_eq!(v.battery_pct, Some(87));
    assert!(!v.offline);
}

#[test]
fn frame_is_a_packed_1bpp_buffer() {
    let fb = render(&view(Garment::Jacket, rain(50, true), -3.4));
    assert_eq!(fb.buffer().len(), siwaj_core::render::FRAME_BYTES);
    assert!(fb.buffer().contains(&0xFF), "background is unset bits");
    assert!(
        fb.buffer().iter().any(|b| *b != 0xFF),
        "ink is cleared bits"
    );
}
