//! Streamable HTTP transport ([[N0033]] r0006 Step 4.2).
//!
//! MCP 2025-03-26 spec의 Streamable HTTP를 `rmcp::transport::streamable_http_server`로 노출.
//! 같은 axum listener에 휴먼용 백엔드(`crate::http_backend::router`)를 `/api` 아래 nest,
//! MCP service를 `/mcp` 아래 nest. vault resolution은 두 wire layer가 공유.
//!
//! 게이팅 ([[N0115]] S3a): auth 미초기화(keystore 빈/없음)면 `new_http`가 `Permissions::READ`
//! anonymous fallback + loopback 가드 유지(회귀 0). 활성 키가 있으면 auth 모드 — `/mcp`는
//! Bearer 필수 + per-request 권한 유도, allowed_hosts 완화(Bearer가 게이트). `/api`+static은
//! 모드 무관 loopback 고정.

use std::net::SocketAddr;
use std::sync::Arc;

use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::{
    StreamableHttpServerConfig, StreamableHttpService,
};

use crate::vault::VaultResolution;
use crate::vault::keystore::KeyRegistry;

use super::ElfMcpServer;

/// axum Router를 빌드 — `/mcp`에 MCP Streamable HTTP service, `/api`에 휴먼용 read-only
/// 백엔드 router(entries/bundle/lineage/search/validate, [[N0106]] P1), fallback에 임베드
/// 휴먼 뷰어 FE(`/` → index.html).
///
/// `run_http`와 in-process integration test가 공유. test는 본 fn으로 같은 router를
/// 짓고 `tower::ServiceExt::oneshot`으로 호출 — bind/listen 없이 multi-call session
/// 유지를 검증할 수 있다 ([[N0033]] r0006 codex review 1 권고).
pub fn build_app(resolution: VaultResolution) -> axum::Router {
    // auth 미초기화(empty registry) — anonymous READ-only. Bearer 유도는 Phase 3.
    build_app_with_registry(resolution, Arc::new(KeyRegistry::empty()))
}

/// `build_app`의 registry 주입 변형 ([[N0115]] S3a). auth 모드 serve와 integration test가
/// 직접 registry를 넘긴다 — 테스트는 `KeyRegistry::from_records(..)`로 process-global
/// `USERPROFILE` 변경 없이 키를 주입할 수 있다(Phase 3, env swap 회피).
pub fn build_app_with_registry(
    resolution: VaultResolution,
    key_registry: Arc<KeyRegistry>,
) -> axum::Router {
    // 휴먼 백엔드(`/api`)에 주입할 launch vault root — MCP service로 resolution이 move되기 전 확보.
    let vault_root = Arc::new(resolution.path.clone());
    let session_manager = Arc::new(LocalSessionManager::default());

    // StreamableHttpServerConfig는 `#[non_exhaustive]` — builder method 사용.
    // stateful_mode=true: `Mcp-Session-Id` 발급/보존 (multi-call session 유지의 source of truth).
    //
    // allowed_hosts (DNS-rebinding 가드, 기본 loopback-only): auth **초기화 시에만** 완화한다
    // ([[N0115]] route scope). Bearer가 실 게이트이므로 host 검증은 Bearer auth에 위임 —
    // 토큰 없는 rebinding 공격은 어차피 `build_call_context`에서 reject된다. 이 완화는 `/mcp`
    // service config에만 적용되고 `/api`+static의 `host_guard`(loopback 고정)는 절대 건드리지
    // 않는다 — 뷰어/`/api`는 외부 노출 안 함(Axis E).
    let mut config = StreamableHttpServerConfig::default().with_stateful_mode(true);
    if key_registry.is_initialized() {
        config = config.disable_allowed_hosts();
    }

    let mcp_service = StreamableHttpService::new(
        // factory는 매 연결마다 호출 — registry는 Arc clone(move-in)으로 공유.
        move || Ok(ElfMcpServer::new_http(resolution.clone(), key_registry.clone())),
        session_manager,
        config,
    );

    axum::Router::new()
        .nest_service("/mcp", mcp_service)
        .nest("/api", crate::http_backend::router(vault_root))
        // 루트(`/`)와 자산 경로 → 임베드된 휴먼 뷰어 FE([[N0106]] P1).
        .fallback(crate::http_backend::static_handler)
}

/// Streamable HTTP transport로 MCP 서버 구동 + 휴먼 백엔드/뷰어 마운트 (blocking).
///
/// - `/mcp`: MCP Streamable HTTP service (StreamableHttpService).
/// - `/api`: 휴먼용 read-only 백엔드 router ([[N0106]] P1).
/// - `/`   : 임베드된 휴먼 뷰어 FE.
pub async fn run_http(resolution: VaultResolution, addr: SocketAddr) -> anyhow::Result<()> {
    // keystore를 1회 로드 — 활성 키 존재 = auth 초기화됨([[N0115]] 게이팅 모델).
    let key_registry = Arc::new(KeyRegistry::load_from_disk());
    let auth_initialized = key_registry.is_initialized();

    // 외부 노출(non-loopback bind) 의도인데 auth 미초기화 → 익명 노출 거부 + first-run init 안내.
    // "Bearer 필수"는 addr이 아니라 초기화 상태로 트리거하되, addr은 "익명 노출 차단" 가드.
    if refuse_anonymous_exposure(&addr, auth_initialized) {
        anyhow::bail!(
            "외부 노출 주소({addr})로 구동하려면 API key 초기화가 필요합니다.\n\
             먼저 `elf key new --label <name> --permissions <read|write|admin>`로 키를 발급한 뒤 다시 실행하세요.\n\
             (loopback 주소면 키 없이 익명 read-only로 구동됩니다.)"
        );
    }

    let app = build_app_with_registry(resolution, key_registry);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let mode = if auth_initialized {
        "auth mode (Bearer API key 필수, per-request 권한 유도)"
    } else {
        "anonymous READ-only (auth 미초기화 — loopback 가드 유지)"
    };
    eprintln!(
        "[elf] HTTP transport listening on {addr} (viewer: /, MCP: /mcp, API: /api) — {mode}"
    );
    axum::serve(listener, app).await?;
    Ok(())
}

/// non-loopback bind(외부 노출 의도)인데 auth 미초기화면 기동 거부 ([[N0115]] 게이팅 모델).
/// loopback은 키 없이 익명 read-only 허용(회귀 0), 초기화되면 addr 무관 허용(Bearer가 게이트).
fn refuse_anonymous_exposure(addr: &SocketAddr, auth_initialized: bool) -> bool {
    !addr.ip().is_loopback() && !auth_initialized
}

#[cfg(test)]
mod tests {
    use super::refuse_anonymous_exposure;
    use std::net::SocketAddr;

    fn addr(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    #[test]
    fn loopback_uninitialized_is_allowed() {
        assert!(!refuse_anonymous_exposure(&addr("127.0.0.1:7878"), false));
        assert!(!refuse_anonymous_exposure(&addr("[::1]:7878"), false));
    }

    #[test]
    fn nonloopback_uninitialized_is_refused() {
        assert!(refuse_anonymous_exposure(&addr("0.0.0.0:7878"), false));
        assert!(refuse_anonymous_exposure(&addr("192.168.1.10:7878"), false));
    }

    #[test]
    fn initialized_is_allowed_regardless_of_addr() {
        // reverse-proxy(loopback bind + 외부 도달)도 인증되므로 허용.
        assert!(!refuse_anonymous_exposure(&addr("127.0.0.1:7878"), true));
        assert!(!refuse_anonymous_exposure(&addr("0.0.0.0:7878"), true));
    }
}
