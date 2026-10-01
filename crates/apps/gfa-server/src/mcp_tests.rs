use super::{
    tests::{config, fixture},
    *,
};
use gfa_core::Viewer;
use gfa_mcp::McpServer;
use rmcp::{model::CallToolRequestParams, ServiceExt};
use serde_json::json;

#[tokio::test]
async fn mcp_discovery_output_schemas_match_success_and_error_results() -> Result<(), ServerError> {
    let directory = tempfile::tempdir()?;
    let app = fixture(&config(&directory)).await?;
    let adapter = McpServer::new(app.service.clone(), Viewer::Player(0))?;
    let tools = adapter.tool_definitions();
    assert_eq!(tools.len(), 10);
    for tool in tools.iter().filter(|tool| {
        matches!(
            tool.name.as_ref(),
            "list_games" | "list_opponents" | "get_game_info"
        )
    }) {
        assert!(tool
            .description
            .as_ref()
            .is_some_and(|text| text.contains("Example:")));
        let schema = serde_json::to_value(tool.output_schema.as_ref().ok_or("output schema")?)?;
        let validator = jsonschema::validator_for(&schema)?;
        let arguments = if tool.name == "list_games" {
            json!({})
        } else {
            json!({"game_id":"chess"})
        };
        let result = adapter.invoke(&tool.name, arguments).await?;
        assert_eq!(result.is_error, Some(false));
        validator
            .validate(
                result
                    .structured_content
                    .as_ref()
                    .ok_or("structured output")?,
            )
            .map_err(|error| error.to_string())?;
        assert!(!result.content.is_empty());
        let error = adapter.invoke(&tool.name, json!({"unknown":true})).await?;
        assert_eq!(error.is_error, Some(true));
        validator
            .validate(
                error
                    .structured_content
                    .as_ref()
                    .ok_or("structured error")?,
            )
            .map_err(|error| error.to_string())?;
        assert!(!error.content.is_empty());
    }
    let unknown = adapter
        .invoke("get_game_info", json!({"game_id":"missing"}))
        .await?;
    assert_eq!(unknown.is_error, Some(true));
    assert_eq!(
        unknown.structured_content.ok_or("error")?["error"]["code"],
        "UNKNOWN_GAME"
    );
    assert!(adapter.invoke("missing_tool", json!({})).await.is_err());
    assert!(McpServer::new(app.service.clone(), Viewer::Omniscient).is_err());
    let oversized = adapter
        .invoke("get_game_info", json!({"game_id":"x".repeat(70_000)}))
        .await?;
    assert_eq!(oversized.is_error, Some(true));
    app.store.close().await;
    Ok(())
}

#[tokio::test]
async fn mcp_official_client_negotiates_and_reads_the_real_catalog() -> Result<(), ServerError> {
    let directory = tempfile::tempdir()?;
    let app = fixture(&config(&directory)).await?;
    let adapter = McpServer::new(app.service.clone(), Viewer::Spectator)?;
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let server = tokio::spawn(async move { adapter.serve(server_io).await });
    let client =
        tokio::time::timeout(std::time::Duration::from_secs(10), ().serve(client_io)).await??;
    let server = server.await??;
    let tools = client.list_all_tools().await?;
    assert_eq!(tools.len(), 10);
    let result = client
        .call_tool(CallToolRequestParams::new("list_games"))
        .await?;
    let data = result.structured_content.ok_or("structured catalog")?;
    let ids = data["data"]["games"]
        .as_array()
        .ok_or("games")?
        .iter()
        .map(|game| game["id"].as_str().unwrap_or(""))
        .collect::<Vec<_>>();
    assert_eq!(ids, ["chess", "connect4", "sudoku", "tictactoe"]);
    let error = client
        .call_tool(
            CallToolRequestParams::new("get_game_info").with_arguments(
                json!({"game_id":"missing"})
                    .as_object()
                    .ok_or("args")?
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(error.is_error, Some(true));
    client.cancel().await?;
    server.waiting().await?;
    app.store.close().await;
    Ok(())
}

#[tokio::test]
async fn mcp_match_briefing_matches_the_authorized_rest_service_view() -> Result<(), ServerError> {
    let directory = tempfile::tempdir()?;
    let app = fixture(&config(&directory)).await?;
    let created = app
        .service
        .create_match(
            serde_json::from_value(json!({"game_id":"chess","seed":7}))?,
            Viewer::Player(0),
        )
        .await?;
    let adapter = McpServer::new(app.service.clone(), Viewer::Player(0))?;
    let output = adapter
        .invoke(
            "get_game_info",
            json!({"game_id":"chess","match_id":created.match_id}),
        )
        .await?;
    let compact = output.structured_content.ok_or("briefing")?["data"].clone();
    let section = compact["sections"].as_array().ok_or("sections")?.iter()
        .find(|section|section["id"]=="match").ok_or("match section")?;
    assert_eq!(section["data"]["state"]["board"],created.observation.text);
    assert_eq!(section["data"]["state"]["you"],0);
    assert!(section["data"]["state"].get("action_mask").is_none());
    assert!(section["data"]["state"].get("observation").is_none());
    assert!(compact["approx_tokens"].as_u64().ok_or("tokens")? < 3000);
    let full = adapter.invoke("get_game_info",json!({"game_id":"chess","match_id":created.match_id,"detail":"full"})).await?;
    let expected = app.service.get_match_info(&created.match_id,Viewer::Player(0),gfa_api_types::InfoDetail::Full).await?;
    assert_eq!(full.structured_content.ok_or("full briefing")?["data"],serde_json::to_value(expected)?);
    let resource = serde_json::to_value(adapter.resource(&format!("gfa://matches/{}/info",created.match_id)).await?)?;
    let text = resource["contents"][0]["text"].as_str().ok_or("resource text")?;
    assert!(text.len() < 12_000,"{} bytes",text.len());
    assert!(!text.contains("\"action_mask\":["));
    let wrong = adapter
        .invoke(
            "get_game_info",
            json!({"game_id":"sudoku","match_id":created.match_id}),
        )
        .await?;
    assert_eq!(wrong.is_error, Some(true));
    let config_override = adapter
        .invoke(
            "get_game_info",
            json!({"game_id":"chess","match_id":created.match_id,"config":{}}),
        )
        .await?;
    assert_eq!(config_override.is_error, Some(true));
    app.store.close().await;
    Ok(())
}
