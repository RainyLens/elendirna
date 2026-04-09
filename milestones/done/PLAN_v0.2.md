# v0.2 구현 계획

> 설계 기준: [ROADMAP.md](ROADMAP.md) §v0.2 · [DESIGN.md](../DESIGN.md)
> 전제 조건: v0.1 성공 기준 달성 (✅ 완료)

---

## 목표

v0.1이 "데이터를 안전하게 쌓는" 도구라면, v0.2는 "쌓인 데이터를 AI가 활용하는" 도구다.

두 축으로 진행한다:

- **탐색 레이어**: `entry list`, `bundle`, `graph`, `query`, `revision list`
- **MCP 서버**: `elf serve --mcp` — AI가 vault를 직접 tool로 호출

이 두 축은 독립적으로 진행 가능하나, `query` tool은 sqlite 인덱스를 공유한다.

---

## 구현 원칙

- **CLI와 MCP는 동일한 코어를 공유한다.** `src/mcp/`는 `src/vault/`를 직접 호출한다. CLI 핸들러를 거치지 않는다.
- **sqlite는 파생 캐시다.** 언제나 `elf validate`로 재생성 가능해야 한다. sqlite 없이도 나머지 기능이 동작해야 한다.
- **stdio transport 먼저, SSE는 나중.** Claude Desktop에서 검증 후 SSE 추가.
- **각 Phase는 `cargo test`가 통과해야 다음으로 넘어간다.**

---

## 의존 관계 그래프

```
Phase 0 (lib 분리 + tokio 도입)
    ↓
Phase 1 (entry list + revision list)     ← 탐색 레이어 시작
    ↓
Phase 2 (bundle)
    ↓
Phase 3 (sqlite 인덱스 도입)
    ↓
Phase 4 (query)
    ↓
Phase 5 (graph)                          ← sqlite 위에서 동작
    |
    | (Phase 0과 병렬로 시작 가능)
    ↓
Phase 6 (MCP 서버 — stdio transport)
    ↓
Phase 7 (sync record + agent 필드 공식화)
    ↓
Phase 8 (통합 테스트 · 성공 기준 검증)
```

---

## Phase 0 — lib 분리 + 의존성 추가

**목표:** CLI와 MCP 서버가 공유할 수 있는 코어 레이어 확보.

### 현황

`src/lib.rs`는 이미 존재하고 `src/vault/`, `src/schema/` 등을 pub으로 노출한다.
다만 `src/cli/*.rs`의 핸들러 함수들은 출력 로직과 vault 조작이 혼재한다.

### 할 일

- [ ] `src/cli/entry.rs`의 `run_new`, `run_show` 등에서 vault 조작 부분을 별도 함수로 추출.
  - 예: `entry_new(vault_root, title, baseline, tags) -> Result<Entry, ElfError>`
  - CLI 핸들러는 이 함수를 호출하고 출력만 담당.
  - MCP tool도 동일 함수를 호출하고 JSON 반환.
- [ ] `Cargo.toml`에 의존성 추가:
  ```toml
  rmcp     = { version = "0.5", features = ["server", "macros", "schemars", "transport-io"] }
  tokio    = { version = "1", features = ["full"] }
  schemars = "0.8"
  ```
- [ ] `src/main.rs`에 `Serve` 서브커맨드 추가 (구현은 Phase 6).

---

## Phase 1 — `elf entry list` + `elf revision list`

**목표:** ID를 몰라도 vault 탐색이 가능한 최소 진입점 확보.

### `elf entry list`

```
elf entry list [--tag <TAG>] [--status <STATUS>] [--baseline <ID>]
```

- `entries/*/manifest.toml` 전체 스캔 → 필터링 → 정렬 출력
- `--json` 시 배열 출력

```
N0001  oauth2 login integration   [stable]  2026-04-01
N0003  jwt 토큰 보안 취약점         [draft]   2026-04-05
```

### `elf revision list <id>`

```
elf revision list N0042
```

