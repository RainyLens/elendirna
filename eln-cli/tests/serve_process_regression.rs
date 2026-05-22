//! N0089 / N0090 process suicide regression test.
//!
//! 시나리오: `elf serve --mcp`를 cwd가 vault 아닌 위치에서 spawn + fake `USERPROFILE`/`HOME`에
//! 기존 vault 박혀 있는 상태. v0.5.4까지는 auto-init이 `AlreadyInitialized`로 process suicide →
//! Desktop transport close → re-spawn 무한 루프 발현. v0.6.0(N0090)부터는 Fallback init이
//! 기존 vault를 채택하고 stderr warning 후 Ok → process 살아있음.
//!
//! codex 사전 검토 권고 (D 항목):
//! - USERPROFILE + HOME 둘 다 fake set (cross-platform 안전)
//! - ELF_VAULT 반드시 env_remove (serve가 ELF_VAULT를 home fallback보다 먼저 봄)
//! - cwd는 fake home **밖** (그렇지 않으면 find_local_vault_root가 home vault hit → CwdSearchHome 분기 → fallback init 자체가 트리거 안 됨)
//! - child.stdin은 `Stdio::piped()`로 열고 핸들 유지 (stdin 닫히면 stdio server 정상 종료 → false failure)
//! - kill + wait 모두 호출

use assert_cmd::cargo::CommandCargoExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// CI/Windows 호환: fake home에 vault init은 binary로 한 번 호출 (`elf init --global` 류 대신
/// `elf init <path>`로 fake home에 박기). 다음 spawn에서 그 vault를 발견하게 함.
fn init_vault_at(path: &std::path::Path) {
    let mut cmd = Command::cargo_bin("elf").expect("cargo binary 'elf' 찾기 실패");
    cmd.arg("init")
        .arg(path)
        .arg("--name")
        .arg("regression-fake-home")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let status = cmd.status().expect("init 실행 실패");
    assert!(status.success(), "fake home vault init 실패");
}

#[test]
fn serve_mcp_survives_when_home_already_initialized_and_cwd_is_non_vault() {
    let fake_home = tempfile::tempdir().expect("fake home tempdir");
    let fake_cwd = tempfile::tempdir().expect("fake cwd tempdir");

    // 1. fake home에 미리 vault 박음 (config.toml 존재)
    init_vault_at(fake_home.path());
    assert!(fake_home.path().join(".elendirna/config.toml").exists());

    // 2. fake cwd는 vault 아님 (init 안 함)
    assert!(!fake_cwd.path().join(".elendirna").exists());

    // 3. serve --mcp spawn — fake home + non-vault cwd
    let mut cmd = Command::cargo_bin("elf").expect("cargo binary 'elf' 찾기 실패");
    cmd.arg("serve")
        .arg("--mcp")
        .env_remove("ELF_VAULT")
        .env("USERPROFILE", fake_home.path())
        .env("HOME", fake_home.path())
        .current_dir(fake_cwd.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("elf serve --mcp spawn 실패");
    // stdin 핸들은 drop하지 않고 유지 (drop되면 stdio server가 정상 종료 → false failure).
    // child가 scope 안에 살아있는 동안 stdin도 함께 유지된다.
    let _stdin_keep_alive = child.stdin.take();

    // 4. 3초 poll loop — process가 살아있는지 확인. 더 안정적인 timing.
    let start = Instant::now();
    let timeout = Duration::from_secs(3);
    let poll_interval = Duration::from_millis(200);
    let mut early_exit: Option<std::process::ExitStatus> = None;
    while start.elapsed() < timeout {
        match child.try_wait() {
            Ok(Some(status)) => {
                early_exit = Some(status);
                break;
            }
            Ok(None) => {
                // 아직 살아있음 — 계속 poll
            }
            Err(e) => panic!("try_wait 실패: {e}"),
        }
        std::thread::sleep(poll_interval);
    }

    // 5. cleanup — 어떤 결과든 kill + wait
    let _ = child.kill();
    let _ = child.wait();

    // 6. assertion — N0089 회귀였다면 3초 안에 exit 했을 것 (이전 v0.5.4 동작).
    //    본 PR fix로 살아있어야 함.
    assert!(
        early_exit.is_none(),
        "N0089 regression: elf serve --mcp exited within 3s (status={early_exit:?}). \
         Expected stdio server to stay alive after Fallback init adopts existing vault."
    );
}
