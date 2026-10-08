//! Embedded UI and safe single-page application fallback.
use axum::{
    extract::Request,
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;
#[derive(RustEmbed)]
#[folder = "../../web/dist"]
#[allow_missing = true]
struct Assets;
/// Serve embedded assets, or the SPA shell for non-API GET paths.
pub async fn serve(req: Request) -> Response {
    if req.uri().path().starts_with("/api") {
        return crate::error::ApiError::not_found().into_response();
    }
    if req.method() != axum::http::Method::GET && req.method() != axum::http::Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let path = req.uri().path().trim_start_matches('/');
    if path.split('/').any(|p| p == "..") {
        return crate::error::ApiError::not_found().into_response();
    }
    let direct = Assets::get(path);
    let is_asset = direct.is_some();
    let asset = direct.or_else(|| Assets::get("index.html"));
    match asset {
        Some(asset)=>{
            let mime=if is_asset {mime_guess::from_path(path).first_or_octet_stream()} else {mime_guess::mime::TEXT_HTML};
            let mut response=(StatusCode::OK,asset.data.into_owned()).into_response();
            response.headers_mut().insert("content-type",HeaderValue::from_str(mime.as_ref()).unwrap_or(HeaderValue::from_static("application/octet-stream")));
            let cache=if is_asset && path.starts_with("assets/") && path.contains('-') {"public, max-age=31536000, immutable"} else {"no-cache"};
            response.headers_mut().insert("cache-control",HeaderValue::from_static(cache));response
        }
        None=>axum::response::Html("<!doctype html><html><head><title>visp-db-access</title></head><body><h1>UI not built</h1><p>Build web/ and rebuild the server to embed the console.</p></body></html>").into_response(),
    }
}
