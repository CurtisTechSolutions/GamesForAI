use super::{
    tests::{config, fixture},
    *,
};
use gfa_core::Viewer;
use gfa_mcp::McpServer;
use rmcp::{
    model::{ServerNotification, SubscriptionFilter},
    ServiceExt,
};
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn mcp_live_resources_notify_on_external_commits_and_cancel() -> Result<(), ServerError> {
    let directory = tempfile::tempdir()?;
    let app = fixture(&config(&directory)).await?;
    let initial = app
        .service
        .create_match(
            serde_json::from_value(json!({"game_id":"tictactoe"}))?,
            Viewer::Player(0),
        )
        .await?;
    let uri = format!("gfa://matches/{}/state", initial.match_id);
    let adapter = McpServer::new(app.service.clone(), Viewer::Spectator)?;
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(async move { adapter.serve(server_io).await });
    let client = rmcp::service::serve_client_with_lifecycle(
        (),
        client_io,
        rmcp::service::ClientLifecycleMode::Discover {
            preferred_versions: vec![rmcp::model::ProtocolVersion::V_2026_07_28],
        },
    )
    .await?;
    let server = task.await??;
    let filter = SubscriptionFilter::builder()
        .resource_subscriptions([uri.clone(), "file:///secret".into()])
        .tools_list_changed()
        .build();
    let mut subscription = client.listen(filter).await?;
    assert_eq!(
        subscription.acknowledged().resource_subscriptions,
        Some(vec![uri.clone()])
    );
    assert_ne!(subscription.acknowledged().tools_list_changed, Some(true));
    let notification = tokio::time::timeout(Duration::from_secs(5), subscription.next())
        .await??
        .ok_or("initial notification")?;
    assert!(
        matches!(notification,ServerNotification::ResourceUpdatedNotification(update) if update.params.uri==uri)
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(250), subscription.next())
            .await
            .is_err()
    );
    // Change the service directly, outside this MCP connection.
    app.service
        .make_move(
            &initial.match_id,
            serde_json::from_value(
                json!({"seat":0,"turn":0,"action":"r1c1","reasoning":"private live reasoning"}),
            )?,
            None,
        )
        .await?;
    let notification = tokio::time::timeout(Duration::from_secs(5), subscription.next())
        .await??
        .ok_or("move notification")?;
    assert!(!serde_json::to_string(&notification)?.contains("private live reasoning"));
    assert!(
        matches!(notification,ServerNotification::ResourceUpdatedNotification(update) if update.params.uri==uri)
    );
    // Draw offers change state without increasing the action turn.
    app.service
        .offer_draw(
            &initial.match_id,
            gfa_api_types::ControlRequest { seat: 0, turn: 1 },
        )
        .await?;
    let notification = tokio::time::timeout(Duration::from_secs(5), subscription.next())
        .await??
        .ok_or("control notification")?;
    assert!(
        matches!(notification,ServerNotification::ResourceUpdatedNotification(update) if update.params.uri==uri)
    );
    subscription.cancel().await?;
    assert!(subscription.next().await?.is_none());
    client.cancel().await?;
    server.waiting().await?;
    app.store.close().await;
    Ok(())
}

#[tokio::test]
async fn mcp_live_resources_bound_uri_filters() -> Result<(), ServerError> {
    let directory = tempfile::tempdir()?;
    let app = fixture(&config(&directory)).await?;
    let adapter = McpServer::new(app.service.clone(), Viewer::Player(0))?;
    use rmcp::ServerHandler;
    let uris = (0..30)
        .map(|index| format!("gfa://matches/m_{index}/state"))
        .collect::<Vec<_>>();
    let accepted = adapter
        .accepted_subscription_filter(
            &SubscriptionFilter::builder()
                .resource_subscriptions(uris)
                .build(),
        )
        .ok_or("filter")?;
    assert_eq!(
        accepted.resource_subscriptions.as_ref().map(Vec::len),
        Some(8)
    );
    for uri in [
        "gfa://matches/../state",
        "gfa://matches/x/state?seat=1",
        "gfa://games/chess/info",
        "gfa://matches//state",
    ] {
        let accepted = adapter
            .accepted_subscription_filter(
                &SubscriptionFilter::builder()
                    .resource_subscriptions([uri])
                    .build(),
            )
            .ok_or("filter")?;
        assert!(accepted
            .resource_subscriptions
            .unwrap_or_default()
            .is_empty());
    }
    app.store.close().await;
    Ok(())
}

