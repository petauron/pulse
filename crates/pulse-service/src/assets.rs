use axum::{
    body::Body,
    extract::State,
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;
use serde_json::Value;
use tokio::sync::Semaphore;

use crate::AppState;

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
struct WebAssets;
static THEME_ASSET_PERMITS: Semaphore = Semaphore::const_new(8);

fn explicitly_configured_https_origin(settings: &Value, key: &str) -> Option<String> {
    let raw = settings.get(key)?.as_str()?;
    https_origin(raw)
}

fn https_origin(raw: &str) -> Option<String> {
    let url = reqwest::Url::parse(raw.trim()).ok()?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    Some(url.origin().ascii_serialization())
}

fn theme_csp(settings: &Value, active_theme: &str) -> String {
    let mut connect = Vec::new();
    if let Some(origin) = explicitly_configured_https_origin(settings, "costRateApiUrl") {
        connect.push(origin);
    } else if active_theme == "LuminaPlus" {
        // LuminaPlus v1.3.5 defaults to this endpoint, explicitly approved for this theme.
        connect.push("https://api.frankfurter.dev".to_owned());
    }
    let mut images = Vec::new();
    for key in ["backgroundImage", "backgroundImageMobile"] {
        // LuminaPlus supports a light|dark URL pair for each viewport.
        if let Some(raw) = settings.get(key).and_then(Value::as_str) {
            for url in raw.split('|').take(2) {
                if let Some(origin) = https_origin(url)
                    && !images.contains(&origin)
                {
                    images.push(origin);
                }
            }
        }
    }
    let mut media = Vec::new();
    for key in ["backgroundVideo", "backgroundVideoDark"] {
        if let Some(origin) = explicitly_configured_https_origin(settings, key)
            && !media.contains(&origin)
        {
            media.push(origin);
        }
    }
    format!(
        "default-src 'self'; connect-src 'self'{}; img-src 'self' data: blob:{}; media-src 'self' blob:{}; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; font-src 'self' data:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'",
        csp_sources(&connect),
        csp_sources(&images),
        csp_sources(&media),
    )
}

fn csp_sources(origins: &[String]) -> String {
    if origins.is_empty() {
        String::new()
    } else {
        format!(" {}", origins.join(" "))
    }
}

pub(crate) async fn static_asset(State(state): State<AppState>, uri: Uri) -> Response {
    let requested_path = uri.path().trim_start_matches('/');
    if !requested_path.starts_with("admin")
        && !requested_path.starts_with("login")
        && !requested_path.starts_with("api/")
    {
        let Some(_permit) = THEME_ASSET_PERMITS.acquire().await.ok() else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        let store = state.themes.clone();
        let request = requested_path.to_owned();
        if let Ok(Some((served_path, bytes, settings, active_theme))) =
            tokio::task::spawn_blocking(move || {
                store.read_asset(&request).map(|(served_path, bytes)| {
                    (served_path, bytes, store.settings(), store.active())
                })
            })
            .await
        {
            let mime = mime_guess::from_path(&served_path).first_or_octet_stream();
            let cache = if served_path == "index.html" {
                "no-cache"
            } else {
                "public, max-age=300"
            };
            return Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref())
                .header(header::CACHE_CONTROL, cache)
                .header("x-content-type-options", "nosniff")
                .header("referrer-policy", "no-referrer")
                .header("x-frame-options", "DENY")
                .header(
                    "content-security-policy",
                    theme_csp(&settings, &active_theme),
                )
                .body(Body::from(bytes))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
        }
    }
    let asset_path = if requested_path.is_empty() {
        "index.html"
    } else {
        requested_path
    };
    let found_requested_asset = WebAssets::get(asset_path);
    let (served_path, asset) = match found_requested_asset {
        Some(asset) => (asset_path, asset),
        None if requested_path.starts_with("api/")
            || requested_path
                .rsplit('/')
                .next()
                .is_some_and(|segment| segment.contains('.')) =>
        {
            return StatusCode::NOT_FOUND.into_response();
        }
        None => match WebAssets::get("index.html") {
            Some(asset) => ("index.html", asset),
            None => return StatusCode::NOT_FOUND.into_response(),
        },
    };
    let mime = mime_guess::from_path(served_path).first_or_octet_stream();
    let cache_control = if served_path == "index.html" {
        "no-cache"
    } else if served_path == "favicon.ico"
        || served_path.starts_with("assets/flags/")
        || served_path.starts_with("assets/logo/")
    {
        "public, max-age=3600"
    } else {
        "public, max-age=31536000, immutable"
    };

    let rates_origin = if served_path == "index.html" {
        match state.database(crate::storage::Storage::settings).await {
            Ok(settings) if settings.daily_exchange_rates => " https://api.frankfurter.dev",
            _ => "",
        }
    } else {
        ""
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CACHE_CONTROL, cache_control)
        .header("x-content-type-options", "nosniff")
        .header("referrer-policy", "no-referrer")
        .header("x-frame-options", "DENY")
        .header(
            "content-security-policy",
            format!("default-src 'self'; connect-src 'self'{}; img-src 'self' data: blob:; script-src 'self'; style-src 'self' 'unsafe-inline'; font-src 'self' data:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'", rates_origin),
        )
        .body(Body::from(asset.data.into_owned()))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn external_origins_require_explicit_https_settings() {
        let base = theme_csp(&json!({}), "Other");
        assert!(base.contains("connect-src 'self';"));
        assert!(!base.contains("frankfurter"));
        let lumina = theme_csp(&json!({}), "LuminaPlus");
        assert!(lumina.contains("connect-src 'self' https://api.frankfurter.dev;"));
        let configured = theme_csp(
            &json!({
                "costRateApiUrl": "https://api.example.test/rates?base=USD",
                "backgroundImage": "https://images.example.test/a.jpg | https://dark.example.test/b.jpg",
                "backgroundVideo": "https://media.example.test/a.mp4",
                "backgroundVideoDark": "http://insecure.example.test/a.mp4"
            }),
            "LuminaPlus",
        );
        assert!(configured.contains("connect-src 'self' https://api.example.test;"));
        assert!(configured.contains(
            "img-src 'self' data: blob: https://images.example.test https://dark.example.test;"
        ));
        assert!(configured.contains("media-src 'self' blob: https://media.example.test;"));
        assert!(!configured.contains("insecure.example.test"));
    }
}
