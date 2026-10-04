use super::*;
use axum::{
    body::{to_bytes, Body},
    extract::ConnectInfo,
    http::{header, Request, StatusCode},
};
use tower::ServiceExt;

type TestResult = Result<(), ServerError>;

fn bundle(directory: &tempfile::TempDir) -> Result<PathBuf, ServerError> {
    let root = directory.path().join("browser");
    std::fs::create_dir_all(root.join("assets"))?;
    std::fs::write(
        root.join("index.html"),
        "<!doctype html><title>GamesForAI</title>",
    )?;
    std::fs::write(root.join("assets/app.js"), "document.title = 'GamesForAI';")?;
    std::fs::write(root.join("assets/app.css"), "body { color: green; }")?;
    std::fs::write(root.join(".env"), "DO_NOT_SERVE")?;
    std::fs::write(directory.path().join("private.txt"), "DO_NOT_SERVE")?;
    Ok(root)
}

async fn request(
    router: &axum::Router,
    method: &str,
    path: &str,
    host: &str,
    origin: Option<&str>,
    local_peer: bool,
) -> Result<axum::response::Response, ServerError> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, host);
    if let Some(origin) = origin {
        builder = builder.header(header::ORIGIN, origin);
    }
    let mut request = builder.body(Body::empty())?;
    if local_peer {
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))));
    }
    Ok(router.clone().oneshot(request).await?)
}

#[tokio::test]
async fn browser_build_loads_with_correct_mime_types_and_head_support() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut config = tests::config(&directory);
    config.web_dir = Some(bundle(&directory)?);
    let app = tests::fixture(&config).await?;
    for (path, mime, expected) in [
        ("/", "text/html", "<!doctype html>"),
        ("/index.html", "text/html", "GamesForAI"),
        ("/assets/app.js", "text/javascript", "document.title"),
        ("/assets/app.css", "text/css", "color: green"),
    ] {
        let response = request(&app.router, "GET", path, "127.0.0.1:8080", None, true).await?;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert!(response.headers()[header::CONTENT_TYPE]
            .to_str()?
            .starts_with(mime));
        assert_eq!(
            response.headers()[header::X_CONTENT_TYPE_OPTIONS],
            "nosniff"
        );
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let content = to_bytes(response.into_body(), 4096).await?;
        assert!(std::str::from_utf8(&content)?.contains(expected));
        let head = request(&app.router, "HEAD", path, "127.0.0.1:8080", None, true).await?;
        assert_eq!(head.status(), StatusCode::OK);
        assert!(to_bytes(head.into_body(), 4096).await?.is_empty());
    }
    assert_eq!(
        request(&app.router, "POST", "/", "127.0.0.1:8080", None, true)
            .await?
            .status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    // Browser hosting must not swallow API errors or change registered API routes.
    let (status, health) = tests::call(
        &app.router,
        "GET",
        "/healthz",
        serde_json::Value::Null,
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(health["status"], "ok");
    let (status, missing) = tests::call(
        &app.router,
        "GET",
        "/v1/missing",
        serde_json::Value::Null,
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(missing["error"]["code"], "ROUTE_NOT_FOUND");
    app.store.close().await;
    Ok(())
}

#[tokio::test]
async fn browser_files_keep_local_access_guards_and_do_not_expose_other_paths() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut config = tests::config(&directory);
    config.web_dir = Some(bundle(&directory)?);
    let app = tests::fixture(&config).await?;
    for path in ["/", "/index.html", "/assets/app.js"] {
        for (host, origin, peer) in [
            ("foreign.example:8080", None, true),
            ("127.0.0.1:8080", Some("https://foreign.example"), true),
            ("127.0.0.1:8080", None, false),
        ] {
            assert_eq!(
                request(&app.router, "GET", path, host, origin, peer)
                    .await?
                    .status(),
                StatusCode::FORBIDDEN
            );
        }
    }
    for path in [
        "/.env",
        "/private.txt",
        "/gfa.sqlite",
        "/Cargo.toml",
        "/unknown",
        "/assets/missing.js",
        "/assets/../.env",
        "/assets/%2e%2e/.env",
        "/assets/..%5cprivate.txt",
        "/assets/",
        "/v1/assets/app.js",
    ] {
        let response = request(&app.router, "GET", path, "127.0.0.1:8080", None, true).await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        let body = to_bytes(response.into_body(), 4096).await?;
        let text = std::str::from_utf8(&body)?;
        assert!(!text.contains("DO_NOT_SERVE") && !text.contains("<!doctype html>"));
    }
    app.store.close().await;
    Ok(())
}

#[tokio::test]
async fn api_only_root_points_to_docs_and_missing_build_fails_before_database_creation(
) -> TestResult {
    let directory = tempfile::tempdir()?;
    let config = tests::config(&directory);
    let app = tests::fixture(&config).await?;
    let response = request(&app.router, "GET", "/", "127.0.0.1:8080", None, true).await?;
    assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(response.headers()[header::LOCATION], "/docs/");
    app.store.close().await;

    let mut invalid = Config {
        database: Database::Sqlite(directory.path().join("never-created.sqlite")),
        web_dir: Some(directory.path().join("missing-build")),
        ..Config::default()
    };
    for path in [
        directory.path().join("missing-build"),
        directory.path().to_owned(),
    ] {
        invalid.web_dir = Some(path);
        let error = match tests::fixture(&invalid).await {
            Ok(_) => return Err("missing browser build was accepted".into()),
            Err(error) => error,
        };
        assert!(error.to_string().contains("make build-web"));
        assert!(!directory.path().join("never-created.sqlite").exists());
    }
    Ok(())
}
