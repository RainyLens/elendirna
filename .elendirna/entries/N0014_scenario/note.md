---
id: "N0014"
title: "SCENARIO"
baseline: null
tags: []
---

# 사용 시나리오: 3일간의 아이디어 탐구

> 이 문서는 Elendirna v0.1의 실제 사용 흐름을 보여주는 워크스루입니다.
> CLI 출력은 설계 기준의 예시이며, 실제 구현 전 UX 검토용입니다.

---

## 배경

pluvia는 "지식 검색"에 관한 생각을 정리하고 싶다. 처음엔 벡터 검색이 답이라고 생각했지만, 며칠 사이에 그 가정이 흔들리고, 새로운 관점이 생겨난다. Elendirna는 그 **변화의 흔적**을 남기는 도구다.

---

## Day 1 — 첫 아이디어 기록

### vault 초기화

```sh
$ elf init ~/notes/thinking
```
```
Initialized vault at ~/notes/thinking
  Created .elendirna/config.toml
  Created CLAUDE.md
  Created README.md
Run `elf help --json` to discover commands.
```

생성된 `CLAUDE.md`:
```markdown
# Elendirna vault

이 저장소는 `elf` CLI로만 수정합니다. 직접 파일 편집 금지.
시작 시 `elf help --json`으로 명령 표면을 확인하고, 작업 종료 시 `elf sync record`로 기록하세요.
스키마/규칙 위반은 `elf validate`가 보고합니다 — 에러의 `fix` 필드를 따르면 됩니다.
```

---

### 첫 번째 entry: 벡터 검색이 답이다

```sh
$ elf entry new "벡터 검색이 지식 검색의 답이다" --tags search,knowledge
```
```
Created entry N0001: "벡터 검색이 지식 검색의 답이다"
Path: entries/N0001_벡터_검색이_지식_검색의_답이다/
```

생성된 `entries/N0001_벡터_검색이_지식_검색의_답이다/note.md`:
```markdown
---
id: N0001
title: 벡터 검색이 지식 검색의 답이다
tags: [search, knowledge]
---

# 벡터 검색이 지식 검색의 답이다

임베딩 공간에서 의미적 유사도를 계산하면 키워드 검색이 못 잡는
맥락을 잡을 수 있다. RAG 파이프라인의 핵심이 여기 있다고 생각한다.

주요 가정:
- 의미적으로 가까운 것이 관련성도 높다
- 전역 맥락(벡터 공간 전체)이 지역 맥락(연결 구조)보다 중요하다
```

---

## Day 2 — 가정이 흔들린다

논문을 읽다가 벡터 검색이 놓치는 부분을 발견했다. 노드 간 **경로** 자체가 의미를 갖는 경우다. 생각이 바뀌었다. 새 entry를 쓰지 않고, **N0001에 revision을 남긴다** — 전체를 다시 쓰는 게 아니라 무엇이 어떻게 달라졌는지만 기록한다.

### revision 추가: 가정 수정

```sh
$ elf revision add N0001 --delta "Perozzi et al. (DeepWalk)를 읽고 가정 수정.
벡터 유사도는 '비슷한 것'을 잘 찾지만, '어떻게 연결돼 있는가'를 잃는다.
지식 그래프에서 A→B→C의 경로 자체가 추론의 근거가 되는 경우,
벡터 검색은 A와 C가 가깝다는 것만 알고 B를 거친다는 사실을 버린다.
'전역 맥락 > 지역 맥락'이라는 가정을 보류한다."
```
```
Added revision N0001@r001
Path: revisions/N0001/r001.md
```

생성된 `revisions/N0001/r001.md`:
```markdown
---
baseline: N0001@r000
created: 2026-04-08T11:23:00Z
---

## Delta

Perozzi et al. (DeepWalk)를 읽고 가정 수정.
벡터 유사도는 '비슷한 것'을 잘 찾지만, '어떻게 연결돼 있는가'를 잃는다.
지식 그래프에서 A→B→C의 경로 자체가 추론의 근거가 되는 경우,
벡터 검색은 A와 C가 가깝다는 것만 알고 B를 거친다는 사실을 버린다.
'전역 맥락 > 지역 맥락'이라는 가정을 보류한다.
```

---

## Day 3 — 새로운 방향, 새 entry

단순히 N0001을 수정하는 수준을 넘어 **다른 프레임으로** 보기 시작했다. 이건 N0001의 수정이 아니라 파생된 별개의 생각이다. 새 entry를 만들되 `--baseline N0001`로 계보를 잇는다.

