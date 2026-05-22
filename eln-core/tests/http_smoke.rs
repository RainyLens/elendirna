//! Streamable HTTP transport smoke test ([[N0033]] r0006 Step 4.6 / codex review 1).
//!
//! `build_app`으로 axum Router를 in-process로 짓고 `tower::ServiceExt::oneshot`으로
//! 호출. `LocalSessionManager`(Arc 내부 상태)는 Router clone 사이에서 공유되므로
//! multi-call session 유지를 verify할 수 있다.
//!
//! 검증 시나리오:
//! 1. POST /mcp `initialize` → 200 + `Mcp-Session-Id` header
//! 2. POST /mcp `notifications/initialized` (no response)
//! 3. POST /mcp `tools/list` (with session id) → entry_list, revision_add 등 tool 목록
//! 4. POST /mcp `tools/call entry_list` (READ) → 200 result
//! 5. POST /mcp `tools/call revision_add` (WRITE) → `PermissionDenied` (-32001 + `data.kind=permission_denied`)
//! 6. POST /mcp `tools/call session_start` (HTTP path) → `session_id`가 transport id와 일치
//! 7. GET /api/health → 200 "ok"

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode};
use eln_core::cli::init::{InitArgs, run as init_run};
use eln_core::mcp_server::http::build_app;
use eln_core::vault::{VaultOrigin, VaultResolution};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

fn setup_vault() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    init_run(InitArgs {
        path: dir.path().to_path_buf(),
        dry_run: false,
        name: Some("http-smoke".to_string()),
        global: false,
    })
    .unwrap();
    dir
}

/// SSE 또는 JSON 응답에서 첫 JSON-RPC payload를 추출.
///
/// stateful_mode=true에서 응답은 보통 `text/event-stream`. 단일 응답이라도
/// `event: message\ndata: {json}\n\n` 형식이 일반적. plain `application/json`도 허용.
fn parse_rpc_payload(body: &str, content_type: &str) -> Value {
    if content_type.contains("application/json") {
        return serde_json::from_str(body).expect("invalid JSON body");
    }
    // SSE: `data: ` 줄들을 모아서 JSON 파싱
    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("data: ") {
            if let Ok(v) = serde_json::from_str::<Value>(rest) {
                return v;
            }
        }
    }
    panic!("no JSON payload found in SSE body: {body:?}");
}

async fn post_mcp(
    app: &axum::Router,
    rpc: Value,
    session_id: Option<&str>,
) -> (StatusCode, Option<String>, String, String) {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri("/mcp")
        // rmcp Streamable HTTP는 default allowed_hosts에 localhost/127.0.0.1이 들어 있어
        // Host header 필수 (DNS rebinding 방어). in-process test도 명시 필요.
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream");
    if let Some(sid) = session_id {
        builder = builder.header("mcp-session-id", sid);
    }
    let req = builder
        .body(Body::from(rpc.to_string()))
        .expect("build request");
    let response = app
        .clone()
        .oneshot(req)
        .await
        .expect("router oneshot succeeds");
    let status = response.status();
    let returned_session = response
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let body = String::from_utf8(bytes.to_vec()).expect("utf8 body");
    (status, returned_session, content_type, body)
}

