use axum::{
    body::Body,
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
struct WebAssets;

pub(crate) async fn static_asset(uri: Uri) -> Response {
    let requested_path = uri.path().trim_start_matches('/');
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

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CACHE_CONTROL, cache_control)
        .header("x-content-type-options", "nosniff")
        .header("referrer-policy", "no-referrer")
        .header("x-frame-options", "DENY")
        .header(
            "content-security-policy",
            "default-src 'self'; connect-src 'self'; img-src 'self' data: blob:; script-src 'self'; style-src 'self' 'unsafe-inline'; font-src 'self' data:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'",
        )
        .body(Body::from(asset.data.into_owned()))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}