### 파생 entry: 그래프 탐색이 대안이다

```sh
$ elf entry new "그래프 탐색 기반 지식 검색" --baseline N0001 --tags search,knowledge,graph
```
```
Created entry N0002: "그래프 탐색 기반 지식 검색"
Path: entries/N0002_그래프_탐색_기반_지식_검색/
  baseline: N0001
```

N0002의 `note.md`를 편집한다:

```sh
$ elf entry edit N0002
# $EDITOR 열림 → 저장 후 종료
# manifest.toml의 updated 자동 갱신
```

편집 후 `note.md`:
```markdown
---
id: N0002
title: 그래프 탐색 기반 지식 검색
baseline: N0001
tags: [search, knowledge, graph]
---

# 그래프 탐색 기반 지식 검색

N0001에서 벡터 검색의 한계를 인식한 뒤, 그래프 구조를 활용하는 방향으로 선회.

핵심 직관:
- 노드보다 엣지(관계)가 더 풍부한 정보를 담는다
- 검색은 "가장 비슷한 것 찾기"가 아니라 "가장 짧은 추론 경로 찾기"일 수 있다

아직 검증되지 않은 가정:
- 그래프가 충분히 밀할 때만 작동하지 않을까?
- 희소 그래프에서도 유효한지 확인 필요. → see N0001
```

---

### 두 entry를 연결

N0001과 N0002는 대립하는 게 아니라 같은 문제를 다른 각도로 보는 것이다. 공식 연결을 만든다.

```sh
$ elf link N0001 N0002
```
```
Linked N0001 ↔ N0002
```

내부적으로 두 `manifest.toml` 모두 업데이트:
```toml
# N0001/manifest.toml
links = ["N0002"]

# N0002/manifest.toml
links = ["N0001"]
```

---

### 무결성 검사

```sh
$ elf validate
```
```
Validating vault 'thinking' (2 entries, 1 revision)...

✓ All checks passed
```

---

### 현재 상태 확인

```sh
$ elf entry show N0002
```
```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
N0002 — 그래프 탐색 기반 지식 검색
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
status:   draft
tags:     search, knowledge, graph
baseline: N0001 ("벡터 검색이 지식 검색의 답이다")
links:    N0001
created:  2026-04-09
updated:  2026-04-09

─── note ──────────────────────────────────────

그래프 탐색 기반 지식 검색

N0001에서 벡터 검색의 한계를 인식한 뒤...
(이하 note.md 본문)
```

---

## 3일 후 vault 구조

```
~/notes/thinking/
├── .elendirna/
│   ├── config.toml
│   └── sync.jsonl        ← 3일간의 에이전트/사용자 액션 기록
├── entries/
│   ├── N0001_벡터_검색이_지식_검색의_답이다/
│   │   ├── manifest.toml  (links: [N0002])
│   │   └── note.md
│   └── N0002_그래프_탐색_기반_지식_검색/
│       ├── manifest.toml  (baseline: N0001, links: [N0001])
│       └── note.md
├── revisions/
│   └── N0001/
│       └── r001.md        ← "전역 맥락 > 지역 맥락 가정 보류"
├── CLAUDE.md
└── README.md
```

**아이디어 계보 (그래프로 보면):**

```
N0001 "벡터 검색"
  │
  ├─[r001]─▶ "전역 맥락 가정 보류" (revision)
  │
  └─[파생]─▶ N0002 "그래프 탐색"
                │
                └─[연결]─▶ N0001  (양방향)
```

---

## 이 시나리오가 보여주는 것

| 상황 | Elendirna의 응답 |
|------|----------------|
| 생각이 바뀌었다 | `revision add --delta "..."` — 변화의 이유를 서술형으로 남긴다 |
| 완전히 다른 프레임이 생겼다 | `entry new --baseline N####` — 계보를 유지하며 새 entry |
| 두 아이디어가 연결된다 | `link N#### N####` — 양방향 공식 관계 |
| 본문에서 자연스럽게 언급 | `→ see N####` — 인라인 참조, validate가 dangling 탐지 |
| 다 맞게 기록됐는지 확인 | `validate` — 구조 무결성 보장 |

**기록되지 않는 것**: `lr=0.001`, `batch_size=64` 같은 수치. 이 툴은 **왜 생각이 바뀌었는가**를 추적한다.
