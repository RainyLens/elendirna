# Elendirna — 기본 설계 문서 v0

> Agent-friendly knowledge base CLI, inspired by ELF (Eli's Lab Framework).

**Status:** Draft · Last updated: 2026-04-07

---

## 0. 문서의 목적

Elendirna는 ELF의 철학적 코어를 계승하되, **개인 지식 보존 + AI 워크플로 자동화**를 1차 목적으로 재구성한 별개 구현체이다. 본 문서는 v0.1 MVP 착수 전 설계 합의를 위한 기준 문서이며, 구현이 진행됨에 따라 ADR(Architecture Decision Record)을 통해 업데이트된다.

---

## 1. 동기와 목표

### 1.1 배경

ELF는 R&D 워크플로를 위한 폴더 컨벤션 + 마크다운 로깅 표준이다. 강력한 통찰(Base-Delta, Session-Trial 식별자 분리, SSoT, Protocol-first)을 담고 있으나, 강제력이 전적으로 사람과 AI의 자발적 준수에 의존한다. AI 에이전트에게 작업을 위임할 때 이는 다음 문제로 이어진다:

1. **토큰 낭비** — 컨벤션 전체를 시스템 프롬프트로 매번 주입해야 함
2. **포맷팅 할루시네이션** — LLM이 규칙을 부분적으로 잊거나 변형
3. **이식성 부재** — 특정 LLM/에이전트 런타임에 종속

### 1.2 해결 전략

ELF의 컨벤션을 CLI 도구의 코드 경계 안으로 캡슐화한다. LLM은 규칙을 기억할 필요 없이 도구를 호출하면 되고, 잘못된 입력은 결정론적으로 거부된다.

### 1.3 측정 가능한 목표

- 동등한 entry 1건 처리 시 시스템 프롬프트 토큰 ≥70% 감소 (CLAUDE.md 라인 수 대비)
- `elf validate`가 보고하는 스키마 위반: 출시 후 vault에서 0건 유지
- 셸 실행 환경을 가진 모든 에이전트 런타임에서 동일하게 동작 (Claude Code, Cursor, Aider, 직접 호출)

### 1.4 비목표

- 다중 사용자 / 동시 편집 / 서버 동기화
- 셸이 없는 에이전트 환경 (API-only 챗봇 등)
- 일반 노트 앱 대체 (Obsidian/Notion 경쟁 의도 없음)

---

## 2. 철학 (ELF에서 계승)

| 원칙 | 의미 |
|------|------|
| **Base-Delta** | 매 entry/revision은 baseline 참조 + 변경분만 기록. 반복 기재 회피 |
| **Identifier/Attribute 분리** | 파일명에는 식별자만 (`N0042_*`). 메타데이터는 manifest에 |
| **Single Source of Truth** | 한 vault에 노트, 자료, 그래프, 메타데이터 모두 공존 |
| **Protocol-first** | CLI는 스키마의 enforcer일 뿐. 데이터는 도구 없이도 사람이 읽을 수 있어야 함 |

마지막 원칙의 따름정리: **도구가 사라져도 데이터는 살아남는다.** 5년 후 다른 도구로 마이그레이션할 때 vault 디렉터리를 그대로 들고 갈 수 있어야 한다.

---

## 3. 디렉터리 레이아웃

```
vault/
├── .elendirna/
│   ├── config.toml          # vault 설정, schema_version
│   ├── index.sqlite         # 파생 인덱스 (재생성 가능, .gitignore)
│   └── sync.jsonl           # AI handoff append-only log
├── entries/
│   └── N0042_rust_ownership/
│       ├── manifest.toml    # 구조화 메타데이터
│       ├── note.md          # 사람용 narrative (frontmatter 포함)
│       └── attachments/     # entry-local 자료
├── assets/                  # immutable 원본 (CLI가 mutation 거부)
│   ├── pdf/
│   ├── img/
│   └── web/
├── revisions/
│   └── N0042/
│       ├── r001.md          # delta-only
│       └── r002.md
├── CLAUDE.md                # 3-line agent manifest
└── README.md                # 사람용 vault 구조 설명 (init이 자동 생성)
```

**설계 노트**

- 번호 prefix(`N0042_`)는 `entries/` 내부에만 한정. 카테고리 분류는 manifest의 `tags` 필드로 일임 — 폴더 재구성 비용 회피
- `assets/`는 원본 자산(PDF, 이미지, 웹 캡처) 전용. CLI가 mutation을 거부함으로써 immutability 강제
- `revisions/`는 entry별 delta 체인. 별도 디렉터리 유지 이유는 검토 포인트 §11 참조
- `index.sqlite`는 manifest 합집합에서 재생성 가능한 파생물. 사용자는 존재를 의식할 필요 없음

---

## 4. 데이터 스키마

### 4.1 `manifest.toml`

```toml
schema_version = 1
id = "N0042"
title = "Rust ownership"
created = 2026-04-07T14:30:00Z
updated = 2026-04-07T14:30:00Z
tags = ["rust", "language-design"]
baseline = "N0031"              # base-delta 체인의 부모 (optional)
links = ["N0019", "N0033"]      # 양방향 cross-reference
sources = ["assets/pdf/rust_book_ch4.pdf"]
status = "draft"                # draft | stable | archived
```

### 4.2 `note.md` frontmatter

`manifest.toml`의 핵심 필드를 YAML frontmatter로 중복하여 사람과 도구 양쪽 가독성을 확보:

```markdown
---
id: N0042
title: Rust ownership
baseline: N0031
tags: [rust, language-design]
---

# Rust ownership

본문은 평범한 마크다운...
```

frontmatter와 manifest.toml 사이의 일관성은 `elf validate`가 검증한다. **manifest가 single source of truth이며, frontmatter는 derived view.**

### 4.3 `revisions/N####/r###.md`

```markdown
---
baseline: N0042@r000
created: 2026-04-08T09:15:00Z
---

## Delta

이전 버전에서 ownership과 borrow의 구분을 추가...
```

단독으로 열어도 "이건 N0042의 r000에서 갈라진 변경분"임이 즉시 보인다.

### 4.4 `config.toml`

```toml
schema_version = 1
vault_name = "personal"
created = 2026-04-07T14:00:00Z
editor = "$EDITOR"  # entry edit이 호출할 에디터
```

### 4.5 `sync.jsonl`

Append-only JSONL. 한 줄 = 한 에이전트 액션:

```jsonl
{"ts": "2026-04-07T14:35:12Z", "agent": "claude-code", "action": "entry.new", "id": "N0042", "session": "abc123"}
{"ts": "2026-04-07T14:38:01Z", "agent": "claude-code", "action": "revision.add", "id": "N0042", "rev": "r001"}
```

사람이 직접 읽기는 피곤하므로 `elf sync log [--tail N]`이 렌더러 역할.

---

## 5. 명령 표면

### 5.1 명령 목록

| 명령 | 역할 |
|------|------|
| `elf init [path]` | vault 스캐폴드 생성 (`.elendirna/`, `CLAUDE.md`, `README.md`) |
| `elf entry new <title> [--baseline N####] [--tags ...]` | ID 자동 채번, manifest + note 생성 |
| `elf entry edit <id>` | `$EDITOR` 호출, 종료 시 `updated` 자동 갱신 |
| `elf entry show <id>` | 단일 entry의 manifest + note 출력 |
| `elf revision add <id> --delta <text>` | base-delta 로깅 |
| `elf link <from> <to>` | 양방향 cross-ref 추가, 양쪽 manifest 갱신 |
| `elf bundle <id>` | baseline → revision 체인을 시간 순 raw delta chain으로 export (AI 컨텍스트 주입용) |
| `elf validate` | 스키마, dangling link, baseline cycle, 네이밍 검증 |
| `elf graph [--format dot\|json]` | 의존 그래프 export |
| `elf query <expr>` | sqlite 인덱스 기반 검색 (`tags:rust AND baseline:N0031`) |
| `elf sync log [--tail N]` | AI handoff 로그 조회 |
| `elf sync record <action>` | 에이전트가 작업 완료 시 호출 |
| `elf doctor` | 빠른 상태 점검 (validate + index 일관성) |
| `elf migrate --to <N>` | 스키마 버전 마이그레이션 |

### 5.2 전역 플래그

- `--json` — 구조화 출력 (모든 명령에서 동작)
- `--dry-run` — mutating 명령에서 실제 변경 없이 결과 미리보기
- `--vault <path>` — 명시적 vault 경로 지정 (기본값: 상위 디렉터리 탐색)

---

## 6. Agent I/O Contract

CLI의 1차 사용자가 AI 에이전트라는 전제 하에 다음을 1급 요구사항으로 둔다.

### 6.1 출력 모드

- 기본: 사람용 pretty print
- `--json`: 구조화 출력. 스키마는 `elf help --json`으로 조회 가능
- 출력 스키마는 `schema_version`과 함께 마이너 버전에서도 호환성 보장

### 6.2 에러 형식

stderr에 다음 구조로 출력:

```json
{
  "error": "naming_violation",
  "code": "E1001",
  "message": "Invalid entry name",
  "hint": "Entry names must match N\\d{4}_[a-z_]+",
  "fix": "Suggested name: N0042_rust_ownership"
}
```

Exit code는 카테고리 단위:

- `0` — success
- `1` — validation error
- `2` — not found
- `3` — conflict (already exists, baseline cycle 등)
- `4` — I/O error
- `5` — schema version mismatch

### 6.3 Idempotency

모든 mutating 명령은 멱등성을 보장한다:

- `entry new`를 동일 인자로 두 번 호출 → 두 번째는 기존 entry 반환 + `code: "already_exists"` (exit 3)
- `link`를 이미 존재하는 쌍에 호출 → no-op

### 6.4 Dry-run

모든 mutating 명령에서 `--dry-run` 동작. JSON으로 "이 명령을 실행하면 어떤 파일이 어떻게 변할지" 미리 표시.

### 6.5 Help discovery

`elf help --json`이 단일 호출로 전체 명령 표면(서브커맨드, 플래그, 입출력 스키마)을 반환. CLAUDE.md에 명령 카탈로그를 적어둘 필요가 없어진다.

---

## 7. CLAUDE.md (agent manifest)

`elf init`이 vault 루트에 자동 생성:

```markdown
# Elendirna vault

이 저장소는 `elf` CLI로만 수정합니다. 직접 파일 편집 금지.
시작 시 `elf help --json`으로 명령 표면을 확인하고, 작업 종료 시 `elf sync record`로 기록하세요.
스키마/규칙 위반은 `elf validate`가 보고합니다 — 에러의 `fix` 필드를 따르면 됩니다.
아이디어 계보를 사람이 읽을 수 있게 합성할 때: `elf bundle <id>` 출력(raw delta chain)을 받아 시간 순으로 서술하세요. CLI는 압축된 체인만 냅니다 — unzip은 당신의 몫입니다.
```

이 네 줄이 이전 버전 ELF의 수백 줄 컨벤션 문서를 대체한다. 토큰 절감 목표(§1.3)의 핵심.

**설계 원칙:** `bundle`의 readable 합성은 CLI의 책임이 아니다. CLI는 항상 raw delta chain만 출력하고, "unzip"(서술형 재구성)은 받아서 처리하는 AI 에이전트의 언어 능력에 위임한다. vault에는 AI가 생성한 텍스트가 저장되지 않는다 — Protocol-first 원칙 유지.

---

## 8. Human-readability 보증

| 파일 | 가독성 | 비고 |
|------|--------|------|
| `note.md` | ✅ 완전 | YAML frontmatter는 GitHub/Obsidian이 렌더링하거나 무시 |
| `manifest.toml` | ✅ 완전 | TOML 선택의 핵심 이유. 주석 지원 |
| `config.toml` | ✅ 완전 | 동일 |
| `revisions/*.md` | ✅ 완전 | frontmatter의 `baseline` 필드로 단독 가독성 보장 |
| `assets/*` | ✅ 완전 | 원본 그대로 (PDF/이미지/HTML) |
| `sync.jsonl` | ⚠️ 부분 | 텍스트지만 packed. CLI 렌더러(`elf sync log`)로 보완 |
| `index.sqlite` | ❌ 바이너리 | 파생물. `.gitignore`에 포함, 사용자 의식 불필요 |
| `README.md` | ✅ 완전 | `elf init`이 자동 생성. 5년 후의 본인을 위한 지도 |

결론: `index.sqlite`(파생물)와 `sync.jsonl`(렌더러로 보완) 외 vault 전체가 CLI 없이도 읽히는 상태. Protocol-first 철학이 그대로 보존된다.

---

## 9. 스키마 버저닝

- `config.toml`의 `schema_version`이 절대 기준
- 마이너 변경은 backward-compatible field 추가만 허용
- 메이저 변경은 `elf migrate --to N`으로 마이그레이션. 항상 `revisions/.backup-vN/` 생성
- 스키마 변화는 `docs/adr/`에 ADR로 기록
- CLI 메이저 버전과 schema_version은 정렬 (CLI v2.x ↔ schema v2)

---

## 10. v0.1 MVP 범위

### 포함

- `init`, `entry new`, `entry edit`, `entry show`
- `revision add`
- `link`
- `validate`
- `--json` 출력 + structured error
- `CLAUDE.md` / `README.md` 자동 생성
- `manifest.toml` ↔ `note.md` frontmatter 일관성 검증

### 미포함 (v0.2 이후)

- `bundle`, `query`, `graph`
- `sync log` / `sync record` (JSONL 자체는 v0.1부터 기록)
- `doctor`, `migrate`
- sqlite 인덱스 (v0.1은 manifest 직접 스캔)

### 성공 기준

본인이 v0.1 빌드 후 1주일 동안 personal note 5건 이상을 손으로 파일을 건드리지 않고 CLI만으로 작성/수정 가능.

---

## 11. 검토 포인트 (Open Questions)

다음 항목은 v0.1 착수 전 결정 필요:

모든 Open Questions가 결정됐습니다.

1. ~~**`bundle` 명령의 출력 포맷**~~ ✅ **결정 (2026-04-07)**
   - **결정:** raw 마크다운 delta chain. CLI는 항상 압축된 체인만 출력.
   - **근거:** "unzip"(readable 합성)은 표현 합성 문제이므로 CLI가 아닌 AI 에이전트의 언어 능력에 위임한다. CLAUDE.md에 안내 패턴을 포함하여 skill로 처리. vault에 AI 생성 텍스트가 저장되지 않으므로 Protocol-first 원칙 유지.
   - **영향:** `--readable` 플래그 불필요. JSON envelope 불필요. CLI에 AI API 의존성 없음.

2. ~~**`revisions/`의 위치**~~ ✅ **결정 (2026-04-07)**
   - **결정:** A — 최상위 별도 디렉터리 (`revisions/`).
   - **근거:** cross-entry revision view가 용이하다. entry 간 delta 체인 비교 및 `elf bundle`의 체인 수집이 단일 경로 스캔으로 가능.

3. ~~**`assets/` immutability 강제 메커니즘**~~ ✅ **결정 (2026-04-07)**
   - **결정:** A — CLI 거부만. 파일시스템 권한 변경 없음.
   - **근거:** v0.1 단순성 우선. 외부 도구 우회는 사용자 자율에 맡긴다. 향후 필요 시 체크섬 매니페스트로 확장 가능하나 가능성 낮음.

4. ~~**`baseline` 체인의 깊이 제한**~~ ✅ **결정 (2026-04-07)**
   - **결정:** 무한 허용. 사이클 탐지(DFS)로 충분.
   - **근거:** 아이디어 계보는 깊이를 인위적으로 제한할 이유가 없다. 성능 문제가 실제로 발생하면 DFS에 memoization을 추가하는 방향으로 대응.

5. ~~**ID 채번 전략**~~ ✅ **결정 (2026-04-07)**
   - **결정:** 단순 증가 (`N0001`, `N0002`, ...). 날짜 정보는 ID에 포함하지 않고 `manifest.toml`의 `created` 필드에 별도 기록.
   - **근거:** ID는 식별자일 뿐 — 시간 정보를 ID에 내장하면 재정렬·이관 시 혼란. 날짜는 메타데이터에 위임하면 두 역할이 깔끔히 분리된다.

---

## 12. 구현 스택 결정

- **언어**: Rust (2024 edition 이상)
- **CLI 파싱**: clap (derive)
- **TOML**: toml
- **YAML frontmatter**: serde_yaml + 마크다운 본문 분리
- **JSON 출력**: serde_json
- **에러**: thiserror (라이브러리 경계) + anyhow (애플리케이션 경계)
- **sqlite (v0.2+)**: rusqlite
- **테스트**: cargo test + insta (snapshot)

**패키지 구성**

```toml
[package]
name = "elendirna"
version = "0.0.1"
edition = "2024"

[[bin]]
name = "elf"
path = "src/main.rs"
```

`cargo install elendirna` 시 시스템에 설치되는 실행 파일은 `elf`. 본가 ELF 프로젝트에 대한 호환성 표시이자 사용자 손가락의 편의.

---

## 13. 라이선스 및 크레딧

- 본 프로젝트는 **[ELF (Eli's Lab Framework)](https://github.com/ProjectEli/ELF)** (by Eli, projectschnee@gmail.com)의 프로토콜 아이디어에서 영감을 받음
- ELF의 프로토콜 부분은 CC BY 4.0이며, 본 프로젝트는 그 조건 하에 파생됨을 명시
- Elendirna 자체의 라이선스는 MIT (LICENSE 파일 참조)

---

## 부록 A. 용어집

- **vault** — Elendirna가 관리하는 단일 저장소 루트
- **entry** — 하나의 지식 단위. 디렉터리 + manifest + note로 구성
- **revision** — entry의 base-delta 체인 상의 변경분
- **baseline** — revision/entry가 파생된 부모 참조
- **bundle** — LLM 컨텍스트 주입용 export 단위 (entry + 자손 + 자산)
- **manifest** — entry의 구조화 메타데이터 (`manifest.toml`)
- **handoff** — 에이전트 세션 간 작업 이어붙이기 (`sync.jsonl`)

---

## 부록 B. ELF로부터의 변경점 요약

| ELF | Elendirna | 변경 이유 |
|-----|-----------|-----------|
| 폴더 컨벤션 + .bat | CLI 도구 | 강제력 확보, 토큰 절감 |
| Session/Trial 어휘 | Entry/Revision 어휘 | personal note 도메인 적합성 |
| `0_~6_` 번호 폴더 | 평평한 카테고리 + tags | 재구성 비용 회피 |
| 마크다운 컨벤션 문서 | manifest.toml + 검증기 | 기계가독성 |
| AI_Sync.md (역연대순 마크다운) | sync.jsonl + CLI 렌더러 | 동시성, 파싱 안정성 |
| 프롬프트 기반 규칙 | CLAUDE.md 3줄 + `elf help --json` | 토큰 절감, 동기화 부담 제거 |
| 19개 README 영어 + 한국어 | — | 유지보수 부채 회피 |
