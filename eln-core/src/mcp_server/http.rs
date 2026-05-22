//! Streamable HTTP transport ([[N0033]] r0006 Step 4.2).
//!
//! MCP 2025-03-26 spec의 Streamable HTTP를 `rmcp::transport::streamable_http_server`로 노출.
//! 같은 axum listener에 휴먼용 백엔드(`crate::http_backend::router`)를 `/api` 아래 nest,
//! MCP service를 `/mcp` 아래 nest. vault resolution은 두 wire layer가 공유.
//!
//! S2 한정 가드: `ElfMcpServer::new_http`는 `Permissions::READ`만 부여 — 외부 write 차단.
//! S3 ApiKey auth 도착 후 transport-level grant derivation으로 교체.

use std::net::SocketAddr;
use std::sync::Arc;

use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::{
    StreamableHttpServerConfig, StreamableHttpService,
};

use crate::vault::VaultResolution;

use super::ElfMcpServer;

/// axum Router를 빌드 — `/mcp`에 MCP Streamable HTTP service,
/// `/api`에 휴먼용 백엔드 router (S2에선 `/api/health` 1개).
///
/// `run_http`와 in-process integration test가 공유. test는 본 fn으로 같은 router를
/// 짓고 `tower::ServiceExt::oneshot`으로 호출 — bind/listen 없이 multi-call session
/// 유지를 검증할 수 있다 ([[N0033]] r0006 codex review 1 권고).
pub fn build_app(resolution: VaultResolution) -> axum::Router {
    let session_manager = Arc::new(LocalSessionManager::default());
    // StreamableHttpServerConfig는 `#[non_exhaustive]` — builder method 사용.
    // stateful_mode=true: `Mcp-Session-Id` 발급/보존 (multi-call session 유지의 source of truth).
    let config = StreamableHttpServerConfig::default().with_stateful_mode(true);

    let mcp_service = StreamableHttpService::new(
        move || Ok(ElfMcpServer::new_http(resolution.clone())),
        session_manager,
        config,
    );

    axum::Router::new()
        .nest_service("/mcp", mcp_service)
        .nest("/api", crate::http_backend::router())
}

/// Streamable HTTP transport로 MCP 서버 구동 + 휴먼 백엔드 마운트 (blocking).
///
/// - `/mcp`: MCP Streamable HTTP service (StreamableHttpService).
/// - `/api`: 휴먼용 백엔드 router (S2에선 `/api/health` 1개).
pub async fn run_http(resolution: VaultResolution, addr: SocketAddr) -> anyhow::Result<()> {
    let app = build_app(resolution);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!(
        "[elf] HTTP transport listening on {addr} (MCP: /mcp, API: /api) — Permissions::READ only (S2)"
    );
    axum::serve(listener, app).await?;
    Ok(())
}
