//! 휴먼용 HTTP 백엔드 ([[N0106]] P1 — localhost read-only 뷰어).
//!
//! `mcp_server::http::run_http`에서 `/api` 아래 nest. 모든 엔드포인트는 GET·read-only이며
//! `vault::ops` / `vault::index` / `schema::validate`를 그대로 호출해 vault 도메인을 그대로
//! 노출한다(쓰기 없음 — write는 P2 composer 단계). `127.0.0.1` 바인드 자체가 신뢰 경계이고,
//! 추가로 Host 가드로 DNS rebinding/CSRF를 차단한다.

mod api;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::extract::Request;
use axum::http::{StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use rust_embed::RustEmbed;

/// `/api` 핸들러가 공유하는 상태 — launch vault root.
#[derive(Clone)]
pub struct ApiState {
    pub vault_root: Arc<PathBuf>,
}

/// `/api` 라우터. `vault_root`는 `build_app`이 `VaultResolution`에서 1회 해석해 주입한다.
pub fn router(vault_root: Arc<PathBuf>) -> Router {
    let state = ApiState { vault_root };
    Router::new()
        .route("/health", get(health))
        .route("/meta", get(api::meta))
        // read (P1)
        .route("/entries", get(api::list_entries).post(api::create_entry))
        .route("/entries/{id}", get(api::show_entry))
        .route("/entries/{id}/bundle", get(api::bundle_entry))
        .route("/lineage/{id}", get(api::lineage))
        .route("/search", get(api::search))
        .route("/validate", get(api::validate))
        // write (P2) — Origin/Sec-Fetch 가드는 write_guard 레이어가 적용
        .route("/entries/{id}/revisions", post(api::create_revision))
        .route("/entries/{id}/status", put(api::set_status))
        .route("/entries/{id}/tags", put(api::set_tags))
        .route("/entries/{id}/links", post(api::add_link))
        .with_state(state)
        .layer(middleware::from_fn(write_guard))
        .layer(middleware::from_fn(host_guard))
}

async fn health() -> &'static str {
    "ok"
}

/// 휴먼 뷰어 전용 axum 앱 — `/api`(read-only) + 루트 임베드 FE. MCP(`/mcp`)는 없다.
/// `--mcp` 없이 `elf serve` 했을 때 사용 — MCP 런타임과 독립으로 side-by-side 구동 가능([[N0106]]).
pub fn viewer_app(vault_root: Arc<PathBuf>) -> Router {
    Router::new()
        .nest("/api", router(vault_root))
        .fallback(static_handler)
}

/// 뷰어 전용 HTTP 서버 구동 (blocking). `/` → 임베드 FE, `/api` → read-only 백엔드.
pub async fn run_viewer(vault_root: PathBuf, addr: SocketAddr) -> anyhow::Result<()> {
    let app = viewer_app(Arc::new(vault_root));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("[elf] viewer listening on http://{addr}/ (API: /api) — read-only, MCP 없음");
    axum::serve(listener, app).await?;
    Ok(())
}

/// 빌드된 FE 정적 자산(`web/dist/`)을 바이너리에 임베드.
/// release: 바이너리 내장. debug: 컴파일 시점 절대경로에서 런타임 read(rust-embed 기본).
#[derive(RustEmbed)]
#[folder = "web/dist"]
struct WebAssets;

/// 루트 정적 서빙 — `build_app`의 fallback으로 마운트. `/` → index.html,
/// 그 외 임베드 자산. 미발견 경로는 SPA(hash 라우팅) fallback으로 index.html.
/// `/mcp`·`/api`는 각자 nest에서 처리되므로 여기로 내려오지 않는다.
pub async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    if let Some(file) = WebAssets::get(path) {
        let mime = file.metadata.mimetype().to_string();
        return ([(header::CONTENT_TYPE, mime)], file.data.into_owned()).into_response();
    }
    match WebAssets::get("index.html") {
        Some(file) => (
            [(header::CONTENT_TYPE, "text/html".to_string())],
            file.data.into_owned(),
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "viewer assets not embedded").into_response(),
    }
}

/// localhost 하드닝: Host 헤더가 있으면 loopback host만 허용한다(포트는 무관).
/// 헤더 부재는 통과(HTTP/1.1 브라우저는 항상 Host를 보내며, in-process 호출은 신뢰).
/// DNS rebinding은 공격자 도메인(Host=attacker.example)으로 들어오므로 여기서 막힌다.
async fn host_guard(req: Request, next: Next) -> Response {
    if let Some(host) = req.headers().get(axum::http::header::HOST) {
        let ok = host
            .to_str()
            .ok()
            .map(|h| {
                let host_only = h.rsplit_once(':').map(|(h, _)| h).unwrap_or(h);
                let host_only = host_only.trim_start_matches('[').trim_end_matches(']');
                host_only == "localhost" || host_only == "127.0.0.1" || host_only == "::1"
            })
            .unwrap_or(false);
        if !ok {
            return (StatusCode::FORBIDDEN, "forbidden host").into_response();
        }
    }
    next.run(req).await
}

/// write 하드닝 ([[N0106]] C2) — write 메서드(POST/PUT/PATCH/DELETE)에만 적용.
/// 브라우저는 `Origin`·`Sec-Fetch-Site`를 위조 불가하게 자동 부착하므로,
///  - `Sec-Fetch-Site: cross-site`/`cross-origin` → 거부
///  - `Origin`이 있으면 요청 Host와 동일 origin일 때만 허용(다른 사이트의 fetch 차단)
///  - `Origin` 부재(curl/CLI 등 비-브라우저) → 허용
/// GET 등 read는 통과(기존 Host 가드만). DNS rebinding은 Host 가드가, 브라우저발 CSRF는 여기서 막는다.
async fn write_guard(req: Request, next: Next) -> Response {
    let is_write = matches!(req.method().as_str(), "POST" | "PUT" | "PATCH" | "DELETE");
    if !is_write {
        return next.run(req).await;
    }
    let headers = req.headers();

    if let Some(sfs) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        if sfs.eq_ignore_ascii_case("cross-site") || sfs.eq_ignore_ascii_case("cross-origin") {
            return (StatusCode::FORBIDDEN, "cross-site write blocked").into_response();
        }
    }

    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let host = headers
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        // "http://127.0.0.1:7879" → "127.0.0.1:7879" 와 Host 비교(same-origin).
        let origin_host = origin.split("://").nth(1).unwrap_or("");
        if origin_host.is_empty() || origin_host != host {
            return (StatusCode::FORBIDDEN, "cross-origin write blocked").into_response();
        }
    }

    next.run(req).await
}
