# Changelog

## [0.8.0] — 2026-05-30

### npm 배포 채널 (N0097)

elendirna를 Rust 툴체인 없이 설치할 수 있는 npm 채널 추가. crates.io `eln-cli`(v0.7.0 eln 패밀리 분리)에 이은 두 번째 배포 표면.

- **`elendirna`** npm 패키지 — `npx -y elendirna` 또는 `npm i -g elendirna`로 `elf` 바이너리 사용
- 매커니즘: **단일 패키지에 6 플랫폼 prebuilt 바이너리를 전부 동봉**(`binaries/<os>-<cpu>/elf[.exe]`), Node launcher가 런타임에 호스트 맞는 1개를 선택. postinstall 없음·설치 시 네트워크 없음 → `--ignore-scripts`/오프라인/사설 레지스트리/읽기전용 FS 안전. (초기엔 플랫폼별 `optionalDependencies` 분할을 시도했으나, 유사 unscoped 이름 다수의 급속 publish가 npm 스팸 탐지 E403에 걸려 단일 패키지로 전환.)
- Node launcher: stdin/stdout/stderr·exit code·종료 시그널 전달 (장수명 stdio MCP 서버 모드 포함)
- 지원: linux/darwin/win32 × x64/arm64. Linux는 glibc ≥ 2.35(Ubuntu 22.04 베이스라인) — musl/Alpine 미지원 시 `cargo install eln-cli`
- 신규 GitHub Actions `release.yml` — arch별 네이티브 러너 크로스 빌드 매트릭스 → 단일 `elendirna` 패키지에 동봉 → publish. `workflow_dispatch` + `confirm` 게이트로 비가역 publish 보호
- (`eln` unscoped 이름은 npm name-similarity로 사용 불가 — elendirna 단독 게시)

### serve MCP 스니펫 — npm wrapper 인식

- `elf serve`가 출력하는 MCP config 스니펫의 `command`를, npm wrapper로 기동된 경우 `current_exe()`(node_modules 내부 절대경로, 재설치 시 깨짐) 대신 안정 명령(`elendirna`/`eln`)으로 emit. launcher가 `ELN_LAUNCHER_CMD` 주입, `resolve_elf_bin()`이 우선 사용

### 내부 변경

- 워크스페이스 버전 0.7.0 → 0.8.0 (crates.io `eln-core`/`eln-cli` ↔ npm 정렬). `eln-plugin-sdk`는 독립 버전 라인(0.1.0) 유지
- `[profile.release]`: `strip = "symbols"`, `lto = "thin"`, `codegen-units = 1` — npm 동봉 바이너리 축소
- `npm/` 스캐폴딩: 메인 wrapper(launcher + 동봉 binaries) + `sync-version`/`assemble-bundle` 스크립트

---

## [0.6.2] — 2026-05-21

### 운영 발견 fix (N0010 r0001 누적 3건)

`elf validate` 가 실제 vault 운영 중 누적한 발견 3건을 한 묶음으로 처리. F1과 F3는 frontmatter quote escape 처리 결함이라는 같은 뿌리, F2는 독립된 markdown 스캐너 문제.

#### F1 — `--fix` 라운드트립 결함

증상: `elf validate --fix` 가 "N개 항목 자동 수정됨"을 보고하지만 두 번째 `validate` 에서 같은 consistency warning 재출현. N0058 같은 따옴표 포함 title 케이스에서 관찰.

원인: `src/schema/manifest.rs` 의 `NoteFrontmatter::parse()` 가 `trim_matches('"')` 로 값 양 끝의 따옴표 *전부* 를 벗기고, `to_string()` 이 값 안의 `"` 를 escape 없이 다시 감쌌음. read→write→read 라운드트립 불일치.

- `yaml_quote(&str)` / `yaml_unquote(&str)` 헬퍼 신설 ([src/schema/manifest.rs](src/schema/manifest.rs))
- `parse()` 의 모든 `trim_matches('"')` 사이트를 `yaml_unquote` 로 교체
- `to_string()` 의 id/title/baseline/tags 직렬화 모두 `yaml_quote` 경유

#### F2 — dangling inline ref false-positive

증상: cmd-validate 문서화 entry(N0010) 본문의 illustrative `→ see N0099` 가 dangling으로 잡힘 — 실제 broken link가 아닌 예제일 뿐.

원인: `check_dangling` 의 regex 가 본문을 그대로 스캔, fenced code block / inline code / blockquote 인식 부재.

