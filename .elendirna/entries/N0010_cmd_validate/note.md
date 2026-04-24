---
id: "N0010"
title: "cmd-validate"
baseline: null
tags: []
---

# `elf validate` 설계 문서

## 목적

vault 전체의 무결성을 검사한다. 스키마 위반, dangling 참조, baseline 사이클, manifest ↔ frontmatter 불일치를 탐지하여 보고한다. CI 파이프라인에서 자동 실행될 수 있도록 종료 코드로 결과를 전달한다.

---

## CLI 인터페이스

```
elf validate [--fix]
```

| 플래그 | 타입 | 필수 | 설명 |
|--------|------|------|------|
| `--fix` | bool | ❌ | 자동 수정 가능한 항목 수정 후 재검사 |

### 예시

```sh
elf validate
elf validate --fix
elf validate --json    # 구조화 출력 (CI 연동용)
```

---

## 동작 흐름

1. vault 루트 탐지.
2. 아래 검사를 순서대로 실행하여 이슈 목록 수집.
3. `--fix` 플래그 시: 자동 수정 가능한 항목 수정 후 재검사.
4. 이슈 없으면 `✓ All checks passed` + 종료 코드 0.
5. 이슈 있으면 목록 출력 + 종료 코드 1.

---

## 검사 항목

### 1. 네이밍 규칙 (Naming)

| 대상 | 규칙 |
|------|------|
| EntryID | `N\d{4}` 형식 |
| 디렉터리명 | `N\d{4}_[a-z0-9_가-힣]+` |
| revision 파일 | `r\d{3}\.md` |

```
WARN [naming] entries/n42_rust/ — expected N0042_rust/
```

### 2. Manifest 스키마 (Schema)

`manifest.toml`에 필수 필드(`schema_version`, `id`, `title`, `created`, `updated`, `status`) 누락 또는 타입 불일치.

```
ERROR [schema] entries/N0042_rust/manifest.toml — missing field: status
```

### 3. Manifest ↔ Frontmatter 불일치 (Consistency)

`manifest.toml`의 `id`, `title`, `baseline`, `tags`가 `note.md` YAML frontmatter와 다른 경우. manifest가 single source of truth이므로 frontmatter가 틀린 것으로 간주.

```
WARN [consistency] entries/N0042_rust/note.md — frontmatter.title differs from manifest
  manifest: "Rust ownership 개념 정리"
  note.md:  "Rust ownership"
  fix: update note.md frontmatter
```

`--fix` 시 manifest 값으로 frontmatter를 자동 갱신.

### 4. Dangling 참조 (Dangling Refs)

`manifest.toml`의 `links`, `baseline`, `sources` 필드 및 `note.md`/`revisions/*.md` 본문의 `→ see N####` 패턴이 존재하지 않는 entry나 asset을 가리키는 경우.

```
ERROR [dangling] entries/N0042_rust/manifest.toml — links["N0099"] not found
ERROR [dangling] revisions/N0042/r001.md:7 — "→ see N0099" not found
```

### 5. Baseline 사이클 (Cycle)

`baseline` 체인을 따라가면 자기 자신으로 돌아오는 순환 참조.

```
ERROR [cycle] N0042 → N0031 → N0019 → N0042 (cycle detected)
```

### 6. Orphan Revision

`revisions/<id>/` 에 revision 파일이 있지만 해당 entry가 `entries/`에 없는 경우.

```
WARN [orphan] revisions/N0099/ — entry "N0099" not found
```

### 7. Asset 무결성 (Assets)

`sources` 필드에 기재된 파일이 `assets/`에 실재하지 않는 경우.

```
ERROR [dangling] entries/N0042_rust/manifest.toml — sources["assets/pdf/rust_book.pdf"] not found
```

---

## 출력 형식

```
Validating vault 'personal' (14 entries, 23 revisions)...

  ERROR [dangling]     entries/N0042_rust/manifest.toml — links["N0099"] not found
  ERROR [cycle]        N0019 → N0031 → N0019 (cycle detected)
  WARN  [consistency]  entries/N0042_rust/note.md — frontmatter.title differs from manifest
  WARN  [naming]       entries/n42_rust/ — expected N0042_rust/

4 issue(s) found (2 errors, 2 warnings)
```

`--fix` 후:
```
  FIXED [consistency] entries/N0042_rust/note.md — frontmatter updated from manifest
  FIXED [naming]      entries/n42_rust/ → N0042_rust/
  ERROR [dangling]    entries/N0042_rust/manifest.toml — links["N0099"] not found (cannot auto-fix)
  ERROR [cycle]       N0019 → N0031 → N0019 (cannot auto-fix)

2 issue(s) remaining (2 errors, 0 warnings)
```

---

## 에러 처리

| 상황 | 종료 코드 |
|------|-----------|
| 모든 검사 통과 | 0 |
| 이슈 존재 | 1 |
| vault 루트 아님 | 1 (E2001) |
| manifest 파싱 실패 | 1 (E4003) |

---

## 구현 노트

- 이슈는 `Vec<Issue>` 로 수집. `Issue { severity, kind, path, message, fix }` 구조체.
- `--fix` 자동 수정 가능 항목: `consistency` (frontmatter ← manifest), `naming` (파일/디렉터리 rename).
- `cycle` 탐지: DFS + visited set. 깊이 제한은 Open Question §11.4에서 결정.
- `--json` 출력 시 이슈 배열 + 요약 카운트를 JSON으로:
  ```json
  {
    "vault": "personal",
    "entries": 14,
    "issues": [
      { "severity": "error", "kind": "dangling", "path": "...", "message": "...", "fix": null }
    ],
    "summary": { "errors": 2, "warnings": 2 }
  }
  ```
- CI 사용 예: `elf validate --json | jq '.summary.errors == 0'`
