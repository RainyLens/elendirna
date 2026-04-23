# Elendirna

> Model-independent context restoration for AI-assisted work. Inspired by [ELF (Eli's Lab Framework)](https://github.com/ProjectEli/ELF).

![crates.io](https://img.shields.io/crates/v/elendirna.svg) [![Rust](https://github.com/RainyLens/elendirna/actions/workflows/rust.yml/badge.svg)](https://github.com/RainyLens/elendirna/actions/workflows/rust.yml)

Elendirna는 작업 맥락을 **Base-Delta** 구조로 기록해, 세션이 끊기거나 AI 모델이 바뀌어도 "왜 여기까지 왔는지"를 다시 복원할 수 있게 합니다. CLI는 vault 규칙을 강제하고, MCP 서버는 AI 에이전트가 필요한 entry, revision chain, linked context를 선택적으로 읽도록 돕습니다.

## Install

```bash
cargo install elendirna
```

## Quick start

```bash
elf init my-vault
cd my-vault
elf entry new "Rust ownership"
elf entry edit N0001
elf revision add N0001 --delta "borrow checker 관련 내용 추가"
elf link N0001 N0002
elf validate
```

## Vault layout

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
│       └── attachments/
├── assets/                  # immutable 원본 (CLI가 mutation 거부)
├── revisions/
│   └── N0042/
│       ├── r001.md          # delta-only
│       └── r002.md
├── CLAUDE.md                # Claude Code agent manifest
├── GEMINI.md                # Gemini CLI agent manifest
├── AGENTS.md                # Codex / OpenAI Agents manifest
└── README.md
```

`demo_vault/`에서 실제 사용 예시를 확인할 수 있습니다.

## Commands

| 명령 | 역할 |
|------|------|
| `elf init [path]` | vault 스캐폴드 생성 |
| `elf entry new <title>` | entry 생성 (ID 자동 채번) |
| `elf entry edit <id>` | `$EDITOR`로 note.md 편집 |
| `elf entry show <id>` | manifest + note 출력 |
| `elf revision add <id> --delta <text>` | base-delta 로깅 |
| `elf link <from> <to>` | 양방향 cross-ref 추가 |
| `elf bundle <id>` | baseline → revision 체인 export (AI 컨텍스트용) |
| `elf validate` | 스키마, dangling link, cycle 검증 |
| `elf graph [--format dot\|mermaid\|json]` | 의존 그래프 export |
| `elf query <expr>` | sqlite 인덱스 기반 검색 |
| `elf sync record --summary <text>` | AI 세션 요약 기록 |
| `elf sync log [--tail N]` | AI handoff 로그 조회 |
| `elf serve --mcp` | MCP 서버 구동 (stdio, Claude Desktop 등) |

전역 플래그: `--json`, `--dry-run`

MCP 서버 vault 경로 우선순위: `--vault` 플래그 → `ELF_VAULT` 환경변수 → CWD walk-up

## Context Policy

Elendirna는 private vault로 dogfooding하지만, raw vault 상태는 공개 저장소에 포함하지 않습니다.

의도한 작업 흐름은 다음과 같습니다.

- private vault: 날것의 작업 기억, revision, sync log, 탐색적 메모
- public repo: 검토된 코드, 문서, fixture, 릴리스 산출물
- promotion path: private vault의 통찰 → 검토된 설계 노트 / issue / patch → public repository

이 정책은 AI와 함께 만든 사고 과정을 세션과 모델을 넘어 이어가되, 검토되지 않은 작업 기억을 기본적으로 공개하지 않기 위한 경계입니다.

## Try it with AI

MCP 서버를 세팅한 뒤, AI에게 이렇게 말해보세요:

> "이 vault의 유지보수 이력을 알려줘"

```bash
# Claude Desktop / 다른 MCP 클라이언트 설정
elf serve --mcp --vault /path/to/elendirna
```

AI는 `sync_record` 로그와 revision chain을 통해 vault가 어떤 결정을 거쳐 지금 모습이 됐는지 컨텍스트를 복원합니다. 공개 저장소에는 raw project vault가 포함되지 않으므로, 실제 프로젝트 유지보수 vault를 연결하려면 별도의 private vault 경로를 사용하세요.

## License

MIT — see [LICENSE](LICENSE).

Protocol concepts (Base-Delta, SSoT, Session-Trial identifiers) are derived from
[ELF (Eli's Lab Framework)](https://github.com/ProjectEli/ELF) by Eli (projectschnee@gmail.com),
licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