- pulldown-cmark 0.10 (default-features 끔) dep 추가
- `scan_inline_refs(content, &Regex) -> Vec<String>` 헬퍼 신설 ([src/schema/validate.rs](src/schema/validate.rs))
- `check_dangling` 의 note.md / revision 파일 스캔 두 사이트 모두 헬퍼 경유로 교체. fenced code block / inline code / blockquote 안의 ref는 무시

#### F3 — consistency diff 메시지 가독성

증상: 값에 따옴표가 포함된 mismatch 케이스에서 diff 메시지가 escape 표현 차이를 시각적으로 같은 plain text로 출력 — 사용자 진단 어려움.

- `check_consistency` diff 메시지를 `{val:?}` Debug 포맷으로 변경 ([src/schema/validate.rs](src/schema/validate.rs)). `\"`, `\\`, 비인쇄 문자가 escape 표현으로 드러남

### 내부 변경

- `pulldown-cmark = "0.10"` (default-features = false) 의존성 추가
- `cargo test --all`: 119 tests passing (lib 86 + integration 6 + mcp_integration 19 + serve_process 1 + sqlite 7). 신규 unit test 4개:
  - `frontmatter_quote_roundtrip_preserves_inner_quotes` (F1)
  - `apply_fixes_resolves_consistency_in_one_pass` (F1 end-to-end)
  - `dangling_inline_ref_skips_fenced_code_block` (F2)
  - `consistency_diff_message_uses_debug_format` (F3)

### Scope OUT (별 트랙)

- `apply_fixes` silent-fail 추적 로깅 — Change 1로 원인 자체가 사라져 불필요
- serde_yaml 기반 frontmatter 파서 전체 교체 — over-engineering
- multi-line YAML / block scalar / 다른 escape 케이스 — 실측 발견 시 별 트랙

---

## [0.6.1] — 2026-05-20

### 주요 변경 (breaking)

#### Message `scope` 축 추가 (N0091 r0006)

`Message { level, kind, message }` → `Message { level, kind, message, scope }`. `scope: MessageScope`는 사실의 시점 축을 표현:

- `call`: 이 한 번의 호출에만 유효 — `escalated_write`, `validate:*`, `attach_collision`
- `session`: 현재 MCP session(session_start ~ 종료) 동안 유의미 (현재 emit site 없음, reserve)
- `instance`: MCP server process lifetime 사실 — `init_context_fallback` (launch 시점 결정)
- **MCP + CLI JSON contract 양쪽에 `scope` 필드 신설**. `Message::info/warning/error` 빌더 시그니처에 `scope` 인자 추가 (호출처 모두 update)
- `lowercase` serde rename — `"scope": "call"|"session"|"instance"`

#### `session_start` 응답에 `session_id` 노출 (B1)

- 매 `session_start` 호출은 UUID v4 형식 `session_id`를 응답에 포함 (`"session_id": "xxxxxxxx-...-4xxx-..."`)
- 같은 stdio process 내 재호출도 새 session으로 간주 — 새 UUID 발급 + SessionState 신규 entry
- **노출 위치**: `session_start` 응답에만. `vault_meta` / 다른 tool 응답에는 포함 X (사용자 명시 결정)

#### `vault_meta` init_fallback inject 조건 좁힘 (M-2)

v0.6.0 known issue 해소:

- 기존: launch path와 resolved path만 비교 → 같은 home vault를 explicit `vault='global'`로 본 응답에도 `init_context_fallback` warning 부착됨
- v0.6.1: launch resolution 전체(path + origin) 일치 시에만 inject. `FallbackGlobal`(launch)와 `ExplicitGlobal`(call)/`Alias("global")`(call)은 다른 resolution → alias 응답에 launch 사실이 새지 않음
- 새 helper `resolutions_match(a, b)`: path canonicalize + origin variant equality

### 내부 변경

