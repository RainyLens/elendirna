# Elendirna 개발 로드맵

> Source of truth: [README.md](../README.md) §10 (MVP 범위), §11 (Open Questions)
> 최종 업데이트: 2026-04-07 (OQ-5 결정 반영)

---

## 개발 철학

- **Protocol-first**: 데이터 포맷과 스키마를 구현보다 먼저 확정한다. 코드는 교체돼도 데이터는 살아남아야 한다.
- **Use it to build it**: v0.1부터 Elendirna vault를 사용해 Elendirna 자체의 설계 변화를 기록한다.
- **작은 성공 기준**: 각 milestone의 성공 기준은 툴이 아닌 **실제 사용 행동**으로 정의한다.

---

## v0.1 — MVP (착수 전 결정 사항 포함)

### 착수 전 결정 필요 (Open Questions, README §11)

모든 Open Questions 결정 완료. v0.1 착수 가능.

| # | 질문 | 결정 | 비고 |
|---|------|------|------|
| ~~OQ-1~~ | ~~`revisions/`의 위치~~ | ✅ **최상위 별도 디렉터리** | cross-entry 스캔 용이 |
| ~~OQ-2~~ | ~~ID 채번 전략~~ | ✅ **단순 증가 `N0001`** | 날짜는 `manifest.toml`의 `created` 필드에 분리 기록 |
| ~~OQ-3~~ | ~~`assets/` immutability~~ | ✅ **CLI 거부만** | 사용자 자율 허용. 확장 가능성 낮음 |
| ~~OQ-4~~ | ~~`baseline` 체인 깊이~~ | ✅ **무한 허용** | DFS 사이클 탐지. 성능 이슈 시 memoization으로 대응 |
| ~~OQ-5~~ | ~~`bundle` 출력 포맷~~ | ✅ **raw delta chain** | readable 합성은 CLAUDE.md 안내로 AI 에이전트에 위임 |

### 구현 범위

| 커맨드 | 상세 문서 | 상태 |
|--------|-----------|------|
| `elf init [path]` | [cmd-init.md](cmd-init.md) | 🔲 미착수 |
| `elf entry new` | [cmd-entry.md](cmd-entry.md) | 🔲 미착수 |
| `elf entry edit` | [cmd-entry.md](cmd-entry.md) | 🔲 미착수 |
| `elf entry show` | [cmd-entry.md](cmd-entry.md) | 🔲 미착수 |
| `elf revision add` | [cmd-revision.md](cmd-revision.md) | 🔲 미착수 |
| `elf link` | [cmd-link.md](cmd-link.md) | 🔲 미착수 |
| `elf validate` | [cmd-validate.md](cmd-validate.md) | 🔲 미착수 |
| `--json` 전역 출력 | README §6 | 🔲 미착수 |
| structured error (exit code) | README §6.2 | 🔲 미착수 |
| `CLAUDE.md` / `README.md` 자동 생성 | README §7 | 🔲 미착수 |

### 의존성 (Cargo.toml 추가 예정)

```toml
[dependencies]
clap = { version = "4", features = ["derive"] }
toml = "0.8"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"
thiserror = "1"
anyhow = "1"

[dev-dependencies]
insta = "1"         # snapshot 테스트
tempfile = "3"      # 임시 vault 생성
```

### 성공 기준

> v0.1 빌드 후 **1주일 동안 personal note 5건 이상**을 손으로 파일을 건드리지 않고 CLI만으로 작성·수정 가능.

세부 체크리스트:
- [ ] `elf validate`가 vault에서 0 errors를 보고
- [ ] `CLAUDE.md` 4줄만으로 새 에이전트 세션에 컨텍스트 전달 가능 (bundle readable 합성 안내 포함)
- [ ] `elf entry show --json` 출력이 외부 스크립트에서 파싱 가능

---

## v0.2 — 탐색 레이어 + MCP 서버