#[tokio::test]
async fn http_smoke_initialize_session_tools_and_readonly_guard() {
    let dir = setup_vault();
    let resolution = VaultResolution {
        path: dir.path().to_path_buf(),
        origin: VaultOrigin::ExplicitPath,
    };
    let app = build_app(resolution);

    // 1) initialize → 200 + Mcp-Session-Id header
    let initialize = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": { "name": "http-smoke", "version": "0.0.0" }
        }
    });
    let (status, sid, ct, body) = post_mcp(&app, initialize, None).await;
    assert_eq!(status, StatusCode::OK, "initialize status (body={body})");
    let sid = sid.expect("server must emit Mcp-Session-Id on initialize");
    assert!(!sid.is_empty(), "session id non-empty");
    let init_payload = parse_rpc_payload(&body, &ct);
    assert_eq!(init_payload["jsonrpc"], "2.0");
    assert_eq!(init_payload["id"], 1);
    assert!(init_payload["result"].is_object(), "initialize result");

    // 2) initialized notification — session 활성화 (응답 본문 없음)
    let initialized = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    let (status, _, _, _) = post_mcp(&app, initialized, Some(&sid)).await;
    assert!(
        status.is_success() || status == StatusCode::ACCEPTED,
        "initialized notification accepted, got {status}"
    );

    // 3) tools/list — multi-call session 유지 확인 (같은 sid로 후속 요청 성공)
    let tools_list = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });
    let (status, _, ct, body) = post_mcp(&app, tools_list, Some(&sid)).await;
    assert_eq!(status, StatusCode::OK);
    let payload = parse_rpc_payload(&body, &ct);
    let tools = payload["result"]["tools"]
        .as_array()
        .expect("tools array");
    let tool_names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(tool_names.contains(&"entry_list"), "entry_list registered");
    assert!(tool_names.contains(&"revision_add"), "revision_add registered");
    assert!(tool_names.contains(&"session_start"), "session_start registered");

    // 4) tools/call entry_list — READ는 통과
    let entry_list = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": { "name": "entry_list", "arguments": {} }
    });
    let (status, _, ct, body) = post_mcp(&app, entry_list, Some(&sid)).await;
    assert_eq!(status, StatusCode::OK);
    let payload = parse_rpc_payload(&body, &ct);
    assert!(payload["error"].is_null(), "entry_list error={:?}", payload["error"]);
    assert!(payload["result"].is_object(), "entry_list result");

    // 5) tools/call revision_add — WRITE는 PermissionDenied(-32001) 거절 (S2 READ-only 가드)
    let revision_add = json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {
            "name": "revision_add",
            "arguments": { "id": "N0001", "delta": "blocked" }
        }
    });
    let (status, _, ct, body) = post_mcp(&app, revision_add, Some(&sid)).await;
    assert_eq!(status, StatusCode::OK, "JSON-RPC error → HTTP 200");
    let payload = parse_rpc_payload(&body, &ct);
    // ErrorData가 tool error로 반환되는 두 경로 모두 허용:
    // (a) JSON-RPC error field 직접 (rmcp가 ToolError를 envelope에 직접 던지는 경우)
    // (b) tools/call result.isError = true + content[*].text = error description (MCP spec)
    let direct_err = &payload["error"];
    let is_error_path = payload["result"]["isError"].as_bool().unwrap_or(false);
    let is_error_content = payload["result"]["content"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|c| c["text"].as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    let saw_permission_denied = (direct_err.is_object()
        && (direct_err["code"].as_i64() == Some(-32001)
            || direct_err["data"]["kind"].as_str() == Some("permission_denied")))
        || (is_error_path && is_error_content.to_lowercase().contains("permission"))
        || is_error_content.to_lowercase().contains("permission");
    assert!(
        saw_permission_denied,
        "revision_add은 HTTP READ-only 가드로 PermissionDenied 떨어져야 함. payload={payload}"
    );

    // 6) tools/call session_start — HTTP path: transport 발급 session_id를 echo
    let session_start = json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "tools/call",
        "params": { "name": "session_start", "arguments": {} }
    });
    let (status, _, ct, body) = post_mcp(&app, session_start, Some(&sid)).await;
    assert_eq!(status, StatusCode::OK);
    let payload = parse_rpc_payload(&body, &ct);
    // tools/call의 result는 보통 `{ content: [{type:"text", text: "<json>"}] }`.
    // structuredContent가 있으면 그쪽 우선.
    let structured = &payload["result"]["structuredContent"];
    let echoed_sid = if let Some(s) = structured["session_id"].as_str() {
        s.to_string()
    } else if let Some(text) = payload["result"]["content"][0]["text"].as_str() {
        let parsed: Value = serde_json::from_str(text).expect("session_start content json");
        parsed["session_id"]
            .as_str()
            .expect("session_id in result")
            .to_string()
    } else {
        panic!("no session_id in session_start response: {payload}");
    };
    assert_eq!(
        echoed_sid, sid,
        "HTTP session_start은 transport Mcp-Session-Id를 echo해야 함 (이중 권위 회피)"
    );

    // 7) GET /api/health → 200 "ok"
    let health = Request::builder()
        .method(Method::GET)
        .uri("/api/health")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(health).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
    assert_eq!(&bytes[..], b"ok");
}
