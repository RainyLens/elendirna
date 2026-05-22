//! 휴먼용 HTTP 백엔드.
//!
//! S2: skeleton + `GET /health`. 실 vault tool noun endpoint는 다음 phase 묵힘.
//! `mcp_server::http::run_http`에서 `/api` 아래 nest.

use axum::Router;
use axum::routing::get;

pub fn router() -> Router {
    Router::new().route("/health", get(health))
}

async fn health() -> &'static str {
    "ok"
}
