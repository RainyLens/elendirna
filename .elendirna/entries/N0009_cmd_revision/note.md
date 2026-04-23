---
id: "N0009"
title: "cmd-revision"
baseline: null
tags: []
---

# `elf revision` 설계 문서

## 목적

entry에 **아이디어의 변화(delta)**를 기록한다. revision은 수치 파라미터 변경이 아니라 사고의 전환점 — 가정의 수정, 접근법의 변경, 새로운 통찰 — 을 서술형으로 남기는 단위다. Base-Delta 원칙에 따라 전체를 다시 쓰는 대신, 무엇이 어떻게 달라졌는지만 기록한다.

---

## CLI 인터페이스

```
elf revision add <id> --delta <text>
```

| 인자 / 플래그 | 타입 | 필수 | 설명 |
|--------------|------|------|------|
| `<id>` | EntryID | ✅ | 대상 entry ID (예: `N0042`) |
| `--delta <text>` | string | ✅ | 아이디어의 변화를 서술하는 자유 형식 텍스트 |

### 예시

```sh
# 가정이 바뀐 경우
elf revision add N0042 --delta "ownership을 '메모리 안전'의 도구로만 봤으나, \
  타입 시스템의 선형성(linearity) 관점으로 재해석. 이후 탐구 방향 변경."

# 접근법 전환
elf revision add N0031 --delta "그래프 순회 대신 벡터 유사도 검색으로 접근을 바꿈. \
  이유: 지역성보다 전역 맥락이 더 중요하다는 판단."

# 새 연결고리 발견
elf revision add N0019 --delta "N0033의 아이디어와 합류점 발견. \
  두 접근이 같은 문제의 다른 면임을 인식. → see N0033"

# 긴 delta는 파일로 전달
elf revision add N0042 --delta "$(cat delta_draft.md)"
```

---

## 동작 흐름

1. vault 루트 탐지.
2. `<id>`로 entry 존재 확인 (`entries/N####_*/manifest.toml`).
3. `revisions/<id>/` 디렉터리가 없으면 생성.
4. 기존 revision 파일 스캔 → 다음 RevisionID 채번: `r{n:03}` (예: `r001`).
   - 첫 revision이면 baseline은 entry 자체의 현재 상태 (`N0042@r000`으로 표기).
5. `revisions/N0042/r001.md` 생성:
   ```markdown
   ---
   baseline: N0042@r000
   created: 2026-04-08T09:15:00Z
   ---

   ## Delta

   ownership을 '메모리 안전'의 도구로만 봤으나,
   타입 시스템의 선형성(linearity) 관점으로 재해석. 이후 탐구 방향 변경.
   ```
6. `manifest.toml`의 `updated` 갱신.
7. `sync.jsonl`에 이벤트 기록.
8. 성공 메시지:
   ```
   Added revision N0042@r001
   Path: revisions/N0042/r001.md
   ```

---

## RevisionID 채번 규칙

- 형식: `r{n:03}` (예: `r001`, `r002`, `r012`)
- baseline 표기: `{EntryID}@{RevisionID}` (예: `N0042@r000`, `N0042@r001`)
  - `@r000`은 entry 생성 시점의 초기 상태를 의미 (파일 없음, 개념적 앵커)
- 채번 기준: `revisions/<id>/` 스캔으로 기존 최대 번호 + 1

---

## 파일시스템 영향

| 경로 | 동작 |
|------|------|
| `revisions/N####/` | 없으면 새로 생성 |
| `revisions/N####/r###.md` | 새로 생성 |
| `entries/N####_*/manifest.toml` | `updated` 갱신 |
| `.elendirna/sync.jsonl` | 이벤트 append |

---

## 데이터 구조

```markdown
---
baseline: N0042@r000        # 이 revision이 파생된 시점
created: 2026-04-08T09:15:00Z
---

## Delta

[아이디어의 변화를 서술형으로 기록. 길이 제한 없음]

<!-- 다른 entry 참조는 "→ see N####" 형식으로 -->
```

**delta에 담을 내용의 예:**

- 이전에 틀렸던 가정과 수정 이유
- 접근법이나 프레임의 전환점
- 새로 발견한 다른 entry와의 연결
- 더 이상 추구하지 않기로 결정한 방향과 그 근거
- 외부 자료(논문, 대화 등)에서 얻은 통찰이 기존 생각을 어떻게 바꿨는지

**delta에 담지 않을 내용:**

- 수치 파라미터 변경 기록 (`lr=0.001` 같은 것 — 이 툴의 목적이 아님)
- entry 전체 재작성 (그럴 경우 `entry new --baseline N####`으로 새 entry)

---

## 에러 처리

| 상황 | 에러 코드 | 메시지 |
|------|-----------|--------|
| vault 루트 아님 | E2001 | `not inside an elf vault` |
| entry 없음 | E2002 | `entry "N0042" not found` |
| `--delta` 미입력 | E1001 | `flag --delta is required` |
| `--delta` 빈 문자열 | E1002 | `delta text cannot be empty` |

---

## 구현 노트

- `--delta` 값은 `String`으로 받아 그대로 저장. 포맷 검증 없음 — 자유 형식이 핵심.
- revision 파일은 사람이 직접 열어 편집해도 되는 단순 마크다운. Protocol-first 원칙상 CLI 없이도 읽기·수정 가능.
- `→ see N####` 패턴은 `elf validate`와 `elf link check`(validate 내부)가 dangling ref 여부를 검증.
- 향후 `elf revision list <id>` 서브커맨드로 entry의 revision 체인을 시간순으로 출력하는 기능 추가 예정 (v0.2).
