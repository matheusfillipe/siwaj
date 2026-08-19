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
        let config = store.load();
        let payload = serde_json::json!({
            "configured": config.is_some(),
            "revision": config.as_ref().map(|c| c.revision).unwrap_or(0),
            "config": config,
        });
        let body = serde_json::to_vec(&payload)?;
        let mut resp = req.into_ok_response()?;
        resp.write_all(&body)?;
        resp.flush()?;
        Ok(())
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
        let config = match store.config_from_submit(submit, geocode) {
            Ok(c) => c,
            Err(e) => {
                let mut resp = req.into_status_response(422)?;
                resp.write_all(e.as_bytes())?;
                return Ok(());
            }
        };
        store.save(&config).map_err(|e| anyhow::anyhow!(e))?;
        log::info!("config saved, revision {}", config.revision);
        let body = serde_json::to_vec(&config)?;
        let mut resp = req.into_ok_response()?;
        resp.write_all(&body)?;
        resp.flush()?;
        Ok(())
    })?;

    server.fn_handler::<AnyError, _>("/api/weather", Method::Get, |req| {
        let Some(config) = store.load() else {
            let mut resp = req.into_status_response(409)?;
            resp.write_all(b"not configured")?;
            return Ok(());
        };
        match weather::fetch(secrets, config.location.lat, config.location.lon) {
            Ok(snapshot) => {
                let max_minutely = snapshot.minutely_mm.iter().copied().fold(0.0_f32, f32::max);
                let payload = serde_json::json!({
                    "feelsLikeC": snapshot.feels_like_c,
                    "hourlyPop": snapshot.hourly_pop,
                    "maxMinutelyMm": max_minutely,
                    "timezoneOffsetSecs": snapshot.timezone_offset_secs,
                });
                let body = serde_json::to_vec(&payload)?;
                let mut resp = req.into_ok_response()?;
                resp.write_all(&body)?;
                resp.flush()?;
            }
            Err(e) => {
                let mut resp = req.into_status_response(502)?;
                resp.write_all(e.as_bytes())?;
            }
        }
        Ok(())
    })?;

    Ok(server)
}