- `MessageScope` enum 신설 ([src/output/message.rs](src/output/message.rs))
- `SessionState` struct 신설 ([src/mcp/mod.rs](src/mcp/mod.rs)) — 기존 전역 `session_local_vault: RwLock<Option<VaultResolution>>` 흡수. 단순 교체 (lifetime 깊이 X — stdio 1:1)
- `ElfMcpServer`: `sessions: RwLock<HashMap<String, SessionState>>` + `current_session_id: RwLock<Option<String>>` 필드. 기존 전역 RwLock 제거
- `current_session_local_vault(&self)` helper 신설 — `resolve_tool_vault` 내부 우선순위 lookup용
- `uuid = { version = "1", features = ["v4"] }` 의존성 추가
- `cargo test --all`: 115 tests passing (lib 82 + integration 6 + mcp_integration 19 + serve_process 1 + sqlite 7). 신규 unit test 4개 (`message_scope_serializes_as_lowercase` / `vault_meta_skip_init_fallback_for_explicit_same_path` / `session_start_emits_distinct_uuid_v4_per_call` / `session_id_only_in_session_start_response`)

### Scope OUT (별 트랙)

- info `messages[]` 통합 (`hint`/`next_action`/`context_hints` 등 info성 필드 통합) — Cons-3 묵힘
- session-scope message emit site 식별
- N0073 비-string param 역직렬화
- 코드/commit vault index 회피 sweep
- MCP initialize 응답 검증 보강 (M-3)
- HTTP/SSE transport (elen-labs로 분리)

---

## [0.6.0] — 2026-05-20

### 주요 변경 (breaking)

#### MCP 응답 contract — `messages[]` 통합 (N0091)

`Message { level, kind, message }` 구조체로 시점성/사후 알림을 통합 (`level`: info / warning / error, syslog 정합):

- **제거**: `warning` (string) 필드 (7개 write tool — escalated_write 메시지)
- **제거**: validate 응답의 별도 `issues[]` 배열 — 각 issue가 `messages[]` 원소로 (`kind: "validate:<naming|schema|consistency|dangling|cycle|orphan|asset>"`)
- **제거**: entry_attach 응답의 `warning` 필드 — collision warning은 `messages[]` 원소로 (`kind: "attach_collision"`)
- **rename**: validate 응답 `errors: u32` → `error_count: usize`, `warnings: u32` → `warning_count: usize`
- **추가**: `messages: Vec<Message>` 필드 (vault_meta가 init context fallback 부착 시 + mark_escalated_if_needed가 escalated write 시 + validate가 각 issue 시 + entry_attach가 collision 시)
- 메시지 phrasing은 이모지/uppercase prefix 없는 일반 텍스트. categorical 구분은 `level` + `kind` field로
- 동일 응답에 vault_meta init_fallback message + mark_escalated message가 공존 가능 (append 동작)

#### CLI JSON contract — `messages[]` 통합

MCP와 동일 contract로 CLI JSON 출력도 migrate:

- `elf entry attach --json`: `warning` 필드 → `data.messages[]` (kind: `attach_collision`)
- `elf validate --json`: `data.errors/warnings/issues` → `data.error_count/warning_count/messages[]`
- `elf bundle`: `--depth` 기본값 1 → 0 (cost-aware default), 미지정 시 linked 있으면 `cost_hint` 필드 응답

#### MCP `bundle` default 변경 — cost-aware

- `bundle(id)` depth default `1 → 0`. revisions만 수집, linked entry는 미수집 (cost 절감)
- depth 미지정 + linked 존재 시 응답에 `cost_hint: String` inject — "linked N entry는 default(depth=0)에서 미수집 (~X bytes 예상). bundle(id, depth=1)로 escalate."
- bytes estimate는 각 link id의 `manifest.toml + note.md` file metadata로 cheap 추정 (open X)

### 주요 기능

#### init context 분리 — Fallback init은 idempotent (N0090)

`elf serve --mcp`가 vault 자동 탐색 실패 시 `~/.elendirna/`에 fallback init하는데, 기존 vault가 있으면 `AlreadyInitialized` 오류로 process suicide → Desktop transport close → re-spawn 무한 루프(N0089) 발현. v0.6.0에서 fix:

- `InitContext { Explicit, Fallback }` enum 신설 ([src/cli/init.rs](src/cli/init.rs))
- `pub fn run(args)`는 Explicit wrapper로 보존 (외부 lib API 무변경) + `pub(crate) fn run_with_context(args, ctx)` 신설
- `Explicit` (CLI `elf init` 명시 호출) → 기존 동작 보존 (`Err(AlreadyInitialized)`)
- `Fallback` (MCP serve auto-init) → stderr warning + `Ok(())` 조기 반환 (기존 vault 채택)
- serve.rs는 init이 기존 vault 채택했으면 `launch_init_fallback: true`로 `ElfMcpServer` builder에 전달
- vault_meta는 resolved vault가 launch path와 일치할 때 messages[]에 `init_context_fallback` warning inject