struct LegacyClient(tokio::sync::mpsc::Sender<String>);
impl rmcp::ClientHandler for LegacyClient {
    fn get_info(&self) -> rmcp::model::ClientConfig {
        rmcp::model::ClientConfig::default()
            .with_protocol_version(rmcp::model::ProtocolVersion::V_2025_11_25)
    }
    async fn on_resource_updated(
        &self,
        params: rmcp::model::ResourceUpdatedNotificationParam,
        _: rmcp::service::NotificationContext<rmcp::RoleClient>,
    ) {
        let _ = self.0.try_send(params.uri);
    }
}
#[tokio::test]
#[allow(deprecated)]
async fn legacy_watch_unsubscribe_is_scoped_to_the_connection() -> Result<(), ServerError> {
    let directory = tempfile::tempdir()?;
    let app = fixture(&config(&directory)).await?;
    let initial = app
        .service
        .create_match(
            serde_json::from_value(json!({"game_id":"tictactoe"}))?,
            Viewer::Player(0),
        )
        .await?;
    let uri = format!("gfa://matches/{}/state", initial.match_id);
    let shared = McpServer::new(app.service.clone(), Viewer::Spectator)?;
    let mut clients = Vec::new();
    let mut servers = Vec::new();
    let mut receivers = Vec::new();
    for _ in 0..2 {
        let (send, receive) = tokio::sync::mpsc::channel(4);
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let handler = shared.clone();
        let task = tokio::spawn(async move { handler.serve(server_io).await });
        let client = LegacyClient(send).serve(client_io).await?;
        client
            .subscribe(rmcp::model::SubscribeRequestParams::new(&uri))
            .await?;
        // Repeated subscriptions do not consume more capacity.
        client
            .subscribe(rmcp::model::SubscribeRequestParams::new(&uri))
            .await?;
        clients.push(client);
        servers.push(task.await??);
        receivers.push(receive);
    }
    clients[0]
        .unsubscribe(rmcp::model::UnsubscribeRequestParams::new(&uri))
        .await?;
    app.service
        .make_move(
            &initial.match_id,
            serde_json::from_value(json!({"seat":0,"turn":0,"action":"r1c1"}))?,
            None,
        )
        .await?;
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), receivers[1].recv())
            .await?
            .as_deref(),
        Some(uri.as_str())
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(250), receivers[0].recv())
            .await
            .is_err()
    );
    for client in clients {
        client.cancel().await?;
    }
    for server in servers {
        server.waiting().await?;
    }
    drop(shared);
    app.store.close().await;
    Ok(())
}

#[tokio::test]
async fn modern_watch_capacity_is_released_after_cancellation() -> Result<(), ServerError> {
    let directory = tempfile::tempdir()?;
    let app = fixture(&config(&directory)).await?;
    let initial = app
        .service
        .create_match(
            serde_json::from_value(json!({"game_id":"tictactoe"}))?,
            Viewer::Player(0),
        )
        .await?;
    let uri = format!("gfa://matches/{}/state", initial.match_id);
    let adapter = McpServer::new(app.service.clone(), Viewer::Spectator)?;
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(async move { adapter.serve(server_io).await });
    let client = rmcp::service::serve_client_with_lifecycle(
        (),
        client_io,
        rmcp::service::ClientLifecycleMode::Discover {
            preferred_versions: vec![rmcp::model::ProtocolVersion::V_2026_07_28],
        },
    )
    .await?;
    let server = task.await??;
    let filter = SubscriptionFilter::builder()
        .resource_subscriptions([uri])
        .build();
    let mut subscriptions = Vec::new();
    for _ in 0..16 {
        let mut subscription = client.listen(filter.clone()).await?;
        assert!(subscription.next().await?.is_some());
        subscriptions.push(subscription);
    }
    // The SDK acknowledges the accepted filter before invoking our handler.
    // The exhausted handler then closes the stream with a protocol error.
    let mut excess = client.listen(filter.clone()).await?;
    assert!(tokio::time::timeout(Duration::from_secs(2), excess.next())
        .await?
        .is_err());
    subscriptions[0].cancel().await?;
    let mut replacement = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let mut candidate = client.listen(filter.clone()).await?;
            if matches!(candidate.next().await, Ok(Some(_))) {
                return Ok::<_, rmcp::service::ServiceError>(candidate);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await??;
    replacement.cancel().await?;
    for subscription in &mut subscriptions {
        subscription.cancel().await?;
    }
    client.cancel().await?;
    server.waiting().await?;
    app.store.close().await;
    Ok(())
}
