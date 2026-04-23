---
id: "N0011"
title: "PLAN_v0.1"
baseline: null
tags: []
---

# v0.1 구현 계획

> 설계 기준: [README.md](../README.md) · [ROADMAP.md](ROADMAP.md)
> 모든 Open Questions 결정 완료 — 착수 가능 상태

---

## 구현 원칙

- **데이터 먼저**: 파일 포맷과 타입 정의가 CLI보다 먼저 완성된다. CLI는 타입 위에 얹히는 얇은 레이어다.
- **Phase 순서 = 의존 관계**: 각 Phase는 이전 Phase의 산출물 위에서만 빌드된다. 순서를 건너뛰지 않는다.
- **각 Phase는 `cargo test`가 통과해야 다음으로 넘어간다.**

---

## 의존 관계 그래프

```
Phase 0 (프로젝트 셋업)
    ↓
Phase 1 (핵심 타입 · vault 추상화)
    ↓
Phase 2 (elf init)
    ↓
Phase 3 (elf entry new / show / edit)
    ↓           ↓
Phase 4      Phase 5
(elf revision) (elf link)
    ↓           ↓
Phase 6 (elf validate)
    ↓
Phase 7 (출력 polish: --json, structured error, --dry-run)
    ↓
Phase 8 (통합 테스트 · 성공 기준 검증)
```

---

## Phase 0 — 프로젝트 셋업

**목표:** 빌드 환경과 공통 인프라 확보.

### Cargo.toml

```toml
[package]
name = "elendirna"
version = "0.0.1"
edition = "2024"

[[bin]]
name = "elf"
path = "src/main.rs"

[dependencies]
clap        = { version = "4", features = ["derive"] }
toml        = "0.8"
serde       = { version = "1", features = ["derive"] }
serde_json  = "1"
serde_yaml  = "0.9"
thiserror   = "2"
anyhow      = "1"
chrono      = { version = "0.4", features = ["serde"] }
walkdir     = "2"

[dev-dependencies]
insta       = { version = "1", features = ["toml", "yaml"] }
tempfile    = "3"
assert_cmd  = "2"
```

### 디렉터리 구조 생성

```
src/
├── main.rs
├── error.rs
├── cli/
│   ├── mod.rs
│   ├── init.rs
│   ├── entry.rs
│   ├── revision.rs
│   ├── link.rs
│   └── validate.rs
├── vault/
│   ├── mod.rs       ← vault 루트 탐지
│   ├── config.rs    ← config.toml 스키마
│   ├── entry.rs     ← entries/ 읽기·쓰기
│   ├── revision.rs  ← revisions/ 읽기·쓰기
│   └── id.rs        ← EntryID / RevisionID 타입
├── schema/
│   ├── mod.rs
│   ├── manifest.rs  ← Manifest 구조체
│   └── validate.rs  ← Issue 타입, 검증 로직
└── output/
    ├── mod.rs
    ├── pretty.rs    ← 사람용 출력
    └── json.rs      ← --json 출력
```

### 태스크

- [ ] Cargo.toml 의존성 추가
- [ ] 위 디렉터리 구조로 빈 모듈 파일 생성 (`mod.rs`, `pub mod` 선언)
- [ ] `error.rs` — `ElfError` enum 뼈대 (thiserror), exit code 매핑 함수
- [ ] `cargo build` 통과 확인

---

## Phase 1 — 핵심 타입 · vault 추상화

**목표:** CLI 없이도 vault를 읽고 쓸 수 있는 Rust 타입 레이어.
**테스트:** 단위 테스트만. CLI 없음.

### `vault/id.rs`

```rust
/// N0042 형식
pub struct EntryId(u32);          // Display: "N0042"

/// r001 형식
pub struct RevisionId(u32);       // Display: "r001"

/// N0042@r001 형식 (bundle/revision의 baseline 표기)
pub struct EntryRevRef {
    entry: EntryId,
    rev: Option<RevisionId>,       // None → @r000 (초기 상태)
}
```

- `EntryId::next(entries_dir)` — `entries/` 스캔 후 최대 번호 + 1 반환
- `RevisionId::next(rev_dir)` — `revisions/<id>/` 스캔 후 최대 번호 + 1
- `EntryId::from_dir_name("N0042_rust_ownership")` — prefix 파싱
- slug 생성: `title_to_slug(title: &str) -> String` (공백→`_`, 특수문자 제거, 최대 40자)

### `schema/manifest.rs`

```rust
#[derive(Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub tags: Vec<String>,
    pub baseline: Option<String>,
    pub links: Vec<String>,
    pub sources: Vec<String>,
    pub status: EntryStatus,       // draft | stable | archived
}

#[derive(Serialize, Deserialize)]
pub struct NoteFrontmatter {
    pub id: String,
    pub title: String,
    pub baseline: Option<String>,
    pub tags: Vec<String>,
}
```