#### `elf entry tag` CLI + MCP — manifest mutability 정식 경로 (N0080)

CRUD의 U 결손 해소 — tag 편집을 tool-mediated path로:

- CLI: `elf entry tag add <id> <tag>` / `tag remove <id> <tag>` / `tag set <id> <tag1,tag2,...>`
- MCP: `entry_tag_add(id, tag)` / `entry_tag_remove(id, tag)` / `entry_tag_set(id, tags[])`
- 멱등: add 중복 시 no-op, remove 없음 시 no-op
- comma parser: trim + dedupe + empty drop
- sync event 기록: `entry.tag.added.{id}.{tag}` / `entry.tag.removed.{id}.{tag}` / `entry.tag.set.{id}`
- 정책 제한(audit / immutable check / anti-taxonomy 가드)은 미래 결정 — 현재는 자유롭게 허용
- title/status/links 등 다른 mutable field는 후속 PR 후보

### 내부

- 신규 module `src/output/message.rs`: `MessageLevel`, `Message`, `push_message`, `issue_kind_str` — CLI/MCP 양쪽 공유
- `ElfMcpServer`: `launch_init_fallback: bool` field + `with_init_fallback(value)` builder
- `vault_meta`를 instance method로 변경 (`launch_init_fallback` 접근 필요)
- `mark_escalated_if_needed`는 `messages[]` array에 push (기존 단일 `warning` string 필드 제거)
- `run_stdio(resolution, launch_init_fallback)` 시그니처 변경
- `pub fn estimate_linked_entry_bytes(vault_root, link_ids) -> u64` 신설 ([src/vault/ops.rs](src/vault/ops.rs))
- `cargo test`: 111+ tests 0 fail (Phase A 분기 / messages[] inject / Phase H tag mutability / process-level regression test 추가)
- 신규 `tests/serve_process_regression.rs` — N0089 회귀 방지 (assert_cmd + fake USERPROFILE/HOME + ELF_VAULT remove + cwd non-vault + stdin piped + 3s poll + kill+wait)

### 알려진 후속 항목

- `vault_meta` init_fallback inject 조건이 path 기반이라 explicit `vault='global'` 호출이 같은 home vault 가리키면 warning이 부착될 수 있음 — alias 경계 정리는 v0.6.1 후보
- process regression test는 process liveness만 검증 — MCP initialize 응답 검증까지는 후속 보강
- info messages[] 통합 (현재 `hint`/`next_action`/`context_hints`/`handover_status` 등 info성 필드는 보존) — v0.6.1+ 검토

---

## [0.5.4] — 2026-05-11

### 패치

#### vault sudo guard 확장 + write 응답 contract 강화

v0.5.3 작업분 흡수 (sudo guard 확장):

