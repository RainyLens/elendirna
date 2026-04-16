# Changelog

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
