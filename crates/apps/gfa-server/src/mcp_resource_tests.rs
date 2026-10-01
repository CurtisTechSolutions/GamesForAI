use super::{mcp_play_tests::invoke,tests::{config,fixture},*};
use gfa_core::Viewer;
use gfa_mcp::McpServer;
use rmcp::{model::{GetPromptRequestParams,ReadResourceRequestParams}, ServiceExt};
use serde_json::{json,Value};

#[tokio::test]
async fn mcp_official_client_reads_resources_and_play_prompt() -> Result<(),ServerError> {
    let directory=tempfile::tempdir()?;
    let app=fixture(&config(&directory)).await?;
    let adapter=McpServer::new(app.service.clone(),Viewer::Player(0))?;
    let created=invoke(&adapter,"create_match",json!({"game_id":"chess","seed":44}),false).await?;
    let id=created["state"]["match_id"].as_str().ok_or("id")?;
    let (client_io,server_io)=tokio::io::duplex(128*1024);
    let task=tokio::spawn(async move{adapter.serve(server_io).await});
    let client=().serve(client_io).await?;
    let server=task.await??;
    let catalog=client.list_all_resources().await?;
    assert_eq!(catalog.len(),4);
    assert!(catalog.iter().any(|resource|resource.uri=="gfa://games/chess/info"));
    let templates=client.list_all_resource_templates().await?;
    assert_eq!(templates.len(),4);
    let prompts=client.list_all_prompts().await?;
    assert_eq!(prompts.len(),1);
    assert_eq!(prompts[0].name,"play_game");
    let rules=client.read_resource(ReadResourceRequestParams::new("gfa://games/chess/info")).await?;
    let rules=serde_json::to_value(rules)?;
    assert!(rules["contents"][0]["text"].as_str().ok_or("rules")?.contains("action_format"));
    let prompt=client.get_prompt(GetPromptRequestParams::new("play_game").with_arguments(json!({"game_id":"chess"}).as_object().ok_or("args")?.clone())).await?;
    let prompt=serde_json::to_value(prompt)?;
    let text=prompt["messages"][0]["content"]["text"].as_str().ok_or("prompt")?;
    assert!(text.contains("make_move") && text.contains("e2e4") && text.contains("terminated"));
    for suffix in ["info","state","replay"] {
        let resource=client.read_resource(ReadResourceRequestParams::new(format!("gfa://matches/{id}/{suffix}"))).await?;
        let resource=serde_json::to_value(resource)?;
        let content=resource["contents"][0]["text"].as_str().ok_or("content")?;
        assert!(!content.is_empty());
        if suffix=="state" {
            let state:Value=serde_json::from_str(content)?;
            assert_eq!(state,created["state"]);
        }
    }
    for uri in ["file:///etc/passwd","gfa://matches/../state","gfa://matches/x/state?seat=1","gfa://games/chess/info/extra","gfa://games/missing/info"] {
        assert!(client.read_resource(ReadResourceRequestParams::new(uri)).await.is_err());
    }
    assert!(client.get_prompt(GetPromptRequestParams::new("unknown")).await.is_err());
    assert!(client.get_prompt(GetPromptRequestParams::new("play_game")).await.is_err());
    client.cancel().await?;
    server.waiting().await?;
    let spectator=McpServer::new(app.service.clone(),Viewer::Spectator)?;
    let resource=serde_json::to_value(spectator.resource(&format!("gfa://matches/{id}/state")).await?)?;
    let state:Value=serde_json::from_str(resource["contents"][0]["text"].as_str().ok_or("state")?)?;
    assert_eq!(state["legal_actions"],json!([]));
    assert!(state["you"].is_null());
    app.store.close().await;
    Ok(())
}
