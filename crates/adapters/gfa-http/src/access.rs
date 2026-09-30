use crate::error::HttpError;
use axum::{
    extract::{ConnectInfo, Request, State}, http::{header, StatusCode},
    middleware::Next, response::{IntoResponse, Response},
};
use std::net::SocketAddr;

#[derive(Clone)]
pub(crate) struct LocalAccess {
    authorities: Vec<String>,
}

impl LocalAccess {
    pub fn new(address: SocketAddr) -> Self {
        Self { authorities: vec![address.to_string(), format!("localhost:{}", address.port())] }
    }

    fn permits(&self, request: &Request) -> bool {
        let Some(ConnectInfo(peer)) = request.extensions().get::<ConnectInfo<SocketAddr>>() else { return false; };
        if !peer.ip().is_loopback() { return false; }
        let headers = request.headers();
        if headers.get_all(header::HOST).iter().count() != 1 || headers.get_all(header::ORIGIN).iter().count() > 1 {
            return false;
        }
        let Some(host) = headers.get(header::HOST).and_then(|value| value.to_str().ok()) else { return false; };
        if !self.authorities.iter().any(|allowed| host.eq_ignore_ascii_case(allowed)) { return false; }
        if let Some(authority) = request.uri().authority() {
            if !authority.as_str().eq_ignore_ascii_case(host) { return false; }
        }
        if let Some(origin) = headers.get(header::ORIGIN) {
            let Ok(origin) = origin.to_str() else { return false; };
            if !origin.eq_ignore_ascii_case(&format!("http://{host}")) { return false; }
        }
        true
    }
}

pub(crate) async fn guard(State(access): State<LocalAccess>, request: Request, next: Next) -> Response {
    let mut response = if access.permits(&request) {
        next.run(request).await
    } else {
        HttpError::new(StatusCode::FORBIDDEN, "UNAUTHORIZED", "This API accepts local requests only", "Connect directly to the loopback address printed by the server.").into_response()
    };
    response.headers_mut().insert(header::CACHE_CONTROL, header::HeaderValue::from_static("no-store"));
    response.headers_mut().insert(header::X_CONTENT_TYPE_OPTIONS, header::HeaderValue::from_static("nosniff"));
    response
}
