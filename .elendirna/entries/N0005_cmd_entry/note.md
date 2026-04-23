---
id: "N0005"
title: "cmd-entry"
baseline: null
tags: []
---

# `elf entry` 설계 문서

## 목적

vault의 핵심 지식 단위인 **entry**를 생성·편집·조회한다. entry는 하나의 아이디어, 개념, 탐구 주제를 담는 컨테이너이며, 필요 시 다른 entry를 `baseline`으로 삼아 파생(Base-Delta)된다.

---

## CLI 인터페이스

```
elf entry new <title> [--baseline <N####>] [--tags <tag,...>]
elf entry edit <id>
elf entry show <id>
```

| 서브커맨드 | 필수 인자 | 설명 |
|-----------|----------|------|
| `new` | `<title>` | 새 entry 생성. ID 자동 채번 |
| `edit` | `<id>` | `$EDITOR`로 note.md 편집 후 `updated` 자동 갱신 |
| `show` | `<id>` | manifest + note 내용 출력 |

### `entry new` 플래그

| 플래그 | 타입 | 필수 | 설명 |
|--------|------|------|------|
| `--baseline <N####>` | EntryID | ❌ | 이 entry가 파생된 부모 entry |
| `--tags <tag,...>` | string | ❌ | 쉼표로 구분된 태그 목록 |

### 예시

```sh
# 새 entry 생성
elf entry new "Rust ownership 개념 정리"

# 기존 entry에서 파생 (아이디어 발전)
elf entry new "borrow checker 한계 탐구" --baseline N0042 --tags rust,advanced

# entry 편집 ($EDITOR 열림)
elf entry edit N0042

# entry 내용 확인
elf entry show N0042
elf entry show N0042 --json   # 구조화 출력
```

---

## EntryID 채번 규칙

- 형식: `N{n:04}` (예: `N0001`, `N0042`, `N1000`)
- 채번 기준: `entries/` 디렉터리를 스캔하여 기존 최대 번호 + 1
- 디렉터리명: `N####_<slug>` — title을 소문자 + 언더스코어로 변환한 slug
  - 예: `"Rust ownership 개념 정리"` → `N0042_rust_ownership_개념_정리`
  - slug 변환 규칙: 공백→`_`, 영숫자·한글·`_`만 허용, 최대 40자

---

## 동작 흐름 — `entry new`

1. vault 루트 탐지 (`.elendirna/config.toml` 존재 확인).
2. `--baseline` 지정 시 해당 entry 존재 여부 확인.
3. 다음 EntryID 채번 → 디렉터리명 생성.
4. `entries/N####_<slug>/` 생성.
5. `manifest.toml` 생성:
   ```toml
   schema_version = 1
   id = "N0042"
   title = "Rust ownership 개념 정리"
   created = "2026-04-07T14:30:00Z"
   updated = "2026-04-07T14:30:00Z"
   tags = ["rust"]
   baseline = "N0031"   # --baseline 지정 시만
   links = []
   sources = []
   status = "draft"
   ```
6. `note.md` 생성 (frontmatter + 빈 본문):
   ```markdown
   ---
   id: N0042
   title: Rust ownership 개념 정리
   baseline: N0031
   tags: [rust]
   ---

   # Rust ownership 개념 정리

   <!-- 아이디어를 자유롭게 작성하세요 -->
   ```
7. `attachments/` 빈 디렉터리 생성.
8. `sync.jsonl`에 이벤트 기록.
9. 성공 메시지:
   ```
   Created entry N0042: "Rust ownership 개념 정리"
   Path: entries/N0042_rust_ownership_개념_정리/
   ```

## 동작 흐름 — `entry edit`

1. `<id>`로 entry 디렉터리 탐색.
2. `$EDITOR` (또는 `config.toml`의 `editor` 필드) 로 `note.md` 열기.
3. 편집기 종료 후 `manifest.toml`의 `updated` 필드를 현재 시각으로 갱신.
4. `note.md`의 frontmatter와 `manifest.toml`의 핵심 필드 일관성 자동 검증 (불일치 시 경고).

## 동작 흐름 — `entry show`

- 기본 출력: manifest 요약 + note.md 전문을 터미널에 렌더링.
- `--json`: manifest 전체 + note 본문을 JSON으로 출력.

---

## 파일시스템 영향 (`entry new`)

| 경로 | 동작 |
|------|------|
| `entries/N####_<slug>/` | 새로 생성 |
| `entries/N####_<slug>/manifest.toml` | 새로 생성 |
| `entries/N####_<slug>/note.md` | 새로 생성 |
| `entries/N####_<slug>/attachments/` | 빈 디렉터리 생성 |
| `.elendirna/sync.jsonl` | 이벤트 append |

---

## 에러 처리

| 상황 | 에러 코드 | 메시지 |
|------|-----------|--------|
| vault 루트 아님 | E2001 | `not inside an elf vault (run elf init first)` |
| `--baseline` entry 없음 | E2002 | `baseline "N0031" not found` |
| `<id>` entry 없음 (edit/show) | E2002 | `entry "N0042" not found` |
| `$EDITOR` 미설정 | E4002 | `editor not configured; set $EDITOR or config.toml editor field` |

---

## 구현 노트

- slug 변환은 Unicode-aware: 한글은 그대로 유지, 공백→`_`, 특수문자 제거.
- `entry edit` 는 편집기 종료 코드가 0일 때만 `updated` 갱신 (편집 취소 시 변경 없음).
- `--baseline`이 없으면 `manifest.toml`에서 `baseline` 필드 자체를 생략 (빈 문자열 X).
- 멱등성: 동일 title로 `entry new` 재호출 시 → `E3001 (already_exists)`, 기존 entry 경로 반환.
