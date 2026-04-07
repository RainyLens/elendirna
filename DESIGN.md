# Elendirna — 설계 문서

> 동기, 철학, 아키텍처 결정을 기록합니다. 구현 로드맵은 [milestones/ROADMAP.md](milestones/ROADMAP.md)를 참조하세요.

---

## 1. 동기

### 1.1 배경

[ELF (Eli's Lab Framework)](https://github.com/ProjectEli/ELF)는 R&D 워크플로를 위한 폴더 컨벤션 + 마크다운 로깅 표준이다. 강력한 통찰(Base-Delta, Session-Trial 식별자 분리, SSoT, Protocol-first)을 담고 있으나, 강제력이 전적으로 사람과 AI의 자발적 준수에 의존한다. AI 에이전트에게 작업을 위임할 때 이는 다음 문제로 이어진다:

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

## 3. Agent I/O Contract

CLI의 1차 사용자가 AI 에이전트라는 전제 하에 다음을 1급 요구사항으로 둔다.

### 3.1 출력 모드

- 기본: 사람용 pretty print
- `--json`: 구조화 출력. 스키마는 `elf help --json`으로 조회 가능
- 출력 스키마는 `schema_version`과 함께 마이너 버전에서도 호환성 보장

### 3.2 에러 형식

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

Exit code 카테고리:

| code | 의미 |
|------|------|
| `0` | success |
| `1` | validation error |
| `2` | not found |
| `3` | conflict (already exists, baseline cycle 등) |
| `4` | I/O error |
| `5` | schema version mismatch |

### 3.3 멱등성

모든 mutating 명령은 멱등성을 보장한다:

- `entry new`를 동일 인자로 두 번 호출 → 두 번째는 기존 entry 반환 + `code: "already_exists"` (exit 3)
- `link`를 이미 존재하는 쌍에 호출 → no-op

### 3.4 CLAUDE.md

`elf init`이 vault 루트에 자동 생성하는 agent manifest:

```markdown
# Elendirna vault

이 저장소는 `elf` CLI로만 수정합니다. 직접 파일 편집 금지.
시작 시 `elf help --json`으로 명령 표면을 확인하고, 작업 종료 시 `elf sync record`로 기록하세요.
스키마/규칙 위반은 `elf validate`가 보고합니다 — 에러의 `fix` 필드를 따르면 됩니다.
아이디어 계보를 사람이 읽을 수 있게 합성할 때: `elf bundle <id>` 출력(raw delta chain)을 받아 시간 순으로 서술하세요.
```

`bundle`의 readable 합성은 CLI의 책임이 아니다. raw delta chain 출력을 AI 에이전트의 언어 능력에 위임함으로써 vault에 AI 생성 텍스트가 저장되지 않는다 — Protocol-first 원칙 유지.

---

## 4. Human-readability 보증

| 파일 | 가독성 | 비고 |
|------|--------|------|
| `note.md` | ✅ 완전 | YAML frontmatter는 GitHub/Obsidian이 렌더링하거나 무시 |
| `manifest.toml` | ✅ 완전 | TOML 선택의 핵심 이유. 주석 지원 |
| `config.toml` | ✅ 완전 | 동일 |
| `revisions/*.md` | ✅ 완전 | frontmatter의 `baseline` 필드로 단독 가독성 보장 |
| `assets/*` | ✅ 완전 | 원본 그대로 (PDF/이미지/HTML) |
| `sync.jsonl` | ⚠️ 부분 | 텍스트지만 packed. `elf sync log`로 보완 |
| `index.sqlite` | ❌ 바이너리 | 파생물. `.gitignore`에 포함, 사용자 의식 불필요 |

---

## 5. 스키마 버저닝

- `config.toml`의 `schema_version`이 절대 기준
- 마이너 변경은 backward-compatible field 추가만 허용
- 메이저 변경은 `elf migrate --to N`으로 마이그레이션. 항상 `revisions/.backup-vN/` 생성
- 스키마 변화는 `docs/adr/`에 ADR로 기록
- CLI 메이저 버전과 schema_version은 정렬 (CLI v2.x ↔ schema v2)

---

## 6. 구현 스택

- **언어**: Rust (2024 edition)
- **CLI 파싱**: clap (derive)
- **TOML**: toml + serde
- **JSON 출력**: serde_json
- **에러**: thiserror
- **sqlite (v0.2+)**: rusqlite
- **테스트**: cargo test + insta (snapshot) + tempfile + assert_cmd

```toml
[[bin]]
name = "elf"
path = "src/main.rs"
```

`cargo install elendirna` 시 설치되는 실행 파일은 `elf`.

---

## 7. 아키텍처 결정 기록 (Open Questions → 결정)

| # | 질문 | 결정 | 근거 |
|---|------|------|------|
| OQ-1 | `revisions/`의 위치 | 최상위 별도 디렉터리 | cross-entry 스캔 용이, `elf bundle` 단일 경로 수집 |
| OQ-2 | ID 채번 전략 | 단순 증가 `N0001` | ID는 식별자일 뿐. 날짜는 `manifest.toml`의 `created`에 분리 |
| OQ-3 | `assets/` immutability | CLI 거부만 | v0.1 단순성 우선. 체크섬 매니페스트는 필요 시 확장 |
| OQ-4 | `baseline` 체인 깊이 | 무한 허용 + DFS 사이클 탐지 | 아이디어 계보를 인위적으로 제한할 이유 없음 |
| OQ-5 | `bundle` 출력 포맷 | raw delta chain | readable 합성은 AI 에이전트 위임. CLI에 AI API 의존성 없음 |

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

## 부록 B. ELF로부터의 변경점

| ELF | Elendirna | 변경 이유 |
|-----|-----------|-----------|
| 폴더 컨벤션 + .bat | CLI 도구 | 강제력 확보, 토큰 절감 |
| Session/Trial 어휘 | Entry/Revision 어휘 | personal note 도메인 적합성 |
| `0_~6_` 번호 폴더 | 평평한 번호 + tags | 재구성 비용 회피 |
| 마크다운 컨벤션 문서 | manifest.toml + 검증기 | 기계가독성 |
| AI_Sync.md (역연대순 마크다운) | sync.jsonl + CLI 렌더러 | 동시성, 파싱 안정성 |
| 프롬프트 기반 규칙 | CLAUDE.md 4줄 + `elf help --json` | 토큰 절감, 동기화 부담 제거 |
