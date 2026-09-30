use super::*;
use axum::{
    body::{to_bytes, Body},
    extract::ConnectInfo,
    http::{header, Request},
};
use gfa_core::GameRegistry;
use gfa_service::{
    AppendResult, Clock, MatchEvent, MatchIds, MatchRecord, MatchStore, StoreError, StoreFuture,
    StoredCommand,
};
use tower::ServiceExt;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct EmptyHost;
impl Clock for EmptyHost {
    fn now_ms(&self) -> u64 {
        0
    }
}
impl MatchIds for EmptyHost {
    fn next_id(&self) -> String {
        "test".into()
    }
    fn next_seed(&self) -> u64 {
        1
    }
}
impl MatchStore for EmptyHost {
    fn create(&self, _: MatchRecord) -> StoreFuture<'_, ()> {
        Box::pin(async { Err(StoreError::Unavailable("unexpected write".into())) })
    }
    fn load<'a>(&'a self, _: &'a str) -> StoreFuture<'a, Option<MatchRecord>> {
        Box::pin(async { Ok(None) })
    }
    fn append<'a>(
        &'a self,
        _: &'a str,
        _: u64,
        _: Vec<MatchEvent>,
        _: Option<StoredCommand>,
    ) -> StoreFuture<'a, AppendResult> {
        Box::pin(async { Err(StoreError::Unavailable("unexpected write".into())) })
    }
}
fn service() -> Arc<GameService> {
    Arc::new(GameService::new(
        GameRegistry::default(),
        Arc::new(EmptyHost),
        Arc::new(EmptyHost),
        Arc::new(EmptyHost),
    ))
}
fn router() -> Result<Router, ApiError> {
    local_router(service(), SocketAddr::from(([127, 0, 0, 1], 8080)))
}
fn request(method: &str, path: &str, body: &str) -> Result<Request<Body>, axum::http::Error> {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "127.0.0.1:8080")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_owned()))?;
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))));
    Ok(request)
}
async fn response(
    request: Request<Body>,
) -> Result<(StatusCode, Value), Box<dyn std::error::Error>> {
    let response = router()?.oneshot(request).await?;
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let status = response.status();
    let body = to_bytes(response.into_body(), MAX_BODY_BYTES * 2).await?;
    Ok((status, serde_json::from_slice(&body)?))
}

#[tokio::test]
async fn health_catalog_and_unknown_resources_return_json() -> TestResult {
    let (status, body) = response(request("GET", "/healthz", "")?).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["mode"], "local");
    assert_eq!(
        response(request("GET", "/v1/games", "")?).await?.1,
        json!([])
    );
    for (path, code) in [
        ("/v1/games/missing", "UNKNOWN_GAME"),
        ("/v1/matches/missing/state", "MATCH_NOT_FOUND"),
        ("/v1/matches/missing/replay", "MATCH_NOT_FOUND"),
        ("/v1/matches/missing/legal-actions", "MATCH_NOT_FOUND"),
        ("/unknown", "ROUTE_NOT_FOUND"),
    ] {
        let (status, body) = response(request("GET", path, "")?).await?;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], code);
    }
    let (status, body) = response(request("DELETE", "/v1/matches", "")?).await?;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(body["error"]["code"], "METHOD_NOT_ALLOWED");
    Ok(())
}

