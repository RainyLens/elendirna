---
id: "N0018"
title: "v0.3 Discussion Notes"
baseline: null
tags:
  - "decision"
  - "proposal"
---
# v0.3 Discussion Notes

> 상태: 미확정 — 논의 자료 모음
> 출처: 2026-04-10 AI 페어 프로그래밍 세션에서 도출된 사항 + 기존 proposals
> 관련: → see N0004 (ROADMAP) · → see N0013 (PLAN_v0.2) · → see N0002 (Export/Migration)

---

## 1. 버그 픽스 (이번 세션에서 발견 및 수정 완료)

### CRLF/LF Frontmatter 파싱 실패
- **증상**: Windows 환경에서 생성된 `note.md`의 본문이 `elf bundle`에서 빈 문자열로 출력됨
- **원인**: `NoteFrontmatter::parse()`와 `parse_revision_file()`이 `\n`만 허용, `\r\n` 무시
- **수정**: `manifest.rs`, `revision.rs`에서 `\r\n` / `\n` 양쪽 유연 처리로 패치 완료
- **교훈**: 크로스 플랫폼 파서는 반드시 양쪽 개행을 허용해야 함. 향후 유사 파서 작성 시 참고

---

## 2. 기능 제안 (Proposals)

### 2-A. Vault Export/Migration Tool (→ see N0002)
- `elf export --target [path]`: 메타데이터 무결성 확인 후 전체 데이터 패키징
- `elf merge [other-vault-path]`: 두 Vault를 안전하게 합치며, `sync.jsonl`을 타임스탬프 기반 병합
- **배경**: 이번 세션에서 `personal_vault` → 루트 Vault 이관 시 수작업 필요했음. `sync.jsonl` 히스토리 단절 문제 확인

### 2-B. `elf init --local-only` 플래그
- `elf init` 시 `.gitkeep`을 `git add --force`하는 로직 스킵
- **배경**: 프로젝트 루트를 개인용 Vault로 쓸 때, `.gitignore`와 충돌하여 수동으로 `git rm --cached` 필요했음
- **판단**: 이번 케이스는 "개발 환경에 Vault를 기생시킨" 특수 케이스. 일반 유스케이스에서는 현행 동작이 올바름. 플래그 추가는 낮은 우선순위

### 2-C. MCP 서버 설정 자동 주입 (→ see N0016)
- `elf serve --mcp` 실행 시 에이전트의 설정 파일(`mcp_config.json` 등)에 서버 설정을 자동 삽입하거나 Snippet 제공
- 기존 Proposal 002에서 상세 기술됨

### 2-D. SSE Transport 지원
- 현재 MCP는 stdio 기반. Claude.ai 웹 등 원격 플랫폼 대상 SSE 연결 필요
- N0013(PLAN_v0.2) OQ-6에서 "v0.3 예정"으로 미결 처리됨

---

## 3. 설계 결정 (이번 세션에서 합의)

### 3-A. ADR은 별도 체계가 아닌 Entry로 흡수
- **기각**: `docs/adr/` 별도 디렉터리 체계
- **채택**: `tags: [decision, architecture]` 컨벤션으로 일반 Entry 내 표현
- **근거**: 별도 디렉터리는 CLI 관리 범위 밖의 파일을 생성하며, "모든 지식은 CLI를 통해 관리한다"는 Protocol-First 원칙에 위배. 기존 `elf query`, `elf bundle`, `elf graph` 도구 체인 위에서 자연스럽게 탐색 가능해야 함

### 3-B. 프로젝트 루트 Dogfooding
- Elendirna 소스코드 저장소 자체를 공식 Vault로 운영
- `milestones/` 하위 15개 설계 문서를 Entry로 이관 완료
- 기존 `PLAN_v0.1_fix`는 독립 Entry가 아닌 `N0011`의 Revision(r001)으로 편입
- `N0011 ↔ N0013` (v0.1 → v0.2) 간 양방향 Link 설정

### 3-C. CLAUDE.md를 프로토콜 계층으로 대체
- **기각**: 정적 파일(`CLAUDE.md`)에 에이전트 가이드라인을 기술하는 방식
- **채택**: MCP 프로토콜 자체에 워크플로 가이드를 내장하는 3단계 접근
- **근거**: `CLAUDE.md`는 에이전트 특정적(Claude 전용)이며, CLI 밖의 정적 파일에 의존하므로 Protocol-First 원칙에 위배

**구현 3단계:**

1. **MCP 도구 설명에 트리거 조건 내장**: 각 도구의 `description`에 "언제 호출하라"를 포함
   - `entry_new`: "새로운 아이디어/주제가 대화에서 등장했을 때"
   - `revision_add`: "기존 아이디어가 수정/보완되었을 때"
   - `link`: "두 아이디어 간 관련성이 확인되었을 때"
   - `sync_record`: "작업 세션 종료 시 반드시"
   - `bundle`: "기존 아이디어의 맥락을 파악해야 할 때"
2. **MCP `server.description`에 워크플로 요약 탑재**: 에이전트가 서버 연결 시점에 즉시 읽게 됨
3. **`elf help --json` 풍부화**: MCP 없이 CLI만 쓰는 에이전트를 위한 폴백

**효과**: 어떤 에이전트든(Claude, Gemini, GPT 등) MCP 서버에 연결하는 순간 워크플로를 자동 이해. 별도 파일 불필요, 소스 코드와 함께 버전 관리됨

### 3-D. MCP 설정 가이드를 동적 자기 서술로 대체
- **기각**: 별도 설정 가이드 문서 제공 (클라이언트별 설정 방법 안내)
- **채택**: `elf serve --mcp` 실행 시, 연결된 클라이언트가 없으면 자기 자신의 설정 snippet을 동적 생성하여 출력
- **근거**: 도구는 자신의 바이너리 경로와 현재 vault 경로를 이미 알고 있으므로 항상 정확한 설정을 생성 가능. 정적 가이드는 MCP 클라이언트(Claude Desktop, Cursor, VS Code 등)의 설정 포맷이 업데이트될 때마다 구식화됨
- **관계**: → see N0016 (Proposal 002)의 `--install` 자동 주입과 연계. 최소한의 구현으로도 "실행하면 답이 나온다" 원칙 달성

---

## 4. 로드맵 원안과의 대조 (N0004 §v0.3)

| 원안 항목 | 이번 세션 판단 |
|-----------|---------------|
| `elf doctor` | 유지. validate + index 일관성 원스톱 점검 |
| `elf migrate --to <N>` | 유지. N0002(Export/Migration)과 통합 검토 |
| `elf help --json` | 유지. CLAUDE.md 의존도를 낮추는 핵심 |
| `elf entry show --bundle` | 유지. 에이전트 one-shot 컨텍스트 로딩 |
| ADR 문서 체계 | **변경**: 별도 `docs/adr/` → Entry + tags 컨벤션으로 흡수 |
| *(신규)* CLAUDE.md → 프로토콜 | 추가. MCP 도구 설명 + server.description으로 대체 |
| *(신규)* MCP 설정 자기 서술 | 추가. `elf serve --mcp`가 설정 snippet 동적 생성 |
| *(신규)* SSE transport | 추가. N0013 OQ-6에서 이관 |
| *(신규)* Export/Merge | 추가. N0002에서 상세 기술 |


