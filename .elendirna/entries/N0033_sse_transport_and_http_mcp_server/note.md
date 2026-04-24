---
id: "N0033"
title: "SSE Transport & HTTP MCP Server 설계 논의"
baseline: null
tags:
  - "design"
  - "mcp"
  - "transport"
  - "multi-vault"
  - "auth"
---

# SSE Transport & HTTP MCP Server 설계 논의

> 선결 조건: Multi-Vault 설계 안착 (→ see N0018 r0015, N0020)
> v0.4 범위 외 — Multi-Vault 완성 후 논의 재개

## 현재 상황

`elf serve --mcp`는 stdio transport만 지원.
Claude Desktop 등 로컬 프로세스와 파이프로 통신하는 단일 사용자 모델.

## SSE/HTTP로 전환 시 발생하는 설계 문제

### cwd 인텐트 신호 붕괴
현재 vault 탐색 원칙: "cwd = 인텐트 신호" (→ see N0018 r0015).
HTTP MCP 서버는 상주 프로세스로 cwd가 의미 없음.
→ 서버 기동 시 vault root를 고정해야 함 → global vault가 자연스러운 root.
→ local vault는 `--vault` 파라미터 또는 `@vault:<alias>:` 로 명시 접근.

### Key 기반 인증 필요
HTTP 서버는 사실상 key 발급 구조 — 어느 클라이언트가 어느 요청을 했는지 식별 필요.

### `author` 필드 강제 가능성
현재 `manifest.toml`에 author 없음.
단일 사용자 로컬 모델에서는 불필요했지만,
HTTP 서버 + 다중 클라이언트 환경에서는 entry/revision 작성자 추적이 필요해짐.
→ `author` 필드를 manifest에 **강제**해야 할 수도 있음.

## 미결 설계 질문

- global vault가 서버 root일 때, local vault 접근은 어떤 방식으로?
- `author` 강제 시 기존 vault(v1, v2) 하위 호환성 처리 방법?
- key 발급/관리는 `elf` CLI가 담당하는가, 별도 설정인가?
