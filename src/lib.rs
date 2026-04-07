pub mod error;
pub mod cli;
pub mod vault;
pub mod schema;
pub mod output;

/// set_current_dir은 프로세스 전역 상태이므로 테스트 전체에서 공유하는 직렬화 락.
/// 단위 테스트 내 모든 모듈이 crate::CWD_LOCK으로 참조한다.
#[cfg(test)]
pub(crate) static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
