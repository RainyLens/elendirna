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
//! 8. (S3.3 / [[N0033]] r0012 M1) `VaultOrigin::FallbackGlobal` injection → handler 응답의
//!    `vault_meta` 직렬화 회귀(`vault_origin="fallback_global"` + `fallback=true`).

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
    let tools = payload["result"]["tools"].as_array().expect("tools array");
    let tool_names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(tool_names.contains(&"entry_list"), "entry_list registered");
    assert!(
        tool_names.contains(&"revision_add"),
        "revision_add registered"
    );
    assert!(
        tool_names.contains(&"session_start"),
        "session_start registered"
    );

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
    assert!(
        payload["error"].is_null(),
        "entry_list error={:?}",
        payload["error"]
    );
    assert!(payload["result"].is_object(), "entry_list result");

    // 5) tools/call revision_add — WRITE는 PermissionDenied(-32001) 거절 (S2 READ-only 가드).
    //    codex review 2 권고: rmcp 1.5.0은 `Err(ErrorData)`를 JSON-RPC `error`로 envelope
    //    하므로 fallback path 허용 없이 직접 assert.
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
    let err = &payload["error"];
    assert!(
        err.is_object(),
        "revision_add은 JSON-RPC error로 거절: {payload}"
    );
    assert_eq!(
        err["code"].as_i64(),
        Some(-32001),
        "ERROR_CODE_PERMISSION_DENIED 정합: {err}"
    );
    assert_eq!(
        err["data"]["kind"].as_str(),
        Some("permission_denied"),
        "ToolError::json_rpc_data().kind 식별자 정합: {err}"
    );

    // 5b) tools/call entry_new — S3.2 어댑터 위임 후, transport caller capability gate가
    //     mcp_server의 `ensure_write_permitted`가 아니라 handler 내부 `ctx.permissions.contains(WRITE)`
    //     로 흡수됐는지 회귀-안전 가드. PermissionDenied 응답 code/data.kind는 동일.
    let entry_new = json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "tools/call",
        "params": {
            "name": "entry_new",
            "arguments": { "title": "blocked-on-http-read-only" }
        }
    });
    let (status, _, ct, body) = post_mcp(&app, entry_new, Some(&sid)).await;
    assert_eq!(status, StatusCode::OK, "JSON-RPC error → HTTP 200");
    let payload = parse_rpc_payload(&body, &ct);
    let err = &payload["error"];
    assert!(
        err.is_object(),
        "entry_new은 JSON-RPC error로 거절: {payload}"
    );
    assert_eq!(
        err["code"].as_i64(),
        Some(-32001),
        "ERROR_CODE_PERMISSION_DENIED 정합 (handler ctx.permissions gate): {err}"
    );
    assert_eq!(
        err["data"]["kind"].as_str(),
        Some("permission_denied"),
        "ToolError::json_rpc_data().kind 식별자 정합: {err}"
    );

    // 6) tools/call session_start — HTTP path: transport 발급 session_id를 echo
    let session_start = json!({
        "jsonrpc": "2.0",
        "id": 6,
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

/// codex review 2 권고: `DELETE /mcp` 후 같은 sid 재사용이 실패해야
/// `LocalSessionManager.close_session` 회귀를 catch할 수 있다.
#[tokio::test]
async fn http_smoke_delete_session_invalidates_sid() {
    let dir = setup_vault();
    let resolution = VaultResolution {
        path: dir.path().to_path_buf(),
        origin: VaultOrigin::ExplicitPath,
    };
    let app = build_app(resolution);

    // initialize → sid 발급
    let initialize = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": { "name": "http-smoke-delete", "version": "0.0.0" }
        }
    });
    let (status, sid, _, _) = post_mcp(&app, initialize, None).await;
    assert_eq!(status, StatusCode::OK);
    let sid = sid.expect("Mcp-Session-Id on initialize");

    let initialized = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    let _ = post_mcp(&app, initialized, Some(&sid)).await;

    // DELETE /mcp with sid → session close
    let delete = Request::builder()
        .method(Method::DELETE)
        .uri("/mcp")
        .header("host", "localhost")
        .header("mcp-session-id", &sid)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(delete).await.unwrap();
    assert!(
        response.status().is_success() || response.status() == StatusCode::NO_CONTENT,
        "DELETE /mcp should succeed, got {}",
        response.status()
    );

    // 같은 sid로 tools/list → 실패해야 함 (LocalSessionManager invalidated sid)
    let tools_list = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });
    let (status, _, _, body) = post_mcp(&app, tools_list, Some(&sid)).await;
    // rmcp 1.5.0: invalid session은 HTTP 404 또는 4xx로 응답. JSON-RPC envelope error는 보통 200 + error
    // 어느 쪽이든 success(2xx)로 정상 처리되어선 안 됨.
    assert!(
        status.is_client_error()
            || (status == StatusCode::OK && body.to_lowercase().contains("session")),
        "stale sid는 거절되어야 함, status={status} body={body}"
    );
}

