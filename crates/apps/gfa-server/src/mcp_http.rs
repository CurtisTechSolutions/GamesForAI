use super::ServerError;
use gfa_core::Viewer;
use gfa_mcp::McpServer;
use gfa_service::GameService;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use std::{net::SocketAddr, sync::Arc};

/// Host-owned transport lifetime; shutdown cancels all SDK streams before draining HTTP.
#[derive(Clone)]
pub(super) struct McpHttp(StreamableHttpServerConfig);
impl McpHttp {
    pub(super) fn stop(&self) {
        self.0.cancellation_token.cancel();
    }
}
pub(super) fn router(
    service: Arc<GameService>,
    address: SocketAddr,
    seat: Option<u8>,
) -> Result<(axum::Router, McpHttp), ServerError> {
    let adapter = McpServer::new(service, seat.map_or(Viewer::Spectator, Viewer::Player))?;
    // Stateless operation supports current and legacy MCP clients without keeping
    // an unbounded session table. Older clients may initialize before each request.
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_max_request_body_bytes(gfa_http::MAX_BODY_BYTES)
        .with_allowed_hosts([address.to_string(), format!("localhost:{}", address.port())])
        .with_allowed_origins([
            format!("http://{address}"),
            format!("http://localhost:{}", address.port()),
        ]);
    let transport = StreamableHttpService::new(
        move || Ok(adapter.clone()),
        Arc::new(LocalSessionManager::default()),
        config.clone(),
    );
    let router = axum::Router::new().route_service("/mcp", transport);
    Ok((
        gfa_http::protect_local_routes(router, address)?,
        McpHttp(config),
    ))
}
