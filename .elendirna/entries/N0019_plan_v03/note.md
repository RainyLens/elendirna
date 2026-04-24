---
id: "N0019"
title: "PLAN v0.3"
baseline: null
tags:
  - "plan"
---
# PLAN v0.3 — 에이전트 워크플로 완성

> 전제 조건: v0.2 성공 기준 달성 (✅)
> 논의 근거: → see N0018 (v0.3 Discussion Notes, r0000~r0008)
> 이전 계획: → see N0013 (PLAN v0.2)

---

## 테마

**"에이전트가 자연스럽게 쓴다"** — 도구의 존재를 아는 것에서, 적시에 쓰는 것으로.

---

## 의존 관계 그래프

```
Phase 1 (CLI 입력 경로 확장)
    ↓
Phase 2 (bundle 고도화)
    ↓           
Phase 3 (MCP 자기 서술)
    ↓
Phase 4 (운영 도구)
```

---

## Phase 1 — CLI 입력 경로 확장

**목표:** 에이전트/스크립트가 CLI 우회 없이 본문을 주입할 수 있게 한다.

### 태스크

- [ ] `elf entry new <title> --body-from <file>`: 지정 파일의 내용을 note.md 본문으로 삽입
- [ ] `elf entry new <title> --body-stdin`: stdin을 본문으로 삽입 (`cat file | elf entry new "title" --body-stdin`)
- [ ] `elf revision add <id> --delta-from <file>`: delta를 파일에서 읽기
- [ ] 인코딩: 모든 입력을 UTF-8로 정규화 (CRLF → LF 변환 포함)
- [ ] 테스트: 한글 포함 파일 입력 시 깨짐 없음 확인

**완료 기준:** 마이그레이션 세션과 동일한 작업을 CLI만으로 수행 가능

---

## Phase 2 — bundle 고도화

**목표:** 에이전트가 컨텍스트 예산에 맞춰 탐색 범위를 조절한다.

### 태스크

- [ ] `bundle --depth N`: linked entries 그래프 탐색 깊이 지정 (기본값: 1, 현행 유지)
  - `--depth 0`: 자기 자신 + revisions만
  - `--depth 2+`: depth>1은 manifest 메타데이터만 포함 (본문 생략)
- [ ] `elf entry show --bundle`: show + bundle 통합 one-shot 출력
- [ ] `elf help --json`: 전체 명령 표면을 구조화 JSON으로 출력
  - 각 커맨드별 인자, 설명, 트리거 조건 포함

**완료 기준:** `elf bundle N0018 --depth 2`가 N0018 + 직접 링크(전문) + 2홉 링크(메타데이터만) 구조로 출력

---

## Phase 3 — MCP 자기 서술

**목표:** 정적 파일(CLAUDE.md) 없이 에이전트가 워크플로를 자동 이해한다.

### 태스크

- [ ] MCP 도구 `description` 강화 — 모든 도구에 트리거 조건 명시 (§DESIGN.md 3.5 준수)
  - `entry_new`: "새로운 아이디어/주제가 대화에서 등장했을 때 호출"
  - `revision_add`: "기존 아이디어가 수정/보완되었을 때 호출"
  - `link`: "두 아이디어 간 관련성이 확인되었을 때 호출"
  - `sync_record`: "작업 세션 마무리 시 호출하면 좋습니다"
  - `bundle`: "기존 아이디어의 맥락을 파악해야 할 때 호출"
- [ ] `server.description` 강화: 사관(史官) 톤의 워크플로 요약 탑재
- [ ] `elf serve --mcp` 미연결 시 설정 snippet 동적 출력 (바이너리 경로 + vault 경로 자동 반영)

**완료 기준:** MCP 서버에 연결한 새 에이전트가 별도 가이드 없이 entry/revision/link/sync를 자연스럽게 사용

---

## Phase 4 — 운영 도구

**목표:** vault 자가 유지보수 원스톱 지원.

### 태스크

- [ ] `elf doctor`: validate + index.sqlite 재생성 + 구조 점검을 한 번에 실행
  - validate 결과 요약 + 자동 수정 가능 항목 표시
  - `--fix` 플래그로 일괄 자동 수정

**완료 기준:** `elf doctor --fix` 한 번으로 vault가 건강 상태로 복원

---

## 성공 기준

- [ ] 새 에이전트 세션이 CLAUDE.md 없이 MCP 연결만으로 vault를 자연스럽게 사용
- [ ] 외부 파일 15개를 CLI만으로 (파일 직접 조작 없이) vault에 이관 가능
- [ ] `elf bundle --depth 2` 출력이 토큰 예산 내에서 유의미한 맥락을 제공
- [ ] `elf doctor --fix` 후 `elf validate` 0 errors

