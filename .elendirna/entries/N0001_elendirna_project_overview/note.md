---
id: "N0001"
title: "Elendirna Project Overview"
baseline: null
tags: []
---
# Elendirna Project Overview

> **Agent-friendly knowledge base CLI.**
> 사람과 AI 사이의 일관된 "공유 기억(Shared Memory)"을 보존하고 관리하는 CLI 플랫폼.

## 1. 프로젝트 목적 및 철학

Elendirna는 기존 ELF(Eli's Lab Framework)의 철학을 계승하며, AI 에이전트와 사람이 긴 세션이나 여러 프로젝트를 거쳐 쌓아 올린 통찰력과 결정들을 휘발되지 않도록 기록합니다. 기존 RAG(Retrieval-Augmented Generation)나 자유형식 마크다운 노트와 달리, **Base-Delta(단일 원본 컨텍스트와 시간 순 변경 사항) 및 CLI 강제성**을 통해 AI가 컨벤션 프롬프트를 매번 기억할 필요 없이 규칙을 지키게 유도합니다.

**지식의 저장 단위 (Vault Layout):**
- `.elendirna/`: Vault 내부 설정, index.sqlite, `sync.jsonl` (Agent Handoff log)
- `entries/`: 사람용 `note.md`와 기계용 `manifest.toml`이 결합된 개별 지식 노드
- `revisions/`: `r001.md`, `r002.md` 와 같은 시간의 흐름(Delta) 로깅 디렉터리
- `assets/`: PDF, 이미지 등 불변(Immutable) 원본 파일

## 2. 주요 아키텍처 특징

1. **에이전트 I/O Contract**
   - 명령의 출력을 `--json` 포맷으로 제공하여 에이전트 처리를 도움.
   - 오류 발생 시 명확하게 `validation error`, `conflict` 등을 나타내어, 에이전트가 `fix` 필드를 통해 자가 수정 가능하도록 디자인됨 (`code: E1001` 등).
2. **단일 진실 공급원 (SSoT) 원칙**
   - 구조화 데이터는 Vault 안에 모두 존재하며 다른 DB에 지나치게 의존하지 않습니다. 도구가 사라져도 순수 MarkDown 파일로 데이터를 알아볼 수 있습니다 (Protocol-First).
3. **Rust 기반의 고속 CLI**
   - 바이너리 하나로 배포되며 `cargo install elendirna`를 통해 시스템 어디서나 사용할 수 있음.

## 3. 기능 및 명령어

* `elf init [path]`: 새 Vault 생성
* `elf entry new <title>` / `edit <id>` / `show <id>`: 새로운 Node를 만들고 조회
* `elf revision add <id> --delta <text>`: 변경된 내역을 Base-Delta 패턴으로 추가
* `elf link <from> <to>`: 지식 간 양방향 참조 (Cross-ref)
* `elf bundle <id>`: Revision 체인을 시간 순서대로 묶어, AI가 컨텍스트를 완벽하게 재구성할 수 있도록 함
* `elf validate`: Vault 스키마, 링크 연결 등 전체 워크스페이스 검증기
* `elf sync record --summary <text>`: 각 AI 세션의 작업 내용을 로그로 관리

## 4. 현재 상황 및 주요 제안 사항 (Proposals)

### Proposal 002: MCP (Model Context Protocol) Server Auto-add
(*milestones/proposals/002-mcp-server-config-auto-add.md*)

**개요:** Vault 생성(init) 후 AI 에이전트(Claude Desktop, Cursor 등)가 Elendirna를 쉽게 인식할 수 있도록 MCP 설정 JSON 구성 관리를 자동화합니다.
**현안:**
- `elf init` 또는 `elf serve --mcp --auto-init` 호출 시, 에이전트 설정 파일에 바로 적용할 수 있는 JSON 스니펫을 출력하거나(`.elendirna/mcp_server_snippet.json` 경로 등).
- `--target` 등의 플래그를 통해 커스텀(글로벌) `settings.json` 내 `mcpServers.elendirna` 항목에 설정을 자동 병합(Injection)해주는 기능 추가.

**효과:** 무설정(Zero-Config)에 가까운 프로젝트 초기화 경험 제공 및 에이전트 연동의 구동 시간 단축.
