# Elendirna Vault — Codex / OpenAI Agents

이 vault는 `elf` CLI 및 MCP 서버(`elf serve --mcp`)로 관리되는 지식 저장소입니다.
**파일을 직접 편집하지 마세요.** 반드시 아래 도구를 사용하세요.

---

## 세션 시작 프로토콜

새 세션이 시작되면 다음 순서로 컨텍스트를 복원하세요:

1. **`sync_record` 최근 기록 확인** — 이전 세션에서 무엇을 했는지 파악
   - `sync_log` tool 또는 `elf sync log --tail 5` 로 확인
2. **관련 entry 탐색** — `query(tag=..., title_contains=...)` 로 작업 범위 파악
3. **핵심 entry 로드** — `bundle(id)` 로 revision chain + 링크된 entry 전체 수신

## 세션 종료 프로토콜

세션을 마칠 때 반드시 `sync_record` tool을 호출하세요:

```
summary: "오늘 한 작업의 핵심 변화 한두 줄"
entries: ["N0001", ...]   ← 작업한 entry ID 목록
agent:   "codex"          ← 또는 현재 모델명
```

---

## 사용 가능한 MCP Tool

| Tool | 설명 |
|---|---|
| `entry_list` | vault 전체 entry 목록. `tag`, `status` 필터 지원 |
| `entry_show` | entry manifest + note body 조회 |
| `entry_new` | 새 entry 생성. `title`, `baseline`, `tags` |
| `revision_add` | entry에 delta 추가. 생각의 변화를 기록 |
| `bundle` | entry + revision chain + 링크된 entry 수집. **컨텍스트 복원의 핵심** |
| `query` | sqlite 인덱스 기반 검색. `tag`, `status`, `title_contains`, `baseline` |
| `sync_record` | 세션 요약을 sync.jsonl에 기록 |
| `validate` | vault 무결성 검사 + index.sqlite 재생성 |

---

## 규칙

- entry note(`note.md`)를 직접 수정하지 않습니다 — `revision_add`로 delta를 쌓으세요
- 다른 entry를 참조할 때 본문에 `→ see N####` 패턴을 사용하세요
- `validate`는 dangling link, orphan revision, 스키마 오류를 모두 검사합니다
- delta는 "무엇이 왜 바뀌었는가"를 중심으로 작성하세요 (전체 재작성 X)