- `VaultOrigin::CwdSearchHome` variant 추가 — process CWD가 home과 일치할 때 결과 path는 global vault와 같지만 origin이 구분됨
- `is_home_vault_root` helper — Windows USERPROFILE/HOME, junction, trailing slash, `\\?\` canonical path 차이를 단일 helper로 흡수 (양쪽 canonicalize 실패 시 보수적 false)
- `elf serve --mcp` 시작 시 `find_local_vault_root` 결과를 home과 비교해 origin wrap
- write 가드 확장 — 기존 `FallbackGlobal`만 보던 7곳을 `ensure_write_confirmed` helper로 centralize, `FallbackGlobal | CwdSearchHome` 모두 `confirm=true` 없이 reject (Claude Desktop처럼 process CWD가 home인 host에서 silent global write 함정 차단)
- 에러 메시지 분기: `FallbackGlobal` → "writing to fallback-global vault...", `CwdSearchHome` → "writing to host-default global vault — cwd is at home, vault resolved to a global location"

v0.5.4 작업분 (write 응답 contract):

- `mark_escalated_if_needed` helper — guarded origin(`FallbackGlobal | CwdSearchHome`) + `confirm=true`로 가드를 통과한 write 응답에 `escalated_write: true` + `warning: "🚨 GLOBAL_WRITE_EXECUTED — ..."` 필드 inject
- 7개 write tool 응답에 적용 — guarded origin 통과는 silent 성공이 아닌 시각적 escalation 신호로 노출
- `confirm` schema description 갱신 — "true로 통과 시 응답에 escalated_write:true + warning 필드 동봉" 명시

#### 응답 contract (v0.5.4 기준)

- `vault` — resolved absolute path
- `vault_kind` — `local` | `global`
- `vault_origin` — `explicit_path` | `explicit_global` | `alias:<name>` | `env_var` | `cwd_search` | `cwd_search_home` | `fallback_global`
- `fallback: true` — origin이 `fallback_global`일 때만
- `escalated_write: true` + `warning` — guarded origin + `confirm=true` 통과 시에만

### 내부

- `cargo test --lib`: 61 passed (58 + 3 신규 escalated_write tests)
- `cargo test --test mcp_integration`: 19 passed (회귀 0)
- `cargo fmt`: pass

---

## [0.5.1] — 2026-04-27

### 패치

#### vault 출처(provenance) 추적 + FallbackGlobal 쓰기 보호
- `VaultOrigin` enum 추가 (`ExplicitPath`, `ExplicitGlobal`, `Alias`, `EnvVar`, `CwdSearch`, `FallbackGlobal`)
- `VaultResolution { path, origin }` 구조체 추가 — vault 경로와 그 출처를 함께 전달
- `find_local_vault_root()` 추가 — CWD 탐색 전용, 글로벌 폴백 없음 (FallbackGlobal 감지 기반)
- `elf serve --mcp` → `run_stdio(VaultResolution)` 시그니처 변경, provenance 캡처
- 모든 MCP 응답에 `vault_origin` 필드 포함 (`explicit_path`, `cwd_search`, `fallback_global` 등)
- FallbackGlobal vault 감지 시 `fallback: true` 필드 추가
- mutating 도구 7개 (`entry_new`, `entry_status`, `revision_add`, `sync_record`, `validate`, `entry_attach`, `entry_detach`)에 `confirm: bool` 파라미터 추가
- FallbackGlobal vault에 `confirm=true` 없이 쓰기 시도 시 `invalid_params` 오류 반환

---

## [0.5.0] — 2026-04-25

### 주요 기능

#### MCP Multi-Vault
- 모든 MCP tool에 `vault` 파라미터 추가 — 호출 단위로 vault 선택 가능
- `resolve_tool_vault()` 도입: 우선순위는 explicit `vault` 파라미터 → session-local default → server default
- `session_start` 결과에 현재 default vault 정보 포함, 이후 호출이 자연스럽게 같은 vault에 묶이도록 안내

#### Attachment MVP
- `entry_attach` / `entry_detach` MCP tool + 대응 CLI 신규
- assets 저장은 `create_new` 모드 — 동일 stored_filename 충돌 시 거부
- manifest 기반 collision check + detach 시 orphan asset 자동 정리
- `AttachmentResult`에 `size` 필드 포함

#### FlexibleEntries
- `sync_record`의 `entries` 필드를 flexible 역직렬화로 처리 — 단일 ID 문자열, 배열, 객체 형식을 모두 받아들임
- 외부 caller(특히 AI agent)의 작은 형식 차이로 sync 기록이 거부되던 문제 해소

### 내부 변경
- 죽은 `_label` 파라미터, `stored_filename` 잔여 코드 정리
- detach 동작 주석 정정
- crates.io 메타데이터 정리 (repository URL → `elen-labs/elendirna`)
- 전체 테스트 풀 스위트 통과, clippy `-D warnings` 클린

---

## [0.4.4] — 2026-04-22

### 패치
- N0041 관련 vault 운영 버그 수정 및 세션 회고 반영
- `vault_kind` 감지 로직 보강 (v0.4.3 후속)

---

## [0.4.3] — 2026-04-22

### 패치
- 모든 MCP tool 응답에 `vault` / `vault_kind` 필드 포함 — 멀티 vault 환경에서 호출 결과의 출처를 명시
- `vault_info()`의 `vault_kind` 감지 로직 정정

---

## [0.4.2] — 2026-04-16

### 패치

#### 타임스탬프 로컬 offset 보존
- revision, manifest, sync 이벤트 모든 timestamp를 `Utc::now()` → `Local::now()`로 변경
- 파일에 `+09:00` 등 로컬 offset이 그대로 기록됨 (예: `2026-04-16T15:30:00+09:00`)
- 내부 타입 `DateTime<Utc>` → `DateTime<FixedOffset>` — offset 정보가 파싱 후에도 보존
- 기존 `+00:00` 데이터는 `parse_from_rfc3339`로 역호환 파싱

---

## [0.4.1] — 2026-04-16

### 패치

#### MCP 온보딩 개선
- `session_start` 툴 추가 — AI용 세션 랜딩 가이드. 새 세션·모델 교체·컨텍스트 초기화 후 첫 호출로 vault 상태와 행동 방침을 반환. vault가 비어 있으면 시딩 유도 지침 포함
- `seed` MCP 프롬프트 추가 — 사용자용 대화형 온보딩. `prompts/get("seed")`로 User/Assistant 메시지 쌍을 주입하여 신규 사용자의 첫 아이디어 입력을 안내

---

## [0.4.0] — 2026-04-16

### 주요 기능

#### Multi-Vault 지원
- `--vault <PATH>` / `--global` 전역 플래그 추가 — 모든 서브커맨드에서 vault를 명시 지정할 수 있음
- vault 결정 로직을 `resolve_vault_root()` 단일 진입점으로 통일
  - 우선순위: `--vault` → `--global` → `ELF_VAULT` → cwd 상위 탐색 → global 폴백
- `--vault` 첫 사용 시 해당 vault의 `vault_name`을 `~/.elendirna/config.toml [vaults]`에 자동 alias 등록 — 이후 `@vault:<alias>:N####` 형태로 cross-vault 링크 참조 가능
- `global` / `local` 은 예약 alias (등록 불가)