> 상세 계획: [PLAN_v0.2.md](PLAN_v0.2.md)
> **전제 조건**: v0.1 성공 기준 달성 (✅)

두 축으로 진행: **탐색 레이어**와 **MCP 서버 통합**. CLI와 MCP는 동일한 코어(`src/vault/`)를 공유.

| 커맨드 / 기능 | 상세 문서 | 비고 |
|--------|-----------|------|
| `elf entry list` | PLAN_v0.2 Phase 1 | 초기 탐색 진입점 |
| `elf revision list <id>` | PLAN_v0.2 Phase 1 | delta 체인 시간순 출력 |
| `elf bundle <id>` | PLAN_v0.2 Phase 2 | raw delta chain 출력. AI 컨텍스트 복원의 핵심 |
| sqlite 인덱스 도입 | PLAN_v0.2 Phase 3 | `index.sqlite` 파생 캐시. `elf validate`로 재생성 |
| `elf query <expr>` | PLAN_v0.2 Phase 4 | sqlite 기반 검색 |
| `elf graph` | [cmd-graph.md](cmd-graph.md) | DOT / Mermaid / JSON. Phase 3 이후 |
| `elf serve --mcp` | PLAN_v0.2 Phase 6 | stdio transport. Claude Desktop 대상 |
| `elf sync record` | PLAN_v0.2 Phase 7 | agent 필드 공식화. 세션 간 핸드오프 |

**memory 대체 시나리오**: MCP tool(`bundle`, `query`, `sync_record`)을 통해 AI가 vault를 능동적으로 탐색. 세션 시작 시 전체 주입 대신 필요한 것만 on-demand 조회.

**sqlite 도입 원칙**: 항상 `elf validate`로 재생성 가능. sqlite 없이도 MCP 기본 기능(`entry_show`, `bundle`) 동작.

---

## v0.3 — 에이전트 워크플로 완성

| 기능 | 설명 |
|------|------|
| `elf doctor` | validate + index 일관성 원스톱 점검 |
| `elf migrate --to <N>` | 스키마 버전 마이그레이션 |
| `elf help --json` | 전체 명령 표면 구조화 출력 (CLAUDE.md 대체 가능) |
| `elf entry show --bundle` | show + bundle 통합 (에이전트 one-shot 컨텍스트 로딩) |
| ADR 문서 체계 | `docs/adr/` 에 스키마 변경 기록 시작 |

---

## 구현 순서 권장

```
1. 데이터 스키마 확정 (OQ-1~4 결정)
   ↓
2. manifest.toml 파서/직렬화 (toml + serde)
   ↓
3. vault 탐지 로직 (상위 디렉터리 walk)
   ↓
4. elf init
   ↓
5. elf entry new → edit → show
   ↓
6. elf revision add
   ↓
7. elf link
   ↓
8. elf validate (검사 항목 순서: naming → schema → consistency → dangling → cycle)
   ↓
9. --json 전역 출력 + structured error 정비
   ↓
10. 통합 테스트 (insta snapshot, 실제 vault 생성/검증)
```

---

## 모듈 구조 초안

```
src/
├── main.rs               # clap 진입점, 서브커맨드 dispatch
├── cli/
│   ├── init.rs
│   ├── entry.rs
│   ├── revision.rs
│   ├── link.rs
│   └── validate.rs
├── vault/
│   ├── mod.rs            # vault 루트 탐지, config 로드
│   ├── config.rs         # config.toml 스키마
│   ├── entry.rs          # manifest.toml + note.md 읽기/쓰기
│   ├── revision.rs       # revisions/ 읽기/쓰기
│   └── id.rs             # EntryID / RevisionID 채번
├── schema/
│   ├── manifest.rs       # Manifest 구조체 + serde
│   └── validate.rs       # 검증 로직, Issue 타입
├── output/
│   ├── pretty.rs         # 사람용 출력 포맷터
│   └── json.rs           # --json 출력 포맷터
└── error.rs              # thiserror 에러 타입 + exit code 매핑
```
