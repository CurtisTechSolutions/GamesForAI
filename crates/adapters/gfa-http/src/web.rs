//! Host-selected browser assets. The app uses hash routes, so unknown API paths
//! must keep their normal JSON errors instead of falling back to index.html.
use axum::{response::Redirect, routing::get, Router};
use std::{io, path::Path};
use tower_http::services::{ServeDir, ServeFile};

/// Serve a trusted Vite production build, or direct API-only visitors to the docs.
///
/// The host must wrap these routes in `protect_local_routes`. Only index.html and
/// assets/ are exposed; the build directory's other files are never served.
/// Files remain on disk and must be kept available for the server's lifetime.
pub fn browser_router(directory: Option<&Path>) -> io::Result<Router> {
    let Some(directory) = directory else {
        return Ok(Router::new().route("/", get(|| async { Redirect::temporary("/docs/") })));
    };
    let directory = directory.canonicalize().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "Browser build unavailable at {}. Run make build-web, then pass --web-dir web/apps/site/dist.",
                directory.display()
            ),
        )
    })?;
    let index = directory.join("index.html");
    let assets = directory.join("assets");
    if !index.is_file() || !assets.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Browser build requires index.html and assets/. Run make build-web before serving it.",
        ));
    }
    Ok(Router::new()
        .route_service("/", ServeFile::new(index.clone()))
        .route_service("/index.html", ServeFile::new(index))
        .nest_service(
            "/assets",
            ServeDir::new(assets).append_index_html_on_directories(false),
        ))
}
