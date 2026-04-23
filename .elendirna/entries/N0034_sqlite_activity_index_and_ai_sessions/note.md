---
id: "N0034"
title: "SQLite Activity Index & AI Sessions"
baseline: null
tags:
  - "design"
  - "sqlite"
  - "sessions"
  - "ai-behavior"
  - "cold-start"
---

# SQLite Activity Index & AI Sessions

> → see N0028 (sync.jsonl 비대화 분석 및 Thin Link 전략 — §2.C SQLite Integration)
> → see N0019 r0005 (Recap Entry / cold start 문제)
> → see N0004 r0003 (elf sessions 유보 사유)

## 출발점

N0028 §2.C는 sync.jsonl을 `index.sqlite`의 `history` 테이블로 흡수하자고 제안.
현재 sqlite는 "entry 메타데이터 캐시"이지만, 이를 **"vault 전체 활동 인덱스"** 로 확장하는 방향.

## 설계 방향

### Source of Truth 원칙 유지
- sync.jsonl + manifest = source of truth
- sqlite = 파생 캐시. `elf validate`로 언제든 재생성 가능

### 인덱싱 대상 확장

| 현재 | 확장 후 |
|------|---------|
| entry 메타데이터 (id, title, tags, status) | + sync.record 이벤트 (세션 요약, 관련 entry, agent, ts) |
| | + status 변경 이력 (언제, 누가, draft→stable 등) |
| | + revision 메타데이터 (created, baseline chain) |

### `elf sessions` 구현 경로
`sync.record` 이벤트가 sqlite에 인덱싱되면 `elf sessions`는 단순 SQL 조회로 구현.
sync.jsonl 전체 파싱 불필요.

```
elf sessions [--tail N] [--entry N####] [--agent <name>]
```

## 연결되는 기능들

- **`bundle --since <session_id>`** — sessions 테이블의 ts로 revision 범위 특정 가능
- **`elf entry status`** — status 변경 이벤트를 sqlite에 기록 → 이력 조회 가능
- **AI cold start** — `elf sessions --tail 5` 한 번으로 최근 맥락 수신
- **N0030 SSE Transport** — HTTP 서버 환경에서 author별 세션 필터링 가능

## 미결 설계 질문

- sync.jsonl을 완전히 대체하는가, 병행 유지하는가?
  - 병행 유지 권장: jsonl은 append-only 감사 로그, sqlite는 조회 최적화
- `elf validate` 재생성 시 sync.jsonl → sqlite 방향만 허용 (역방향 불가)
- revision 본문(delta 텍스트)까지 인덱싱하면 full-text search 가능 — 범위에 포함할지?