#[tokio::test]
async fn malformed_json_content_type_and_body_limits_share_error_envelope() -> TestResult {
    for (body, expected) in [
        ("{".to_owned(), StatusCode::BAD_REQUEST),
        ("{}".to_owned(), StatusCode::UNPROCESSABLE_ENTITY),
        (
            json!({"game_id":"missing","unknown":true}).to_string(),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "x".repeat(MAX_BODY_BYTES + 1),
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
    ] {
        let (status, body) = response(request("POST", "/v1/matches", &body)?).await?;
        assert_eq!(status, expected);
        assert_eq!(body["error"]["code"], "INVALID_REQUEST");
        assert!(body["error"]["hint"]
            .as_str()
            .is_some_and(|hint| !hint.is_empty()));
    }
    let mut req = request("POST", "/v1/matches", "{}")?;
    req.headers_mut().remove(header::CONTENT_TYPE);
    assert_eq!(response(req).await?.0, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    Ok(())
}

#[tokio::test]
async fn invalid_queries_and_duplicate_retry_headers_are_rejected() -> TestResult {
    for query in [
        "seat=256",
        "seat=-1",
        "seat=abc",
        "seat=0&seat=1",
        "viewer=omniscient",
    ] {
        let (status, body) = response(request(
            "GET",
            &format!("/v1/matches/missing/state?{query}"),
            "",
        )?)
        .await?;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "INVALID_REQUEST");
    }
    let mut req = request(
        "POST",
        "/v1/matches/missing/actions",
        r#"{"seat":0,"turn":0,"action":"r1c1"}"#,
    )?;
    req.headers_mut().append("idempotency-key", "a".parse()?);
    req.headers_mut().append("idempotency-key", "b".parse()?);
    assert_eq!(response(req).await?.0, StatusCode::BAD_REQUEST);
    Ok(())
}

#[tokio::test]
async fn local_mode_refuses_foreign_peers_hosts_and_origins() -> TestResult {
    assert!(local_router(service(), SocketAddr::from(([0, 0, 0, 0], 8080))).is_err());
    for mode in 0..6 {
        let mut req = request("GET", "/healthz", "")?;
        match mode {
            0 => {
                req.extensions_mut().remove::<ConnectInfo<SocketAddr>>();
            }
            1 => {
                req.extensions_mut()
                    .insert(ConnectInfo(SocketAddr::from(([192, 0, 2, 1], 50000))));
            }
            2 => {
                req.headers_mut()
                    .insert(header::HOST, "attacker.example:8080".parse()?);
            }
            3 => {
                req.headers_mut()
                    .insert(header::ORIGIN, "http://attacker.example".parse()?);
            }
            4 => {
                req.headers_mut().insert(header::ORIGIN, "null".parse()?);
            }
            _ => {
                req.headers_mut()
                    .append(header::HOST, "localhost:8080".parse()?);
            }
        }
        let (status, body) = response(req).await?;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "UNAUTHORIZED");
    }
    let mut req = request("GET", "/healthz", "")?;
    req.headers_mut()
        .insert(header::HOST, "localhost:8080".parse()?);
    req.headers_mut()
        .insert(header::ORIGIN, "http://localhost:8080".parse()?);
    assert_eq!(response(req).await?.0, StatusCode::OK);
    Ok(())
}

#[test]
fn domain_errors_preserve_recovery_context_and_http_status() {
    for (code, status) in [
        ("STALE_TURN", StatusCode::CONFLICT),
        ("MATCH_FINISHED", StatusCode::CONFLICT),
        ("IDEMPOTENCY_CONFLICT", StatusCode::CONFLICT),
        ("ILLEGAL_ACTION", StatusCode::UNPROCESSABLE_ENTITY),
        ("STORAGE_UNAVAILABLE", StatusCode::SERVICE_UNAVAILABLE),
        ("INVALID_EVENT_LOG", StatusCode::INTERNAL_SERVER_ERROR),
    ] {
        let mut error = ApiError::new(code, "problem", "retry hint");
        error.details = json!({"turn": 7});
        let mapped = HttpError::from(error);
        assert_eq!(mapped.status, status);
        assert_eq!(mapped.error.details["turn"], 7);
    }
}

#[tokio::test]
async fn interactive_docs_are_bundled_and_share_local_access_guards() -> TestResult {
    let app = router()?;
    let response = app.clone().oneshot(request("GET", "/docs", "")?).await?;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()[header::LOCATION], "/docs/");
    for (path, content_type, content) in [
        ("/docs/", "text/html", "swagger-ui"),
        ("/docs/swagger-ui.css", "text/css", ".swagger-ui"),
        (
            "/docs/swagger-ui-bundle.js",
            "javascript",
            "SwaggerUIBundle",
        ),
        (
            "/docs/swagger-initializer.js",
            "javascript",
            "/v1/openapi.json",
        ),
    ] {
        let response = app.clone().oneshot(request("GET", path, "")?).await?;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert!(response.headers()[header::CONTENT_TYPE]
            .to_str()?
            .contains(content_type));
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let body = to_bytes(response.into_body(), 8 * 1024 * 1024).await?;
        let body = std::str::from_utf8(&body)?;
        assert!(body.contains(content), "{path}");
        if path.ends_with("initializer.js") {
            assert!(body.contains(r#""validatorUrl": "none""#));
            assert!(body.contains(r#""queryConfigEnabled": false"#));
        }
        let mut rejected = request("GET", path, "")?;
        rejected
            .headers_mut()
            .insert(header::ORIGIN, "https://example.org".parse()?);
        assert_eq!(
            app.clone().oneshot(rejected).await?.status(),
            StatusCode::FORBIDDEN
        );
    }
    Ok(())
}