/// S3.3 — HTTP global fallback resolution smoke ([[N0033]] r0012 M1).
///
/// HTTP transport가 `VaultOrigin::FallbackGlobal`로 들어온 호출에서 응답까지 origin이
/// 정확히 흘러가는지 회귀 catch. `serve.rs:139-141`의 실 fallback resolution은 transport
/// 분기 *전*에 공유되므로 별도 process spawn 없이 in-process injection 한 표면만 검증한다.
/// 회귀 hot zone은 `mcp_server/mod.rs:164` (origin → string) + `mcp_server/mod.rs:171-173`
/// (FallbackGlobal-only `fallback:true` 표식).
///
/// 옵션 (b) process-level (`USERPROFILE`/`HOME` swap + non-vault cwd)는 Windows에서 env가
/// process-global이라 fragile + serve.rs 분기 자체 변경 시점에 별 phase로 분가.
#[tokio::test]
async fn http_smoke_global_fallback_initialize_health_and_entry_list() {
    let dir = setup_vault();
    let resolution = VaultResolution {
        path: dir.path().to_path_buf(),
        origin: VaultOrigin::FallbackGlobal,
    };
    let app = build_app(resolution);

    // 1) initialize → Mcp-Session-Id
    let initialize = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": { "name": "http-smoke-fallback", "version": "0.0.0" }
        }
    });
    let (status, sid, _, body) = post_mcp(&app, initialize, None).await;
    assert_eq!(status, StatusCode::OK, "initialize status (body={body})");
    let sid = sid.expect("Mcp-Session-Id on initialize");
    assert!(!sid.is_empty(), "session id non-empty");

    // 2) initialized notification — protocol 활성화
    let initialized = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    let (status, _, _, _) = post_mcp(&app, initialized, Some(&sid)).await;
    assert!(
        status.is_success() || status == StatusCode::ACCEPTED,
        "initialized accepted, got {status}"
    );

    // 3) GET /api/health → 200 "ok" — 휴먼 BE는 vault origin과 직교
    let health = Request::builder()
        .method(Method::GET)
        .uri("/api/health")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(health).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
    assert_eq!(&bytes[..], b"ok");

    // 4) tools/call entry_list — handler 응답 path에 vault_meta가 FallbackGlobal로 직렬화되는지.
    //    `mcp_server/mod.rs:702 merge_vault_meta`가 handler result에 vault_meta 키를 top-level로
    //    extend → tools/call 응답의 result.structuredContent (또는 result.content[0].text JSON)에
    //    `vault_origin="fallback_global"` + `fallback=true`로 도달해야 함.
    let entry_list = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": { "name": "entry_list", "arguments": {} }
    });
    let (status, _, ct, body) = post_mcp(&app, entry_list, Some(&sid)).await;
    assert_eq!(status, StatusCode::OK);
    let payload = parse_rpc_payload(&body, &ct);
    assert!(
        payload["error"].is_null(),
        "entry_list error={:?}",
        payload["error"]
    );

    // structuredContent 우선, fallback으로 content[0].text의 JSON parse (session_start 추출 패턴과 동일).
    let structured = &payload["result"]["structuredContent"];
    let inner: Value = if structured.is_object() {
        structured.clone()
    } else if let Some(text) = payload["result"]["content"][0]["text"].as_str() {
        serde_json::from_str(text).expect("entry_list content json")
    } else {
        panic!("no entry_list payload extractable: {payload}");
    };

    assert_eq!(
        inner["vault_origin"].as_str(),
        Some("fallback_global"),
        "vault_origin 직렬화 (mcp_server/mod.rs:164) — origin이 응답까지 흘러가야 함: {inner}"
    );
    assert_eq!(
        inner["fallback"].as_bool(),
        Some(true),
        "FallbackGlobal-only `fallback:true` 표식 (mcp_server/mod.rs:171-173): {inner}"
    );
    assert_eq!(
        inner["ok"].as_bool(),
        Some(true),
        "entry_list ok=true (READ 통과): {inner}"
    );
}