- `revisions/N0042/r*.md` 스캔 → frontmatter 파싱 → 시간순 출력
- `--json` 시 배열 출력

---

## Phase 2 — `elf bundle <id>`

**목표:** AI가 하나의 entry와 관련 컨텍스트를 한 번에 수신할 수 있는 export 단위 확보.

> OQ-5 결정사항: raw delta chain 출력. AI가 delta들을 시간 순으로 이어붙여 현재 상태를 재구성(unzip)한다.

```
elf bundle N0042 [--depth <N>]
```

### 출력 구조

```
=== BUNDLE: N0042 ===
--- manifest ---
<manifest.toml 내용>

--- note ---
<note.md 본문>

--- revisions ---
[N0042@r001]
baseline: N0042@r000
<r001.md delta 내용>

[N0042@r002]
baseline: N0042@r001
<r002.md delta 내용>

--- linked entries (depth=1) ---
<N0031 manifest 요약>
```

- `--json` 시 구조화 출력
- `--depth N`: linked entry 재귀 깊이 (기본 1)
- MCP tool `bundle`의 핵심 경로 — 세션 간 컨텍스트 복원에 사용

---

## Phase 3 — sqlite 인덱스 도입

**목표:** `query`와 `graph`의 성능 기반 확보.

### 인덱스 스키마 (`.elendirna/index.sqlite`)

```sql
CREATE TABLE entries (
    id      TEXT PRIMARY KEY,   -- "N0042"
    title   TEXT NOT NULL,
    slug    TEXT NOT NULL,
    status  TEXT NOT NULL,
    created TEXT NOT NULL,
    updated TEXT NOT NULL,
    baseline TEXT              -- nullable
);

CREATE TABLE tags (
    entry_id TEXT REFERENCES entries(id),
    tag      TEXT NOT NULL
);

CREATE TABLE links (
    from_id TEXT REFERENCES entries(id),
    to_id   TEXT REFERENCES entries(id)
);

CREATE TABLE revisions (
    entry_id TEXT REFERENCES entries(id),
    rev_id   TEXT NOT NULL,    -- "r001"
    baseline TEXT NOT NULL,
    created  TEXT NOT NULL,
    PRIMARY KEY (entry_id, rev_id)
);
```

### 관리 명령

- `elf validate`가 manifest ↔ index 일관성 점검 + 재생성 담당 (기존 역할 확장)
- `index.sqlite`는 `.gitignore`에 추가 (이미 설계됨)

### 의존성 추가

```toml
rusqlite = { version = "0.31", features = ["bundled"] }
```

---

## Phase 4 — `elf query <expr>`

**목표:** sqlite 인덱스 기반 entry 검색.

```
elf query "tag:rust AND status:draft"
elf query "title contains ownership"
elf query "baseline:N0031"
```

- 검색 표현식 파서 (단순 AND/OR/contains/tag:/status:/baseline: 지원)
- 결과는 `entry list`와 동일한 형식
- MCP tool `query`로 노출 — AI가 관련 entry를 탐색하는 주요 경로

---

## Phase 5 — `elf graph`

> 설계 문서: [cmd-graph.md](cmd-graph.md)

- sqlite 인덱스의 `links`, `entries` 테이블을 기반으로 그래프 구성
- Phase 3 이후 구현 (sqlite 의존)

---

## Phase 6 — MCP 서버 (`elf serve --mcp`)

**목표:** Claude Desktop에서 elendirna vault를 MCP tool로 직접 호출.

### 진입점

```
elf serve --mcp [--vault <path>]
```

- stdio transport (Claude Desktop 대상)
- `ELF_VAULT` 환경변수로 vault 경로 지정 가능

### 구현 위치

```
src/
└── mcp/
    └── mod.rs    # ElfMcpServer 정의, #[tool_router]
```

### Tool Surface

