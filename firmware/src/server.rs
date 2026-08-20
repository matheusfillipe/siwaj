use embedded_svc::http::{Headers, Method};
use esp_idf_svc::http::server::{Configuration, EspHttpServer};
use esp_idf_svc::http::server::Request;
use esp_idf_svc::io::{Read, Write};

use crate::secrets::Secrets;
use crate::store::Store;
use crate::weather;

static INDEX_HTML: &str = include_str!("../../web/index.html");
static STYLES_GZ: &[u8] = include_bytes!("../../web/dist/styles.css.gz");
static APP_GZ: &[u8] = include_bytes!("../../web/dist/app.js.gz");

type AnyError = anyhow::Error;

fn serve_gz(req: Request<&mut esp_idf_svc::http::server::EspHttpConnection<'_>>, body: &[u8], ctype: &str) -> Result<(), AnyError> {
    let mut resp = req.into_response(
        200,
        Some("OK"),
        &[
            ("Content-Type", ctype),
            ("Content-Encoding", "gzip"),
            ("Cache-Control", "no-cache"),
        ],
    )?;
    resp.write_all(body)?;
    resp.flush()?;
    Ok(())
}

fn serve_json(
    req: Request<&mut esp_idf_svc::http::server::EspHttpConnection<'_>>,
    payload: &impl serde::Serialize,
) -> Result<(), AnyError> {
    let body = serde_json::to_vec(payload)?;
    let mut resp = req.into_ok_response()?;
    resp.write_all(&body)?;
    resp.flush()?;
    Ok(())
}

/// POST /api/config policy: geocode the city, bump the revision, stamp the
/// time. Storage only happens after this succeeds.
fn assemble_config(
    store: &Store,
    submit: siwaj_core::ConfigSubmit,
    geocode: Option<(f64, f64)>,
) -> anyhow::Result<siwaj_core::Config> {
    use anyhow::Context;

    let Some((lat, lon)) = geocode else {
        anyhow::bail!(
            "could not resolve '{}' to coordinates (is the API key provisioned?)",
            submit.location_name
        );
    };
    let current = store.load().context("stored config unreadable")?;
    let config = siwaj_core::Config {
        schema_version: siwaj_core::CONFIG_SCHEMA_VERSION,
        revision: current
            .as_ref()
            .map(|c| c.revision.wrapping_add(1))
            .unwrap_or(1),
        date_modified_unix: crate::now_unix(),
        thresholds: submit.thresholds,
        rain_threshold_pct: submit.rain_threshold_pct,
        refresh_minutes: submit.refresh_minutes,
        location: siwaj_core::Location {
            name: submit.location_name,
            lat,
            lon,
        },
    };
    config.validate().context("invalid config")?;
    Ok(config)
}

pub fn start(store: &'static Store, secrets: &'static Secrets) -> Result<EspHttpServer<'static>, anyhow::Error> {
    let mut server = EspHttpServer::new(&Configuration {
        stack_size: 12288,
        max_uri_handlers: 8,
        ..Default::default()
    })?;

    server.fn_handler::<AnyError, _>("/", Method::Get, |req| {
        let mut resp = req.into_response(
            200,
            Some("OK"),
            &[("Content-Type", "text/html; charset=utf-8")],
        )?;
        resp.write_all(INDEX_HTML.as_bytes())?;
        resp.flush()?;
        Ok(())
    })?;

    server.fn_handler::<AnyError, _>("/styles.css", Method::Get, |req| {
        serve_gz(req, STYLES_GZ, "text/css")
    })?;

    server.fn_handler::<AnyError, _>("/app.js", Method::Get, |req| {
        serve_gz(req, APP_GZ, "text/javascript")
    })?;

    server.fn_handler::<AnyError, _>("/api/config", Method::Get, |req| {
        let config = match store.load() {
            Ok(config) => config,
            Err(e) => {
                log::error!("stored config unreadable: {e}");
                None
            }
        };
        let payload = siwaj_core::ConfigState {
            configured: config.is_some(),
            revision: config.as_ref().map(|c| c.revision).unwrap_or(0),
            config,
        };
        serve_json(req, &payload)
    })?;

    server.fn_handler::<AnyError, _>("/api/config", Method::Post, |mut req| {
        let len = req.content_len().unwrap_or(0) as usize;
        if len == 0 || len > 2048 {
            let mut resp = req.into_status_response(400)?;
            resp.write_all(b"bad request")?;
            return Ok(());
        }
        let mut buf = vec![0u8; len];
        req.read_exact(&mut buf)?;
        let submit: siwaj_core::ConfigSubmit = match serde_json::from_slice(&buf) {
            Ok(s) => s,
            Err(_) => {
                let mut resp = req.into_status_response(400)?;
                resp.write_all(b"invalid payload")?;
                return Ok(());
            }
        };
        let geocode = weather::geocode(secrets, &submit.location_name);
        let config = match assemble_config(store, submit, geocode) {
            Ok(c) => c,
            Err(e) => {
                let mut resp = req.into_status_response(422)?;
                resp.write_all(e.to_string().as_bytes())?;
                return Ok(());
            }
        };
        store.save(&config)?;
        log::info!("config saved, revision {}", config.revision);
        serve_json(req, &config)
    })?;

    server.fn_handler::<AnyError, _>("/api/weather", Method::Get, |req| {
        let config = match store.load() {
            Ok(Some(config)) => config,
            Ok(None) => {
                let mut resp = req.into_status_response(409)?;
                resp.write_all(b"not configured")?;
                return Ok(());
            }
            Err(e) => {
                let mut resp = req.into_status_response(500)?;
                resp.write_all(e.to_string().as_bytes())?;
                return Ok(());
            }
        };
        match weather::fetch(secrets, config.location.lat, config.location.lon) {
            Ok(snapshot) => {
                let payload = siwaj_core::WeatherProbe {
                    feels_like_c: snapshot.feels_like_c,
                    next_hour_pop_frac: snapshot.next_hour_pop_frac,
                    max_minutely_mm: snapshot.minutely_mm.iter().copied().fold(0.0_f32, f32::max),
                    timezone_offset_secs: snapshot.timezone_offset_secs,
                };
                serve_json(req, &payload)?;
            }
            Err(e) => {
                let mut resp = req.into_status_response(502)?;
                resp.write_all(e.to_string().as_bytes())?;
            }
        }
        Ok(())
    })?;

    Ok(server)
}
