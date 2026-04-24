---
id: "N0008"
title: "cmd-link"
baseline: null
tags: []
---

# `elf link` 설계 문서

## 목적

두 entry 사이에 **양방향 cross-reference**를 생성한다. `baseline`이 "이 아이디어는 저 아이디어에서 파생됐다"는 수직 계보라면, `link`는 "이 두 아이디어는 연결된다"는 수평 관계다. 두 entry의 `manifest.toml` 모두에 상대방 ID를 기록하여 단방향 누락을 방지한다.

link의 유효성 검사(dangling ref 탐지)는 `elf validate`가 담당한다.

---

## CLI 인터페이스

```
elf link <from> <to>
```

| 인자 | 타입 | 필수 | 설명 |
|------|------|------|------|
| `<from>` | EntryID | ✅ | 연결의 시작 entry |
| `<to>` | EntryID | ✅ | 연결의 대상 entry |

### 예시

```sh
# N0042와 N0019를 서로 연결
elf link N0042 N0019

# 확인
elf entry show N0042 --json | jq '.manifest.links'
# → ["N0019"]

# 멱등성: 이미 존재하는 링크는 no-op
elf link N0042 N0019   # → OK (no change)
```

---

## 동작 흐름

1. vault 루트 탐지.
2. `<from>`, `<to>` 두 entry 모두 존재 확인.
3. 이미 링크가 존재하면 no-op으로 성공 반환 (멱등성 보장).
4. `<from>`의 `manifest.toml`에 `<to>` 추가:
   ```toml
   links = ["N0019"]   # 기존 links 배열에 append
   ```
5. `<to>`의 `manifest.toml`에 `<from>` 추가 (양방향):
   ```toml
   links = ["N0042"]
   ```
6. 양쪽 manifest의 `updated` 갱신.
7. `sync.jsonl`에 이벤트 기록.
8. 성공 메시지:
   ```
   Linked N0042 ↔ N0019
   ```

---

## 파일시스템 영향

| 경로 | 동작 |
|------|------|
| `entries/N####_*/manifest.toml` (`<from>`) | `links` 배열에 `<to>` 추가, `updated` 갱신 |
| `entries/N####_*/manifest.toml` (`<to>`) | `links` 배열에 `<from>` 추가, `updated` 갱신 |
| `.elendirna/sync.jsonl` | 이벤트 append |

---

## `→ see` 인라인 참조와의 관계

`manifest.toml`의 `links` 배열 외에도, `note.md`나 `revisions/*.md` 본문에서 `→ see N####` 형식의 인라인 참조를 사용할 수 있다. 두 메커니즘의 차이:

| | `elf link` (manifest) | `→ see` (인라인) |
|--|----------------------|-----------------|
| 목적 | 공식적인 양방향 관계 선언 | 서술 중 자연스러운 언급 |
| 자동화 | CLI가 양쪽 동시 기록 | 사람/에이전트가 직접 작성 |
| 검증 | `elf validate`가 dangling 탐지 | 동일 |
| 방향성 | 양방향 | 단방향 (작성 위치 기준) |

두 방식 모두 `elf validate`가 유효성을 검사한다.

---

## 에러 처리

| 상황 | 에러 코드 | 메시지 |
|------|-----------|--------|
| vault 루트 아님 | E2001 | `not inside an elf vault` |
| `<from>` entry 없음 | E2002 | `entry "N0042" not found` |
| `<to>` entry 없음 | E2002 | `entry "N0019" not found` |
| 자기 자신에 링크 | E1003 | `cannot link an entry to itself` |
| 이미 존재하는 링크 | — (no-op) | `N0042 ↔ N0019 already linked (no change)` |

---

## 구현 노트

- TOML 배열 업데이트: manifest 전체를 파싱 후 `links` 배열 수정 → 재직렬화. 순서는 ID 오름차순 정렬 유지.
- 두 manifest 쓰기 중 하나가 실패하면 트랜잭션처럼 롤백(임시 파일 전략): `manifest.toml.tmp` 먼저 쓰고 rename.
- 향후 `elf link remove <from> <to>` 서브커맨드로 링크 제거 기능 추가 예정 (v0.2).