| MCP Tool | 대응 CLI | 선행 Phase |
|---|---|---|
| `entry_list` | `elf entry list` | Phase 1 |
| `entry_show` | `elf entry show` | v0.1 |
| `entry_new` | `elf entry new` | v0.1 |
| `revision_add` | `elf revision add` | v0.1 |
| `bundle` | `elf bundle` | Phase 2 |
| `query` | `elf query` | Phase 4 |
| `sync_record` | `elf sync record` | Phase 7 |
| `validate` | `elf validate` | v0.1 |

### 구현 패턴 (rmcp)

```rust
#[derive(Deserialize, schemars::JsonSchema)]
struct EntryShowParams { id: String }

#[tool_router]
impl ElfMcpServer {
    #[tool(name = "entry_show", description = "entry 내용과 manifest 조회")]
    fn entry_show(
        &self,
        Parameters(EntryShowParams { id }): Parameters<EntryShowParams>
    ) -> Json<serde_json::Value> {
        // vault::entry::Entry::find_by_id 직접 호출
        // cli::entry::run_show를 거치지 않음
    }
}
```

### Claude Desktop 설정

```json
{
  "mcpServers": {
    "elendirna": {
      "command": "elf",
      "args": ["serve", "--mcp"],
      "env": { "ELF_VAULT": "/path/to/vault" }
    }
  }
}
```

### memory 대체 시나리오

```
1. 세션 시작 → sync_record 조회 → 최근 N건으로 이전 컨텍스트 파악
2. 필요한 entry → bundle(id) 호출 → revision 체인 포함 전체 컨텍스트 수신
3. 탐색 필요 → query("tag:rust") 호출 → 관련 entry 목록 수신
4. 작업 완료 → sync_record 호출 → 세션 요약 vault에 기록
```

---

## Phase 7 — `elf sync record` + agent 필드 공식화

**목표:** AI 세션 간 핸드오프 로그를 정식 커맨드로 승격.

### CLI

```
elf sync record --summary <text> [--agent <name>] [--entries <id,...>]
elf sync log [--tail N] [--agent <name>]
```

### sync.jsonl 레코드 스키마

```json
{
  "ts": "2026-04-09T10:00:00Z",
  "event": "sync.record",
  "agent": "claude-sonnet-4-6",
  "summary": "N0042 revision 추가. ownership → linearity 프레임 전환.",
  "entries": ["N0042"],
  "session_id": "optional-uuid"
}
```

- `agent` 필드: `ELF_AGENT` 환경변수 또는 `--agent` 플래그
- MCP tool `sync_record`로 노출 — AI가 세션 종료 시 자동 기록

---

## Phase 8 — 통합 테스트 + 성공 기준 검증

### 성공 기준

- [ ] `elf serve --mcp` 실행 시 Claude Desktop이 tool 목록을 정상 수신
- [ ] Claude가 `bundle(N0001)` 호출 → revision 체인 포함 컨텍스트 수신
- [ ] Claude가 `query("tag:rust")` 호출 → entry 목록 반환
- [ ] `elf validate`가 sqlite index 재생성 후 0 errors 보고
- [ ] `elf serve --mcp` 없이 기존 CLI 기능 전부 동작 (MCP는 선택적 레이어)

### 테스트 구조

- `tests/mcp_integration.rs`: `elf serve --mcp` 프로세스 시작 → stdio로 JSON-RPC 메시지 송수신
- `tests/sqlite_integration.rs`: index 생성 → query → validate → 재생성 일관성 확인

---

## Open Questions (v0.2)

| # | 질문 | 상태 |
|---|------|------|
| OQ-6 | SSE transport 지원 시점 (claude.ai web 타겟) | 미결 — v0.3 예정 |
| OQ-7 | `query` 표현식 문법 범위 (full-text search 포함 여부) | 미결 |
| OQ-8 | `bundle --depth` 기본값과 토큰 크기 상한 | 미결 |
| OQ-9 | `sync.jsonl` 로테이션 정책 (무제한 append vs. 최대 N건) | 미결 |