- `Manifest::read(path)` / `Manifest::write(path)`
- `NoteFrontmatter::read(note_path)` — `---\n...\n---` 파싱
- `NoteFrontmatter::write(note_path, body)` — frontmatter 교체, 본문 보존

### `vault/config.rs`

```rust
#[derive(Serialize, Deserialize)]
pub struct VaultConfig {
    pub schema_version: u32,
    pub vault_name: String,
    pub created: DateTime<Utc>,
    pub editor: String,            // 기본값: "$EDITOR"
}
```

### `vault/mod.rs` — vault 루트 탐지

```rust
pub fn find_vault_root(start: &Path) -> Result<PathBuf, ElfError>
```
- 현재 디렉터리부터 `.elendirna/config.toml` 을 찾아 상위로 walk
- 없으면 `ElfError::NotAVault`

### 태스크

- [ ] `vault/id.rs` — 타입 정의, `next()`, `from_dir_name()`, `title_to_slug()`
- [ ] `schema/manifest.rs` — `Manifest`, `NoteFrontmatter` 구조체 + read/write
- [ ] `vault/config.rs` — `VaultConfig` 구조체 + read/write
- [ ] `vault/mod.rs` — `find_vault_root()`
- [ ] 단위 테스트: slug 변환, ID 파싱, manifest 직렬화/역직렬화, frontmatter 파싱

---

## Phase 2 — `elf init`

**목표:** 첫 번째로 동작하는 커맨드. vault 디렉터리 구조 생성.

### 동작

1. 경로 확인 (기본: 현재 디렉터리)
2. `.elendirna/` 이미 존재 시 → `ElfError::AlreadyInitialized`
3. 디렉터리 트리 생성
4. `config.toml` 기록
5. `CLAUDE.md` 템플릿 생성 (소스 내 `const` 문자열)
6. `README.md` 템플릿 생성
7. `.gitignore`에 `.elendirna/index.sqlite` 추가 (없으면 생성)
8. `sync.jsonl` 첫 줄 기록: `vault.init` 이벤트

### 상수 (src/cli/init.rs)

```rust
const CLAUDE_MD: &str = r#"# Elendirna vault

이 저장소는 `elf` CLI로만 수정합니다. 직접 파일 편집 금지.
시작 시 `elf help --json`으로 명령 표면을 확인하고, 작업 종료 시 `elf sync record`로 기록하세요.
스키마/규칙 위반은 `elf validate`가 보고합니다 — 에러의 `fix` 필드를 따르면 됩니다.
아이디어 계보를 사람이 읽을 수 있게 합성할 때: `elf bundle <id>` 출력(raw delta chain)을 받아 시간 순으로 서술하세요. CLI는 압축된 체인만 냅니다 — unzip은 당신의 몫입니다.
"#;
```

### 태스크

- [ ] `cli/init.rs` — clap 서브커맨드 정의, 로직 구현
- [ ] `--dry-run` 지원: 생성될 파일 목록 출력 후 종료
- [ ] 테스트 (tempfile 사용):
  - [ ] 정상 초기화 후 파일 구조 검증
  - [ ] 중복 초기화 시 exit code 3 확인
  - [ ] `--dry-run` 시 파일 미생성 확인

---

## Phase 3 — `elf entry new / show / edit`

**목표:** entry 생성·조회·편집. vault의 핵심 CRUD.

### `elf entry new`

1. vault 루트 탐지
2. `--baseline` 지정 시 존재 확인
3. 다음 EntryID 채번 → slug 생성 → 디렉터리명 결정
4. `entries/<id>_<slug>/` 생성
5. `manifest.toml` 기록
6. `note.md` 생성 (frontmatter + 빈 본문 템플릿)
7. `attachments/` 빈 디렉터리 생성
8. `sync.jsonl` append

멱등성: 동일 title로 재호출 → 기존 entry 반환 + exit 3

### `elf entry show`

- manifest 요약 + note.md 전문 출력
- `--json`: `{ "manifest": {...}, "note": "..." }` 구조

### `elf entry edit`

- `$EDITOR` (또는 `config.toml`의 editor 필드) 로 `note.md` 열기
- 편집기 종료 후 manifest `updated` 갱신
- frontmatter ↔ manifest 일관성 경고 (불일치 시 `WARN`)

### `vault/entry.rs`

```rust
pub struct Entry {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

impl Entry {
    pub fn find_all(vault_root: &Path) -> Vec<Entry>
    pub fn find_by_id(vault_root: &Path, id: &EntryId) -> Option<Entry>
    pub fn create(vault_root, id, title, baseline, tags) -> Result<Entry>
}
```

