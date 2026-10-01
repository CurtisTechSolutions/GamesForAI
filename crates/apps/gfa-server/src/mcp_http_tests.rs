use super::{tests::{config,fixture},*};
use axum::{body::{to_bytes,Body},extract::ConnectInfo,http::{header,Request,StatusCode}};
use serde_json::{json,Value};
use tower::ServiceExt;

async fn request(router:&axum::Router,body:Value,host:&str,origin:Option<&str>,peer:Option<SocketAddr>) -> Result<(StatusCode,axum::http::HeaderMap,Value),ServerError> {
    let mut request=Request::builder().method("POST").uri("/mcp").header(header::HOST,host)
        .header(header::CONTENT_TYPE,"application/json").header(header::ACCEPT,"application/json, text/event-stream")
        .header("MCP-Protocol-Version","2025-11-25");
    if let Some(origin)=origin { request=request.header(header::ORIGIN,origin); }
    let mut request=request.body(Body::from(body.to_string()))?;
    if let Some(peer)=peer {request.extensions_mut().insert(ConnectInfo(peer));}
    let response=router.clone().oneshot(request).await?;
    let status=response.status();
    let headers=response.headers().clone();
    let bytes=to_bytes(response.into_body(),1024*1024).await?;
    let body=serde_json::from_slice(&bytes).unwrap_or_else(|_|json!({"text":String::from_utf8_lossy(&bytes)}));
    Ok((status,headers,body))
}
fn rpc(name:&str,args:Value) -> Value { json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}}) }
fn peer() -> Option<SocketAddr> {Some(SocketAddr::from(([127,0,0,1],50000)))}

#[tokio::test]
async fn streamable_http_runs_games_and_enforces_local_boundary() -> Result<(),ServerError> {
    let directory=tempfile::tempdir()?;
    let app=fixture(&config(&directory)).await?;
    let init=json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}});
    let (status,headers,hello)=request(&app.router,init,"127.0.0.1:8080",None,peer()).await?;
    assert_eq!(status,StatusCode::OK,"{hello}");
    assert_eq!(hello["result"]["protocolVersion"],"2025-11-25");
    assert!(!headers.contains_key("Mcp-Session-Id"));
    assert_eq!(headers[header::CACHE_CONTROL],"no-store");
    let (status,_,catalog)=request(&app.router,rpc("list_games",json!({})),"localhost:8080",Some("http://localhost:8080"),peer()).await?;
    assert_eq!(status,StatusCode::OK,"{catalog}");
    assert_eq!(catalog["result"]["structuredContent"]["data"]["games"].as_array().map(Vec::len),Some(4));
    let (status,_,created)=request(&app.router,rpc("create_match",json!({"game_id":"tictactoe","opponent":"random","seed":7})),"127.0.0.1:8080",None,peer()).await?;
    assert_eq!(status,StatusCode::OK,"{created}");
    let created=&created["result"]["structuredContent"]["data"];
    let id=created["state"]["match_id"].as_str().ok_or("id")?;
    let action=created["state"]["legal_actions"][0].clone();
    let (_,_,moved)=request(&app.router,rpc("make_move",json!({"match_id":id,"action":action})),"127.0.0.1:8080",None,peer()).await?;
    assert_eq!(moved["result"]["structuredContent"]["data"]["state"]["turn"],2,"{moved}");
    let (_,_,illegal)=request(&app.router,rpc("make_move",json!({"match_id":id,"action":"invalid"})),"127.0.0.1:8080",None,peer()).await?;
    assert_eq!(illegal["result"]["isError"],true);
    for (host,origin,remote) in [
        ("evil.test:8080",None,peer()),
        ("127.0.0.1:8080",Some("https://evil.test"),peer()),
        ("127.0.0.1:8080",Some("null"),peer()),
        ("127.0.0.1:8080",None,None),
        ("127.0.0.1:8080",None,Some(SocketAddr::from(([203,0,113,10],1)))),
    ] {
        assert_eq!(request(&app.router,rpc("list_games",json!({})),host,origin,remote).await?.0,StatusCode::FORBIDDEN);
    }
    let oversized=rpc("list_games",json!({"padding":"x".repeat(70_000)}));
    assert_eq!(request(&app.router,oversized,"127.0.0.1:8080",None,peer()).await?.0,StatusCode::PAYLOAD_TOO_LARGE);
    app.mcp.stop();
    app.store.close().await;
    Ok(())
}
#[tokio::test]
async fn streamable_http_viewer_is_configured_by_launcher() -> Result<(),ServerError> {
    let directory=tempfile::tempdir()?;
    let mut cfg=config(&directory);
    cfg.mcp_seat=Some(1);
    let app=fixture(&cfg).await?;
    let (_,_,created)=request(&app.router,rpc("create_match",json!({"game_id":"chess","opponent":"random","play_as":1,"seed":7})),"127.0.0.1:8080",None,peer()).await?;
    assert_eq!(created["result"]["structuredContent"]["data"]["state"]["turn"],1,"{created}");
    app.mcp.stop();
    app.store.close().await;
    cfg.mcp_seat=None;
    let spectator=fixture(&cfg).await?;
    let (_,_,result)=request(&spectator.router,rpc("create_match",json!({"game_id":"chess"})),"127.0.0.1:8080",None,peer()).await?;
    assert_eq!(result["result"]["structuredContent"]["error"]["code"],"FORBIDDEN","{result}");
    spectator.mcp.stop();
    spectator.store.close().await;
    Ok(())
}
