---
id: "N0003"
title: "COMMANDS"
baseline: null
tags: []
---

# Elendirna CLI — 커맨드 개요

> 이 문서는 진입점 인덱스입니다. 각 커맨드의 상세 설계는 링크된 문서를 참조하세요.
> **Source of truth: [README.md](../README.md)**

---

## 바이너리

`cargo install elendirna`으로 설치되는 실행 파일은 `elf`.

```sh
elf <command> [flags]
```

---

## 커맨드 목록

### v0.1 MVP

| 커맨드 | 설명 | 상세 문서 |
|--------|------|-----------|
| `elf init [path]` | vault 스캐폴드 생성 | [cmd-init.md](cmd-init.md) |
| `elf entry new <title> [--baseline N####] [--tags ...]` | entry 생성, ID 자동 채번 | [cmd-entry.md](cmd-entry.md) |
| `elf entry edit <id>` | `$EDITOR`로 note.md 편집 | [cmd-entry.md](cmd-entry.md) |
| `elf entry show <id>` | entry manifest + note 출력 | [cmd-entry.md](cmd-entry.md) |
| `elf revision add <id> --delta <text>` | 아이디어 변화 기록 | [cmd-revision.md](cmd-revision.md) |
| `elf link <from> <to>` | 양방향 cross-reference 생성 | [cmd-link.md](cmd-link.md) |
| `elf validate [--fix]` | 스키마·참조·일관성 검증 | [cmd-validate.md](cmd-validate.md) |

### v0.2 이후

| 커맨드 | 설명 | 상세 문서 |
|--------|------|-----------|
| `elf graph [--format dot\|json\|mermaid]` | 아이디어 계보 그래프 export | [cmd-graph.md](cmd-graph.md) |
| `elf bundle <id>` | LLM 컨텍스트 주입용 export | _(설계 예정)_ |
| `elf query <expr>` | sqlite 인덱스 기반 검색 | _(설계 예정)_ |
| `elf sync log [--tail N]` | AI handoff 로그 조회 | _(설계 예정)_ |
| `elf sync record <action>` | 에이전트 작업 완료 기록 | _(설계 예정)_ |
| `elf doctor` | validate + index 일관성 점검 | _(설계 예정)_ |
| `elf migrate --to <N>` | 스키마 버전 마이그레이션 | _(설계 예정)_ |

---

## 전역 플래그 (모든 커맨드 공통)

| 플래그 | 설명 |
|--------|------|
| `--json` | 구조화 JSON 출력 (AI 에이전트 연동용) |
| `--dry-run` | mutating 커맨드에서 실제 변경 없이 결과 미리보기 |
| `--vault <path>` | 명시적 vault 경로 지정 (기본: 상위 디렉터리 탐색) |

---

## 빠른 워크플로 예시

```sh
# 1. vault 초기화
elf init ~/notes/my-vault && cd ~/notes/my-vault

# 2. 첫 아이디어 entry 생성
elf entry new "벡터 검색의 한계"

# 3. 파생 아이디어 생성
elf entry new "그래프 기반 대안 탐색" --baseline N0001 --tags knowledge,graph

# 4. 아이디어 변화 기록 (수치가 아닌 생각의 전환)
elf revision add N0002 --delta "지역성이 전역 맥락보다 더 중요하다는 가정을 수정. \
  벡터와 그래프의 하이브리드 접근이 필요할 수 있음."

# 5. 관련 entry 연결
elf link N0001 N0002

# 6. 무결성 검사
elf validate
```

---

## 개발 계획

→ [ROADMAP.md](ROADMAP.md)