#### `elf entry status`
- `elf entry status <id> <status>` 서브커맨드 추가
- 허용 값: `draft` → `stable` → `archived`
- manifest `status` + `updated` 갱신, `sync.jsonl`에 `status.changed` 이벤트 기록
- 에이전트가 `query --status stable`로 확정된 지식만 빠르게 필터링할 수 있는 기반 마련

#### `bundle` 고도화 — 컨텍스트 예산 제어
- `--depth N` 옵션: linked entry 탐색 깊이를 에이전트가 직접 제어
  - `0`: 자신 + revision chain만 (linked entry 수집 없음)
  - `1`: 직접 linked entry의 note body 전문 포함 (기본값, 기존 동작)
  - `2+`: 2홉 이상은 note body 없이 manifest 메타데이터만 수집 (`shallow: true` 표시)
- `--since <spec>` 옵션: 지정 시점 이후 revision만 포함 (entry body는 항상 포함)
  - `N####@r####` 형식: 해당 revision 이후
  - RFC 3339 timestamp 형식: 해당 시각 이후

#### MCP 자기서술 강화
- 모든 MCP tool `description`에 트리거 조건("언제 이 tool을 써야 하는가") + 직접 파일 접근 금지 안내 삽입
- `server.instructions`에 세션 시작/종료 프로토콜 + 컨텍스트 예산 패턴 내장 — CLAUDE.md 없이도 에이전트가 워크플로를 자동 이해
- `entry_status` MCP tool 신규 추가
- `bundle` MCP tool에 `depth` / `since` 파라미터 추가

#### `elf serve` — MCP config snippet 출력
- `elf serve` (`--mcp` 없이) 호출 시 에러 대신 MCP config snippet을 stdout에 출력
- 현재 `elf` 바이너리 경로 + vault 경로를 자동 삽입하므로 복사해서 바로 사용 가능

#### `elf help [--json]`
- `elf help` — 커맨드 표면 요약 출력 (사람 읽기용)
- `elf help --json` — 커맨드 목록, 파라미터, 트리거 조건, 워크플로 가이드를 JSON으로 출력 (AI-readable)

### 내부 변경
- `VaultConfig`에 `vaults: HashMap<String, String>` 필드 추가 (backward-compatible, 기존 config.toml 파싱 영향 없음)
- `VaultArgs` 구조체 + `resolve_vault_root()` / `parse_vault_alias()` / `resolve_vault_alias()` 신규
- `BundleOptions` / `BundleSince` 타입 + `bundle_with_opts()` 함수 신규
- `cli/help.rs` 신규 파일
- 전체 테스트 41개 통과 (단위 + 통합 + MCP + SQLite)

---

## [0.3.2] — 2026-04-13

### 기능
- MCP 서버 시작 시 vault가 없으면 `~/.elendirna/` 전역 vault 자동 초기화
- MCP 서버 시작 시 v1 vault를 v2 compact layout으로 자동 마이그레이션

