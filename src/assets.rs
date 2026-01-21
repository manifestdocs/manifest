//! Embedded web assets for serving the SvelteKit SPA.

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../manifest-web/build"]
struct Assets;

/// Serve embedded static assets with SPA fallback.
///
/// Tries to serve the exact file requested, falling back to index.html
/// for client-side routing.
pub async fn static_handler(req: Request<Body>) -> impl IntoResponse {
    let path = req.uri().path().trim_start_matches('/');

    // Redirect directories without trailing slash to include it
    // This ensures relative paths in HTML resolve correctly
    if !path.is_empty() && !path.ends_with('/') && !path.contains('.') {
        let index_path = format!("{}/index.html", path);
        if Assets::get(&index_path).is_some() {
            let redirect_uri = format!("/{}/", path);
            return Redirect::permanent(&redirect_uri).into_response();
        }
    }

    serve_asset(path).into_response()
}

fn serve_asset(path: &str) -> Response {
    // Try exact file match first
    if let Some(content) = Assets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return serve_content(content.data.into_owned(), mime.as_ref());
    }

    // Try directory index (e.g., /docs -> /docs/index.html)
    let index_path = if path.is_empty() {
        "index.html".to_string()
    } else {
        format!("{}/index.html", path.trim_end_matches('/'))
    };
    if let Some(content) = Assets::get(&index_path) {
        return serve_content(content.data.into_owned(), "text/html");
    }

    // SPA fallback: serve root index.html for client-side routing
    if let Some(content) = Assets::get("index.html") {
        return serve_content(content.data.into_owned(), "text/html");
    }

    // No assets embedded (build not run yet)
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from(
            "Web assets not found. Run 'pnpm build' in manifest-web first.",
        ))
        .unwrap()
}

fn serve_content(data: Vec<u8>, content_type: &str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(data))
        .unwrap()
}
