//! production hardening — `register_vault_alias`가 home(글로벌) vault를 skip하는지 잠그는 회귀 테스트.
//!
//! `--vault <home>`로 글로벌 vault에 접근할 때 vault_name을 글로벌 config에 자기-alias로
//! 등록하던 동작을 제거했음을 검증한다. home이 아닌 vault는 그대로 정상 등록되어야 한다.
//!
//! HOME/USERPROFILE을 임시 디렉터리로 격리(프로세스당 1회)하여 호스트 글로벌 config를
//! 건드리지 않는다. `register_vault_alias`는 글로벌 config를 read-modify-write 하므로
//! 모든 검증을 단일 테스트 함수에서 순차로 수행하여 RMW race를 피한다.

use eln_core::vault::config::VaultConfig;
use std::sync::OnceLock;
use tempfile::TempDir;

/// 이 테스트 바이너리 전체가 공유하는 격리된 HOME(USERPROFILE).
fn temp_home() -> &'static TempDir {
    static HOME: OnceLock<TempDir> = OnceLock::new();
    HOME.get_or_init(|| {
        let home = tempfile::tempdir().unwrap();
        // SAFETY: get_or_init이 이 클로저를 프로세스당 1회만 실행하므로 set_var는 단 한 번이며,
        // 이후 env::var read와 happens-before가 성립한다.
        unsafe {
            std::env::set_var("USERPROFILE", home.path());
            std::env::set_var("HOME", home.path());
        }
        home
    })
}

#[test]
fn home_vault_alias_is_skipped_but_others_register() {
    let home = temp_home();

    // 1) home(글로벌) vault를 가리키는 등록은 skip되어야 한다 (자기-alias 방지).
    VaultConfig::register_vault_alias(home.path(), "self-home-alias").unwrap();
    let after_home = VaultConfig::read_global();
    assert!(
        !after_home.vaults.contains_key("self-home-alias"),
        "home vault는 자기-alias를 글로벌 config에 등록하면 안 된다"
    );

    // 2) home이 아닌 vault는 정상 등록되어야 한다 (control — 회귀 방지).
    let other = tempfile::tempdir().unwrap();
    VaultConfig::register_vault_alias(other.path(), "other-vault-alias").unwrap();
    let after_other = VaultConfig::read_global();
    assert_eq!(
        after_other
            .vaults
            .get("other-vault-alias")
            .map(String::as_str),
        Some(other.path().to_string_lossy().as_ref()),
        "home이 아닌 vault는 글로벌 config에 alias로 등록되어야 한다"
    );
}