### 버그 수정
- `os error 3`: vault_root가 `.elendirna`를 직접 가리킬 때 경로 정규화 누락 수정
- `atomic_write` PID 기반 임시 파일로 동시 쓰기 충돌 방지
- SQLite WAL 모드 및 `busy_timeout` 설정으로 동시 접근 안정성 향상

### 문서
- Proposal 003: MCP 서버 자동 설정
- Proposal 004: 첨부파일 지원 및 무결성 검사
- README에 dogfooding 섹션 추가

---

## [0.3.1] — 2026-04-13

> v0.3.0 릴리스 직후 Cargo.toml 버전 조정 (0.3.0 → 0.3.1). 기능 변경 없음.

---

## [0.3.0] — 2026-04-13

### 주요 변경 (breaking)
- **Compact Layout (schema v2)**: `entries/`, `revisions/`, `assets/` 디렉터리를 `.elendirna/` 하위로 이동
  - 기존 v1 vault는 폴백으로 그대로 동작 (하위 호환 유지)
  - `data_root()`: `.elendirna/entries/` 존재 여부로 v1/v2 자동 판단
- `CURRENT_SCHEMA_VERSION`: 1 → 2

### 기능
- `elf migrate`: v1 → v2 compact layout 이관 커맨드 (`--dry-run` 지원)
- `elf init --global`: 홈 디렉터리(`~/.elendirna/`)에 전역 vault 초기화
- `find_vault_root`: cwd 상위 탐색 실패 시 `~/.elendirna/` 폴백

### 기타
- 전체 테스트 63개 통과
- 프로젝트 자체 vault도 v2로 migrate 완료

---

## [0.2.4] — 2026-04-10

### 변경
- `thiserror` 1 → 2 업그레이드 반영

### 버그 수정
- Windows CRLF 관련 파싱 버그 수정 (v0.3 브레인스토밍 세션 중 발견)

### 내부
- v0.3 설계 결정 사항 브레인스토밍 및 dogfooding 세션
- MCP 서버 자동 설정 Proposal 초안 작성
- 통합 테스트 추가

---

## [0.2.3] — 2026-04-09

### 버그 수정
- MCP 응답의 `outputSchema`에 `type: "object"` 강제 지정하여 MCP spec 준수

---

## [0.2.0] — 2026-04-09

### 주요 기능
- **MCP 서버** (`elf serve --mcp`): AI 에이전트가 vault를 직접 조작할 수 있는 JSON-RPC over stdio 서버
  - 제공 tool: `entry_list`, `entry_show`, `entry_new`, `revision_add`, `bundle`, `query`, `sync_record`, `validate`
- **SQLite 인덱스** + **`elf query`**: tag, status, title_contains, baseline 기반 전문 검색
- **`elf bundle`**: entry + revision delta chain + 링크된 entry 전체를 하나의 컨텍스트로 수집
- **sync record** (`sync.jsonl`): 세션 요약을 append-only로 기록하는 세션 로그
- **`ELF_VAULT` 환경변수**: 전역/폴더별 vault 경로 명시적 지정 지원
- Gemini CLI / Codex 대응 agent 안내 파일 (`demo_vault`)

### 인프라
- GitHub Actions CI 워크플로우 추가 (Rust build + test)
- v0.2 설계 문서 작성 (`DESIGN.md`, MCP 서버 통합 원칙)
- v0.2 milestone 문서 `done/`으로 이동

---

## [0.1.0] — 2026-04-07

### 초기 구현

**CLI 커맨드 (Phase 0~8 완전 구현)**

| 커맨드 | 설명 |
|---|---|
| `elf init` | vault 초기화 (`--dry-run`, `CLAUDE.md` 자동 생성, `git add -f`) |
| `elf entry new` | entry 생성 (slug 충돌 멱등성, `--baseline`, stdin 지원) |
| `elf entry show` | manifest + note 출력 (`--json`: 본문만) |
| `elf entry edit` | `$EDITOR` 호출 + frontmatter → manifest 역반영 |
| `elf revision add` | revision 추가 (`--delta` 또는 stdin 파이프) |
| `elf link` | 양방향 링크 (원자적 쓰기, 정렬 유지) |
| `elf validate` | 7단계 검사 (Naming / Schema / Consistency / Dangling / Cycle / Orphan / Asset) |

**테스트 구조**
- 단위 테스트: `src/cli/tests.rs`, `src/vault/tests.rs`, `src/schema/tests.rs`
- 통합 테스트: `tests/integration.rs` (45 tests, all pass)