### 태스크

- [ ] `vault/entry.rs` — `Entry` 타입, `find_all()`, `find_by_id()`, `create()`
- [ ] `cli/entry.rs` — `new`, `show`, `edit` 서브커맨드
- [ ] frontmatter 파싱: `---\n` 경계 기준 split, serde_yaml 역직렬화
- [ ] `$EDITOR` 호출: `std::process::Command`
- [ ] 테스트:
  - [ ] `entry new` → 파일 구조, manifest 내용 검증 (insta snapshot)
  - [ ] `entry new --baseline` → baseline 필드 기록 확인
  - [ ] `entry show --json` → JSON 스키마 검증
  - [ ] 존재하지 않는 baseline → exit 2 확인

---

## Phase 4 — `elf revision add`

**목표:** 아이디어 변화(delta)를 revision 파일로 기록.

### `vault/revision.rs`

```rust
pub struct Revision {
    pub entry_id: EntryId,
    pub rev_id: RevisionId,
    pub baseline: EntryRevRef,   // N0042@r000
    pub created: DateTime<Utc>,
    pub delta: String,
}

impl Revision {
    pub fn list(vault_root, entry_id) -> Vec<Revision>
    pub fn create(vault_root, entry_id, delta) -> Result<Revision>
}
```

- `RevisionId::next()`: `revisions/<id>/` 스캔. 파일 없으면 `r001`
- baseline 자동 설정: 직전 revision이 있으면 `N####@r{prev}`, 없으면 `N####@r000`

### revision 파일 포맷

```markdown
---
baseline: N0042@r000
created: 2026-04-08T09:15:00Z
---

## Delta

<delta 텍스트 그대로>
```

### 태스크

- [ ] `vault/revision.rs` — `Revision` 타입, `list()`, `create()`
- [ ] `cli/revision.rs` — `add` 서브커맨드 (`--delta` 플래그)
- [ ] manifest `updated` 자동 갱신
- [ ] 테스트:
  - [ ] 첫 revision → `r001`, baseline `N####@r000` 확인
  - [ ] 두 번째 revision → `r002`, baseline `N####@r001` 확인
  - [ ] `--delta` 빈 문자열 → exit 1 확인

---

## Phase 5 — `elf link`

**목표:** 두 entry 사이 양방향 cross-reference 생성.

### 핵심 구현 포인트

- 두 manifest 모두 원자적 업데이트: 임시 파일(`manifest.toml.tmp`) 먼저 쓰고 rename
- 이미 존재하는 링크 → no-op, exit 0
- `links` 배열은 ID 오름차순 정렬 유지

### 태스크

- [ ] `cli/link.rs` — `<from> <to>` 인자, 양쪽 manifest 업데이트 로직
- [ ] 원자적 쓰기 유틸리티 (Phase 3에서 먼저 쓰였으면 재사용)
- [ ] 테스트:
  - [ ] 링크 생성 → 양쪽 manifest `links` 필드 확인
  - [ ] 중복 링크 → no-op, exit 0
  - [ ] 자기 자신 링크 → exit 1
  - [ ] 없는 entry → exit 2

---

## Phase 6 — `elf validate`

**목표:** vault 전체 무결성 검사. 가장 복잡한 커맨드.

### `schema/validate.rs`

```rust
pub enum Severity { Error, Warning }
pub enum IssueKind { Naming, Schema, Consistency, Dangling, Cycle, Orphan, Asset }

pub struct Issue {
    pub severity: Severity,
    pub kind: IssueKind,
    pub path: PathBuf,
    pub message: String,
    pub fix: Option<AutoFix>,
}

pub enum AutoFix {
    RenameFile { from: PathBuf, to: PathBuf },
    UpdateFrontmatter { path: PathBuf, field: String, value: String },
}
```

### 검사 순서 (순서가 중요 — 앞 검사 통과 후 뒤 검사)

1. **Naming** — ID 형식, 디렉터리명, revision 파일명
2. **Schema** — manifest 필수 필드 존재, 타입
3. **Consistency** — manifest ↔ frontmatter `id`, `title`, `baseline`, `tags`
4. **Dangling** — `links`, `baseline`, `sources`, `→ see N####` 인라인 패턴
5. **Cycle** — baseline 체인 DFS 순회 (방문 집합으로 사이클 탐지)
6. **Orphan** — `revisions/<id>/` 존재하나 entry 없음
7. **Asset** — `sources` 파일이 `assets/`에 실재하는지

### `→ see` 인라인 참조 정규식

```rust
// note.md, revisions/*.md 본문에서 추출
let re = Regex::new(r"→ see\s+(N\d{4})").unwrap();
```

