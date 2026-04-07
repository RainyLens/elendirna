# Elendirna

> Agent-friendly knowledge base CLI. Inspired by [ELF (Eli's Lab Framework)](https://github.com/ProjectEli/ELF).
![crates.io](https://img.shields.io/crates/v/elendirna.svg)
개인 지식을 **Base-Delta** 구조로 보존합니다. AI 에이전트가 컨벤션을 기억하지 않아도 되도록, CLI가 규칙을 강제합니다.

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
├── CLAUDE.md                # agent manifest (elf init이 생성)
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
| `elf graph [--format dot\|json]` | 의존 그래프 export |
| `elf query <expr>` | sqlite 인덱스 기반 검색 |
| `elf sync log [--tail N]` | AI handoff 로그 조회 |

전역 플래그: `--json`, `--dry-run`, `--vault <path>`

## License

MIT — see [LICENSE](LICENSE).

Protocol concepts (Base-Delta, SSoT, Session-Trial identifiers) are derived from
[ELF (Eli's Lab Framework)](https://github.com/ProjectEli/ELF) by Eli (projectschnee@gmail.com),
licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
