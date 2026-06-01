//! [[N0115]] P2#4 — session_start의 vault confine 게이트 e2e.
//!
//! `call_session_start`은 `dispatch_tool`을 우회해 target vault의 entry_count/recent_sessions를
//! **직접** 반환하므로 독립 confine 게이트를 가진다. 그 게이트가 실제 HTTP 요청 경로에서
//! 발화하는지(선언 밖 vault 거부 + 데이터 누출 차단 + scope_denied audit) 검증한다.
//!
//! `vault='global'` 해석은 `home_vault_root()` → `USERPROFILE`/`HOME` 의존이라, launch와 다른
//! 초기화 vault로 풀려면 env swap이 필요하다. http_smoke 바이너리는 매 tool 호출이
//! `is_home_vault_root`로 USERPROFILE을 읽어 env swap이 race를 일으키므로(그 파일 주석 참조),
//! 이 테스트는 **전용 파일 = 전용 프로세스**로 격리한다 — 프로세스별 env는 독립이라 동시 독자가
//! 없어 안전하다.

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode};
use eln_core::cli::init::{InitArgs, run as init_run};
use eln_core::mcp_server::http::build_app;
use eln_core::vault::{VaultOrigin, VaultResolution};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

fn init_vault(name: &str) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    init_run(InitArgs {
        path: dir.path().to_path_buf(),
        dry_run: false,
        name: Some(name.to_string()),
        global: false,
    })
    .unwrap();
    dir
}

/// SSE/JSON 응답에서 첫 JSON-RPC payload 추출.
fn parse_payload(body: &str, content_type: &str) -> Value {
    if content_type.contains("application/json") {
        return serde_json::from_str(body).expect("json body");
    }
    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("data: ")
            && let Ok(v) = serde_json::from_str::<Value>(rest)
        {
            return v;
        }
    }
    panic!("no JSON payload in SSE: {body:?}");
}

async fn post(
    app: &axum::Router,
    rpc: Value,
    sid: Option<&str>,
) -> (StatusCode, Option<String>, String, String) {
    let mut b = Request::builder()
        .method(Method::POST)
        .uri("/mcp")
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream");
    if let Some(s) = sid {
        b = b.header("mcp-session-id", s);
    }
    let resp = app
        .clone()
        .oneshot(b.body(Body::from(rpc.to_string())).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let ret_sid = resp
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    (status, ret_sid, ct, String::from_utf8(bytes.to_vec()).unwrap())
}

#[tokio::test]
async fn session_start_cross_vault_is_denied_and_audited() {
    // launch vault(A)와, 'global'이 가리킬 별개 초기화 vault(home/H)를 만든다.
    let launch = init_vault("launch");
    let home = init_vault("home-global");

    // env 격리: 이 프로세스에만 적용. 전용 파일이라 동시 USERPROFILE 독자 없음.
    let prev_userprofile = std::env::var("USERPROFILE").ok();
    let prev_home = std::env::var("HOME").ok();
    // SAFETY: 전용 테스트 프로세스 단독 실행 — 동시 env 접근 없음 (파일 doc 참조).
    unsafe {
        std::env::set_var("USERPROFILE", home.path());
        std::env::set_var("HOME", home.path());
    }

    let app = build_app(VaultResolution {
        path: launch.path().to_path_buf(),
        origin: VaultOrigin::ExplicitPath,
    });

    // handshake → Mcp-Session-Id 확보.
    let init = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "protocolVersion": "2025-03-26", "capabilities": {},
                    "clientInfo": { "name": "scope-e2e", "version": "0.0.0" } }
    });
    let (status, sid, _, body) = post(&app, init, None).await;
    assert_eq!(status, StatusCode::OK, "initialize: {body}");
    let sid = sid.expect("Mcp-Session-Id");
    let _ = post(
        &app,
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        Some(&sid),
    )
    .await;

    // session_start(vault='global') → 'global'은 launch(A) 밖 → confine 거부.
    let ss = json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": { "name": "session_start", "arguments": { "vault": "global" } }
    });
    let (status, _, ct, body) = post(&app, ss, Some(&sid)).await;
    assert_eq!(status, StatusCode::OK, "transport status (JSON-RPC envelope): {body}");
    let payload = parse_payload(&body, &ct);

    // 1) -32001 거부.
    assert_eq!(
        payload["error"]["code"], -32001,
        "선언 밖 vault session_start은 -32001 거부: {payload}"
    );
    // 2) 데이터 누출 차단 — entry_count/recent_sessions가 응답에 없어야 함.
    assert!(
        payload["result"].is_null(),
        "거부 시 vault data(result) 미반환: {payload}"
    );
    assert!(
        !body.contains("entry_count") && !body.contains("recent_sessions"),
        "거부 응답에 vault data 누출 없음: {body}"
    );

    // 3) scope_denied audit이 launch(A)에 기록.
    let audit =
        std::fs::read_to_string(launch.path().join(".elendirna").join("audit.jsonl")).unwrap_or_default();
    assert!(
        audit.lines().any(|l| serde_json::from_str::<Value>(l)
            .map(|v| v["outcome"] == "scope_denied" && v["reason"] == "cross_vault")
            .unwrap_or(false)),
        "scope_denied audit 라인: {audit}"
    );

    // env 복원.
    // SAFETY: 위와 동일 — 단독 프로세스.
    unsafe {
        match prev_userprofile {
            Some(v) => std::env::set_var("USERPROFILE", v),
            None => std::env::remove_var("USERPROFILE"),
        }
        match prev_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }
}
