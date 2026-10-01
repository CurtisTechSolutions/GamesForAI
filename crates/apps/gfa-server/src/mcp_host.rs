use super::{application, runner, Config, ServerError};
use gfa_core::Viewer;
use gfa_mcp::McpServer;
use rmcp::{service::QuitReason, ServiceExt};
use std::{future::Future, net::{Ipv4Addr, SocketAddr}};

/// Serve a trusted local MCP client over stdin/stdout, without opening a TCP listener.
/// The selected seat is authorized by the process launcher; None is read-only spectator.
/// All diagnostics must go to stderr because stdout contains only MCP messages.
pub async fn serve_stdio(
    config: Config,
    seat: Option<u8>,
    shutdown: impl Future<Output = ()> + Send,
) -> Result<(), ServerError> {
    let app = application(&config, SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await?;
    let adapter = McpServer::new(app.service.clone(), seat.map_or(Viewer::Spectator, Viewer::Player))?;
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let runner = tokio::spawn(runner::run(app.service.clone(), stopped));
    tokio::pin!(shutdown);
    let result = async {
        let connection = tokio::select! {
            result = adapter.serve(rmcp::transport::stdio()) => result?,
            _ = &mut shutdown => return Ok::<(),ServerError>(()),
        };
        let cancel = connection.cancellation_token();
        let waiting = connection.waiting();
        tokio::pin!(waiting);
        let reason = tokio::select! {
            result = &mut waiting => result?,
            _ = &mut shutdown => {
                cancel.cancel();
                waiting.await?
            },
        };
        if let QuitReason::JoinError(error) = reason {
            return Err(error.into());
        }
        Ok(())
    }.await;
    let _ = stop.send(true);
    let runner_result = runner.await;
    app.updates.close();
    app.updates.wait_closed().await;
    app.store.close().await;
    runner_result?;
    result
}
