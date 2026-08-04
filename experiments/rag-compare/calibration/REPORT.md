# RAG comparison calibration report

작성일: 2026-08-04

## 판정 요약

8문항 calibration에서는 `strong`보다 `c3`가 current/history conflict 한
건을 추가로 해결했다. 수동 task 판정은 Strong 7/8, C3 8/8이었다. 다만
표본이 작고 calibration 중 gold와 prompt를 보정했으므로 이 수치를 제품
효과의 증거로 사용하면 안 된다.

다음 30문항 pilot에는 Strong과 C3만 올리는 것을 권한다. C1과 C2는
독립적인 정답 증가가 없었고, C4는 C3 대비 추가 정답 없이 context만
늘렸다.

## 고정 조건

- snapshot: 133 entries, 415 revisions, 548 chunks
- snapshot SHA-256:
  `69908d289e25c1d5add464a0847ab750d3846b0ecfeae7810199c463483d2b6f`
- graph SHA-256:
  `09abe0fd040c7773c1e453e23792670b6256d15d80e0286a02da18026b526804`
- 실험 기록 entry `N0139`는 retrieval에서 제외
- embedding: `bge-m3`
- reader: `gemma4:12b`, temperature 0, reasoning effort `none`
- top-k 5, context 상한 24,000 chars
- temporal anchor limit 1, graph neighbor limit 1
- reader output 상한 512 tokens

## 측정 보정

초기 dry run의 `required_refs` 하나만 세는 방식은 flat document가 해당
entry의 모든 revision을 덮어 exact revision 성능을 부풀릴 수 있었다.
다음 항목을 추가했다.

- `primary_refs`와 복수 `acceptable_ref_sets`
- entry recall과 exact revision localization precision
- retrieval source와 최종 reader context의 분리
- route-aware archived/draft filter
- corpus preparation, cache load, warm query latency 분리
- citation presence, malformed citation, context-valid citation precision
- reader truncation과 empty-final 검출
- 문항별 durable checkpoint와 `--resume`

## Retrieval calibration

Gold가 있는 7문항 기준이다. Calibration 답변 검토 과정에서 C004의
`N0133@r0002`/`r0003`, C007의 `N0034@r0001` + `N0041@r0004`가 직접
근거임을 `elf bundle`로 확인하고 alternative gold로 추가했다. 이
사후 보정은 calibration에만 허용하며 pilot에서는 scoring 전에 gold를
동결해야 한다.

| System | Acceptable source success | Acceptable context success | Mean context chars | Strong 대비 |
|---|---:|---:|---:|---:|
| Strong | 6/7 | 6/7 | 11,003 | 1.000x |
| C1 | 6/7 | 6/7 | 11,553 | 1.050x |
| C2 | 6/7 | 6/7 | 14,697 | 1.336x |
| C3 | 6/7 | 7/7 | 15,340 | 1.394x |
| C4 | 6/7 | 7/7 | 16,166 | 1.469x |

C3의 추가 성공은 C006이다. 검색이 `N0131@r0005`를 잡은 뒤 C3가
successor `N0131@r0006`을 context에 추가하여 진단과 실제 후속 조치를
함께 제공했다. C4의 authored graph expansion은 추가 성공이 없었다.

최초 embedding 준비는 16.77초였다. Warm run은 cache load 75.97ms,
corpus prepare 9.55ms, 전체 준비 115.20ms였고 질의 latency는 전 시스템
약 50ms였다.

## Reader calibration

첫 reader 시도는 실패한 실험으로 보존한다. thinking-capable 모델이
512 completion tokens를 전부 `message.reasoning`에 쓰고 final content를
비운 채 종료했다. 이후 `reasoning_effort=none`, empty-final 오류 처리,
checkpoint/resume을 적용했다.

최종 Strong/C3 run 결과:

| Metric | Strong | C3 |
|---|---:|---:|
| 수동 task success | 7/8 | 8/8 |
| Acceptable context success | 6/7 | 7/7 |
| Correct abstention | 1/1 | 1/1 |
| Citation presence (answerable) | 7/7 | 6/7 |
| Context-valid citation precision | 96.4% | 100% |
| Truncation | 0/8 | 0/8 |
| Mean reader latency | 6.39s | 7.04s |
| p50 / p95 reader latency | 6.61s / 8.36s | 6.78s / 10.44s |
| Mean prompt tokens | 5,286 | 7,394 |

C006에서 Strong은 r0005에 적힌 `composer 시제 주석` 후보를 실제로
성능을 뒤집은 후속 조치라고 오인했다. C3는 successor r0006을 받아
실제 적용된 revision-level embedding과 33위→3위 변화를 답했다. 이것이
이번 calibration에서 확인된 유일한 명확한 native 효과다.

C3의 비용은 Strong 대비 context 1.394x, reader latency 약 1.10x, prompt
tokens 약 1.40x다. 설정한 1.5x context gate 안이지만 여유가 크지는 않다.
C3는 C002에서 정답은 맞았으나 citation을 생략해 citation presence가
85.7%였다. Pilot에서는 citation 누락을 별도 실패 축으로 유지해야 한다.

단순 required-claim substring 점수는 한국어 표현 변형에 민감하여 수동
task 판정과 어긋났다. Pilot의 1차 answer metric으로 사용하지 않고,
blinded human rubric과 citation 검증의 보조 smoke check로만 사용한다.

## 운영 중 발견한 결함과 조치

1. embedding/index 준비 시간이 첫 시스템 latency에 섞이던 문제를 분리했다.
2. set 순회 때문에 C4 graph context가 프로세스마다 달라지던 문제를 정렬로 고쳤다.
3. 전체 run 종료 때만 JSONL을 쓰던 문제를 행별 checkpoint와 resume으로 바꿨다.
4. thinking tokens가 final answer budget을 소진하던 문제를 explicit reasoning control로 고쳤다.
5. 빈 final, malformed citation, citation 누락, truncation을 별도 검출한다.
6. 최종 회귀 테스트는 9/9 통과했다.

## 다음 pilot 진입 조건

조건부 GO다. 다음 라운드는 calibration 문항을 재사용하지 않고 30개를
새로 작성한다.

- 시스템은 Strong 대 C3 두 개만 사용한다.
- 질문과 gold를 run 전에 동결하고 사후 gold 추가를 금지한다.
- current decision, rationale, superseded avoidance, interrupted work,
  handoff, absent evidence를 각 5개씩 구성한다.
- 시스템 이름을 숨긴 paired human scoring을 사용한다.
- task correctness, stale temporal error, citation presence/validity,
  abstention, context/latency 비용을 함께 본다.
- C3가 temporal/rationale/handoff에서 15%p 이상 개선하지 못하거나
  reference 질문을 5%p 이상 악화시키면 native 우위 주장을 기각한다.