### `--fix` 자동 수정 대상

| 검사 | 자동 수정 가능 |
|------|--------------|
| Naming | ✅ 파일/디렉터리 rename |
| Consistency | ✅ frontmatter를 manifest 값으로 덮어쓰기 |
| 나머지 | ❌ 사용자 개입 필요 |

### 태스크

- [ ] `schema/validate.rs` — `Issue`, `AutoFix` 타입
- [ ] 검사 1~7 구현 (각각 별도 함수)
- [ ] Cycle 탐지: DFS + `HashSet<String>` visited
- [ ] `→ see` 스캔: `regex` crate (Cargo.toml 추가)
- [ ] `--fix` 자동 수정 실행
- [ ] `cli/validate.rs` — 결과 집계, 출력, exit code
- [ ] 테스트 (insta snapshot):
  - [ ] 정상 vault → `✓ All checks passed`
  - [ ] 각 검사별 위반 케이스 1개씩
  - [ ] Cycle: A→B→A 케이스
  - [ ] `--fix` 후 재검사 통과

---

## Phase 7 — 출력 polish

**목표:** `--json`, structured error, `--dry-run` 전체 커맨드에 일관 적용.

### `--json` 출력 구조 (모든 커맨드 공통)

```json
{
  "command": "entry.new",
  "ok": true,
  "data": { ... }
}
```

### structured error (stderr)

```json
{
  "error": "not_found",
  "code": "E2002",
  "message": "entry \"N0099\" not found",
  "hint": "Run `elf entry new` to create an entry",
  "fix": null
}
```

exit code 매핑:

| 코드 | 의미 |
|------|------|
| 0 | success |
| 1 | validation error |
| 2 | not found |
| 3 | conflict (already exists, cycle 등) |
| 4 | I/O error |
| 5 | schema version mismatch |

### 태스크

- [ ] `output/json.rs` — 성공/에러 직렬화 헬퍼
- [ ] `output/pretty.rs` — 터미널 출력 포맷터
- [ ] 전역 `--json` 플래그 (clap `AppSettings` 또는 상위 커맨드 인자)
- [ ] 모든 커맨드에서 `--json` 분기 처리
- [ ] `--dry-run`: `init`, `entry new`, `link`에 적용

---

## Phase 8 — 통합 테스트 · 성공 기준 검증

**목표:** 전체 워크플로를 처음부터 끝까지 실행. v0.1 성공 기준 체크.

### 통합 테스트 시나리오 (SCENARIO.md 기반)

```rust
// tests/integration.rs
#[test]
fn scenario_3day() {
    let dir = tempdir().unwrap();

    // Day 1
    elf(&dir, &["init"]);
    elf(&dir, &["entry", "new", "벡터 검색이 지식 검색의 답이다"]);
    // → N0001 생성 확인

    // Day 2
    elf(&dir, &["revision", "add", "N0001", "--delta", "가정 수정..."]);
    // → revisions/N0001/r001.md 확인

    // Day 3
    elf(&dir, &["entry", "new", "그래프 탐색", "--baseline", "N0001"]);
    elf(&dir, &["link", "N0001", "N0002"]);
    elf(&dir, &["validate"]);
    // → exit 0, "✓ All checks passed"
}
```

### 성공 기준 체크리스트

- [ ] `elf validate`가 vault에서 0 errors 보고
- [ ] `CLAUDE.md` 4줄만으로 새 에이전트 세션에 컨텍스트 전달 가능
- [ ] `elf entry show --json` 출력이 `jq`로 파싱 가능
- [ ] 1주일간 personal note 5건을 손으로 파일 건드리지 않고 CLI만으로 작성·수정

---

## 추가 의존성 (Cargo.toml 보완)

Phase 6의 `→ see` 스캔에 `regex` crate 필요:

```toml
regex = "1"
```

---

## 파일 변경 요약

| Phase | 신규 파일 | 수정 파일 |
|-------|-----------|-----------|
| 0 | `error.rs`, 모든 `mod.rs` 뼈대 | `Cargo.toml` |
| 1 | `vault/id.rs`, `schema/manifest.rs`, `vault/config.rs` | `vault/mod.rs` |
| 2 | `cli/init.rs` | `main.rs`, `cli/mod.rs` |
| 3 | `cli/entry.rs`, `vault/entry.rs` | `main.rs` |
| 4 | `cli/revision.rs`, `vault/revision.rs` | `main.rs` |
| 5 | `cli/link.rs` | `main.rs` |
| 6 | `cli/validate.rs`, `schema/validate.rs` | `Cargo.toml` (regex 추가) |
| 7 | `output/pretty.rs`, `output/json.rs` | 모든 `cli/*.rs` |
| 8 | `tests/integration.rs` | — |
